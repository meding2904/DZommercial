//! The OpenAI-compatible client, ported from `client.go`.
//!
//! Error mapping is part of the security contract: the provider body never
//! reaches the browser, and every failure becomes one [`UpstreamError`] that
//! the handler maps to a plain 502.

use std::time::Duration;

use reqwest::header;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{DEFAULT_LANGUAGE, is_supported_language};

/// Provider bodies larger than this are rejected without being decoded.
pub const MAX_RESPONSE_BYTES: usize = 2 << 20;
/// Longest accepted answer, in Unicode scalar values.
pub const MAX_ANSWER_RUNES: usize = 8000;

/// Why a configured endpoint cannot be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// The settings form switched the analyst off.
    #[error("ai plugin is disabled")]
    Disabled,
    /// The key, model or base URL is missing or malformed.
    #[error("ai plugin is not configured")]
    NotConfigured,
}

/// The stored settings that matter to the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsConfig {
    pub api_key: String,
    pub model: String,
    pub language: String,
}

/// A failed upstream call.
///
/// A non-zero `status_code` is an HTTP response status; zero marks a transport
/// or decoding failure. `reason` never carries the response body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamError {
    pub status_code: u16,
    pub reason: String,
}

impl UpstreamError {
    #[must_use]
    pub fn new(status_code: u16, reason: impl Into<String>) -> Self {
        Self {
            status_code,
            reason: reason.into(),
        }
    }

    /// A transport, encoding or decoding failure.
    #[must_use]
    pub fn network(reason: impl Into<String>) -> Self {
        Self::new(0, reason)
    }

    /// The status class Go renders in the warning log.
    #[must_use]
    pub fn status_class(&self) -> String {
        if self.status_code > 0 {
            format!("{}xx", self.status_code / 100)
        } else {
            "network".to_string()
        }
    }

    /// The detail Go renders in the warning log.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.reason
    }
}

impl std::fmt::Display for UpstreamError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.status_code > 0 {
            write!(formatter, "ai upstream returned HTTP {}", self.status_code)
        } else {
            write!(formatter, "ai upstream request failed: {}", self.reason)
        }
    }
}

impl std::error::Error for UpstreamError {}

#[derive(Debug, Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 2],
    temperature: f64,
    max_tokens: u32,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ChatCompletionResponse {
    choices: Option<Vec<Option<ChatChoice>>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ChatChoice {
    message: Option<ChatMessageResponse>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ChatMessageResponse {
    content: Option<serde_json::Value>,
}

/// An OpenAI-compatible chat client.
pub struct Client {
    base_url: String,
    api_key: String,
    model: String,
    http: reqwest::Client,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Client")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

/// Trims and validates the endpoint configuration shared by both constructors.
fn normalize(
    base_url: &str,
    api_key: &str,
    model: &str,
) -> Result<(String, String, String), ConfigError> {
    let base_url = base_url.trim().trim_end_matches('/').to_string();
    let api_key = api_key.trim().to_string();
    let model = model.trim().to_string();
    if base_url.is_empty() || api_key.is_empty() || model.is_empty() {
        return Err(ConfigError::NotConfigured);
    }
    let Ok(parsed) = Url::parse(&base_url) else {
        return Err(ConfigError::NotConfigured);
    };
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(ConfigError::NotConfigured);
    }
    Ok((base_url, api_key, model))
}

/// Builds the production client: no redirects, with the configured timeout.
///
/// A provider redirect must not turn the server into a request forwarder, so
/// [`reqwest::redirect::Policy::none`] is always installed here. `timeout` of
/// zero means "no timeout", like `http.Client{Timeout: 0}`.
pub fn new_client(
    base_url: &str,
    api_key: &str,
    model: &str,
    timeout: Duration,
) -> Result<Client, ConfigError> {
    let (base_url, api_key, model) = normalize(base_url, api_key, model)?;
    let mut builder = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none());
    if !timeout.is_zero() {
        builder = builder.timeout(timeout);
    }
    let http = builder.build().map_err(|_| ConfigError::NotConfigured)?;
    Ok(Client {
        base_url,
        api_key,
        model,
        http,
    })
}

/// Builds a client around a caller-supplied HTTP client.
///
/// Intended for tests that need a custom timeout or transport. The supplied
/// client keeps its own redirect policy, so production code must use
/// [`new_client`].
pub fn with_http_client(
    base_url: &str,
    api_key: &str,
    model: &str,
    http: reqwest::Client,
) -> Result<Client, ConfigError> {
    let (base_url, api_key, model) = normalize(base_url, api_key, model)?;
    Ok(Client {
        base_url,
        api_key,
        model,
        http,
    })
}

/// Settings that switch the analyst on and name the key, model and language.
///
/// The order matches the Go checks: disabled wins over incomplete settings,
/// and an unknown language falls back to English before the missing-key check.
pub fn config_from_settings(
    settings: &std::collections::HashMap<String, String>,
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
        language: settings
            .get("language")
            .map_or("", String::as_str)
            .trim()
            .to_string(),
    };
    if !is_supported_language(&config.language) {
        config.language = DEFAULT_LANGUAGE.to_string();
    }
    if config.api_key.is_empty() || config.model.is_empty() {
        return Err(ConfigError::NotConfigured);
    }
    Ok(config)
}

/// Go treats a missing `enabled` key and the literal `true` as enabled; every
/// other value (including an unknown word) disables the plugin.
#[must_use]
pub fn enabled_from_settings(settings: &std::collections::HashMap<String, String>) -> bool {
    matches!(
        settings.get("enabled").map_or("", String::as_str).trim(),
        "" | "true"
    )
}

impl Client {
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Calls `POST {base}/chat/completions` and returns the trimmed answer.
    pub async fn complete(&self, system: &str, user: &str) -> Result<String, UpstreamError> {
        let payload = serde_json::to_vec(&ChatCompletionRequest {
            model: &self.model,
            messages: [
                ChatMessage {
                    role: "system",
                    content: system,
                },
                ChatMessage {
                    role: "user",
                    content: user,
                },
            ],
            temperature: 0.3,
            max_tokens: 500,
        })
        .map_err(|_| UpstreamError::network("the request could not be encoded"))?;

        let request = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {}", self.api_key))
            .body(payload);
        let mut response = request
            .send()
            .await
            .map_err(|error| UpstreamError::network(error.to_string()))?;

        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(UpstreamError::new(status, ""));
        }

        let mut body: Vec<u8> = Vec::new();
        loop {
            let chunk = response
                .chunk()
                .await
                .map_err(|_| UpstreamError::network("the response could not be read"))?;
            let Some(chunk) = chunk else {
                break;
            };
            if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(UpstreamError::network("the response was too large"));
            }
            body.extend_from_slice(&chunk);
        }

        let decoded: Option<ChatCompletionResponse> = serde_json::from_slice(&body)
            .map_err(|_| UpstreamError::network("the response was not valid JSON"))?;
        let Some(decoded) = decoded else {
            return Err(UpstreamError::network("the response had no choices"));
        };
        let Some(choices) = decoded.choices else {
            return Err(UpstreamError::network("the response had no choices"));
        };
        let Some(choice) = choices.into_iter().next() else {
            return Err(UpstreamError::network("the response had no choices"));
        };
        let content = match choice
            .unwrap_or_default()
            .message
            .and_then(|message| message.content)
        {
            None | Some(serde_json::Value::Null) => {
                return Err(UpstreamError::network("the response had no content"));
            }
            Some(serde_json::Value::String(content)) => content,
            Some(_) => {
                return Err(UpstreamError::network(
                    "the provider returned structured content instead of plain text",
                ));
            }
        };

        let content = content.trim();
        if content.is_empty() {
            return Err(UpstreamError::network(
                "the provider returned an empty answer",
            ));
        }
        if content.chars().count() > MAX_ANSWER_RUNES {
            return Err(UpstreamError::network("the answer was too long"));
        }
        Ok(content.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use axum::Router;
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::{IntoResponse, Response};
    use axum::routing::post;
    use serde_json::{Value, json};

    use super::*;

    #[derive(Clone, Default)]
    struct Capture {
        requests: Arc<Mutex<Vec<(HeaderMap, Value)>>>,
    }

    async fn ok(State(capture): State<Capture>, headers: HeaderMap, body: String) -> Response {
        capture
            .requests
            .lock()
            .expect("capture")
            .push((headers, serde_json::from_str(&body).expect("request json")));
        (
            StatusCode::OK,
            r#"{"choices":[{"message":{"content":"  A clear answer.  "}}]}"#,
        )
            .into_response()
    }

    async fn upstream_error() -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"error":{"message":"provider secret detail"}}"#,
        )
            .into_response()
    }

    async fn oversized() -> Response {
        let body = format!(
            r#"{{"choices":[{{"message":{{"content":"{}"}}}}]}}"#,
            "a".repeat(MAX_RESPONSE_BYTES + 1)
        );
        (StatusCode::OK, body).into_response()
    }

    async fn long_answer() -> Response {
        let body = format!(
            r#"{{"choices":[{{"message":{{"content":"{}"}}}}]}}"#,
            "a".repeat(MAX_ANSWER_RUNES + 1)
        );
        (StatusCode::OK, body).into_response()
    }

    async fn structured() -> Response {
        (
            StatusCode::OK,
            r#"{"choices":[{"message":{"content":{"answer":"no"}}}]}"#,
        )
            .into_response()
    }

    async fn no_choices() -> Response {
        (StatusCode::OK, r#"{"choices":[]}"#).into_response()
    }

    async fn null_choices() -> Response {
        (StatusCode::OK, r#"{"choices":null}"#).into_response()
    }

    async fn null_choice() -> Response {
        (StatusCode::OK, r#"{"choices":[null]}"#).into_response()
    }

    async fn no_content() -> Response {
        (StatusCode::OK, r#"{"choices":[{"message":{}}]}"#).into_response()
    }

    async fn invalid_json() -> Response {
        (StatusCode::OK, "not json").into_response()
    }

    async fn redirect() -> Response {
        (StatusCode::FOUND, [("location", "/elsewhere")]).into_response()
    }

    async fn slow() -> Response {
        tokio::time::sleep(Duration::from_millis(300)).await;
        (
            StatusCode::OK,
            r#"{"choices":[{"message":{"content":"late"}}]}"#,
        )
            .into_response()
    }

    async fn start(app: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let address = listener.local_addr().expect("mock address");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn completes_a_chat_and_preserves_the_wire_shape() {
        let capture = Capture::default();
        let base_url = start(
            Router::new()
                .route("/chat/completions", post(ok))
                .with_state(capture.clone()),
        )
        .await;

        let client = new_client(
            &base_url,
            "sk-test-secret",
            "gpt-4o-mini",
            Duration::from_secs(5),
        )
        .expect("client");
        let answer = client
            .complete("system prompt", "user prompt")
            .await
            .expect("complete");
        assert_eq!(answer, "A clear answer.");

        let (headers, payload) = capture.requests.lock().expect("capture").remove(0);
        assert_eq!(headers["authorization"], "Bearer sk-test-secret");
        assert_eq!(headers["content-type"], "application/json");
        assert_eq!(
            payload,
            json!({
                "model": "gpt-4o-mini",
                "messages": [
                    {"role": "system", "content": "system prompt"},
                    {"role": "user", "content": "user prompt"}
                ],
                "temperature": 0.3,
                "max_tokens": 500
            })
        );
    }

    #[test]
    fn rejects_incomplete_configuration_before_a_request() {
        assert_eq!(
            new_client("", "sk-key", "gpt-4o-mini", Duration::from_secs(1)).unwrap_err(),
            ConfigError::NotConfigured
        );
        assert_eq!(
            new_client(
                "http://127.0.0.1:1",
                "",
                "gpt-4o-mini",
                Duration::from_secs(1)
            )
            .unwrap_err(),
            ConfigError::NotConfigured
        );
        assert_eq!(
            new_client("http://127.0.0.1:1", "sk-key", "", Duration::from_secs(1)).unwrap_err(),
            ConfigError::NotConfigured
        );
        assert_eq!(
            new_client("ftp://127.0.0.1", "sk-key", "m", Duration::from_secs(1)).unwrap_err(),
            ConfigError::NotConfigured
        );
        assert_eq!(
            new_client("not a url", "sk-key", "m", Duration::from_secs(1)).unwrap_err(),
            ConfigError::NotConfigured
        );
    }

    #[test]
    fn settings_decode_disabled_and_missing_values() {
        let mut settings = HashMap::from([
            ("enabled".to_string(), "false".to_string()),
            ("api_key".to_string(), "sk-key".to_string()),
            ("model".to_string(), "gpt-4o-mini".to_string()),
            ("language".to_string(), "en".to_string()),
        ]);
        assert_eq!(
            config_from_settings(&settings).unwrap_err(),
            ConfigError::Disabled
        );

        settings.insert("enabled".to_string(), "true".to_string());
        settings.remove("api_key");
        assert_eq!(
            config_from_settings(&settings).unwrap_err(),
            ConfigError::NotConfigured
        );

        settings.insert("api_key".to_string(), "  sk-key  ".to_string());
        settings.remove("model");
        assert_eq!(
            config_from_settings(&settings).unwrap_err(),
            ConfigError::NotConfigured
        );

        settings.insert("model".to_string(), " gpt-4o-mini ".to_string());
        settings.insert("language".to_string(), "de".to_string());
        let config = config_from_settings(&settings).expect("config");
        assert_eq!(config.api_key, "sk-key");
        assert_eq!(config.model, "gpt-4o-mini");
        assert_eq!(config.language, DEFAULT_LANGUAGE);
    }

    #[test]
    fn enabled_defaults_to_true_and_accepts_only_true() {
        assert!(enabled_from_settings(&HashMap::new()));
        for value in ["", "  ", "true", " true "] {
            assert!(enabled_from_settings(&HashMap::from([(
                "enabled".to_string(),
                value.to_string()
            )])));
        }
        for value in ["false", "0", "yes", "TRUE", "on"] {
            assert!(!enabled_from_settings(&HashMap::from([(
                "enabled".to_string(),
                value.to_string()
            )])));
        }
    }

    #[tokio::test]
    async fn maps_upstream_status_without_leaking_the_body() {
        let base_url = start(Router::new().route("/chat/completions", post(upstream_error))).await;
        let client = new_client(
            &base_url,
            "sk-test-secret",
            "gpt-4o-mini",
            Duration::from_secs(5),
        )
        .expect("client");
        let error = client
            .complete("system", "user")
            .await
            .expect_err("upstream error");
        assert_eq!(error.status_code, 500);
        assert_eq!(error.status_class(), "5xx");
        assert!(!error.to_string().contains("provider secret detail"));
        assert!(!error.detail().contains("provider secret detail"));
        assert_eq!(error.to_string(), "ai upstream returned HTTP 500");
    }

    #[test]
    fn maps_network_error_text() {
        let error = UpstreamError::network("connection refused");
        assert_eq!(error.status_class(), "network");
        assert_eq!(error.detail(), "connection refused");
        assert_eq!(
            error.to_string(),
            "ai upstream request failed: connection refused"
        );
    }

    #[tokio::test]
    async fn rejects_a_response_over_the_byte_cap() {
        let base_url = start(Router::new().route("/chat/completions", post(oversized))).await;
        let client = new_client(
            &base_url,
            "sk-test-secret",
            "gpt-4o-mini",
            Duration::from_secs(30),
        )
        .expect("client");
        let error = client
            .complete("system", "user")
            .await
            .expect_err("oversized");
        assert_eq!(error.detail(), "the response was too large");
    }

    #[tokio::test]
    async fn rejects_an_answer_over_the_rune_cap() {
        let base_url = start(Router::new().route("/chat/completions", post(long_answer))).await;
        let client = new_client(
            &base_url,
            "sk-test-secret",
            "gpt-4o-mini",
            Duration::from_secs(5),
        )
        .expect("client");
        let error = client
            .complete("system", "user")
            .await
            .expect_err("long answer");
        assert_eq!(error.detail(), "the answer was too long");
    }

    #[tokio::test]
    async fn rejects_structured_content() {
        let base_url = start(Router::new().route("/chat/completions", post(structured))).await;
        let client = new_client(
            &base_url,
            "sk-test-secret",
            "gpt-4o-mini",
            Duration::from_secs(5),
        )
        .expect("client");
        let error = client
            .complete("system", "user")
            .await
            .expect_err("structured");
        assert_eq!(
            error.detail(),
            "the provider returned structured content instead of plain text"
        );
    }

    #[tokio::test]
    async fn rejects_missing_choices_and_content() {
        let empty = start(Router::new().route("/chat/completions", post(no_choices))).await;
        let client = new_client(&empty, "sk", "model", Duration::from_secs(5)).expect("client");
        assert_eq!(
            client
                .complete("s", "u")
                .await
                .expect_err("no choices")
                .detail(),
            "the response had no choices"
        );

        let missing = start(Router::new().route("/chat/completions", post(no_content))).await;
        let client = new_client(&missing, "sk", "model", Duration::from_secs(5)).expect("client");
        assert_eq!(
            client
                .complete("s", "u")
                .await
                .expect_err("no content")
                .detail(),
            "the response had no content"
        );

        let null = start(Router::new().route("/chat/completions", post(null_choices))).await;
        let client = new_client(&null, "sk", "model", Duration::from_secs(5)).expect("client");
        assert_eq!(
            client
                .complete("s", "u")
                .await
                .expect_err("null choices")
                .detail(),
            "the response had no choices"
        );

        let null = start(Router::new().route("/chat/completions", post(null_choice))).await;
        let client = new_client(&null, "sk", "model", Duration::from_secs(5)).expect("client");
        assert_eq!(
            client
                .complete("s", "u")
                .await
                .expect_err("null choice")
                .detail(),
            "the response had no content"
        );
    }

    #[tokio::test]
    async fn rejects_invalid_json() {
        let base_url = start(Router::new().route("/chat/completions", post(invalid_json))).await;
        let client = new_client(&base_url, "sk", "model", Duration::from_secs(5)).expect("client");
        assert_eq!(
            client.complete("s", "u").await.expect_err("json").detail(),
            "the response was not valid JSON"
        );
    }

    #[tokio::test]
    async fn does_not_follow_provider_redirects() {
        let base_url = start(Router::new().route("/chat/completions", post(redirect))).await;
        let client = new_client(&base_url, "sk", "model", Duration::from_secs(5)).expect("client");
        let error = client.complete("s", "u").await.expect_err("redirect");
        assert_eq!(error.status_code, 302);
        assert_eq!(error.status_class(), "3xx");
    }

    #[tokio::test]
    async fn maps_a_timeout_to_a_transport_error() {
        let base_url = start(Router::new().route("/chat/completions", post(slow))).await;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_millis(50))
            .build()
            .expect("test client");
        let client = with_http_client(&base_url, "sk", "model", http).expect("client");
        let error = client.complete("s", "u").await.expect_err("timeout");
        assert_eq!(error.status_code, 0);
        assert_eq!(error.status_class(), "network");
    }
}
