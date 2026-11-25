//! The two admin HTTP handlers and their route builder.
//!
//! The builder returns the routes with no middleware, so the orchestrator can
//! merge it into `Router<AppState>` directly. The handlers apply the Go admin
//! group themselves, in the same order as `mountPluginRoutes`:
//! `requireUser` -> `requireAdmin` -> `adminMutationLimit` (unsafe methods
//! only) -> the plugin-enabled gate -> the plugin handler. This is the same
//! pattern as `azahar_plugin_codemode::codemode_routes`; unlike a
//! `from_fn_with_state` layer chain it needs no state value at build time.

use std::collections::HashMap;
use std::error::Error as _;
use std::sync::PoisonError;
use std::time::Instant;

use axum::Router;
use axum::body::to_bytes;
use axum::extract::{RawQuery, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use azahar_httpapi::extract::BODY_LIMIT;
use azahar_httpapi::handlers_checkout::ClientIp;
use azahar_httpapi::middleware::{forbidden, internal_error, unauthorized};
use azahar_httpapi::respond::{ApiProblem, json_response, problem, problem_fields};
use azahar_httpapi::session_layer::SessionUser;
use azahar_httpapi::state::AppState;
use azahar_plugins::{Env, Problem as PluginProblem};
use azahar_store::{StoreError, User, format_time};
use chrono::{Duration as ChronoDuration, NaiveTime, Utc};
use http_body_util::LengthLimitError;
use libsql::params;
use serde::{Deserialize, Serialize};

use crate::client::{ConfigError, config_from_settings, new_client};
use crate::prompts::{ProductSales, build_product_prompt, build_store_prompt};
use crate::{DEFAULT_DAYS, PLUGIN_ID};

/// Largest page of saved runs the endpoint returns.
pub const RUNS_DEFAULT_LIMIT: i32 = 20;
/// Largest accepted `limit` value.
pub const RUNS_MAX_LIMIT: i32 = 50;

/// Route prefix the Go registry mounts for this plugin.
pub const ROUTE_PREFIX: &str = "/api/plugins/ai";

/// `POST /analyze` body.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AnalyzeRequest {
    pub kind: String,
    pub product_id: String,
    pub days: i64,
}

/// One saved analysis row, in the Go `runDTO` field order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunDto {
    pub id: String,
    pub kind: String,
    pub subject: String,
    pub model: String,
    pub analysis: String,
    pub created_at: String,
}

#[derive(Serialize)]
struct AnalyzeResponse {
    run: RunDto,
}

#[derive(Serialize)]
struct RunsResponse {
    runs: Vec<RunDto>,
}

/// Renders a plugin refusal with the shared API envelope.
#[must_use]
pub fn plugin_refusal(problem: PluginProblem) -> ApiProblem {
    ApiProblem {
        status: StatusCode::from_u16(problem.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        code: problem.code,
        message: problem.message,
        fields: problem.fields,
    }
}

fn shape_error() -> ApiProblem {
    problem(
        422,
        "invalid_json",
        "The request body does not match the expected shape.",
    )
}

fn preparation_failed() -> ApiProblem {
    problem(500, "internal_error", "The analysis could not be prepared.")
}

/// Port of `plugins.DecodeBody[T]`: content-type prefix check, the 1 MiB
/// ceiling, a zero value for a `null` body, unknown fields folded into the
/// shape message, and a single-object rule.
async fn decode_plugin_body(request: Request) -> Result<AnalyzeRequest, ApiProblem> {
    let content_type = request
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !content_type.to_lowercase().starts_with("application/json") {
        return Err(problem(
            415,
            "unsupported_media_type",
            "Send the request body as application/json.",
        ));
    }

    let bytes = match to_bytes(request.into_body(), BODY_LIMIT.saturating_add(1)).await {
        Ok(bytes) => bytes,
        Err(error) => {
            if error
                .source()
                .is_some_and(|source| source.is::<LengthLimitError>())
            {
                return Err(problem(
                    413,
                    "payload_too_large",
                    "The request body is too large.",
                ));
            }
            return Err(problem(
                400,
                "invalid_json",
                "The request body could not be read.",
            ));
        }
    };
    if bytes.len() > BODY_LIMIT {
        return Err(problem(
            413,
            "payload_too_large",
            "The request body is too large.",
        ));
    }
    if String::from_utf8_lossy(&bytes).trim().is_empty() {
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            HashMap::from([("body".to_string(), "Send a JSON object.".to_string())]),
        ));
    }

    let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
    let value = match serde_json::Value::deserialize(&mut deserializer) {
        Ok(value) => value,
        Err(_) => return Err(shape_error()),
    };
    if deserializer.end().is_err() {
        return Err(problem(422, "invalid_json", "Send one JSON object."));
    }
    if value.is_null() {
        return Ok(AnalyzeRequest::default());
    }
    let Some(object) = value.as_object() else {
        return Err(shape_error());
    };
    let mut payload = AnalyzeRequest::default();
    for (key, item) in object {
        match key.to_ascii_lowercase().as_str() {
            "kind" => payload.kind = string_field(item)?,
            "product_id" => payload.product_id = string_field(item)?,
            "days" => {
                payload.days = match item {
                    serde_json::Value::Null => 0,
                    serde_json::Value::Number(number) => number.as_i64().ok_or_else(shape_error)?,
                    _ => return Err(shape_error()),
                }
            }
            _ => return Err(shape_error()),
        }
    }
    Ok(payload)
}

fn string_field(value: &serde_json::Value) -> Result<String, ApiProblem> {
    match value {
        serde_json::Value::Null => Ok(String::new()),
        serde_json::Value::String(value) => Ok(value.clone()),
        _ => Err(shape_error()),
    }
}

/// The Go `pluginHandler` gate: an unregistered or effectively disabled
/// plugin answers 404 before the handler runs.
fn ensure_plugin(state: &AppState) -> Result<(), ApiProblem> {
    let registry = state.plugins.read().unwrap_or_else(PoisonError::into_inner);
    if registry.get(PLUGIN_ID).is_none() {
        return Err(problem(404, "not_found", "Not found."));
    }
    if registry.effective_enabled(PLUGIN_ID) {
        Ok(())
    } else {
        Err(problem(404, "not_found", "Plugin not found."))
    }
}

/// The Go `requireUser` + `requireAdmin` chain. The session layer already
/// resolved the signed-in user id; the row is re-read so a deleted account or a
/// role change takes effect immediately, as in the Go guard.
async fn admin_user(state: &AppState, session_user: Option<&str>) -> Result<User, ApiProblem> {
    let Some(user_id) = session_user else {
        return Err(unauthorized());
    };
    match state.store.user_by_id(user_id).await {
        Ok(user) if user.role == "admin" => Ok(user),
        Ok(_) => Err(forbidden()),
        Err(StoreError::NotFound) => Err(unauthorized()),
        Err(_) => Err(internal_error()),
    }
}

/// `adminMutationLimit`: every unsafe admin request shares one bucket. The Go
/// middleware skips GET/HEAD; the runs route never calls this.
fn check_admin_limit(state: &AppState, ip: &str) -> Option<Response> {
    let (allowed, wait) = state.admin_limiter.allow(ip, Instant::now());
    if allowed {
        return None;
    }
    let seconds = wait.as_secs_f64().ceil().max(1.0) as u64;
    let mut response =
        problem(429, "rate_limited", "Too many requests. Try again later.").into_response();
    if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
        response.headers_mut().insert(header::RETRY_AFTER, value);
    }
    Some(response)
}

/// `POST /api/plugins/ai/admin/analyze`.
pub async fn analyze(
    State(state): State<AppState>,
    session_user: SessionUser,
    ClientIp(ip): ClientIp,
    request: Request,
) -> Result<Response, ApiProblem> {
    let user = admin_user(&state, session_user.id()).await?;
    if let Some(response) = check_admin_limit(&state, &ip) {
        return Ok(response);
    }
    ensure_plugin(&state)?;
    let body: AnalyzeRequest = decode_plugin_body(request).await?;

    let kind = body.kind.trim();
    let product_id = body.product_id.trim();
    let days = if body.days == 0 {
        i64::from(DEFAULT_DAYS)
    } else {
        body.days
    };

    let mut fields = HashMap::new();
    if kind != "store" && kind != "product" {
        fields.insert("kind".to_string(), "Choose store or product.".to_string());
    }
    if !matches!(days, 7 | 30 | 90) {
        fields.insert("days".to_string(), "Choose 7, 30 or 90.".to_string());
    }
    if kind == "product" && product_id.is_empty() {
        fields.insert("product_id".to_string(), "Choose a product.".to_string());
    }
    if !fields.is_empty() {
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            fields,
        ));
    }

    // The product lookup runs before the settings read, exactly like Go, so a
    // deleted product answers 404 even when the stored settings are unreadable.
    let product = if kind == "product" {
        match state.store.product_by_id(product_id, false).await {
            Ok(product) => Some(product),
            Err(StoreError::NotFound) => {
                return Err(problem(404, "not_found", "That product no longer exists."));
            }
            Err(_) => {
                tracing::warn!(plugin = PLUGIN_ID, "ai product lookup failed");
                return Err(internal_error());
            }
        }
    } else {
        None
    };

    let environment = Env::new(state.store.clone(), state.cfg.plugin_secret_key.clone());
    let settings = match environment.settings(PLUGIN_ID).await {
        Ok(settings) => settings,
        Err(_) => {
            tracing::warn!(plugin = PLUGIN_ID, "ai settings could not be read");
            return Err(problem(
                409,
                "plugin_settings_unreadable",
                "These settings cannot be read with the current PLUGIN_SECRET_KEY. Enter them again to replace the old values.",
            ));
        }
    };

    let config = match config_from_settings(&settings) {
        Ok(config) => config,
        Err(ConfigError::Disabled) => {
            return Err(problem(
                409,
                "plugin_disabled",
                "Enable the plugin in its settings before asking for an analysis.",
            ));
        }
        Err(ConfigError::NotConfigured) => {
            return Err(problem(
                409,
                "plugin_not_configured",
                "Open the AI plugin settings and add the API key and model.",
            ));
        }
    };

    let (system, user_prompt, subject) = if kind == "store" {
        let now = Utc::now();
        let to = chrono::DateTime::<Utc>::from_naive_utc_and_offset(
            now.date_naive().and_time(NaiveTime::MIN),
            Utc,
        );
        let span = ChronoDuration::days(days - 1);
        let from = to - span;
        let previous_to = from - ChronoDuration::days(1);
        let previous_from = previous_to - span;

        let metrics = match state
            .store
            .metrics(from, to, previous_from, previous_to)
            .await
        {
            Ok(metrics) => metrics,
            Err(_) => {
                tracing::warn!(plugin = PLUGIN_ID, "ai metrics lookup failed");
                return Err(preparation_failed());
            }
        };
        let days = i32::try_from(days).unwrap_or(DEFAULT_DAYS);
        let (system, facts) = build_store_prompt(&config.language, days, &metrics);
        (system, facts, format!("store:{days}d"))
    } else {
        let Some(product) = product.as_ref() else {
            return Err(preparation_failed());
        };
        let sales_days = 30;
        let from = Utc::now() - ChronoDuration::days(i64::from(sales_days));
        let sales = match product_sales(&state, &product.id, &format_time(&from)).await {
            Ok(sales) => sales,
            Err(_) => {
                tracing::warn!(plugin = PLUGIN_ID, "ai product sales lookup failed");
                return Err(preparation_failed());
            }
        };
        let (system, facts) = build_product_prompt(&config.language, product, &sales);
        (system, facts, product.name.clone())
    };

    let client = match new_client(
        &state.cfg.ai_base_url,
        &config.api_key,
        &config.model,
        state.cfg.ai_timeout,
    ) {
        Ok(client) => client,
        Err(_) => {
            return Err(problem(
                409,
                "plugin_not_configured",
                "Open the AI plugin settings and add the API key and model.",
            ));
        }
    };

    let analysis = match client.complete(&system, &user_prompt).await {
        Ok(analysis) => analysis,
        Err(error) => {
            tracing::warn!(
                plugin = PLUGIN_ID,
                status_class = %error.status_class(),
                detail = %error.detail(),
                "ai upstream request failed"
            );
            return Err(problem(
                502,
                "ai_upstream_error",
                "The AI provider did not return a usable answer. Try again shortly.",
            ));
        }
    };

    let run = RunDto {
        id: azahar_ids::new("air"),
        kind: kind.to_string(),
        subject,
        model: config.model,
        analysis,
        created_at: format_time(&Utc::now()),
    };
    if state
        .store
        .conn()
        .execute(
            "INSERT INTO plugin_ai_runs (id, actor_user_id, kind, subject, model, analysis, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                run.id.clone(),
                user.id.clone(),
                run.kind.clone(),
                run.subject.clone(),
                run.model.clone(),
                run.analysis.clone(),
                run.created_at.clone()
            ],
        )
        .await
        .is_err()
    {
        tracing::warn!(plugin = PLUGIN_ID, "ai run could not be saved");
        return Err(problem(
            500,
            "internal_error",
            "The analysis could not be saved.",
        ));
    }

    Ok(json_response(200, &AnalyzeResponse { run }))
}

async fn product_sales(
    state: &AppState,
    product_id: &str,
    from: &str,
) -> Result<ProductSales, StoreError> {
    let mut rows = state
        .store
        .conn()
        .query(
            "SELECT COALESCE(SUM(oi.quantity), 0), COALESCE(SUM(oi.line_total_cents), 0),
                    COUNT(DISTINCT oi.order_id)
             FROM order_items oi
             JOIN orders o ON o.id = oi.order_id
             WHERE oi.product_id = ? AND o.status <> 'cancelled' AND o.created_at >= ?",
            params![product_id, from],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Err(StoreError::Database("product sales: no rows".to_string()));
    };
    Ok(ProductSales {
        days: 30,
        units: row.get(0)?,
        revenue_cents: row.get(1)?,
        orders: row.get(2)?,
    })
}

fn limit_from_query(raw: Option<&str>) -> Result<i32, ApiProblem> {
    let raw = raw.unwrap_or_default().trim();
    if raw.is_empty() {
        return Ok(RUNS_DEFAULT_LIMIT);
    }
    match raw.parse::<i32>() {
        Ok(value) if (1..=RUNS_MAX_LIMIT).contains(&value) => Ok(value),
        _ => Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            HashMap::from([(
                "limit".to_string(),
                "Choose a limit from 1 to 50.".to_string(),
            )]),
        )),
    }
}

/// `GET /api/plugins/ai/admin/runs[?limit=]`.
pub async fn runs(
    State(state): State<AppState>,
    session_user: SessionUser,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiProblem> {
    admin_user(&state, session_user.id()).await?;
    ensure_plugin(&state)?;

    let limit_value = query.as_deref().and_then(|raw| {
        url::form_urlencoded::parse(raw.as_bytes())
            .find(|(key, _)| key == "limit")
            .map(|(_, value)| value.into_owned())
    });
    let limit = limit_from_query(limit_value.as_deref())?;

    let mut rows = state
        .store
        .conn()
        .query(
            "SELECT id, kind, subject, model, analysis, created_at
             FROM plugin_ai_runs ORDER BY created_at DESC, id DESC LIMIT ?",
            params![limit],
        )
        .await
        .map_err(|_| {
            tracing::warn!(plugin = PLUGIN_ID, "ai runs lookup failed");
            internal_error()
        })?;

    let mut runs = Vec::with_capacity(usize::try_from(limit).unwrap_or(0));
    loop {
        let row = rows.next().await.map_err(|_| {
            tracing::warn!(plugin = PLUGIN_ID, "ai run could not be read");
            internal_error()
        })?;
        let Some(row) = row else {
            break;
        };
        let run = RunDto {
            id: row.get(0).map_err(|_| internal_error())?,
            kind: row.get(1).map_err(|_| internal_error())?,
            subject: row.get(2).map_err(|_| internal_error())?,
            model: row.get(3).map_err(|_| internal_error())?,
            analysis: row.get(4).map_err(|_| internal_error())?,
            created_at: row.get(5).map_err(|_| internal_error())?,
        };
        runs.push(run);
    }

    Ok(json_response(200, &RunsResponse { runs }))
}

/// The two admin routes for the orchestrator to merge.
///
/// The handlers perform the Go admin guard chain themselves, so this router
/// needs no extra middleware and can be merged into `Router<AppState>`
/// directly. `from_fn_with_state` would have required an `AppState` value at
/// build time; this builder's signature has none.
pub fn ai_routes() -> Router<AppState> {
    Router::new()
        .route(&format!("{ROUTE_PREFIX}/admin/analyze"), post(analyze))
        .route(&format!("{ROUTE_PREFIX}/admin/runs"), get(runs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_limit_parsing_accepts_only_the_documented_range() {
        assert_eq!(limit_from_query(None).expect("default"), 20);
        assert_eq!(limit_from_query(Some("")).expect("blank"), 20);
        assert_eq!(limit_from_query(Some(" 1 ")).expect("one"), 1);
        assert_eq!(limit_from_query(Some("50")).expect("fifty"), 50);
        for rejected in ["0", "-1", "51", "abc", "2.5", "1e2", "+51"] {
            let error = limit_from_query(Some(rejected)).expect_err(rejected);
            assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(error.code, "validation_error");
            assert_eq!(
                error
                    .fields
                    .as_ref()
                    .and_then(|fields| fields.get("limit"))
                    .map(String::as_str),
                Some("Choose a limit from 1 to 50.")
            );
        }
        assert_eq!(limit_from_query(Some("+20")).expect("plus"), 20);
    }

    #[test]
    fn plugin_refusals_keep_the_shared_envelope() {
        let converted = plugin_refusal(PluginProblem::new(
            409,
            "plugin_disabled",
            "Enable the plugin in its settings before asking for an analysis.",
            None,
        ));
        assert_eq!(converted.status, StatusCode::CONFLICT);
        assert_eq!(converted.code, "plugin_disabled");
        assert!(converted.fields.is_none());
    }
}

#[cfg(test)]
mod decode_tests {
    use super::*;
    use axum::body::Body;

    fn request(body: &str) -> Request {
        Request::builder()
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("request")
    }

    #[tokio::test]
    async fn accepts_go_case_insensitive_keys_and_nulls() {
        let payload =
            decode_plugin_body(request(r#"{"Kind":"store","Product_ID":null,"Days":null}"#))
                .await
                .expect("go accepts mixed case and nulls");
        assert_eq!(payload.kind, "store");
        assert_eq!(payload.product_id, "");
        assert_eq!(payload.days, 0);
    }

    #[tokio::test]
    async fn accepts_64_bit_days_for_validation() {
        let payload = decode_plugin_body(request(r#"{"kind":"store","days":4294967326}"#))
            .await
            .expect("go decodes into int");
        assert_eq!(payload.days, 4_294_967_326);
    }

    #[tokio::test]
    async fn rejects_unknown_and_wrong_shapes() {
        assert!(
            decode_plugin_body(request(r#"{"kind":"store","other":1}"#))
                .await
                .is_err()
        );
        assert!(decode_plugin_body(request(r#"{"kind":1}"#)).await.is_err());
    }
}
