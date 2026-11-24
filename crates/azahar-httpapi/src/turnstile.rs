//! Cloudflare Turnstile verification, ported from `turnstile.go`.
//!
//! The same 5-second timeout, the same 1 MiB response cap and the same
//! success/error contract: an empty or oversized token is a plain `false`, an
//! HTTP or decoding failure is an error the checkout renders as
//! `captcha_unavailable`.

use std::time::Duration;

use reqwest::{Client, StatusCode};
use serde::Deserialize;

/// Tokens are at most this many bytes before they are refused outright.
pub const TURNSTILE_MAX_TOKEN_LENGTH: usize = 2048;
/// Cloudflare's siteverify endpoint.
pub const TURNSTILE_VERIFY_URL: &str = "https://challenges.cloudflare.com/turnstile/v0/siteverify";
/// The Go client's request timeout.
pub const TURNSTILE_TIMEOUT: Duration = Duration::from_secs(5);
const RESPONSE_LIMIT: usize = 1 << 20;

#[derive(Debug, thiserror::Error)]
pub enum TurnstileError {
    #[error("turnstile verify: build client: {0}")]
    Client(String),
    #[error("turnstile verify: {0}")]
    Transport(String),
    #[error("turnstile verify: unexpected status {0}")]
    Status(i32),
    #[error("turnstile verify: decode response: {0}")]
    Decode(String),
}

#[derive(Debug, Default, Deserialize)]
struct TurnstileResult {
    #[serde(default)]
    success: bool,
    // Go decodes and ignores `error-codes`; keeping the field documents that
    // an error code list does not change the outcome.
    #[serde(default, rename = "error-codes")]
    #[allow(dead_code)]
    error_codes: Vec<String>,
}

/// Verifies one Turnstile token against Cloudflare.
///
/// # Errors
///
/// Returns [`TurnstileError`] for a transport failure, a non-200 response or a
/// response body that is not the expected JSON.
pub async fn verify_turnstile(
    secret: &str,
    token: &str,
    remote_ip: &str,
) -> Result<bool, TurnstileError> {
    let client = Client::builder()
        .timeout(TURNSTILE_TIMEOUT)
        .build()
        .map_err(|error| TurnstileError::Client(error.to_string()))?;
    verify_with(&client, TURNSTILE_VERIFY_URL, secret, token, remote_ip).await
}

async fn verify_with(
    client: &Client,
    endpoint: &str,
    secret: &str,
    token: &str,
    remote_ip: &str,
) -> Result<bool, TurnstileError> {
    let token = token.trim();
    if secret.is_empty() || token.is_empty() || token.len() > TURNSTILE_MAX_TOKEN_LENGTH {
        return Ok(false);
    }

    // Go's url.Values.Encode sorts keys; this is the same field order and the
    // same application/x-www-form-urlencoded escaping. The serializer lives in
    // its own block so its non-Send borrow is dropped before the request
    // future is created.
    let form = {
        let mut serializer = url::form_urlencoded::Serializer::new(String::new());
        serializer.append_pair("remoteip", remote_ip);
        serializer.append_pair("response", token);
        serializer.append_pair("secret", secret);
        serializer.finish()
    };

    let response = client
        .post(endpoint)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(form)
        .send()
        .await
        .map_err(|error| TurnstileError::Transport(error.to_string()))?;

    let status = response.status();
    if status != StatusCode::OK {
        return Err(TurnstileError::Status(status.as_u16().into()));
    }

    let body = read_limited(response).await?;
    let result: TurnstileResult =
        serde_json::from_slice(&body).map_err(|error| TurnstileError::Decode(error.to_string()))?;
    Ok(result.success)
}

/// Reads at most 1 MiB, the Go `io.LimitReader` cap.
async fn read_limited(mut response: reqwest::Response) -> Result<Vec<u8>, TurnstileError> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| TurnstileError::Transport(error.to_string()))?
    {
        let remaining = RESPONSE_LIMIT.saturating_sub(body.len());
        if remaining == 0 {
            break;
        }
        let take = remaining.min(chunk.len());
        body.extend_from_slice(&chunk[..take]);
        if take < chunk.len() {
            break;
        }
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::Router;
    use axum::extract::Form;
    use axum::routing::post;
    use serde_json::json;

    use super::*;

    type SeenForm = Arc<Mutex<Option<Vec<(String, String)>>>>;

    async fn local_server(app: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        format!("http://{address}/verify")
    }

    #[tokio::test]
    async fn an_empty_secret_or_token_is_rejected_without_a_request() {
        assert!(
            !verify_turnstile("", "token", "203.0.113.7")
                .await
                .expect("no network")
        );
        assert!(
            !verify_turnstile("secret", "", "203.0.113.7")
                .await
                .expect("no network")
        );
        assert!(
            !verify_turnstile("secret", &"a".repeat(2049), "203.0.113.7")
                .await
                .expect("no network")
        );
    }

    #[tokio::test]
    async fn the_form_and_success_flag_match_the_go_contract() {
        let seen: SeenForm = Arc::new(Mutex::new(None));
        let captured = seen.clone();
        let app = Router::new().route(
            "/verify",
            post(move |Form(fields): Form<Vec<(String, String)>>| {
                let captured = captured.clone();
                async move {
                    *captured.lock().expect("capture") = Some(fields);
                    axum::Json(json!({"success": true, "error-codes": []}))
                }
            }),
        );
        let endpoint = local_server(app).await;
        let client = Client::builder()
            .timeout(TURNSTILE_TIMEOUT)
            .build()
            .expect("client");

        let verified = verify_with(&client, &endpoint, "secret key", " tok en ", "203.0.113.7")
            .await
            .expect("verify");
        assert!(verified);
        let fields = seen.lock().expect("seen").clone().expect("form");
        assert_eq!(
            fields,
            vec![
                ("remoteip".to_string(), "203.0.113.7".to_string()),
                ("response".to_string(), "tok en".to_string()),
                ("secret".to_string(), "secret key".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn success_false_is_a_refusal_without_an_error() {
        let app = Router::new().route(
            "/verify",
            post(|| async {
                axum::Json(json!({"success": false, "error-codes": ["invalid-input-response"]}))
            }),
        );
        let endpoint = local_server(app).await;
        let client = Client::builder().build().expect("client");
        assert!(
            !verify_with(&client, &endpoint, "secret", "token", "203.0.113.7")
                .await
                .expect("verify")
        );
    }

    #[tokio::test]
    async fn a_non_200_response_is_an_error() {
        let app = Router::new().route(
            "/verify",
            post(|| async { (StatusCode::SERVICE_UNAVAILABLE, "down") }),
        );
        let endpoint = local_server(app).await;
        let client = Client::builder().build().expect("client");
        let error = verify_with(&client, &endpoint, "secret", "token", "203.0.113.7")
            .await
            .expect_err("status error");
        assert!(matches!(error, TurnstileError::Status(503)));
    }

    #[tokio::test]
    async fn invalid_json_is_a_decode_error() {
        let app = Router::new().route("/verify", post(|| async { "not json" }));
        let endpoint = local_server(app).await;
        let client = Client::builder().build().expect("client");
        let error = verify_with(&client, &endpoint, "secret", "token", "203.0.113.7")
            .await
            .expect_err("decode error");
        assert!(matches!(error, TurnstileError::Decode(_)));
    }

    #[tokio::test]
    async fn a_slow_endpoint_times_out() {
        let app = Router::new().route(
            "/verify",
            post(|| async {
                tokio::time::sleep(Duration::from_millis(300)).await;
                axum::Json(json!({"success": true}))
            }),
        );
        let endpoint = local_server(app).await;
        let client = Client::builder()
            .timeout(Duration::from_millis(50))
            .build()
            .expect("client");
        let error = verify_with(&client, &endpoint, "secret", "token", "203.0.113.7")
            .await
            .expect_err("timeout");
        assert!(matches!(error, TurnstileError::Transport(_)));
    }
}
