//! Port of `backend/internal/plugins/ai`: an admin-only analyst that explains
//! store and product performance from recorded sales facts and an
//! OpenAI-compatible endpoint.
//!
//! The crate carries the plugin recipe (manifest, settings validation,
//! migration and route metadata), the prompt builders, the upstream client and
//! the two axum handlers. [`ai_routes`] returns both admin routes; the handlers
//! apply the Go admin group (`requireUser` -> `requireAdmin` ->
//! `adminMutationLimit`) themselves, so the orchestrator merges the router
//! without extra layers.

#![forbid(unsafe_code)]

mod client;
mod http;
mod prompts;

use std::collections::HashMap;
use std::sync::LazyLock;

use azahar_plugins::{
    Manifest, Migration, Plugin as PluginApi, Route, SettingField, SettingOption, SettingsValidator,
};
use regex::Regex;

pub use client::{
    Client, ConfigError, MAX_ANSWER_RUNES, MAX_RESPONSE_BYTES, SettingsConfig, UpstreamError,
    config_from_settings, enabled_from_settings, new_client, with_http_client,
};
pub use http::{AnalyzeRequest, RunDto, ai_routes, analyze, plugin_refusal, runs};
pub use prompts::{
    ProductSales, SUPPORTED_LANGUAGES, analysis_instruction, build_product_prompt,
    build_store_prompt, format_dzd, is_supported_language, language_name,
};

/// Plugin id used by the registry, settings table and route prefix.
pub const PLUGIN_ID: &str = "ai";
/// Model a fresh installation starts with.
pub const DEFAULT_MODEL: &str = "gpt-4o-mini";
/// Answer language when the stored value is unknown.
pub const DEFAULT_LANGUAGE: &str = "en";
/// Analysis window when the request omits `days`.
pub const DEFAULT_DAYS: i32 = 30;
/// Answer lengths the plugin accepts.
pub const SUPPORTED_DAYS: [i32; 3] = [7, 30, 90];
/// Largest accepted model name.
pub const MODEL_MAX_LENGTH: usize = 64;

static MODEL_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9._:/-]{1,64}$").expect("static ai model pattern"));

/// The plugin recipe, exactly as the Go `Plugin` value.
#[derive(Debug, Clone, Copy, Default)]
pub struct Plugin;

#[must_use]
pub fn new() -> Plugin {
    Plugin
}

/// True when `model` is accepted by the plugin's settings validation.
#[must_use]
pub fn valid_model(model: &str) -> bool {
    MODEL_PATTERN.is_match(model)
}

fn manifest() -> Manifest {
    Manifest {
        id: PLUGIN_ID.to_string(),
        name: "AI analyst".to_string(),
        version: "1.0.0".to_string(),
        author: "Azahar".to_string(),
        description: "Explains store and product performance in plain language using only recorded sales facts.".to_string(),
        admin_path: "/admin/plugins/ai".to_string(),
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
                max_length: MODEL_MAX_LENGTH as i32,
                default: DEFAULT_MODEL.to_string(),
                ..SettingField::default()
            },
            SettingField {
                key: "language".to_string(),
                label: "Answer language".to_string(),
                field_type: "select".to_string(),
                options: vec![
                    SettingOption {
                        value: "en".to_string(),
                        label: "English".to_string(),
                    },
                    SettingOption {
                        value: "fr".to_string(),
                        label: "Français".to_string(),
                    },
                    SettingOption {
                        value: "ar".to_string(),
                        label: "العربية".to_string(),
                    },
                ],
                required: true,
                default: DEFAULT_LANGUAGE.to_string(),
                ..SettingField::default()
            },
            SettingField {
                key: "enabled".to_string(),
                label: "Enable analysis".to_string(),
                field_type: "boolean".to_string(),
                required: true,
                default: "true".to_string(),
                ..SettingField::default()
            },
        ],
        ..Manifest::default()
    }
}

fn migrations() -> Vec<Migration> {
    vec![Migration {
        version: "001_runs.sql".to_string(),
        sql: "CREATE TABLE IF NOT EXISTS plugin_ai_runs (
  id TEXT PRIMARY KEY,
  actor_user_id TEXT NOT NULL DEFAULT '',
  kind TEXT NOT NULL,
  subject TEXT NOT NULL DEFAULT '',
  model TEXT NOT NULL,
  analysis TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_plugin_ai_runs_created ON plugin_ai_runs(created_at);"
            .to_string(),
    }]
}

fn routes() -> Vec<Route> {
    vec![
        Route {
            method: "POST".to_string(),
            pattern: "/analyze".to_string(),
            admin: true,
        },
        Route {
            method: "GET".to_string(),
            pattern: "/runs".to_string(),
            admin: true,
        },
    ]
}

/// The Go `ValidateSettings`: only the model and language have semantic
/// checks; the generic form merge covers everything else.
fn validate_settings(settings: &HashMap<String, String>) -> HashMap<String, String> {
    let mut fields = HashMap::new();

    let model = settings.get("model").map_or("", String::as_str).trim();
    if !valid_model(model) {
        fields.insert(
            "model".to_string(),
            "Use 1 to 64 letters, numbers, dots, dashes, colons or slashes.".to_string(),
        );
    }

    let language = settings.get("language").map_or("", String::as_str).trim();
    if !is_supported_language(language) {
        fields.insert(
            "language".to_string(),
            "Choose English, Français or العربية.".to_string(),
        );
    }

    fields
}

impl PluginApi for Plugin {
    fn manifest(&self) -> Manifest {
        manifest()
    }

    fn routes(&self) -> Vec<Route> {
        routes()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_matches_the_go_recipe() {
        let manifest = manifest();
        assert_eq!(manifest.id, "ai");
        assert_eq!(manifest.name, "AI analyst");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.author, "Azahar");
        assert_eq!(manifest.admin_path, "/admin/plugins/ai");
        assert_eq!(
            manifest
                .settings
                .iter()
                .map(|field| field.key.as_str())
                .collect::<Vec<_>>(),
            ["api_key", "model", "language", "enabled"]
        );

        let model = &manifest.settings[1];
        assert_eq!(model.field_type, "text");
        assert_eq!(model.default, DEFAULT_MODEL);
        assert_eq!(model.max_length, 64);
        assert!(model.required);

        let language = &manifest.settings[2];
        assert_eq!(language.field_type, "select");
        assert_eq!(language.default, "en");
        assert_eq!(
            language
                .options
                .iter()
                .map(|option| (option.value.as_str(), option.label.as_str()))
                .collect::<Vec<_>>(),
            [("en", "English"), ("fr", "Français"), ("ar", "العربية")]
        );

        let enabled = &manifest.settings[3];
        assert_eq!(enabled.field_type, "boolean");
        assert_eq!(enabled.default, "true");
    }

    #[test]
    fn migrations_carry_the_exact_sql() {
        let migrations = migrations();
        assert_eq!(migrations.len(), 1);
        assert_eq!(migrations[0].version, "001_runs.sql");
        assert!(
            migrations[0]
                .sql
                .starts_with("CREATE TABLE IF NOT EXISTS plugin_ai_runs (")
        );
        assert!(
            migrations[0]
                .sql
                .contains("actor_user_id TEXT NOT NULL DEFAULT ''")
        );
        assert!(migrations[0].sql.contains(
            "CREATE INDEX IF NOT EXISTS idx_plugin_ai_runs_created ON plugin_ai_runs(created_at);"
        ));
    }

    #[test]
    fn routes_are_admin_only() {
        let routes = routes();
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].method, "POST");
        assert_eq!(routes[0].pattern, "/analyze");
        assert!(routes[0].admin);
        assert_eq!(routes[1].method, "GET");
        assert_eq!(routes[1].pattern, "/runs");
        assert!(routes[1].admin);
    }

    #[test]
    fn validation_matches_the_go_acceptance() {
        for model in [
            "gpt-4o-mini",
            "gpt-4o",
            "llama3.1:8b",
            "Qwen/Qwen2.5-7B-Instruct",
            "ft:gpt-4o-mini:azahar:analyst:id",
        ] {
            let fields = validate_settings(&HashMap::from([
                ("model".to_string(), model.to_string()),
                ("language".to_string(), "en".to_string()),
            ]));
            assert!(!fields.contains_key("model"), "model {model} rejected");
        }
        let overlong = "a".repeat(65);
        for model in [
            "",
            "gpt 4o",
            "gpt;4o",
            "gpt\t4o",
            "modèle",
            overlong.as_str(),
        ] {
            let fields = validate_settings(&HashMap::from([
                ("model".to_string(), model.to_string()),
                ("language".to_string(), "en".to_string()),
            ]));
            assert!(fields.contains_key("model"), "model {model} accepted");
        }

        for language in ["en", "fr", "ar"] {
            let fields = validate_settings(&HashMap::from([
                ("model".to_string(), "gpt-4o-mini".to_string()),
                ("language".to_string(), language.to_string()),
            ]));
            assert!(!fields.contains_key("language"), "language {language}");
        }
        for language in ["", "de", "EN", "fr-FR"] {
            let fields = validate_settings(&HashMap::from([
                ("model".to_string(), "gpt-4o-mini".to_string()),
                ("language".to_string(), language.to_string()),
            ]));
            assert!(fields.contains_key("language"), "language {language}");
        }
    }

    #[test]
    fn the_validator_trait_is_exposed() {
        let plugin = new();
        let validator = plugin
            .as_settings_validator()
            .expect("ai validates its settings");
        assert!(
            validator
                .validate_settings(&HashMap::new())
                .contains_key("model")
        );
    }

    #[test]
    fn registers_with_the_registry() {
        let mut registry = azahar_plugins::Registry::new();
        registry
            .register(std::sync::Arc::new(new()))
            .expect("register ai");
    }
}
