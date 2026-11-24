//! The code-mode state machine behind the three admin routes.
//!
//! Port of `backend/internal/plugins/codemode/codemode.go`: preview reads the
//! store, asks the provider for a plan and stores a validated preview with a
//! baseline; apply revalidates, claims and applies the plan in one transaction.
//! The functions here return the exact JSON body the HTTP layer writes.

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Duration;

use azahar_httpapi::ApiProblem;
use azahar_httpapi::respond::{problem, problem_fields};
use azahar_orderintake::HookProvider;
use azahar_plugins::Registry;
use azahar_store::{ProductFilter, Store, StoreError, format_time};
use azahar_storefront::Theme;
use chrono::{DateTime, Utc};
use libsql::params;

use crate::apply::{ApplyError, apply_action};
use crate::client::{
    Client, ConfigError, UpstreamError, config_from_settings, enabled_from_settings,
};
use crate::json;
use crate::plan::{
    ActionResult, BaselineError, Plan, PlanHistoryDto, PlanRecord, ReferenceError,
    encode_result_list, encode_results, new_plan_id, parse_plan, plan_baseline, plan_dto,
    validate_plan_references, validate_plan_shape,
};
use crate::prompt::{PromptOrder, build_prompt, prompt_plugins};

const UNREADABLE_SETTINGS: &str = "These settings cannot be read with the current PLUGIN_SECRET_KEY. Enter them again to replace the old values.";
const PLANS_SQL: &str = "SELECT id, instruction, plan, status, created_at
	 FROM codemode_plans WHERE actor_user_id = ?
	 ORDER BY created_at DESC, id DESC LIMIT ?";

/// Inputs for one preview. `settings == None` means the encrypted row could
/// not be read, which the handler answers with 409 `plugin_settings_unreadable`.
pub struct PreviewInput<'a> {
    pub instruction: &'a str,
    pub actor_user_id: &'a str,
    pub settings: Option<&'a HashMap<String, String>>,
    pub ai_base_url: &'a str,
    pub ai_timeout: Duration,
    pub now: DateTime<Utc>,
}

/// Inputs for one apply.
pub struct ApplyInput<'a> {
    pub plan_id: &'a str,
    pub actor_user_id: &'a str,
    pub settings: Option<&'a HashMap<String, String>>,
    pub now: DateTime<Utc>,
}

/// The code-mode service. It borrows the store, the live registry and the order
/// hook provider for the duration of one request.
pub struct Codemode<'a> {
    pub store: &'a Store,
    pub registry: Option<&'a RwLock<Registry>>,
    pub hooks: &'a dyn HookProvider,
}

impl<'a> Codemode<'a> {
    #[must_use]
    pub fn new(
        store: &'a Store,
        registry: Option<&'a RwLock<Registry>>,
        hooks: &'a dyn HookProvider,
    ) -> Self {
        Self {
            store,
            registry,
            hooks,
        }
    }

    /// `GET /plans`: the current admin's recent previews.
    pub async fn plans(&self, actor_user_id: &str, limit: i64) -> Result<String, ApiProblem> {
        let mut rows = self
            .store
            .conn()
            .query(PLANS_SQL, params![actor_user_id, limit])
            .await
            .map_err(|error| {
                tracing::warn!(error = %error, "codemode plans lookup failed");
                internal("Something went wrong.")
            })?;
        let mut plans = Vec::new();
        loop {
            let row = rows.next().await.map_err(|error| {
                tracing::warn!(error = %error, "codemode plan could not be read");
                internal("Something went wrong.")
            })?;
            let Some(row) = row else { break };
            let scan = |index: i32| {
                row.get::<String>(index).map_err(|error| {
                    tracing::warn!(error = %error, "codemode plan could not be read");
                    internal("Something went wrong.")
                })
            };
            let mut item = PlanHistoryDto {
                id: scan(0)?,
                instruction: scan(1)?,
                summary: String::new(),
                status: scan(3)?,
                created_at: scan(4)?,
            };
            let plan_json = scan(2)?;
            if let Ok(plan) = parse_plan(&plan_json) {
                item.summary = plan.summary;
            }
            plans.push(item);
        }

        let mut body = String::from("{\"plans\":[");
        for (index, item) in plans.iter().enumerate() {
            if index > 0 {
                body.push(',');
            }
            let mut object = json::Object::new();
            object.string_field("id", &item.id);
            object.string_field("instruction", &item.instruction);
            object.string_field("summary", &item.summary);
            object.string_field("status", &item.status);
            object.string_field("created_at", &item.created_at);
            body.push_str(&object.finish());
        }
        body.push_str("]}");
        Ok(body)
    }

    /// `POST /preview`: plan against the current store, validate and store.
    pub async fn preview(&self, input: &PreviewInput<'_>) -> Result<String, ApiProblem> {
        let instruction = input.instruction.trim();
        let length = instruction.chars().count();
        if !(1..=2000).contains(&length) || !azahar_productrules::plain_text(instruction) {
            return Err(problem_fields(
                422,
                "validation_error",
                "Check the marked fields.",
                HashMap::from([(
                    "instruction".to_string(),
                    "Describe the change in 1 to 2000 plain characters.".to_string(),
                )]),
            ));
        }

        let config = config_for(input.settings)?;

        // The baseline is taken before the model sees the store. If a product,
        // order or the theme changes while the plan is being written, apply
        // rejects it.
        let baseline_at = input.now;

        let theme = self
            .load_theme()
            .await
            .map_err(|_| internal("The current theme could not be read."))?;
        let products = self
            .store
            .list_products(&ProductFilter {
                include_inactive: true,
                limit: 500,
                ..ProductFilter::default()
            })
            .await
            .map_err(|error| {
                tracing::warn!(error = %error, "codemode products could not be read");
                internal("The product list could not be read.")
            })?;
        let categories = self.store.categories().await.map_err(|error| {
            tracing::warn!(error = %error, "codemode categories could not be read");
            internal("The category list could not be read.")
        })?;
        let orders = self.recent_orders().await.map_err(|error| {
            tracing::warn!(error = %error, "codemode orders could not be read");
            internal("The order list could not be read.")
        })?;

        let plugins = prompt_plugins(self.registry);
        let system = build_prompt(&theme, &products, &categories, &orders, &plugins);

        let client = Client::new(
            input.ai_base_url,
            &config.api_key,
            &config.model,
            Some(input.ai_timeout),
        )
        .map_err(|_| {
            problem(
                409,
                "plugin_not_configured",
                "Open the Code mode settings and add the API key and model.",
            )
        })?;

        let content = client.plan(&system, instruction).await.map_err(|error| {
            log_upstream_failure(&error);
            problem(
                502,
                "ai_upstream_error",
                "The AI provider did not return a usable plan. Try again shortly.",
            )
        })?;

        let mut plan = parse_plan(&content).map_err(|_| {
            problem(
                422,
                "invalid_plan",
                "The AI provider returned a plan this server cannot apply. Try rephrasing the instruction.",
            )
        })?;

        validate_plan_shape(&mut plan, &theme, self.registry)
            .map_err(|error| problem(422, "invalid_plan", &error.0))?;
        validate_plan_references(self.store, &plan)
            .await
            .map_err(|error| match error {
                ReferenceError::Invalid(invalid) => problem(422, "invalid_plan", &invalid.0),
                ReferenceError::Store(error) => {
                    tracing::warn!(error = %error, "codemode plan references could not be checked");
                    internal("The plan could not be checked.")
                }
            })?;

        let encoded = crate::plan::encode_plan(&plan);
        let baseline =
            plan_baseline(&plan, &products, &orders, &theme).map_err(|error| match error {
                BaselineError::Invalid(invalid) => problem(422, "invalid_plan", &invalid.0),
                BaselineError::Encode(error) => {
                    tracing::warn!(error = %error, "codemode plan baseline could not be encoded");
                    internal("The plan could not be saved.")
                }
            })?;

        let record = PlanRecord {
            id: new_plan_id(),
            actor_user_id: input.actor_user_id.to_string(),
            instruction: instruction.to_string(),
            plan: encoded,
            status: "preview".to_string(),
            baseline,
            created_at: format_time(&baseline_at),
            ..PlanRecord::default()
        };
        self.store
            .conn()
            .execute(
                "INSERT INTO codemode_plans (id, actor_user_id, instruction, plan, status, result, baseline, created_at)
				 VALUES (?, ?, ?, ?, ?, '', ?, ?)",
                params![
                    record.id.clone(),
                    record.actor_user_id.clone(),
                    record.instruction.clone(),
                    record.plan.clone(),
                    record.status.clone(),
                    record.baseline.clone(),
                    record.created_at.clone()
                ],
            )
            .await
            .map_err(|error| {
                tracing::warn!(error = %error, "codemode plan could not be saved");
                internal("The plan could not be saved.")
            })?;

        Ok(format!(
            "{{\"plan\":{}}}",
            plan_dto(&record.id, &plan, &record.status, &record.created_at)
        ))
    }

    /// `POST /apply`: revalidate a stored preview, claim it and apply it once.
    pub async fn apply(&self, input: &ApplyInput<'_>) -> Result<String, ApiProblem> {
        let plan_id = input.plan_id.trim();
        if plan_id.is_empty() || plan_id.len() > 64 {
            return Err(problem_fields(
                422,
                "validation_error",
                "Check the marked fields.",
                HashMap::from([(
                    "plan_id".to_string(),
                    "Choose a saved preview plan.".to_string(),
                )]),
            ));
        }

        let record = match self.load_plan_record(plan_id, input.actor_user_id).await {
            Ok(record) => record,
            Err(StoreError::NotFound) => {
                return Err(problem(404, "not_found", "That plan no longer exists."));
            }
            Err(error) => {
                tracing::warn!(error = %error, "codemode plan could not be read");
                return Err(internal("The plan could not be read."));
            }
        };
        if record.status != "preview" {
            return Err(problem(
                409,
                "plan_already_applied",
                "This plan has already been applied.",
            ));
        }
        ensure_enabled(input.settings)?;

        let mut plan = match parse_plan(&record.plan) {
            Ok(plan) => plan,
            Err(_) => {
                self.fail_plan(plan_id, input.actor_user_id, &[]).await;
                return Err(problem(
                    422,
                    "invalid_plan",
                    "The saved plan is no longer valid. Preview the change again.",
                ));
            }
        };

        let theme = self
            .load_theme()
            .await
            .map_err(|_| internal("The current theme could not be read."))?;
        if let Err(invalid) = validate_plan_shape(&mut plan, &theme, self.registry) {
            self.fail_plan(plan_id, input.actor_user_id, &[]).await;
            return Err(problem(422, "invalid_plan", &invalid.0));
        }
        if let Err(error) = validate_plan_references(self.store, &plan).await {
            match error {
                ReferenceError::Invalid(invalid) => {
                    self.fail_plan(plan_id, input.actor_user_id, &[]).await;
                    return Err(problem(422, "invalid_plan", &invalid.0));
                }
                ReferenceError::Store(error) => {
                    tracing::warn!(error = %error, "codemode plan references could not be checked");
                    return Err(internal("The plan could not be checked."));
                }
            }
        }

        let baseline = match decode_baseline(&record.baseline) {
            Some(baseline) => baseline,
            None => {
                self.fail_plan(plan_id, input.actor_user_id, &[]).await;
                return Err(problem(
                    409,
                    "plan_stale",
                    "This preview has no readable baseline. Preview the change again.",
                ));
            }
        };

        let mut results: Vec<ActionResult> = Vec::with_capacity(plan.actions.len());
        let outcome = self
            .apply_in_transaction(
                plan_id,
                input.actor_user_id,
                &plan,
                &theme,
                baseline,
                input.now,
                &mut results,
            )
            .await;

        match outcome {
            Ok(()) => {}
            Err(ApplyFailure::AlreadyApplied) => {
                return Err(problem(
                    409,
                    "plan_already_applied",
                    "This plan has already been applied.",
                ));
            }
            Err(ApplyFailure::Stale) => {
                self.fail_plan(plan_id, input.actor_user_id, &results).await;
                return Err(problem(
                    409,
                    "plan_stale",
                    "The store changed after this preview. Preview the change again.",
                ));
            }
            Err(ApplyFailure::Action | ApplyFailure::Other) => {
                for result in &mut results {
                    if result.ok {
                        result.ok = false;
                        result.message = format!(
                            "Rolled back because a later action failed. {}",
                            result.message
                        );
                    }
                }
                self.fail_plan(plan_id, input.actor_user_id, &results).await;
                let updated = self
                    .load_plan_record(plan_id, input.actor_user_id)
                    .await
                    .map_err(|error| {
                        tracing::warn!(error = %error, "codemode failed plan could not be read");
                        internal("The plan failed and its result could not be saved.")
                    })?;
                return Ok(apply_body(&updated, &plan, &results));
            }
        }

        let updated = self
            .load_plan_record(plan_id, input.actor_user_id)
            .await
            .map_err(|error| {
                tracing::warn!(error = %error, "codemode applied plan could not be read");
                internal("The plan was applied but could not be read back.")
            })?;
        Ok(apply_body(&updated, &plan, &results))
    }

    #[allow(clippy::too_many_arguments)]
    async fn apply_in_transaction(
        &self,
        plan_id: &str,
        actor_user_id: &str,
        plan: &Plan,
        theme: &Theme,
        baseline: HashMap<String, String>,
        now: DateTime<Utc>,
        results: &mut Vec<ActionResult>,
    ) -> Result<(), ApplyFailure> {
        let tx = self.store.begin_write().await.map_err(|error| {
            tracing::warn!(error = %error, "codemode plan transaction could not start");
            ApplyFailure::Other
        })?;
        let conn = tx.conn();
        let mut baseline = baseline;

        let claimed = claim_plan(conn, plan_id, actor_user_id)
            .await
            .map_err(|error| {
                tracing::warn!(error = %error, "codemode plan could not be claimed");
                ApplyFailure::Other
            })?;
        if !claimed {
            let _ = tx.rollback().await;
            return Err(ApplyFailure::AlreadyApplied);
        }

        // Theme changes are checked once inside the write transaction and every
        // theme action in the plan is applied to that same current theme.
        let mut current_theme = theme.clone();
        if plan
            .actions
            .iter()
            .any(|action| action.action_type == "theme.update")
        {
            let current = self
                .load_theme_tx(conn)
                .await
                .map_err(|_| ApplyFailure::Other)?;
            let encoded = azahar_storefront::encode(&current).map_err(|error| {
                tracing::warn!(error = %error, "codemode theme could not be encoded");
                ApplyFailure::Other
            })?;
            if baseline.get("theme") != Some(&encoded) {
                let _ = tx.rollback().await;
                return Err(ApplyFailure::Stale);
            }
            current_theme = current;
        }

        for action in &plan.actions {
            match apply_action(
                self.store,
                conn,
                actor_user_id,
                plan_id,
                action,
                current_theme.clone(),
                &mut baseline,
                now,
                self.registry,
                self.hooks,
            )
            .await
            {
                Ok((message, next_theme)) => {
                    current_theme = next_theme;
                    results.push(ActionResult {
                        action_type: action.action_type.clone(),
                        ok: true,
                        message,
                    });
                }
                Err(error) => {
                    results.push(ActionResult {
                        action_type: action.action_type.clone(),
                        ok: false,
                        message: error.to_string(),
                    });
                    let _ = tx.rollback().await;
                    return Err(match error {
                        ApplyError::Stale => ApplyFailure::Stale,
                        ApplyError::Message(_) => ApplyFailure::Action,
                    });
                }
            }
        }

        let encoded_results = encode_results(results);
        if let Err(error) = conn
            .execute(
                "UPDATE codemode_plans SET status = ?, result = ? WHERE id = ?",
                params!["applied", encoded_results, plan_id],
            )
            .await
        {
            tracing::warn!(error = %error, "codemode plan could not be marked applied");
            let _ = tx.rollback().await;
            return Err(ApplyFailure::Other);
        }
        tx.commit().await.map_err(|error| {
            tracing::warn!(error = %error, "codemode plan transaction could not commit");
            ApplyFailure::Other
        })
    }

    /// Reads the theme, or the default when the storefront never saved one.
    pub async fn load_theme(&self) -> Result<Theme, ThemeLoadError> {
        match self.store.storefront_settings().await {
            Ok((raw, _)) => parse_theme(&raw),
            Err(StoreError::NotFound) => Ok(Theme::default()),
            Err(error) => {
                tracing::warn!(error = %error, "codemode theme could not be read");
                Err(ThemeLoadError)
            }
        }
    }

    /// Reads the theme inside a write transaction.
    pub async fn load_theme_tx(&self, conn: &libsql::Connection) -> Result<Theme, ThemeLoadError> {
        match self.store.storefront_settings_tx(conn).await {
            Ok((raw, _)) => parse_theme(&raw),
            Err(StoreError::NotFound) => Ok(Theme::default()),
            Err(error) => {
                tracing::warn!(error = %error, "codemode theme could not be read");
                Err(ThemeLoadError)
            }
        }
    }

    /// The twenty newest orders, without customer details.
    pub async fn recent_orders(&self) -> Result<Vec<PromptOrder>, StoreError> {
        let mut rows = self
            .store
            .conn()
            .query(
                "SELECT id, number, status, updated_at FROM orders ORDER BY created_at DESC LIMIT 20",
                (),
            )
            .await?;
        let mut orders = Vec::with_capacity(20);
        while let Some(row) = rows.next().await? {
            orders.push(PromptOrder {
                id: row.get(0)?,
                number: row.get(1)?,
                status: row.get(2)?,
                updated_at: row.get(3)?,
            });
        }
        Ok(orders)
    }

    pub async fn load_plan_record(
        &self,
        plan_id: &str,
        actor_user_id: &str,
    ) -> Result<PlanRecord, StoreError> {
        let mut rows = self
            .store
            .conn()
            .query(
                "SELECT id, actor_user_id, instruction, plan, status, result, baseline, created_at
				 FROM codemode_plans WHERE id = ? AND actor_user_id = ?",
                params![plan_id, actor_user_id],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(StoreError::NotFound);
        };
        Ok(PlanRecord {
            id: row.get(0)?,
            actor_user_id: row.get(1)?,
            instruction: row.get(2)?,
            plan: row.get(3)?,
            status: row.get(4)?,
            result: row.get(5)?,
            baseline: row.get(6)?,
            created_at: row.get(7)?,
        })
    }

    /// Marks a plan failed when it is still previewing or applying. A failure
    /// to write the marker is only logged, like the Go helper.
    pub async fn fail_plan(&self, plan_id: &str, actor_user_id: &str, results: &[ActionResult]) {
        let encoded = encode_results(results);
        if let Err(error) = self
            .store
            .conn()
            .execute(
                "UPDATE codemode_plans SET status = 'failed', result = ?
				 WHERE id = ? AND actor_user_id = ? AND status IN ('preview', 'applying')",
                params![encoded, plan_id, actor_user_id],
            )
            .await
        {
            tracing::warn!(error = %error, "codemode failed plan could not be saved");
        }
    }
}

fn internal(message: &str) -> ApiProblem {
    problem(500, "internal_error", message)
}

/// The theme row exists but cannot be parsed.
#[derive(Debug, thiserror::Error)]
#[error("the current theme could not be read")]
pub struct ThemeLoadError;

fn parse_theme(raw: &str) -> Result<Theme, ThemeLoadError> {
    let mut theme = azahar_storefront::parse(raw).map_err(|error| {
        tracing::warn!(error = %error, "codemode theme could not be parsed");
        ThemeLoadError
    })?;
    theme.updated_at = String::new();
    Ok(theme)
}

fn config_for(
    settings: Option<&HashMap<String, String>>,
) -> Result<crate::client::SettingsConfig, ApiProblem> {
    let Some(settings) = settings else {
        return Err(problem(
            409,
            "plugin_settings_unreadable",
            UNREADABLE_SETTINGS,
        ));
    };
    config_from_settings(settings).map_err(|error| match error {
        ConfigError::Disabled => problem(
            409,
            "plugin_disabled",
            "Enable the plugin in its settings before planning a change.",
        ),
        ConfigError::NotConfigured => problem(
            409,
            "plugin_not_configured",
            "Open the Code mode settings and add the API key and model.",
        ),
    })
}

fn ensure_enabled(settings: Option<&HashMap<String, String>>) -> Result<(), ApiProblem> {
    let Some(settings) = settings else {
        return Err(problem(
            409,
            "plugin_settings_unreadable",
            UNREADABLE_SETTINGS,
        ));
    };
    if enabled_from_settings(settings) {
        Ok(())
    } else {
        Err(problem(
            409,
            "plugin_disabled",
            "Enable the plugin in its settings before applying a plan.",
        ))
    }
}

/// `json.Unmarshal` into `map[string]string`; `null` reads as an empty map and
/// any other shape is unreadable.
fn decode_baseline(raw: &str) -> Option<HashMap<String, String>> {
    if raw.trim().is_empty() {
        return None;
    }
    match serde_json::from_str::<Option<HashMap<String, String>>>(raw) {
        Ok(values) => Some(values.unwrap_or_default()),
        Err(_) => None,
    }
}

async fn claim_plan(
    conn: &libsql::Connection,
    plan_id: &str,
    actor_user_id: &str,
) -> Result<bool, StoreError> {
    let affected = conn
        .execute(
            "UPDATE codemode_plans SET status = 'applying'
			 WHERE id = ? AND actor_user_id = ? AND status = 'preview'",
            params![plan_id, actor_user_id],
        )
        .await
        .map_err(|error| StoreError::Database(error.to_string()))?;
    Ok(affected == 1)
}

fn apply_body(record: &PlanRecord, plan: &Plan, results: &[ActionResult]) -> String {
    let mut object = json::Object::new();
    object.raw_field(
        "plan",
        &plan_dto(&record.id, plan, &record.status, &record.created_at),
    );
    object.raw_field("results", &encode_result_list(results));
    object.finish()
}

fn log_upstream_failure(error: &UpstreamError) {
    tracing::warn!(
        plugin = crate::PLUGIN_ID,
        status_class = %error.status_class(),
        detail = error.detail(),
        "codemode upstream request failed"
    );
}

#[derive(Debug)]
enum ApplyFailure {
    AlreadyApplied,
    Stale,
    Action,
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_null_is_readable_but_empty() {
        assert_eq!(decode_baseline("null"), Some(HashMap::new()));
        assert_eq!(
            decode_baseline("{\"theme\":\"x\"}"),
            Some(HashMap::from([("theme".to_string(), "x".to_string())]))
        );
        assert_eq!(decode_baseline(" "), None);
        assert_eq!(decode_baseline("[]"), None);
        assert_eq!(decode_baseline("{\"theme\":1}"), None);
    }

    #[test]
    fn config_and_enable_map_to_the_go_envelopes() {
        let unknown = config_for(None).expect_err("unreadable");
        assert_eq!(unknown.status.as_u16(), 409);
        assert_eq!(unknown.code, "plugin_settings_unreadable");

        let disabled = config_for(Some(&HashMap::from([(
            "enabled".to_string(),
            "false".to_string(),
        )])))
        .expect_err("disabled");
        assert_eq!(disabled.code, "plugin_disabled");
        assert_eq!(
            disabled.message,
            "Enable the plugin in its settings before planning a change."
        );

        let unconfigured = config_for(Some(&HashMap::new())).expect_err("unconfigured");
        assert_eq!(unconfigured.code, "plugin_not_configured");

        let apply_disabled = ensure_enabled(Some(&HashMap::from([(
            "enabled".to_string(),
            "false".to_string(),
        )])))
        .expect_err("disabled");
        assert_eq!(
            apply_disabled.message,
            "Enable the plugin in its settings before applying a plan."
        );
        assert!(ensure_enabled(Some(&HashMap::new())).is_ok());
    }
}
