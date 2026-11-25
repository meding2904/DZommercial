#![forbid(unsafe_code)]

//! Port of `backend/internal/plugins/codemode`: an admin AI console that plans
//! storefront, catalogue and order changes from a plain instruction and applies
//! only the strict actions the plan validator accepts.
//!
//! The provider never receives tools, code execution or database access. A
//! preview is stored with a fingerprint baseline; apply revalidates, claims the
//! plan atomically and applies every action in one write transaction. Each
//! action writes an audit row carrying the plan id.
//!
//! Route mounting lives in `azahar-httpapi`; [`codemode_routes`] returns the
//! three admin routes for the orchestrator to merge.

mod apply;
mod client;
mod json;
mod plan;
mod prompt;
mod routes;
mod service;

use std::collections::HashMap;

use azahar_plugins::{
    Manifest, Migration, Plugin as PluginTrait, Route, SettingField, SettingsValidator,
};

pub use apply::{ApplyError, apply_action, insert_action_audit};
pub use client::{
    Client, ConfigError, DEFAULT_MODEL, MAX_PLAN_RUNES, MAX_RESPONSE_BYTES, SettingsConfig,
    UpstreamError, config_from_settings, enabled_from_settings,
};
pub use plan::{
    Action, ActionResult, BaselineError, InvalidPlanError, MAX_ACTIONS, Plan, PlanHistoryDto,
    PlanRecord, ProductDraft, ProductPatch, ReferenceError, apply_theme_patch, encode_action,
    encode_plan, encode_product_draft, encode_result_list, encode_results, first_field_message,
    is_null, new_plan_id, normalize_attributes, parse_action, parse_plan, parse_product_patch,
    plan_baseline, plan_dto, product_fingerprint, raw_object, theme_object,
    validate_plan_references, validate_plan_shape, validate_product_attributes,
    validate_product_draft,
};
pub use prompt::{
    PromptCategory, PromptField, PromptOrder, PromptPlugin, PromptProduct, build_prompt,
    prompt_plugins,
};
pub use routes::{APPLY_PATH, PLANS_PATH, PREVIEW_PATH, codemode_routes};
pub use service::{ApplyInput, Codemode, PreviewInput, ThemeLoadError};

/// The plugin id under `/api/plugins/`.
pub const PLUGIN_ID: &str = "codemode";

/// The plugin object registered with the framework.
///
/// The Go `New` optionally carries the registry because its handlers hang off
/// the plugin value. In this port the handlers read the live registry from
/// `AppState`, so the registered object is a marker with no state.
#[derive(Debug, Default, Clone, Copy)]
pub struct Plugin;

/// Constructs the plugin.
#[must_use]
pub fn new() -> Plugin {
    Plugin
}

impl PluginTrait for Plugin {
    fn manifest(&self) -> Manifest {
        manifest()
    }

    fn routes(&self) -> Vec<Route> {
        vec![
            Route {
                method: "GET".to_string(),
                pattern: "/plans".to_string(),
                admin: true,
            },
            Route {
                method: "POST".to_string(),
                pattern: "/preview".to_string(),
                admin: true,
            },
            Route {
                method: "POST".to_string(),
                pattern: "/apply".to_string(),
                admin: true,
            },
        ]
    }

    fn migrations(&self) -> Vec<Migration> {
        migrations()
    }

    fn as_settings_validator(&self) -> Option<&dyn SettingsValidator> {
        Some(self)
    }
}

impl SettingsValidator for Plugin {
    fn validate_settings(&self, settings: &HashMap<String, String>) -> HashMap<String, String> {
        validate_settings(settings)
    }
}

/// The exact Go manifest.
#[must_use]
pub fn manifest() -> Manifest {
    Manifest {
        id: PLUGIN_ID.to_string(),
        name: "Code mode".to_string(),
        version: "1.0.0".to_string(),
        author: "Azahar".to_string(),
        description:
            "Plans and applies storefront, catalogue and order changes from an admin instruction."
                .to_string(),
        admin_path: "/admin/plugins/codemode".to_string(),
        settings: vec![
            SettingField {
                key: "api_key".to_string(),
                label: "API key".to_string(),
                field_type: "secret".to_string(),
                help: "Stored encrypted on the server. It is never sent back to the browser."
                    .to_string(),
                required: true,
                max_length: 512,
                ..SettingField::default()
            },
            SettingField {
                key: "model".to_string(),
                label: "Model".to_string(),
                field_type: "text".to_string(),
                help: "The model name accepted by your OpenAI-compatible endpoint.".to_string(),
                required: true,
                max_length: 64,
                default: DEFAULT_MODEL.to_string(),
                ..SettingField::default()
            },
            SettingField {
                key: "enabled".to_string(),
                label: "Enable planning".to_string(),
                field_type: "boolean".to_string(),
                required: true,
                default: "true".to_string(),
                ..SettingField::default()
            },
        ],
        ..Manifest::default()
    }
}

/// Rejects a blank key, a malformed model and anything but true/false.
#[must_use]
pub fn validate_settings(settings: &HashMap<String, String>) -> HashMap<String, String> {
    let mut fields = HashMap::new();

    if settings
        .get("api_key")
        .map_or("", String::as_str)
        .trim()
        .is_empty()
    {
        fields.insert(
            "api_key".to_string(),
            "Add the API key for the AI provider.".to_string(),
        );
    }
    let model = settings.get("model").map_or("", String::as_str).trim();
    if !crate::client::model_is_valid(model) {
        fields.insert(
            "model".to_string(),
            "Use 1 to 64 letters, numbers, dots, dashes, colons or slashes.".to_string(),
        );
    }
    match settings.get("enabled").map_or("", String::as_str).trim() {
        "true" | "false" => {}
        _ => {
            fields.insert("enabled".to_string(), "Choose true or false.".to_string());
        }
    }
    fields
}

/// The two migrations, byte-identical to the Go SQL.
#[must_use]
pub fn migrations() -> Vec<Migration> {
    vec![
        Migration {
            version: "001_plans.sql".to_string(),
            sql: "CREATE TABLE IF NOT EXISTS codemode_plans (
  id TEXT PRIMARY KEY,
  actor_user_id TEXT NOT NULL DEFAULT '',
  instruction TEXT NOT NULL,
  plan TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'preview',
  result TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL
)"
            .to_string(),
        },
        Migration {
            version: "002_plan_baseline.sql".to_string(),
            sql: "ALTER TABLE codemode_plans ADD COLUMN baseline TEXT NOT NULL DEFAULT ''"
                .to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_matches_go() {
        let manifest = manifest();
        assert_eq!(manifest.id, "codemode");
        assert_eq!(manifest.name, "Code mode");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.admin_path, "/admin/plugins/codemode");
        assert_eq!(manifest.settings.len(), 3);
        let api_key = &manifest.settings[0];
        assert_eq!(api_key.field_type, "secret");
        assert!(api_key.required);
        assert_eq!(api_key.max_length, 512);
        assert_eq!(manifest.settings[1].default, "gpt-4o-mini");
        assert_eq!(manifest.settings[2].default, "true");
        assert_eq!(manifest.settings[2].field_type, "boolean");
    }

    #[test]
    fn migrations_are_single_statements() {
        let migrations = migrations();
        assert_eq!(migrations.len(), 2);
        assert_eq!(migrations[0].version, "001_plans.sql");
        assert!(!migrations[0].sql.contains(';'));
        assert!(migrations[0].sql.contains("id TEXT PRIMARY KEY"));
        assert_eq!(migrations[1].version, "002_plan_baseline.sql");
        assert!(!migrations[1].sql.contains(';'));
        assert!(
            migrations[1]
                .sql
                .contains("baseline TEXT NOT NULL DEFAULT ''")
        );
    }

    #[test]
    fn settings_validation_matches_go() {
        let valid = HashMap::from([
            ("api_key".to_string(), "sk-test".to_string()),
            ("model".to_string(), "gpt-4o-mini".to_string()),
            ("enabled".to_string(), "true".to_string()),
        ]);
        assert!(validate_settings(&valid).is_empty());

        let invalid = HashMap::from([
            ("api_key".to_string(), " ".to_string()),
            ("model".to_string(), "gpt 4o".to_string()),
            ("enabled".to_string(), "yes".to_string()),
        ]);
        let fields = validate_settings(&invalid);
        assert_eq!(
            fields.get("api_key").map(String::as_str),
            Some("Add the API key for the AI provider.")
        );
        assert_eq!(
            fields.get("model").map(String::as_str),
            Some("Use 1 to 64 letters, numbers, dots, dashes, colons or slashes.")
        );
        assert_eq!(
            fields.get("enabled").map(String::as_str),
            Some("Choose true or false.")
        );
    }

    #[test]
    fn routes_are_admin_only() {
        let routes = Plugin.routes();
        assert_eq!(routes.len(), 3);
        assert!(routes.iter().all(|route| route.admin));
        assert_eq!(
            routes
                .iter()
                .map(|route| (route.method.as_str(), route.pattern.as_str()))
                .collect::<Vec<_>>(),
            vec![("GET", "/plans"), ("POST", "/preview"), ("POST", "/apply")]
        );
    }

    #[test]
    fn plugin_registers_and_validates_through_the_trait() {
        let mut registry = azahar_plugins::Registry::new();
        registry
            .register(std::sync::Arc::new(Plugin))
            .expect("register");
        let plugin = registry.get(PLUGIN_ID).expect("registered");
        let validator = plugin.as_settings_validator().expect("validator");
        assert!(
            validator
                .validate_settings(&HashMap::new())
                .contains_key("api_key")
        );
    }
}
