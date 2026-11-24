//! The three admin routes and their request decoding.
//!
//! Port of the handler half of `codemode.go`: the same guards, the same
//! `plugins.DecodeBody` contract (415/413/422 messages) and the same JSON
//! envelopes. Route handlers resolve the session user themselves so the builder
//! needs no state value; the router assembly can merge the result directly.

use std::collections::HashMap;
use std::time::Instant;

use axum::Router;
use axum::body::to_bytes;
use axum::extract::{Query, Request, State};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use azahar_httpapi::AppState;
use azahar_httpapi::extract::BODY_LIMIT;
use azahar_httpapi::handlers_checkout::ClientIp;
use azahar_httpapi::middleware::{forbidden, internal_error, unauthorized};
use azahar_httpapi::respond::{ApiProblem, json_response, problem, problem_fields};
use azahar_httpapi::session_layer::SessionUser;
use azahar_plugin_phone::PhoneHooks;
use azahar_store::{StoreError, User};
use chrono::Utc;
use serde::Deserialize;
use serde_json::Value;

use crate::PLUGIN_ID;
use crate::json;
use crate::service::{ApplyInput, Codemode, PreviewInput};

/// The mounted prefixes, kept next to the handlers so the documentation and
/// tests use one source.
pub const PLANS_PATH: &str = "/api/plugins/codemode/admin/plans";
pub const PREVIEW_PATH: &str = "/api/plugins/codemode/admin/preview";
pub const APPLY_PATH: &str = "/api/plugins/codemode/admin/apply";

/// The code-mode routes for the orchestrator to merge.
pub fn codemode_routes() -> Router<AppState> {
    Router::new()
        .route(PLANS_PATH, get(plans))
        .route(PREVIEW_PATH, post(preview))
        .route(APPLY_PATH, post(apply))
}

async fn plans(
    State(state): State<AppState>,
    session_user: SessionUser,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiProblem> {
    let current = admin_user(&state, session_user.id()).await?;
    plugin_enabled(&state)?;

    let mut limit = 20_i64;
    if let Some(raw) = query.get("limit") {
        let raw = raw.trim();
        if !raw.is_empty() {
            let parsed = raw.parse::<i64>().map_err(|_| limit_problem())?;
            if !(1..=50).contains(&parsed) {
                return Err(limit_problem());
            }
            limit = parsed;
        }
    }

    let hooks = PhoneHooks::new();
    let service = Codemode::new(&state.store, Some(state.plugins.as_ref()), &hooks);
    let body = service.plans(&current.id, limit).await?;
    Ok(raw_body(200, body))
}

async fn preview(
    State(state): State<AppState>,
    session_user: SessionUser,
    ClientIp(ip): ClientIp,
    request: Request,
) -> Result<Response, ApiProblem> {
    let current = admin_user(&state, session_user.id()).await?;
    if let Some(response) = check_admin_limit(&state, &ip) {
        return Ok(response);
    }
    plugin_enabled(&state)?;
    let body = decode_body(request, &["instruction"]).await?;
    let instruction = string_field(&body, "instruction")?;

    let settings = load_settings(&state).await;
    let hooks = PhoneHooks::new();
    let service = Codemode::new(&state.store, Some(state.plugins.as_ref()), &hooks);
    let text = service
        .preview(&PreviewInput {
            instruction: &instruction,
            actor_user_id: &current.id,
            settings: settings.as_ref(),
            ai_base_url: &state.cfg.ai_base_url,
            ai_timeout: state.cfg.ai_timeout,
            now: Utc::now(),
        })
        .await?;
    Ok(raw_body(200, text))
}

async fn apply(
    State(state): State<AppState>,
    session_user: SessionUser,
    ClientIp(ip): ClientIp,
    request: Request,
) -> Result<Response, ApiProblem> {
    let current = admin_user(&state, session_user.id()).await?;
    if let Some(response) = check_admin_limit(&state, &ip) {
        return Ok(response);
    }
    plugin_enabled(&state)?;
    let body = decode_body(request, &["plan_id"]).await?;
    let plan_id = string_field(&body, "plan_id")?;

    let settings = load_settings(&state).await;
    let hooks = PhoneHooks::new();
    let service = Codemode::new(&state.store, Some(state.plugins.as_ref()), &hooks);
    let text = service
        .apply(&ApplyInput {
            plan_id: &plan_id,
            actor_user_id: &current.id,
            settings: settings.as_ref(),
            now: Utc::now(),
        })
        .await?;
    Ok(raw_body(200, text))
}

fn limit_problem() -> ApiProblem {
    problem_fields(
        422,
        "validation_error",
        "Check the marked fields.",
        HashMap::from([(
            "limit".to_string(),
            "Choose a limit from 1 to 50.".to_string(),
        )]),
    )
}

/// `pluginHandler`: a disabled plugin answers 404 after the route guards, the
/// same place the Go handler runs its check.
fn plugin_enabled(state: &AppState) -> Result<(), ApiProblem> {
    let registry = state
        .plugins
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if registry.effective_enabled(PLUGIN_ID) {
        Ok(())
    } else {
        Err(problem(404, "not_found", "Plugin not found."))
    }
}

/// The Go `requireUser` + `requireAdmin` chain.
async fn admin_user(state: &AppState, session_user: Option<&str>) -> Result<User, ApiProblem> {
    let Some(user_id) = session_user else {
        return Err(unauthorized());
    };
    match state.store.user_by_id(user_id).await {
        Ok(user) if user.role == "admin" => Ok(user),
        Ok(_) => Err(forbidden()),
        Err(StoreError::NotFound) => Err(unauthorized()),
        Err(_) => Err(internal_error()),
    }
}

/// `adminMutationLimit` for the two unsafe routes.
fn check_admin_limit(state: &AppState, ip: &str) -> Option<Response> {
    let (allowed, wait) = state.admin_limiter.allow(ip, Instant::now());
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

/// Decrypted settings; any read or decrypt failure becomes the unreadable
/// envelope inside the service.
async fn load_settings(state: &AppState) -> Option<HashMap<String, String>> {
    let env = azahar_plugins::Env::new(state.store.clone(), state.cfg.plugin_secret_key.clone());
    match env.settings(PLUGIN_ID).await {
        Ok(settings) => Some(settings),
        Err(error) => {
            tracing::warn!(error = %error, "codemode settings could not be read");
            None
        }
    }
}

/// Go `plugins.DecodeBody`: content-type prefix, 1 MiB cap, empty-body field,
/// one object, unknown fields and shape errors all keep their exact envelope.
async fn decode_body(
    request: Request,
    allowed: &[&str],
) -> Result<HashMap<String, Value>, ApiProblem> {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !content_type.to_lowercase().starts_with("application/json") {
        return Err(problem(
            415,
            "unsupported_media_type",
            "Send the request body as application/json.",
        ));
    }

    let bytes = match to_bytes(request.into_body(), BODY_LIMIT + 1).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return Err(problem(
                413,
                "payload_too_large",
                "The request body is too large.",
            ));
        }
    };
    if bytes.len() > BODY_LIMIT {
        return Err(problem(
            413,
            "payload_too_large",
            "The request body is too large.",
        ));
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return Err(shape_problem());
    };
    if text.trim().is_empty() {
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            HashMap::from([("body".to_string(), "Send a JSON object.".to_string())]),
        ));
    }

    let mut deserializer = serde_json::Deserializer::from_str(text);
    let value = Value::deserialize(&mut deserializer).map_err(|_| shape_problem())?;
    if deserializer.end().is_err() {
        return Err(problem(422, "invalid_json", "Send one JSON object."));
    }
    let map = match value {
        Value::Null => HashMap::new(),
        Value::Object(map) => map.into_iter().collect::<HashMap<_, _>>(),
        _ => return Err(shape_problem()),
    };
    for key in map.keys() {
        if !allowed.iter().any(|name| name.eq_ignore_ascii_case(key)) {
            return Err(shape_problem());
        }
    }
    Ok(map)
}

fn shape_problem() -> ApiProblem {
    problem(
        422,
        "invalid_json",
        "The request body does not match the expected shape.",
    )
}

/// Go's `json.Unmarshal` into a string: absent and `null` are empty, any other
/// type is a shape error.
fn string_field(map: &HashMap<String, Value>, name: &str) -> Result<String, ApiProblem> {
    let exact = map.get(name);
    let value = exact.or_else(|| {
        map.iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value)
    });
    match value {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(text)) => Ok(text.clone()),
        Some(_) => Err(shape_problem()),
    }
}

fn raw_body(status: u16, text: String) -> Response {
    json_response(status, &json::raw(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request as HttpRequest, header};

    fn request(body: &str, content_type: &str) -> Request {
        HttpRequest::builder()
            .method("POST")
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(body.to_string()))
            .expect("request")
    }

    #[tokio::test]
    async fn decode_matches_the_plugin_context_contract() {
        let missing_type = decode_body(request("{}", "text/plain"), &["instruction"])
            .await
            .expect_err("415");
        assert_eq!(missing_type.status.as_u16(), 415);

        let empty = decode_body(request("  ", "application/json"), &["instruction"])
            .await
            .expect_err("empty");
        assert_eq!(empty.status.as_u16(), 422);
        assert_eq!(
            empty
                .fields
                .expect("fields")
                .get("body")
                .map(String::as_str),
            Some("Send a JSON object.")
        );

        let trailing = decode_body(
            request(
                "{\"instruction\":\"a\"} {}",
                "application/json; charset=utf-8",
            ),
            &["instruction"],
        )
        .await
        .expect_err("trailing");
        assert_eq!(trailing.message, "Send one JSON object.");

        let unknown = decode_body(
            request("{\"instruction\":\"a\",\"x\":1}", "application/json"),
            &["instruction"],
        )
        .await
        .expect_err("unknown");
        assert_eq!(
            unknown.message,
            "The request body does not match the expected shape."
        );

        let null = decode_body(request("null", "application/json"), &["instruction"])
            .await
            .expect("null");
        assert!(null.is_empty());

        let body = decode_body(
            request("{\"Instruction\":\"a\"}", "application/json"),
            &["instruction"],
        )
        .await
        .expect("case-insensitive");
        assert_eq!(string_field(&body, "instruction").expect("string"), "a");

        let wrong = decode_body(
            request("{\"instruction\":1}", "application/json"),
            &["instruction"],
        )
        .await
        .expect("shape");
        assert!(string_field(&wrong, "instruction").is_err());
    }
}
