//! The OpenAI-compatible planner client.
//!
//! Port of `backend/internal/plugins/codemode/client.go`. The provider only
//! ever returns text: the model has no tools, no code execution and no access
//! to the database. Every failure maps to an [`UpstreamError`] that carries no
//! response body, so the handler can answer a plain 502 without leaking
//! provider output.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use serde::Deserialize;

use crate::json;

/// Cap on the provider response body, matching the Go constant `2 << 20`.
pub const MAX_RESPONSE_BYTES: usize = 2 << 20;
/// Cap on the plan text in runes after trimming.
pub const MAX_PLAN_RUNES: usize = 30_000;
/// The model used when the setting is blank.
pub const DEFAULT_MODEL: &str = "gpt-4o-mini";

/// The Go default when no HTTP client is passed to `newClient`.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(25);

static MODEL_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z0-9._:/-]{1,64}$").expect("static model pattern is valid")
});

/// The two expected configuration failures, with the Go error identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("codemode plugin is disabled")]
    Disabled,
    #[error("codemode plugin is not configured")]
    NotConfigured,
}

/// The decrypted provider configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsConfig {
    pub api_key: String,
    pub model: String,
}

/// Accepts exactly the two explicit values; a blank or missing setting means
/// enabled, like the Go helper.
#[must_use]
pub fn enabled_from_settings(settings: &HashMap<String, String>) -> bool {
    matches!(
        settings.get("enabled").map_or("", String::as_str).trim(),
        "" | "true"
    )
}

/// Whether the model name matches `^[A-Za-z0-9._:/-]{1,64}$`.
#[must_use]
pub fn model_is_valid(model: &str) -> bool {
    MODEL_PATTERN.is_match(model)
}

/// Validates the decrypted settings. A blank model falls back to
/// [`DEFAULT_MODEL`]; a blank or malformed key/model is "not configured".
pub fn config_from_settings(
    settings: &HashMap<String, String>,
) -> Result<SettingsConfig, ConfigError> {
    if !enabled_from_settings(settings) {
        return Err(ConfigError::Disabled);
    }
    let mut config = SettingsConfig {
        api_key: settings
            .get("api_key")
            .map_or("", String::as_str)
            .trim()
            .to_string(),
        model: settings
            .get("model")
            .map_or("", String::as_str)
            .trim()
            .to_string(),
    };
    if config.model.is_empty() {
        config.model = DEFAULT_MODEL.to_string();
    }
    if config.api_key.is_empty() {
        return Err(ConfigError::NotConfigured);
    }
    if !MODEL_PATTERN.is_match(&config.model) {
        return Err(ConfigError::NotConfigured);
    }
    Ok(config)
}

/// A failure of the configured endpoint. `status_code == 0` means the request
/// never produced an HTTP response.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub struct UpstreamError {
    pub status_code: u16,
    pub reason: String,
}

impl UpstreamError {
    #[must_use]
    pub fn network(reason: impl Into<String>) -> Self {
        Self {
            status_code: 0,
            reason: reason.into(),
        }
    }

    #[must_use]
    pub fn status(status_code: u16) -> Self {
        Self {
            status_code,
            reason: String::new(),
        }
    }

    /// The status class reported in the server log: `2xx`, `3xx` or `network`.
    #[must_use]
    pub fn status_class(&self) -> String {
        if self.status_code > 0 {
            format!("{}xx", self.status_code / 100)
        } else {
            "network".to_string()
        }
    }

    /// The reason text used in the server log; never sent to the browser.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.reason
    }
}

impl std::fmt::Display for UpstreamError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.status_code > 0 {
            write!(
                formatter,
                "codemode upstream returned HTTP {}",
                self.status_code
            )
        } else {
            write!(
                formatter,
                "codemode upstream request failed: {}",
                self.reason
            )
        }
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    #[serde(default)]
    choices: Vec<Option<ChatChoice>>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    /// Go leaves a `null` message at its zero value.
    #[serde(default)]
    message: Option<ChatChoiceMessage>,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct ChatChoiceMessage {
    #[serde(default)]
    content: Option<serde_json::Value>,
}

/// A validated provider client. Construction never performs I/O.
pub struct Client {
    base_url: String,
    api_key: String,
    model: String,
    http: reqwest::Client,
}

impl Client {
    /// Validates the endpoint and key. `timeout == None` uses Go's 25-second
    /// default; redirects are never followed.
    pub fn new(
        base_url: &str,
        api_key: &str,
        model: &str,
        timeout: Option<Duration>,
    ) -> Result<Self, ConfigError> {
        let base_url = base_url.trim().trim_end_matches('/').to_string();
        let api_key = api_key.trim().to_string();
        let model = model.trim().to_string();
        if base_url.is_empty() || api_key.is_empty() || model.is_empty() {
            return Err(ConfigError::NotConfigured);
        }
        let parsed = url::Url::parse(&base_url).map_err(|_| ConfigError::NotConfigured)?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none_or(str::is_empty)
        {
            return Err(ConfigError::NotConfigured);
        }
        let http = reqwest::Client::builder()
            .timeout(timeout.unwrap_or(DEFAULT_TIMEOUT))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| ConfigError::NotConfigured)?;
        Ok(Self {
            base_url,
            api_key,
            model,
            http,
        })
    }

    /// Sends the system prompt and the admin instruction and returns the plan
    /// text, trimmed.
    pub async fn plan(&self, system: &str, user: &str) -> Result<String, UpstreamError> {
        // Go's `json.Marshal` of the same struct: field order, HTML-safe
        // strings and the `0.2` temperature literal.
        let messages = format!(
            "[{{\"role\":\"system\",\"content\":{}}},{{\"role\":\"user\",\"content\":{}}}]",
            json::string(system),
            json::string(user)
        );
        let mut payload = json::Object::new();
        payload.string_field("model", &self.model);
        payload.raw_field("messages", &messages);
        payload.raw_field("temperature", "0.2");
        payload.int_field("max_tokens", 2000);
        let body = payload.finish().into_bytes();

        let request = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.api_key),
            )
            .body(body)
            .build()
            .map_err(|_| UpstreamError::network("the request could not be built"))?;

        let mut response = self
            .http
            .execute(request)
            .await
            // The key travels in a header, so transport errors never contain it.
            .map_err(|error| UpstreamError::network(error.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            return Err(UpstreamError::status(status.as_u16()));
        }

        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| UpstreamError::network("the response could not be read"))?
        {
            if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err(UpstreamError::network("the response was too large"));
            }
            body.extend_from_slice(&chunk);
        }

        let decoded: ChatCompletionResponse = serde_json::from_slice(&body)
            .map_err(|_| UpstreamError::network("the response was not valid JSON"))?;
        let Some(choice) = decoded.choices.first() else {
            return Err(UpstreamError::network("the response had no choices"));
        };
        let message = choice
            .as_ref()
            .and_then(|choice| choice.message.clone())
            .unwrap_or_default();
        let Some(content) = &message.content else {
            return Err(UpstreamError::network("the response had no content"));
        };
        if content.is_null() {
            return Err(UpstreamError::network("the response had no content"));
        }
        let Some(content) = content.as_str() else {
            return Err(UpstreamError::network(
                "the provider returned structured content instead of text",
            ));
        };
        let content = content.trim();
        if content.is_empty() {
            return Err(UpstreamError::network(
                "the provider returned an empty plan",
            ));
        }
        if content.chars().count() > MAX_PLAN_RUNES {
            return Err(UpstreamError::network("the plan was too long"));
        }
        Ok(content.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn enabled_defaults_to_true() {
        assert!(enabled_from_settings(&HashMap::new()));
        assert!(enabled_from_settings(&settings(&[("enabled", " true ")])));
        assert!(!enabled_from_settings(&settings(&[("enabled", "false")])));
        assert!(!enabled_from_settings(&settings(&[("enabled", "yes")])));
    }

    #[test]
    fn config_requires_a_key_and_a_valid_model() {
        assert_eq!(
            config_from_settings(&settings(&[("enabled", "false"), ("api_key", "k")])),
            Err(ConfigError::Disabled)
        );
        assert_eq!(
            config_from_settings(&settings(&[("api_key", "")])),
            Err(ConfigError::NotConfigured)
        );
        assert_eq!(
            config_from_settings(&settings(&[("api_key", "k"), ("model", "bad model")])),
            Err(ConfigError::NotConfigured)
        );
        let config = config_from_settings(&settings(&[("api_key", " sk ")])).expect("config");
        assert_eq!(config.api_key, "sk");
        assert_eq!(config.model, DEFAULT_MODEL);
    }

    #[test]
    fn client_rejects_incomplete_configuration() {
        for (base, key, model) in [
            ("http://example.test", "", "gpt-4o-mini"),
            ("http://example.test", "sk", ""),
            ("", "sk", "gpt-4o-mini"),
            ("ftp://example.test", "sk", "gpt-4o-mini"),
            ("not a url", "sk", "gpt-4o-mini"),
        ] {
            assert!(
                Client::new(base, key, model, None).is_err(),
                "{base} {key} {model}"
            );
        }
    }

    #[test]
    fn upstream_error_text_matches_go() {
        let network = UpstreamError::network("boom");
        assert_eq!(
            network.to_string(),
            "codemode upstream request failed: boom"
        );
        assert_eq!(network.status_class(), "network");
        let status = UpstreamError::status(302);
        assert_eq!(status.to_string(), "codemode upstream returned HTTP 302");
        assert_eq!(status.status_class(), "3xx");
    }
}
