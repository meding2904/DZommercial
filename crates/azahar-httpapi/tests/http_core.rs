//! HTTP core tests: health, CSRF, auth routes, middleware and JSON decoding.
//!
//! Every test drives the real router with `tower::ServiceExt::oneshot` against
//! an in-memory libSQL database, so the session rows, migrations and store
//! queries are the production code paths.

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header};
use axum::response::Response;
use azahar_config::Config;
use azahar_courier::{Client, Options};
use azahar_httpapi::extract::BODY_LIMIT;
use azahar_httpapi::session_layer::session_layer;
use azahar_httpapi::session_store::LibsqlSessionStore;
use azahar_httpapi::state::configured_ttl;
use azahar_httpapi::{AppState, build_router};
use azahar_plugins::Registry;
use azahar_store::Store;
use libsql::Builder;
use serde_json::{Value, json};
use tower::ServiceExt;

const ORIGIN: &str = "http://localhost:3000";
const CSRF_TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const PASSWORD: &str = "correct horse battery staple";

struct Fixture {
    app: Router,
}

fn dev_config() -> Config {
    Config {
        app_env: "development".to_string(),
        app_origin: ORIGIN.to_string(),
        cookie_secure: false,
        trust_proxy: false,
        session_ttl: std::time::Duration::from_secs(48 * 60 * 60),
        ..Config::default()
    }
}

async fn fixture_with(cfg: Config) -> Fixture {
    let database = Builder::new_local(":memory:")
        .build()
        .await
        .expect("open in-memory database");
    let store = Arc::new(Store::new(database).expect("store"));
    azahar_db::migrate(store.conn()).await.expect("migrate");
    let ttl = configured_ttl(&cfg);
    let state = AppState::new(
        cfg,
        store.clone(),
        Registry::new(),
        Arc::new(Client::new(Options::default())),
    );
    let sessions = session_layer(
        LibsqlSessionStore::new(store.conn().clone()),
        state.cfg.cookie_secure,
        ttl,
    );
    Fixture {
        app: build_router(state, sessions, axum::Router::new()),
    }
}

async fn fixture() -> Fixture {
    fixture_with(dev_config()).await
}

fn request(method: Method, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .expect("build request")
}

fn json_request(uri: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("build request")
}

fn raw_json_request(uri: &str, content_type: &str, body: impl Into<Body>) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, content_type)
        .body(body.into())
        .expect("build request")
}

fn with_cookies(mut request: Request<Body>, cookies: &[(&str, &str)]) -> Request<Body> {
    let value = cookies
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ");
    request.headers_mut().insert(
        header::COOKIE,
        HeaderValue::from_str(&value).expect("cookie"),
    );
    request
}

/// Adds the three headers CSRF requires; the caller supplies cookies.
fn with_csrf_headers(mut request: Request<Body>, origin: Option<&str>) -> Request<Body> {
    request
        .headers_mut()
        .insert("x-requested-with", HeaderValue::from_static("azahar-web"));
    if let Some(origin) = origin {
        request.headers_mut().insert(
            header::ORIGIN,
            HeaderValue::from_str(origin).expect("origin"),
        );
    }
    request
        .headers_mut()
        .insert("x-csrf-token", HeaderValue::from_static(CSRF_TOKEN));
    request
}

fn signed_json(uri: &str, body: &Value) -> Request<Body> {
    with_cookies(
        with_csrf_headers(json_request(uri, body), Some(ORIGIN)),
        &[("azahar_csrf", CSRF_TOKEN)],
    )
}

async fn send(app: &Router, request: Request<Body>) -> Response {
    app.clone().oneshot(request).await.expect("router response")
}

async fn body_json(response: Response) -> (StatusCode, HeaderMap, Value) {
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 2 * BODY_LIMIT)
        .await
        .expect("read body");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("json body")
    };
    (status, headers, value)
}

fn raw_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find(|raw| raw.starts_with(&format!("{name}=")))
        .map(str::to_owned)
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    raw_cookie(headers, name).map(|raw| {
        raw.strip_prefix(&format!("{name}="))
            .unwrap_or_default()
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_string()
    })
}

async fn register(
    app: &Router,
    name: &str,
    email: &str,
    password: &str,
) -> (StatusCode, HeaderMap, Value) {
    let body = json!({"name": name, "email": email, "password": password});
    let request = signed_json("/api/auth/register", &body);
    body_json(send(app, request).await).await
}

async fn login(app: &Router, email: &str, password: &str) -> (StatusCode, HeaderMap, Value) {
    let body = json!({"email": email, "password": password});
    let request = signed_json("/api/auth/login", &body);
    body_json(send(app, request).await).await
}

#[tokio::test]
async fn health_reports_the_service_and_security_headers() {
    let fixture = fixture().await;
    let (status, headers, body) =
        body_json(send(&fixture.app, request(Method::GET, "/api/health")).await).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["service"], "azahar-api");
    assert_eq!(body["env"], "development");
    assert_eq!(body["origin"], ORIGIN);
    let server_time = body["server_time"].as_str().expect("server_time");
    assert_eq!(server_time.len(), 20, "RFC3339 seconds: {server_time}");
    assert!(server_time.ends_with('Z'), "UTC timestamp: {server_time}");

    assert_eq!(
        headers[header::CONTENT_TYPE],
        "application/json; charset=utf-8"
    );
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert_eq!(headers["x-frame-options"], "DENY");
    assert_eq!(headers["referrer-policy"], "no-referrer");
    assert_eq!(
        headers["permissions-policy"],
        "camera=(), microphone=(), geolocation=()"
    );
    assert!(headers.get(header::STRICT_TRANSPORT_SECURITY).is_none());
    let request_id = headers["x-request-id"].to_str().expect("request id");
    assert_eq!(request_id.len(), 16, "8 random bytes as hex");
}

#[tokio::test]
async fn request_id_is_echoed_or_replaced() {
    let fixture = fixture().await;

    let request = Request::builder()
        .uri("/api/health")
        .header("x-request-id", "client-chosen-id")
        .body(Body::empty())
        .expect("request");
    let (_, headers, _) = body_json(send(&fixture.app, request).await).await;
    assert_eq!(headers["x-request-id"], "client-chosen-id");

    let long = "a".repeat(65);
    let request = Request::builder()
        .uri("/api/health")
        .header("x-request-id", &long)
        .body(Body::empty())
        .expect("request");
    let (_, headers, _) = body_json(send(&fixture.app, request).await).await;
    let request_id = headers["x-request-id"].to_str().expect("request id");
    assert_ne!(request_id, long);
    assert_eq!(request_id.len(), 16);
    assert!(request_id.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

#[tokio::test]
async fn csrf_endpoint_issues_and_reuses_the_cookie() {
    let fixture = fixture().await;
    let (status, headers, body) =
        body_json(send(&fixture.app, request(Method::GET, "/api/csrf")).await).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["expires_in"], 604_800);
    let token = body["csrf_token"].as_str().expect("csrf_token").to_string();
    assert_eq!(token.len(), 64);
    assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));

    let raw = raw_cookie(&headers, "azahar_csrf").expect("csrf cookie");
    assert!(raw.contains("Path=/"), "{raw}");
    assert!(raw.contains("SameSite=Lax"), "{raw}");
    assert!(raw.contains("Max-Age=604800"), "{raw}");
    assert!(raw.contains("Expires="), "{raw}");
    assert!(!raw.contains("HttpOnly"), "the page must read it: {raw}");
    assert!(!raw.contains("Secure"), "not secure in development: {raw}");
    assert_eq!(
        cookie_value(&headers, "azahar_csrf").as_deref(),
        Some(token.as_str())
    );
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");

    let request = Request::builder()
        .uri("/api/csrf")
        .header(header::COOKIE, format!("azahar_csrf={token}"))
        .body(Body::empty())
        .expect("request");
    let (_, _, reused) = body_json(send(&fixture.app, request).await).await;
    assert_eq!(reused["csrf_token"], token, "a valid cookie is reused");

    let request = Request::builder()
        .uri("/api/csrf")
        .header(header::COOKIE, "azahar_csrf=not-a-token")
        .body(Body::empty())
        .expect("request");
    let (_, _, replaced) = body_json(send(&fixture.app, request).await).await;
    assert_ne!(replaced["csrf_token"], "not-a-token");
    assert_eq!(replaced["csrf_token"].as_str().map(str::len), Some(64));
}

#[tokio::test]
async fn register_validation_matrix() {
    let fixture = fixture().await;

    let (status, _, body) = register(&fixture.app, "A", "amina@example.com", PASSWORD).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "validation_error");
    assert_eq!(body["error"]["message"], "Check the marked fields.");
    assert_eq!(
        body["error"]["fields"]["name"],
        "Use between 2 and 80 characters."
    );

    let long_name = "n".repeat(81);
    let (status, _, body) = register(&fixture.app, &long_name, "amina@example.com", PASSWORD).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["error"]["fields"]["name"],
        "Use between 2 and 80 characters."
    );

    let (status, _, body) = register(&fixture.app, "Amina", "not-an-email", PASSWORD).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["fields"]["email"], "Enter a valid email.");

    let (status, _, body) = register(&fixture.app, "Amina", "amina@example.com", "short").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["error"]["fields"]["password"],
        "Use between 10 and 200 characters."
    );

    let too_long = "p".repeat(201);
    let (status, _, body) = register(&fixture.app, "Amina", "amina@example.com", &too_long).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["error"]["fields"]["password"],
        "Use between 10 and 200 characters."
    );

    // Missing fields are zero values in Go, then fail validation.
    let (status, _, body) = register(&fixture.app, "", "", "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"]["fields"]["name"].is_string());
    assert!(body["error"]["fields"]["email"].is_string());
    assert!(body["error"]["fields"]["password"].is_string());

    let (status, _, body) =
        body_json(send(&fixture.app, signed_json("/api/auth/register", &json!({}))).await).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "validation_error");
}

#[tokio::test]
async fn json_extractor_rejects_the_documented_failures() {
    let fixture = fixture().await;
    let valid = json!({"email": "nobody@example.com", "password": "1234567890"});

    let (status, _, body) = body_json(
        send(
            &fixture.app,
            with_cookies(
                with_csrf_headers(
                    raw_json_request("/api/auth/login", "text/plain", valid.to_string()),
                    Some(ORIGIN),
                ),
                &[("azahar_csrf", CSRF_TOKEN)],
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(body["error"]["code"], "unsupported_media_type");
    assert_eq!(
        body["error"]["message"],
        "Send the request body as application/json."
    );

    let huge = format!("{{\"email\":\"{}\"}}", "a".repeat(BODY_LIMIT));
    let (status, _, body) = body_json(
        send(
            &fixture.app,
            with_cookies(
                with_csrf_headers(
                    raw_json_request("/api/auth/login", "application/json", huge),
                    Some(ORIGIN),
                ),
                &[("azahar_csrf", CSRF_TOKEN)],
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["error"]["code"], "payload_too_large");
    assert_eq!(body["error"]["message"], "The request body is too large.");

    let (status, _, body) = body_json(
        send(
            &fixture.app,
            with_cookies(
                with_csrf_headers(
                    raw_json_request("/api/auth/login", "application/json", "  \n "),
                    Some(ORIGIN),
                ),
                &[("azahar_csrf", CSRF_TOKEN)],
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "validation_error");
    assert_eq!(body["error"]["fields"]["body"], "Send a JSON object.");

    let (status, _, body) = body_json(
        send(
            &fixture.app,
            with_cookies(
                with_csrf_headers(
                    raw_json_request("/api/auth/login", "application/json", "{"),
                    Some(ORIGIN),
                ),
                &[("azahar_csrf", CSRF_TOKEN)],
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "invalid_json");
    assert_eq!(
        body["error"]["message"],
        "The request body is not valid JSON."
    );

    let (status, _, body) = body_json(
        send(
            &fixture.app,
            with_cookies(
                with_csrf_headers(
                    raw_json_request("/api/auth/login", "application/json", "[]"),
                    Some(ORIGIN),
                ),
                &[("azahar_csrf", CSRF_TOKEN)],
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "invalid_json");

    let (status, _, body) = body_json(
        send(
            &fixture.app,
            signed_json(
                "/api/auth/login",
                &json!({"email": "a@b.c", "password": "1234567890", "extra": true}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "validation_error");
    assert_eq!(
        body["error"]["message"],
        "The request body contains an unknown field."
    );
    assert!(body["error"].get("fields").is_none());

    let (status, _, body) = body_json(
        send(
            &fixture.app,
            signed_json(
                "/api/auth/login",
                &json!({"email": 5, "password": "1234567890"}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "invalid_json");
    assert_eq!(
        body["error"]["message"],
        "The request body does not match the expected shape."
    );

    let trailing = format!("{valid} {{}}");
    let (status, _, body) = body_json(
        send(
            &fixture.app,
            with_cookies(
                with_csrf_headers(
                    raw_json_request("/api/auth/login", "application/json", trailing),
                    Some(ORIGIN),
                ),
                &[("azahar_csrf", CSRF_TOKEN)],
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "invalid_json");
    assert_eq!(body["error"]["message"], "Send one JSON object.");

    let (status, _, _) = body_json(
        send(
            &fixture.app,
            with_cookies(
                with_csrf_headers(
                    raw_json_request(
                        "/api/auth/login",
                        "application/json",
                        r#"{"email":"first@example.com","email":"nobody@example.com","password":"1234567890"}"#,
                    ),
                    Some(ORIGIN),
                ),
                &[("azahar_csrf", CSRF_TOKEN)],
            ),
        )
        .await,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "Go keeps the last duplicate key instead of rejecting it"
    );

    // A charset parameter is accepted, like mime.ParseMediaType.
    let (status, _, _) = body_json(
        send(
            &fixture.app,
            with_cookies(
                with_csrf_headers(
                    raw_json_request(
                        "/api/auth/login",
                        "application/json; charset=utf-8",
                        valid.to_string(),
                    ),
                    Some(ORIGIN),
                ),
                &[("azahar_csrf", CSRF_TOKEN)],
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "the body decoded");
}

#[tokio::test]
async fn register_rejects_duplicate_email() {
    let fixture = fixture().await;
    let (status, _, _) = register(&fixture.app, "Amina", "amina@example.com", PASSWORD).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) =
        register(&fixture.app, "Amina Again", " Amina@Example.COM ", PASSWORD).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "conflict");
    assert_eq!(
        body["error"]["message"],
        "An account with this email already exists."
    );
    assert_eq!(
        body["error"]["fields"]["email"],
        "This email is already registered."
    );
}

#[tokio::test]
async fn register_signs_in_and_me_returns_the_user() {
    let fixture = fixture().await;
    let (status, headers, body) =
        register(&fixture.app, "  Amina  ", " Amina@Example.COM ", PASSWORD).await;

    assert_eq!(status, StatusCode::CREATED);
    let user = &body["user"];
    let user_id = user["id"].as_str().expect("user id");
    assert!(user_id.starts_with("usr_"));
    assert_eq!(user_id.len(), 28);
    assert_eq!(user["name"], "Amina");
    assert_eq!(user["email"], "amina@example.com");
    assert_eq!(user["role"], "customer");

    let raw = raw_cookie(&headers, "azahar_session").expect("session cookie");
    assert!(raw.contains("HttpOnly"), "{raw}");
    assert!(raw.contains("SameSite=Lax"), "{raw}");
    assert!(raw.contains("Path=/"), "{raw}");
    assert!(!raw.contains("Secure"), "not secure in development: {raw}");
    let max_age: i64 = raw
        .split("Max-Age=")
        .nth(1)
        .and_then(|rest| rest.split(';').next())
        .expect("Max-Age")
        .parse()
        .expect("numeric Max-Age");
    assert!(
        (172_700..=172_800).contains(&max_age),
        "48 hour TTL, got {max_age}: {raw}"
    );
    let session = cookie_value(&headers, "azahar_session").expect("session value");

    let me = with_cookies(
        request(Method::GET, "/api/auth/me"),
        &[("azahar_session", &session)],
    );
    let (status, _, body) = body_json(send(&fixture.app, me).await).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["user"]["id"], user_id);
    assert_eq!(body["user"]["email"], "amina@example.com");

    let (status, _, body) =
        body_json(send(&fixture.app, request(Method::GET, "/api/auth/me")).await).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthorized");
    assert_eq!(body["error"]["message"], "Sign in to continue.");

    let unknown = with_cookies(
        request(Method::GET, "/api/auth/me"),
        &[("azahar_session", "not-a-real-session")],
    );
    let (status, headers, _) = body_json(send(&fixture.app, unknown).await).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let removal = raw_cookie(&headers, "azahar_session");
    assert!(
        removal.is_some_and(|raw| raw.contains("Max-Age=0")),
        "a session cookie that names no row is cleared"
    );
}

#[tokio::test]
async fn login_wrong_password_and_unknown_email_are_generic() {
    let fixture = fixture().await;
    register(&fixture.app, "Amina", "amina@example.com", PASSWORD).await;

    let (status, _, body) = login(&fixture.app, "amina@example.com", "wrong password").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthorized");
    assert_eq!(body["error"]["message"], "Email or password is incorrect.");

    let (status, _, body) = login(&fixture.app, "nobody@example.com", PASSWORD).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthorized");
    assert_eq!(body["error"]["message"], "Email or password is incorrect.");

    let (status, _, body) = login(&fixture.app, "", "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["fields"]["email"], "Enter a valid email.");
    assert_eq!(body["error"]["fields"]["password"], "Enter your password.");
}

#[tokio::test]
async fn login_me_logout_flow() {
    let fixture = fixture().await;
    let (registered, _, _) = register(&fixture.app, "Amina", "amina@example.com", PASSWORD).await;
    assert_eq!(registered, StatusCode::CREATED);

    let (status, headers, body) = login(&fixture.app, " AMINA@example.com ", PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["user"]["email"], "amina@example.com");
    let session = cookie_value(&headers, "azahar_session").expect("session cookie");

    let me = with_cookies(
        request(Method::GET, "/api/auth/me"),
        &[("azahar_session", &session)],
    );
    let (status, _, _) = body_json(send(&fixture.app, me).await).await;
    assert_eq!(status, StatusCode::OK);

    let logout = with_cookies(
        with_csrf_headers(json_request("/api/auth/logout", &json!({})), Some(ORIGIN)),
        &[("azahar_csrf", CSRF_TOKEN), ("azahar_session", &session)],
    );
    let (status, headers, body) = body_json(send(&fixture.app, logout).await).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null, "204 has no body");
    let removal = raw_cookie(&headers, "azahar_session").expect("cookie removal header");
    assert!(removal.starts_with("azahar_session=;"), "{removal}");
    assert!(removal.contains("Max-Age=0"), "{removal}");

    let me = with_cookies(
        request(Method::GET, "/api/auth/me"),
        &[("azahar_session", &session)],
    );
    let (status, _, body) = body_json(send(&fixture.app, me).await).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["message"], "Sign in to continue.");
}

#[tokio::test]
async fn csrf_rejects_missing_header_wrong_token_and_bad_origin() {
    let fixture = fixture().await;
    let body = json!({"email": "amina@example.com", "password": "1234567890"});

    let mut missing_header = json_request("/api/auth/login", &body);
    missing_header
        .headers_mut()
        .insert(header::ORIGIN, HeaderValue::from_static(ORIGIN));
    missing_header
        .headers_mut()
        .insert("x-csrf-token", HeaderValue::from_static(CSRF_TOKEN));
    let missing_header = with_cookies(missing_header, &[("azahar_csrf", CSRF_TOKEN)]);
    let (status, _, body) = body_json(send(&fixture.app, missing_header).await).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "csrf_failed");
    assert_eq!(
        body["error"]["message"],
        "The request could not be verified."
    );

    let mut wrong_token = with_csrf_headers(json_request("/api/auth/login", &body), Some(ORIGIN));
    wrong_token.headers_mut().insert(
        "x-csrf-token",
        HeaderValue::from_static(
            "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210",
        ),
    );
    let wrong_token = with_cookies(wrong_token, &[("azahar_csrf", CSRF_TOKEN)]);
    let (status, _, body) = body_json(send(&fixture.app, wrong_token).await).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "csrf_failed");

    let bad_origin = with_cookies(
        with_csrf_headers(
            json_request("/api/auth/login", &body),
            Some("https://evil.example"),
        ),
        &[("azahar_csrf", CSRF_TOKEN)],
    );
    let (status, _, body) = body_json(send(&fixture.app, bad_origin).await).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "csrf_failed");

    let missing_cookie = with_csrf_headers(json_request("/api/auth/login", &body), Some(ORIGIN));
    let (status, _, _) = body_json(send(&fixture.app, missing_cookie).await).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _, _) =
        body_json(send(&fixture.app, request(Method::GET, "/api/health")).await).await;
    assert_eq!(status, StatusCode::OK, "GET bypasses CSRF");
}

#[tokio::test]
async fn csrf_allows_development_origins_and_requires_origin_in_production() {
    let dev = fixture().await;
    let body = json!({"email": "nobody@example.com", "password": "1234567890"});

    let local = with_cookies(
        with_csrf_headers(
            json_request("/api/auth/login", &body),
            Some("http://127.0.0.1:3000"),
        ),
        &[("azahar_csrf", CSRF_TOKEN)],
    );
    let (status, _, _) = body_json(send(&dev.app, local).await).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "local origin passes CSRF");

    let kiri = with_cookies(
        with_csrf_headers(
            json_request("/api/auth/login", &body),
            Some("http://azahar-web.kiri.localhost"),
        ),
        &[("azahar_csrf", CSRF_TOKEN)],
    );
    let (status, _, _) = body_json(send(&dev.app, kiri).await).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let no_origin = with_cookies(
        with_csrf_headers(json_request("/api/auth/login", &body), None),
        &[("azahar_csrf", CSRF_TOKEN)],
    );
    let (status, _, _) = body_json(send(&dev.app, no_origin).await).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "development allows a missing Origin"
    );

    let mut production = dev_config();
    production.app_env = "production".to_string();
    production.app_origin = "https://shop.example".to_string();
    production.cookie_secure = true;
    let prod = fixture_with(production).await;

    let no_origin = with_cookies(
        with_csrf_headers(json_request("/api/auth/login", &body), None),
        &[("azahar_csrf", CSRF_TOKEN)],
    );
    let (status, _, _) = body_json(send(&prod.app, no_origin).await).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let local = with_cookies(
        with_csrf_headers(
            json_request("/api/auth/login", &body),
            Some("http://localhost:3000"),
        ),
        &[("azahar_csrf", CSRF_TOKEN)],
    );
    let (status, _, _) = body_json(send(&prod.app, local).await).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "production is exact-origin");

    let exact = with_cookies(
        with_csrf_headers(
            json_request("/api/auth/login", &body),
            Some("https://shop.example"),
        ),
        &[("azahar_csrf", CSRF_TOKEN)],
    );
    let (status, _, _) = body_json(send(&prod.app, exact).await).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "CSRF passed, login failed"
    );
}

#[tokio::test]
async fn auth_rate_limit_is_keyed_by_forwarded_address() {
    let mut config = dev_config();
    config.trust_proxy = true;
    let fixture = fixture_with(config).await;
    let body = json!({});

    for index in 0..10 {
        let mut request = signed_json("/api/auth/login", &body);
        request.headers_mut().insert(
            "x-forwarded-for",
            HeaderValue::from_str(&format!("198.51.100.{index}")).expect("ip"),
        );
        let (status, _, _) = body_json(send(&fixture.app, request).await).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "distinct address {index} keeps its own bucket"
        );
    }

    for attempt in 0..10 {
        let mut request = signed_json("/api/auth/login", &body);
        request
            .headers_mut()
            .insert("x-forwarded-for", HeaderValue::from_static("203.0.113.9"));
        let (status, _, _) = body_json(send(&fixture.app, request).await).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "attempt {attempt} is inside the window"
        );
    }

    let mut limited = signed_json("/api/auth/login", &body);
    limited
        .headers_mut()
        .insert("x-forwarded-for", HeaderValue::from_static("203.0.113.9"));
    let (status, headers, body) = body_json(send(&fixture.app, limited).await).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["code"], "rate_limited");
    assert_eq!(
        body["error"]["message"],
        "Too many requests. Try again later."
    );
    let retry_after: u64 = headers[header::RETRY_AFTER]
        .to_str()
        .expect("Retry-After")
        .parse()
        .expect("numeric Retry-After");
    assert!(retry_after >= 1, "Retry-After must be at least one second");
}

#[tokio::test]
async fn rate_limit_ignores_forwarded_for_without_trust_proxy() {
    let fixture = fixture_with(dev_config()).await;
    let body = json!({});

    for index in 0..10 {
        let mut request = signed_json("/api/auth/login", &body);
        request.headers_mut().insert(
            "x-forwarded-for",
            HeaderValue::from_str(&format!("198.51.100.{index}")).expect("ip"),
        );
        let (status, _, _) = body_json(send(&fixture.app, request).await).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    let mut limited = signed_json("/api/auth/login", &body);
    limited
        .headers_mut()
        .insert("x-forwarded-for", HeaderValue::from_static("192.0.2.77"));
    let (status, _, _) = body_json(send(&fixture.app, limited).await).await;
    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "without TRUST_PROXY every request shares the peer bucket"
    );
}

#[tokio::test]
async fn production_sets_hsts_and_secure_cookies() {
    let mut config = dev_config();
    config.app_env = "production".to_string();
    config.app_origin = "https://shop.example".to_string();
    config.cookie_secure = true;
    let fixture = fixture_with(config).await;

    let (status, headers, _) =
        body_json(send(&fixture.app, request(Method::GET, "/api/health")).await).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::STRICT_TRANSPORT_SECURITY],
        "max-age=31536000; includeSubDomains"
    );

    let (_, headers, _) =
        body_json(send(&fixture.app, request(Method::GET, "/api/csrf")).await).await;
    let raw = raw_cookie(&headers, "azahar_csrf").expect("csrf cookie");
    assert!(raw.contains("Secure"), "{raw}");
}

#[tokio::test]
async fn unknown_route_and_wrong_method_return_json_envelopes() {
    let fixture = fixture().await;

    let (status, headers, body) =
        body_json(send(&fixture.app, request(Method::GET, "/api/missing")).await).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "application/json; charset=utf-8"
    );
    assert_eq!(body["error"]["code"], "not_found");
    assert_eq!(body["error"]["message"], "Not found.");

    let (status, _, body) =
        body_json(send(&fixture.app, signed_json("/api/health", &json!({}))).await).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(body["error"]["code"], "method_not_allowed");
    assert_eq!(body["error"]["message"], "Method not allowed.");
}
