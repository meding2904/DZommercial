#[allow(dead_code)]
mod common;
#[allow(dead_code)]
mod support;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use azahar_desktop_api::{Environment, FALLBACK_ORIGIN, Settings};
use support::{MockResponse, MockServer};

#[test]
fn health_resolves_origin() {
    let origins = Arc::new(Mutex::new(HashMap::<String, String>::new()));
    let recorded = Arc::clone(&origins);
    let server = MockServer::start(move |request| {
        recorded
            .lock()
            .unwrap()
            .insert(request.path.clone(), request.header("origin"));
        if request.path == "/api/health" {
            return MockResponse::json(
                200,
                r#"{"status":"ok","env":"test","origin":"http://localhost:3000","server_time":"2026-11-21T10:46:28Z"}"#,
            );
        }
        MockResponse::json(200, r#"{"ok":true}"#)
    });
    let test = common::test_client(server.url());
    let health = test.client.health();
    assert!(health.ok);
    assert_eq!(health.status, 200);
    assert_eq!(health.env, "test");
    assert_eq!(health.origin, "http://localhost:3000");
    assert_eq!(health.server_time, "2026-11-21T10:46:28Z");
    assert_eq!(health.message, "");
    assert_eq!(test.client.info().resolved_origin, "http://localhost:3000");

    assert!(test.client.request("GET", "/api/check", None).ok);
    let origins = origins.lock().unwrap();
    assert_eq!(origins["/api/health"], "");
    assert_eq!(origins["/api/check"], "http://localhost:3000");
}

#[test]
fn health_bad_status() {
    let server = MockServer::start(|_| {
        MockResponse::json(
            503,
            r#"{"error":{"code":"unavailable","message":"Try later.","fields":{}}}"#,
        )
    });
    let test = common::test_client(server.url());
    let health = test.client.health();
    assert!(!health.ok);
    assert_eq!(health.status, 503);
    assert!(!health.message.is_empty());
    assert_eq!(test.client.info().resolved_origin, "");
}

#[test]
fn health_network_failure() {
    let transport = common::FailingTransport::new();
    let test = common::client_with(
        "http://127.0.0.1:1",
        Environment::default().origin(""),
        Some(transport),
    );
    let health = test.client.health();
    assert!(!health.ok);
    assert_eq!(health.status, 0);
    assert!(!health.message.is_empty());
}

#[test]
fn health_timeout() {
    let transport = common::FailingTransport::timed_out();
    let test = common::client_with(
        "http://127.0.0.1:1",
        Environment::default().origin(""),
        Some(transport),
    );
    let health = test.client.health();
    assert!(!health.ok);
    assert_eq!(health.status, 0);
    assert!(health.message.contains("in time"), "{}", health.message);
}

#[test]
fn unsafe_request_resolves_origin_from_health() {
    let origins = Arc::new(Mutex::new(HashMap::<String, String>::new()));
    let recorded = Arc::clone(&origins);
    let server = MockServer::start(move |request| {
        recorded
            .lock()
            .unwrap()
            .insert(request.path.clone(), request.header("origin"));
        match request.path.as_str() {
            "/api/health" => {
                MockResponse::json(200, r#"{"env":"test","origin":"http://reported.example"}"#)
            }
            "/api/csrf" => MockResponse::json(200, r#"{"csrf_token":"csrf-token"}"#)
                .with_set_cookie("azahar_csrf=csrf-token; Path=/"),
            "/api/action" => MockResponse::json(200, r#"{"ok":true}"#),
            _ => MockResponse::json(404, "{}"),
        }
    });
    let test = common::test_client(server.url());
    let response = test.client.request("POST", "/api/action", None);
    assert!(response.ok, "{:?}", response.error);

    let origins = origins.lock().unwrap();
    assert_eq!(origins["/api/health"], "");
    assert_eq!(origins["/api/csrf"], "http://reported.example");
    assert_eq!(origins["/api/action"], "http://reported.example");
}

#[test]
fn unsafe_request_falls_back_to_local_origin_for_loopback() {
    let origins = Arc::new(Mutex::new(HashMap::<String, String>::new()));
    let recorded = Arc::clone(&origins);
    let server = MockServer::start(move |request| {
        recorded
            .lock()
            .unwrap()
            .insert(request.path.clone(), request.header("origin"));
        match request.path.as_str() {
            "/api/health" => MockResponse::json(
                503,
                r#"{"error":{"code":"unavailable","message":"Try later.","fields":{}}}"#,
            ),
            "/api/csrf" => MockResponse::json(200, r#"{"csrf_token":"csrf-token"}"#)
                .with_set_cookie("azahar_csrf=csrf-token; Path=/"),
            "/api/action" => MockResponse::json(200, r#"{"ok":true}"#),
            _ => MockResponse::json(404, "{}"),
        }
    });
    let test = common::test_client(server.url());
    assert!(test.client.request("POST", "/api/action", None).ok);
    let origins = origins.lock().unwrap();
    for path in ["/api/csrf", "/api/action"] {
        assert_eq!(origins[path], FALLBACK_ORIGIN, "{path}");
    }
}

#[test]
fn unsafe_request_fails_closed_for_remote_without_origin() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    common::write_settings(
        &profile,
        &Settings {
            api_url: "https://remote.example".to_owned(),
            network_allowed: true,
            network_allowed_host: "remote.example".to_owned(),
            ..Settings::default()
        },
    );
    let transport = common::FailingTransport::new();
    let client = azahar_desktop_api::ClientOptions::new("2.0.0-test", &profile)
        .environment(Environment::default())
        .transport(transport.clone());
    let client = azahar_desktop_api::Client::with_options(client).unwrap();
    let response = client.request("POST", "/api/action", None);
    assert!(!response.ok);
    let error = response.error.expect("error");
    assert_eq!(error.code, "origin_unavailable");
    assert!(error.message.contains("Connection"), "{}", error.message);

    let requests = transport.requests();
    assert_eq!(requests.len(), 1, "only the health probe should run");
    assert!(
        requests[0].url.ends_with("/api/health"),
        "{}",
        requests[0].url
    );
}

#[test]
fn health_uses_configured_origin_for_display() {
    let origin_header = Arc::new(Mutex::new(String::new()));
    let recorded = Arc::clone(&origin_header);
    let server = MockServer::start(move |request| {
        *recorded.lock().unwrap() = request.header("origin");
        MockResponse::json(200, r#"{"origin":"http://server.example"}"#)
    });
    let test = common::test_client_with_origin(server.url(), "http://admin.example");
    let health = test.client.health();
    assert!(health.ok);
    assert_eq!(health.origin, "http://admin.example");
    assert_eq!(*origin_header.lock().unwrap(), "");
}

#[test]
fn unsafe_request_uses_configured_origin_without_health() {
    let server = MockServer::start(|request| match request.path.as_str() {
        "/api/action" => MockResponse::json(200, r#"{"ok":true}"#),
        _ => MockResponse::json(200, r#"{"ok":true}"#),
    });
    let test = common::test_client_with_origin(server.url(), "http://admin.example");
    test.client.seed_csrf("csrf-token");
    let response = test.client.request("POST", "/api/action", None);
    assert!(response.ok, "{:?}", response.error);
    let requests = server.requests();
    assert!(
        !requests.iter().any(|request| request.path == "/api/health"),
        "configured origin triggered a health request"
    );
    let action = requests
        .iter()
        .find(|request| request.path == "/api/action")
        .expect("action request");
    assert_eq!(action.header("origin"), "http://admin.example");
}

#[test]
fn health_body_is_decoded_leniently() {
    let server = MockServer::start(|_| {
        MockResponse::json(
            200,
            r#"{"origin":123,"env":"test","server_time":"2026-11-21T10:46:28Z"}"#,
        )
    });
    let test = common::test_client(server.url());
    let health = test.client.health();
    assert!(health.ok);
    assert_eq!(health.env, "test");
    assert_eq!(health.server_time, "2026-11-21T10:46:28Z");
    // The health origin is only resolved when it is a valid string.
    assert_eq!(test.client.info().resolved_origin, "");
}
