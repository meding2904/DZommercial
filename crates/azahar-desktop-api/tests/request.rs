#[allow(dead_code)]
mod common;
#[allow(dead_code)]
mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use support::{MockResponse, MockServer};

const FIRST_TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SECOND_TOKEN: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[test]
fn request_headers_and_default_body() {
    let server = MockServer::start(|_| MockResponse::json(200, r#"{"ok":true}"#));
    let test = common::test_client(server.url());
    test.client.seed_csrf("csrf-token");

    let response = test.client.request("POST", "/api/echo", None);
    assert!(response.ok, "{:?}", response.error);

    let requests = server.requests();
    let request = requests
        .iter()
        .find(|request| request.path == "/api/echo")
        .expect("echo request");
    assert_eq!(request.body_text(), "{}");
    assert_eq!(request.header("accept"), "application/json");
    assert_eq!(request.header("x-requested-with"), "azahar-web");
    assert_eq!(request.header("content-type"), "application/json");
    assert_eq!(request.header("x-csrf-token"), "csrf-token");
    assert!(request.header("cookie").contains("azahar_csrf=csrf-token"));
    assert!(
        request
            .header("user-agent")
            .starts_with("AzaharDesktop/2.0.0-test (")
    );
}

#[test]
fn request_uses_configured_origin() {
    let server = MockServer::start(|_| MockResponse::json(200, r#"{"ok":true}"#));
    let test = common::test_client_with_origin(server.url(), "http://admin.example");
    let response = test.client.request("GET", "/api/ok", None);
    assert!(response.ok, "{:?}", response.error);
    assert_eq!(
        server.requests()[0].header("origin"),
        "http://admin.example"
    );
}

#[test]
fn request_sends_json_body() {
    let server = MockServer::start(|_| MockResponse::json(200, r#"{"ok":true}"#));
    let test = common::test_client(server.url());
    test.client.seed_csrf("csrf-token");
    let body = serde_json::json!({"name": "Amina", "html": "<b>&</b>"});
    let response = test.client.request("POST", "/api/echo", Some(&body));
    assert!(response.ok, "{:?}", response.error);
    let requests = server.requests();
    let request = requests
        .iter()
        .find(|request| request.path == "/api/echo")
        .expect("echo request");
    let sent: serde_json::Value = serde_json::from_slice(&request.body).expect("json body");
    assert_eq!(sent["name"], "Amina");
    assert_eq!(sent["html"], "<b>&</b>");
    assert!(request.body_text().contains("\\u003cb\\u003e"));
}

#[test]
fn request_does_not_follow_redirects() {
    let target_hits = Arc::new(AtomicUsize::new(0));
    let hits = Arc::clone(&target_hits);
    let server = MockServer::start(move |request| match request.path.as_str() {
        "/api/start" => MockResponse::json(302, "{}").with_header("Location", "/api/target"),
        "/api/target" => {
            hits.fetch_add(1, Ordering::SeqCst);
            MockResponse::json(200, r#"{"ok":true}"#)
        }
        _ => MockResponse::json(404, "{}"),
    });
    let test = common::test_client(server.url());
    let response = test.client.request("GET", "/api/start", None);
    assert!(!response.ok, "redirect was followed");
    let error = response.error.expect("error");
    assert_eq!(error.status, 302);
    assert_eq!(target_hits.load(Ordering::SeqCst), 0);
}

#[test]
fn request_csrf_retry() {
    let state = Arc::new(std::sync::Mutex::new((0_usize, 0_usize)));
    let shared = Arc::clone(&state);
    let server = MockServer::start(move |request| {
        let mut state = shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match request.path.as_str() {
            "/api/csrf" => {
                state.0 += 1;
                let token = if state.0 > 1 {
                    SECOND_TOKEN
                } else {
                    FIRST_TOKEN
                };
                MockResponse::json(200, &format!(r#"{{"csrf_token":"{token}"}}"#))
                    .with_set_cookie(format!("azahar_csrf={token}; Path=/"))
            }
            "/api/login" => {
                state.1 += 1;
                let call = state.1;
                if call == 1 && request.header("x-csrf-token") == FIRST_TOKEN {
                    return MockResponse::json(
                        403,
                        r#"{"error":{"code":"csrf_failed","message":"The request could not be verified.","fields":{}}}"#,
                    );
                }
                if request.header("x-csrf-token") != SECOND_TOKEN {
                    return MockResponse::json(
                        403,
                        r#"{"error":{"code":"csrf_failed","message":"The request could not be verified.","fields":{}}}"#,
                    );
                }
                MockResponse::json(200, r#"{"user":{"name":"Amina"}}"#)
            }
            _ => MockResponse::json(404, "{}"),
        }
    });

    let test = common::test_client(server.url());
    let response = test.client.request(
        "POST",
        "/api/login",
        Some(&serde_json::json!({"email": "a@example.com"})),
    );
    assert!(response.ok, "{:?}", response.error);
    let state = state.lock().unwrap();
    assert_eq!(state.0, 2, "CSRF calls");
    assert_eq!(state.1, 2, "POST calls");
}

#[test]
fn renderer_cannot_request_csrf_token() {
    let server = MockServer::start(|_| MockResponse::json(200, r#"{"csrf_token":"secret"}"#));
    let test = common::test_client(server.url());
    for path in [
        "/api/csrf",
        "/api/csrf?x=1",
        "/api/csrf/",
        "/api/csrf;foo",
        "/api/csrf%3bfoo",
        "/api/csrf.",
        "/api/csrf%20",
        "/api/CSRF",
        "/api//csrf",
        "/api/%63srf",
    ] {
        let response = test.client.request("GET", path, None);
        assert!(!response.ok, "{path} was accepted");
        assert_eq!(
            response.error.map(|error| error.code),
            Some("invalid_path".to_owned()),
            "{path}"
        );
    }
    assert!(server.requests().is_empty(), "CSRF endpoint was reached");
}

#[test]
fn request_csrf_token_from_body_only() {
    let token = "c".repeat(64);
    let shared = Arc::new(std::sync::Mutex::new((String::new(), String::new())));
    let recorder = Arc::clone(&shared);
    let response_token = token.clone();
    let server = MockServer::start(move |request| {
        if request.path == "/api/csrf" {
            return MockResponse::json(200, &format!(r#"{{"csrf_token":"{response_token}"}}"#));
        }
        let mut recorded = recorder
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        recorded.0 = request.header("x-csrf-token");
        recorded.1 = cookie_value(&request.header("cookie"), "azahar_csrf");
        MockResponse::json(200, r#"{"ok":true}"#)
    });
    let test = common::test_client(server.url());
    let response = test.client.request("POST", "/api/action", None);
    assert!(response.ok, "{:?}", response.error);
    let recorded = shared.lock().unwrap();
    assert_eq!(recorded.0, token);
    assert_eq!(recorded.1, token);
}

#[test]
fn request_csrf_unavailable() {
    let server = MockServer::start(|request| match request.path.as_str() {
        "/api/health" => MockResponse::json(200, "{}"),
        "/api/csrf" => MockResponse::json(200, "{}"),
        _ => MockResponse::json(200, r#"{"ok":true}"#),
    });
    let test = common::test_client(server.url());
    let response = test.client.request("POST", "/api/action", None);
    assert!(!response.ok);
    assert_eq!(response.error.expect("error").code, "csrf_unavailable");
    assert!(
        !server
            .requests()
            .iter()
            .any(|request| request.path == "/api/action"),
        "request was sent without a CSRF token"
    );
}

#[test]
fn request_csrf_retries_only_once() {
    let state = Arc::new(std::sync::Mutex::new((0_usize, 0_usize)));
    let shared = Arc::clone(&state);
    let server = MockServer::start(move |request| {
        let mut state = shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match request.path.as_str() {
            "/api/health" => MockResponse::json(200, "{}"),
            "/api/csrf" => {
                state.0 += 1;
                let token = if state.0 > 1 {
                    SECOND_TOKEN
                } else {
                    FIRST_TOKEN
                };
                MockResponse::json(200, &format!(r#"{{"csrf_token":"{token}"}}"#))
                    .with_set_cookie(format!("azahar_csrf={token}; Path=/"))
            }
            _ => {
                state.1 += 1;
                MockResponse::json(
                    403,
                    r#"{"error":{"code":"csrf_failed","message":"The request could not be verified.","fields":{}}}"#,
                )
            }
        }
    });
    let test = common::test_client(server.url());
    let response = test.client.request("POST", "/api/login", None);
    assert!(!response.ok);
    assert_eq!(response.error.expect("error").code, "csrf_failed");
    let state = state.lock().unwrap();
    assert_eq!(state.1, 2, "POST calls");
    assert_eq!(state.0, 2, "CSRF calls");
}

#[test]
fn request_body_limit() {
    let server = MockServer::start(|_| MockResponse::json(200, "{}"));
    let test = common::test_client(server.url());
    let body = serde_json::json!({"value": "a".repeat(azahar_desktop_api::MAX_REQUEST_BODY_BYTES)});
    let response = test.client.request("POST", "/api/echo", Some(&body));
    assert!(!response.ok);
    assert_eq!(response.error.expect("error").code, "payload_too_large");
    assert!(
        server.requests().is_empty(),
        "oversized request reached the server"
    );
}

#[test]
fn response_body_limit() {
    let server = MockServer::start(|_| {
        MockResponse::new(
            200,
            "application/json",
            vec![b' '; azahar_desktop_api::MAX_RESPONSE_BYTES as usize + 1],
        )
    });
    let test = common::test_client(server.url());
    let response = test.client.request("GET", "/api/huge", None);
    assert!(!response.ok);
    assert_eq!(response.error.expect("error").code, "response_too_large");
}

fn cookie_value(header: &str, name: &str) -> String {
    header
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.to_owned())
        .unwrap_or_default()
}
