//! Mock-provider tests for the planner client: request bytes, response
//! mapping and the refusal paths. Every server is loopback only.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::response::Response;
use axum::routing::post;
use azahar_plugin_codemode::{Client, ConfigError, MAX_RESPONSE_BYTES, UpstreamError};

#[derive(Clone)]
struct MockState {
    status: u16,
    body: String,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
}

struct RecordedRequest {
    method: Method,
    path: String,
    authorization: String,
    content_type: String,
    body: String,
}

async fn handler(
    State(state): State<MockState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: String,
) -> Response {
    state
        .requests
        .lock()
        .expect("requests")
        .push(RecordedRequest {
            method,
            path: uri.path().to_string(),
            authorization: headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string(),
            content_type: headers
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string(),
            body,
        });
    let status = StatusCode::from_u16(state.status).unwrap_or(StatusCode::OK);
    let mut response = Response::new(Body::from(state.body.clone()));
    *response.status_mut() = status;
    if status.is_redirection() {
        response.headers_mut().insert(
            header::LOCATION,
            header::HeaderValue::from_static("http://127.0.0.1:1/chat/completions"),
        );
    }
    response
}

async fn serve(status: u16, body: String) -> (String, Arc<Mutex<Vec<RecordedRequest>>>) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let state = MockState {
        status,
        body,
        requests: requests.clone(),
    };
    let app = Router::new()
        .route("/chat/completions", post(handler))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{address}"), requests)
}

fn client(base: &str) -> Client {
    Client::new(
        base,
        "sk-test-secret",
        "gpt-4o-mini",
        Some(std::time::Duration::from_secs(5)),
    )
    .expect("client")
}

fn completion(content: &str) -> String {
    serde_json::json!({"choices": [{"message": {"content": content}}]}).to_string()
}

#[tokio::test]
async fn sends_the_exact_chat_completion_request() {
    let (base, requests) = serve(200, completion("  {\"summary\":\"A.\",\"actions\":[]}  ")).await;
    let content = client(&base)
        .plan("system <prompt>", "user instruction")
        .await
        .expect("plan");
    assert_eq!(content, "{\"summary\":\"A.\",\"actions\":[]}");

    let requests = requests.lock().expect("requests");
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.method, Method::POST);
    assert_eq!(request.path, "/chat/completions");
    assert_eq!(request.authorization, "Bearer sk-test-secret");
    assert_eq!(request.content_type, "application/json");
    assert_eq!(
        request.body,
        "{\"model\":\"gpt-4o-mini\",\"messages\":[{\"role\":\"system\",\"content\":\"system \\u003cprompt\\u003e\"},{\"role\":\"user\",\"content\":\"user instruction\"}],\"temperature\":0.2,\"max_tokens\":2000}"
    );
}

#[tokio::test]
async fn maps_http_failures_without_leaking_the_body() {
    let (base, requests) = serve(429, "provider secret".to_string()).await;
    let error = client(&base).plan("system", "user").await.expect_err("429");
    assert_eq!(error, UpstreamError::status(429));
    assert_eq!(error.status_class(), "4xx");
    assert_eq!(error.detail(), "");
    assert_eq!(error.to_string(), "codemode upstream returned HTTP 429");
    assert!(!error.to_string().contains("provider secret"));
    assert_eq!(requests.lock().expect("requests").len(), 1);
}

#[tokio::test]
async fn refuses_to_follow_redirects() {
    let (base, requests) = serve(302, completion("{}")).await;
    let error = client(&base)
        .plan("system", "user")
        .await
        .expect_err("redirect");
    assert_eq!(error.status_code, 302);
    assert_eq!(error.status_class(), "3xx");
    assert_eq!(requests.lock().expect("requests").len(), 1);
}

#[tokio::test]
async fn rejects_oversized_responses() {
    let content = "a".repeat(MAX_RESPONSE_BYTES + 1);
    let (base, _requests) = serve(200, completion(&content)).await;
    let error = client(&base)
        .plan("system", "user")
        .await
        .expect_err("too large");
    assert_eq!(error.detail(), "the response was too large");
}

#[tokio::test]
async fn maps_every_malformed_answer() {
    let cases = [
        ("not json".to_string(), "the response was not valid JSON"),
        ("{}".to_string(), "the response had no choices"),
        (
            "{\"choices\":[]}".to_string(),
            "the response had no choices",
        ),
        (
            "{\"choices\":[null]}".to_string(),
            "the response had no content",
        ),
        (
            "{\"choices\":[{\"message\":{\"content\":null}}]}".to_string(),
            "the response had no content",
        ),
        (
            "{\"choices\":[{\"message\":{}}]}".to_string(),
            "the response had no content",
        ),
        (
            "{\"choices\":[{\"message\":{\"content\":{\"a\":1}}}]}".to_string(),
            "the provider returned structured content instead of text",
        ),
        (
            "{\"choices\":[{\"message\":{\"content\":\"   \"}}]}".to_string(),
            "the provider returned an empty plan",
        ),
        (completion(&"a".repeat(30_001)), "the plan was too long"),
    ];
    for (body, message) in cases {
        let (base, _requests) = serve(200, body).await;
        let error = client(&base)
            .plan("system", "user")
            .await
            .expect_err(message);
        assert_eq!(error.detail(), message);
    }
}

#[tokio::test]
async fn rejects_incomplete_configuration_before_any_request() {
    let (base, requests) = serve(200, completion("{}")).await;
    for (base_url, key, model) in [
        (base.as_str(), "", "gpt-4o-mini"),
        (base.as_str(), "sk", ""),
        ("", "sk", "gpt-4o-mini"),
        ("ftp://example.test", "sk", "gpt-4o-mini"),
        ("not a url", "sk", "gpt-4o-mini"),
    ] {
        assert_eq!(
            Client::new(base_url, key, model, None).err(),
            Some(ConfigError::NotConfigured)
        );
    }
    assert!(requests.lock().expect("requests").is_empty());
}

#[tokio::test]
async fn trims_a_trailing_slash_in_the_base_url() {
    let (base, requests) = serve(200, completion("{}")).await;
    let client = Client::new(
        &format!("{base}/"),
        " sk ",
        " gpt-4o-mini ",
        Some(std::time::Duration::from_secs(5)),
    )
    .expect("client");
    client.plan("system", "user").await.expect("plan");
    let requests = requests.lock().expect("requests");
    assert_eq!(requests[0].path, "/chat/completions");
    assert_eq!(requests[0].authorization, "Bearer sk");
    assert!(requests[0].body.contains("\"model\":\"gpt-4o-mini\""));
}
