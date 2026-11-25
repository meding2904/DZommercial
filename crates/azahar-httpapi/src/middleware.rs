//! HTTP middleware: security headers, request ids, rate limits, CSRF and the
//! authentication guards.
//!
//! The Go chain in `router.go` runs `securityHeaders`, `withRequestID`,
//! `limitBody`, the general rate limiter and `enforceCSRF` before any route, so
//! `build_router` applies these in the same order around the session layer.

use std::net::SocketAddr;
use std::time::Instant;

use axum::extract::{ConnectInfo, FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use azahar_store::{StoreError, User};
use subtle::ConstantTimeEq;
use tower_sessions::Session;
use tower_sessions::cookie::Cookie;
use url::Url;

use crate::extract::client_ip;
use crate::respond::{ApiProblem, problem};
use crate::session_layer::{SESSION_COOKIE_NAME, USER_ID_KEY};
use crate::state::AppState;

pub const REQUEST_ID_HEADER: &str = "x-request-id";
pub const REQUEST_ID_MAX_LENGTH: usize = 64;

pub const CSRF_COOKIE_NAME: &str = "azahar_csrf";
pub const CSRF_HEADER_NAME: &str = "x-csrf-token";
pub const CSRF_TOKEN_MAX_LENGTH: usize = 128;
pub const WEB_REQUEST_HEADER: &str = "x-requested-with";
pub const WEB_REQUEST_VALUE: &str = "azahar-web";

/// The generated (or accepted) request id, readable by handlers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestId(pub String);

impl<S> FromRequestParts<S> for RequestId
where
    S: Send + Sync,
{
    type Rejection = ApiProblem;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<RequestId>()
            .cloned()
            .ok_or_else(internal_error)
    }
}

/// The signed-in user attached to the request by [`require_user`].
#[derive(Clone, Debug)]
pub struct CurrentUser(pub User);

impl CurrentUser {
    #[must_use]
    pub fn is_admin(&self) -> bool {
        self.0.role == "admin"
    }

    #[must_use]
    pub fn is_staff(&self) -> bool {
        self.0.role == "admin" || self.0.role == "employee"
    }
}

impl<S> FromRequestParts<S> for CurrentUser
where
    S: Send + Sync,
{
    type Rejection = ApiProblem;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<CurrentUser>()
            .cloned()
            .ok_or_else(unauthorized)
    }
}

/// 401 `unauthorized`: the exact message of the Go guard.
#[must_use]
pub fn unauthorized() -> ApiProblem {
    problem(401, "unauthorized", "Sign in to continue.")
}

/// 403 `forbidden`: used when a signed-in account lacks a role.
#[must_use]
pub fn forbidden() -> ApiProblem {
    problem(403, "forbidden", "This account does not have access.")
}

/// 500 `internal_error`: the shared failure text.
#[must_use]
pub fn internal_error() -> ApiProblem {
    problem(500, "internal_error", "Something went wrong.")
}

fn csrf_failed() -> ApiProblem {
    problem(403, "csrf_failed", "The request could not be verified.")
}

/// The Go security header set, plus HSTS in production.
///
/// Headers are only added when the handler has not set them. Go writes these
/// before the handler runs, so a handler such as the image route can replace
/// `Cache-Control` with its own long-lived value.
pub async fn security_headers(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    let defaults = [
        (
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ),
        (header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")),
        (
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ),
        (
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
        ),
        (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
    ];
    for (name, value) in defaults {
        headers.entry(name).or_insert(value);
    }
    if state.cfg.is_production() {
        headers
            .entry(header::STRICT_TRANSPORT_SECURITY)
            .or_insert(HeaderValue::from_static(
                "max-age=31536000; includeSubDomains",
            ));
    }
    response
}

/// Accepts an `X-Request-Id` of up to 64 bytes, otherwise mints 8 random bytes
/// as lowercase hex, and echoes the result to the client.
pub async fn request_id(mut request: Request, next: Next) -> Response {
    let id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= REQUEST_ID_MAX_LENGTH)
        .map(ToOwned::to_owned)
        .unwrap_or_else(new_request_id);
    request.extensions_mut().insert(RequestId(id.clone()));

    let mut response = next.run(request).await;
    if let Ok(value) = HeaderValue::from_str(&id) {
        response.headers_mut().insert(REQUEST_ID_HEADER, value);
    }
    response
}

fn new_request_id() -> String {
    let mut bytes = [0u8; 8];
    match getrandom::fill(&mut bytes) {
        Ok(()) => hex::encode(bytes),
        Err(_) => "request".to_string(),
    }
}

/// The API-wide limiter: 120 requests a minute per client address.
pub async fn general_rate_limit(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    match check_limit(&state, &state.general_limiter, &request) {
        Some(response) => response,
        None => next.run(request).await,
    }
}

/// The stricter limiter mounted on `/api/auth/register` and `/api/auth/login`.
pub async fn auth_rate_limit(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    match check_limit(&state, &state.auth_limiter, &request) {
        Some(response) => response,
        None => next.run(request).await,
    }
}

/// Go's `adminMutationLimit`: every unsafe admin request shares one bucket.
pub async fn admin_mutation_limit(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if matches!(*request.method(), Method::GET | Method::HEAD) {
        return next.run(request).await;
    }
    match check_limit(&state, &state.admin_limiter, &request) {
        Some(response) => response,
        None => next.run(request).await,
    }
}

fn check_limit(
    state: &AppState,
    limiter: &crate::ratelimit::RateLimiter,
    request: &Request,
) -> Option<Response> {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0);
    let ip = client_ip(request.headers(), peer, state.cfg.trust_proxy);
    let (allowed, wait) = limiter.allow(&ip, Instant::now());
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

/// Double-submit CSRF protection, byte-for-byte the Go rules.
pub async fn csrf(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        return next.run(request).await;
    }

    let headers = request.headers();
    if headers
        .get(WEB_REQUEST_HEADER)
        .and_then(|value| value.to_str().ok())
        != Some(WEB_REQUEST_VALUE)
    {
        return csrf_failed().into_response();
    }

    match headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    {
        Some(origin) => {
            if origin != state.cfg.app_origin
                && !(state.cfg.is_development() && is_local_development_origin(origin))
            {
                return csrf_failed().into_response();
            }
        }
        None if !state.cfg.is_development() => return csrf_failed().into_response(),
        None => {}
    }

    let cookie = cookie_value(headers, CSRF_COOKIE_NAME);
    let token = headers
        .get(CSRF_HEADER_NAME)
        .and_then(|value| value.to_str().ok());
    let valid = match (cookie, token) {
        (Some(cookie), Some(token)) => {
            !cookie.is_empty()
                && !token.is_empty()
                && cookie.len() <= CSRF_TOKEN_MAX_LENGTH
                && token.len() <= CSRF_TOKEN_MAX_LENGTH
                && constant_time_eq(cookie.as_bytes(), token.as_bytes())
        }
        _ => false,
    };
    if !valid {
        return csrf_failed().into_response();
    }

    next.run(request).await
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len() && bool::from(left.ct_eq(right))
}

/// Reads one cookie value from every `Cookie` header, first match wins.
#[must_use]
pub fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    for raw in headers.get_all(header::COOKIE) {
        let Ok(raw) = raw.to_str() else { continue };
        let found = Cookie::split_parse(raw)
            .filter_map(Result::ok)
            .find(|cookie| cookie.name() == name);
        if let Some(cookie) = found {
            return Some(cookie.value().to_string());
        }
    }
    None
}

/// The development-only origin allowance: loopback names and Kiri ingress
/// hosts (`<name>.kiri.localhost`) are accepted in addition to `APP_ORIGIN`.
#[must_use]
pub fn is_local_development_origin(origin: &str) -> bool {
    let Ok(parsed) = Url::parse(origin) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = parsed.host() else {
        return false;
    };
    let host = match host {
        url::Host::Domain(domain) => domain.to_ascii_lowercase(),
        url::Host::Ipv4(address) => address.to_string(),
        url::Host::Ipv6(address) => address.to_string(),
    };
    host == "localhost" || host == "127.0.0.1" || host == "::1" || host.ends_with(".localhost")
}

/// Loads the session's user and rejects the request when there is none.
pub async fn require_user(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(session) = request.extensions().get::<Session>().cloned() else {
        return unauthorized().into_response();
    };
    let user_id = match session.get::<String>(USER_ID_KEY).await {
        Ok(Some(user_id)) if !user_id.is_empty() => user_id,
        Ok(_) => {
            // A cookie that no longer maps to a session (expired, deleted or
            // logged out) gets the same removal header the Go guard sent.
            if cookie_value(request.headers(), SESSION_COOKIE_NAME).is_some() {
                let _ = session.flush().await;
            }
            return unauthorized().into_response();
        }
        Err(_) => return internal_error().into_response(),
    };

    match state.store.user_by_id(&user_id).await {
        Ok(user) => {
            request.extensions_mut().insert(CurrentUser(user));
            next.run(request).await
        }
        Err(StoreError::NotFound) => {
            // The session outlived the account. Drop the row and the cookie.
            let _ = session.flush().await;
            unauthorized().into_response()
        }
        Err(_) => internal_error().into_response(),
    }
}

/// Allows admins and employees; each route below this decides whether it also
/// needs [`require_admin`].
pub async fn require_staff(request: Request, next: Next) -> Response {
    let Some(current) = request.extensions().get::<CurrentUser>() else {
        return unauthorized().into_response();
    };
    if !current.is_staff() {
        return forbidden().into_response();
    }
    next.run(request).await
}

/// Allows only the `admin` role.
pub async fn require_admin(request: Request, next: Next) -> Response {
    let Some(current) = request.extensions().get::<CurrentUser>() else {
        return unauthorized().into_response();
    };
    if !current.is_admin() {
        return forbidden().into_response();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    #[test]
    fn local_origins_match_the_go_allowance() {
        for origin in [
            "http://localhost:3000",
            "http://127.0.0.1:3000",
            "http://[::1]:3000",
            "https://azahar-web.kiri.localhost",
        ] {
            assert!(is_local_development_origin(origin), "{origin}");
        }
        for origin in [
            "https://evil.example",
            "ftp://localhost",
            "http://notlocalhost",
            "http://localhost.evil.example",
            "localhost:3000",
        ] {
            assert!(!is_local_development_origin(origin), "{origin}");
        }
    }

    #[test]
    fn cookie_lookup_reads_every_cookie_header() {
        let mut headers = HeaderMap::new();
        headers.append(header::COOKIE, "first=1".parse().unwrap());
        headers.append(header::COOKIE, "azahar_csrf=token; last=2".parse().unwrap());
        assert_eq!(
            cookie_value(&headers, CSRF_COOKIE_NAME).as_deref(),
            Some("token")
        );
    }

    #[test]
    fn constant_time_comparison_rejects_length_changes() {
        assert!(constant_time_eq(b"token", b"token"));
        assert!(!constant_time_eq(b"token", b"tokens"));
        assert!(!constant_time_eq(b"token", b"tokeN"));
    }

    #[test]
    fn status_helpers_match_the_go_messages() {
        assert_eq!(unauthorized().status, StatusCode::UNAUTHORIZED);
        assert_eq!(unauthorized().message, "Sign in to continue.");
        assert_eq!(forbidden().status, StatusCode::FORBIDDEN);
        assert_eq!(forbidden().message, "This account does not have access.");
    }
}

/// Access log in the same fields as the Go `logRequests` middleware, with
/// order tokens redacted. The request id is attached by `request_id`.
pub async fn log_requests(request: Request, next: Next) -> Response {
    let started = std::time::Instant::now();
    let method = request.method().clone();
    let path = redacted_path(request.uri().path());
    let request_id = request
        .extensions()
        .get::<RequestId>()
        .map(|id| id.0.clone())
        .unwrap_or_default();
    let response = next.run(request).await;
    tracing::info!(
        method = %method,
        path = %path,
        status = response.status().as_u16(),
        duration_ms = started.elapsed().as_millis() as u64,
        request_id = %request_id,
        "http request"
    );
    response
}

/// Order tokens are bearer secrets, so the access log never contains them.
pub fn redacted_path(path: &str) -> String {
    if path.starts_with("/api/orders/") && path.len() > "/api/orders/".len() {
        return "/api/orders/[redacted]".to_string();
    }
    path.to_string()
}

#[cfg(test)]
mod log_tests {
    use super::redacted_path;

    #[test]
    fn order_tokens_are_redacted() {
        assert_eq!(
            redacted_path("/api/orders/abc123"),
            "/api/orders/[redacted]"
        );
        assert_eq!(redacted_path("/api/orders"), "/api/orders");
        assert_eq!(redacted_path("/api/products"), "/api/products");
    }
}

/// chi registers only the methods a route asks for; `HEAD` on a GET route is a
/// 405. Axum routes HEAD to GET automatically, so this restores the Go answer.
pub async fn reject_head(request: Request, next: Next) -> Response {
    use axum::response::IntoResponse;
    if request.method() == axum::http::Method::HEAD {
        return crate::respond::problem(405, "method_not_allowed", "Method not allowed.")
            .into_response();
    }
    next.run(request).await
}
