//! Public client bootstrap routes: health and the CSRF token.

use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::Response;
use chrono::{SecondsFormat, Utc};
use serde_json::json;
use time::OffsetDateTime;
use tower_sessions::cookie::{Cookie, SameSite};

use crate::middleware::{CSRF_COOKIE_NAME, cookie_value, internal_error};
use crate::respond::{ApiProblem, json_response};
use crate::state::AppState;

const CSRF_TOKEN_BYTES: usize = 32;
const CSRF_TOKEN_LENGTH: usize = CSRF_TOKEN_BYTES * 2;
const CSRF_COOKIE_AGE_SECS: i64 = 7 * 24 * 60 * 60;

/// `GET /api/health`: a probe that never touches the database.
pub async fn health(State(state): State<AppState>) -> Response {
    json_response(
        200,
        &json!({
            "status": "ok",
            "service": "azahar-api",
            "env": state.cfg.app_env,
            "origin": state.cfg.app_origin,
            "server_time": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        }),
    )
}

/// `GET /api/csrf`: issues or reuses the double-submit token.
pub async fn csrf(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiProblem> {
    let token =
        match cookie_value(&headers, CSRF_COOKIE_NAME).filter(|value| valid_csrf_token(value)) {
            Some(existing) => existing,
            None => new_csrf_token()?,
        };

    let cookie = Cookie::build((CSRF_COOKIE_NAME, token.clone()))
        .path("/")
        .http_only(false)
        .secure(state.cfg.cookie_secure)
        .same_site(SameSite::Lax)
        .expires(OffsetDateTime::now_utc() + time::Duration::days(7))
        .max_age(time::Duration::days(7))
        .build();

    let mut response = json_response(
        200,
        &json!({
            "csrf_token": token,
            "expires_in": CSRF_COOKIE_AGE_SECS,
        }),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(value) = HeaderValue::from_str(&cookie.to_string()) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    Ok(response)
}

fn new_csrf_token() -> Result<String, ApiProblem> {
    let mut bytes = [0u8; CSRF_TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|_| internal_error())?;
    Ok(hex::encode(bytes))
}

/// Go `validCSRFToken`: exactly 64 hexadecimal characters.
#[must_use]
pub fn valid_csrf_token(value: &str) -> bool {
    value.len() == CSRF_TOKEN_LENGTH && hex::decode(value).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csrf_tokens_are_64_hex_characters() {
        let token = new_csrf_token().expect("token");
        assert_eq!(token.len(), 64);
        assert!(valid_csrf_token(&token));
        assert!(valid_csrf_token(&token.to_uppercase()));
        assert!(!valid_csrf_token("not-a-token"));
        assert!(!valid_csrf_token(&token[..63]));
        assert!(!valid_csrf_token(&format!("{token}0")));
    }
}
