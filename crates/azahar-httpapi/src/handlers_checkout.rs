//! Storefront checkout and the image-captcha challenge.
//!
//! Port of `handlers_checkout.go` and the HTTP half of `captcha.go`. The route
//! builder at the bottom carries the same per-route limiters and the challenge
//! header guard as the Go router, so `router.rs` only has to merge it.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::extract::{ConnectInfo, Extension, FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use azahar_auth::{hash_token, new_token};
use azahar_courier::{Desk, Error as CourierError, Wilaya};
use azahar_orderintake::{Destination, Intake, IntakeError};
use azahar_plugin_phone::PhoneHooks;
use serde::Deserialize;
use serde_json::json;

use crate::captcha::{CaptchaCheck, CaptchaStore, captcha_error_message};
use crate::dto::to_order;
use crate::extract::{JsonBody, client_ip};
use crate::middleware::internal_error;
use crate::respond::{ApiProblem, json_response, problem, problem_fields};
use crate::session_layer::SessionUser;
use crate::state::AppState;
use crate::turnstile::verify_turnstile;
use crate::validate::{fields, rune_len, valid_algerian_mobile, valid_email};

/// Keep in sync with `frontend/src/lib/legal.ts`, like the Go constant.
pub const CHECKOUT_TERMS_VERSION: &str = "2026-11-14";
/// The challenge route, used by the header guard.
pub const CHECKOUT_CHALLENGE_PATH: &str = "/api/checkout/challenge";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CheckoutPayload {
    pub name: String,
    pub email: String,
    pub phone: String,
    pub address: String,
    pub wilaya_code: i64,
    pub commune_name: String,
    pub delivery_type: String,
    pub stop_desk_id: String,
    pub notes: String,
    pub items: Option<Vec<CheckoutLine>>,
    pub website: String,
    pub captcha_token: String,
    pub captcha_answer: String,
    pub turnstile_token: String,
    pub terms_accepted: bool,
    pub terms_version: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CheckoutLine {
    pub product_id: String,
    pub quantity: i64,
    /// Go decodes a JSON `null` option value as the empty string.
    pub options: Option<HashMap<String, Option<String>>>,
    pub expected_price_cents: Option<i64>,
}

/// The request's client address, resolved exactly like Go's `clientIP`.
///
/// It reads `ConnectInfo` from the request extensions when the server
/// installed it and tolerates its absence under `oneshot` tests.
pub struct ClientIp(pub String);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0);
        Ok(Self(client_ip(&parts.headers, peer, state.cfg.trust_proxy)))
    }
}

/// `POST /api/checkout`.
pub async fn checkout(
    State(state): State<AppState>,
    Extension(captcha): Extension<Arc<CaptchaStore>>,
    ClientIp(ip): ClientIp,
    SessionUser(session_user): SessionUser,
    body: JsonBody<CheckoutPayload>,
) -> Result<Response, ApiProblem> {
    if let Some(response) = check_limiter(&state.checkout_limiter, &ip) {
        return Ok(response);
    }

    let mut payload = body.payload;
    payload.name = payload.name.trim().to_string();
    payload.email = payload.email.trim().to_lowercase();
    payload.phone = payload.phone.trim().to_string();
    payload.address = payload.address.trim().to_string();
    payload.commune_name = payload.commune_name.trim().to_string();
    payload.delivery_type = payload.delivery_type.trim().to_lowercase();
    payload.stop_desk_id = payload.stop_desk_id.trim().to_string();
    payload.notes = payload.notes.trim().to_string();
    payload.website = payload.website.trim().to_string();
    payload.captcha_token = payload.captcha_token.trim().to_string();
    payload.captcha_answer = payload.captcha_answer.trim().to_string();
    payload.turnstile_token = payload.turnstile_token.trim().to_string();
    payload.terms_version = payload.terms_version.trim().to_string();
    if payload.delivery_type.is_empty() {
        payload.delivery_type = "home".to_string();
    }

    // A person never sees this field. A filled value is a bot signal, so reject
    // before doing any order work.
    if !payload.website.is_empty() {
        return Err(problem(
            403,
            "bot_detected",
            "The order could not be verified.",
        ));
    }

    let mut problems = HashMap::new();
    if !payload.terms_accepted || payload.terms_version != CHECKOUT_TERMS_VERSION {
        problems.insert(
            "terms".to_string(),
            "Accept the current terms of service before placing the order.".to_string(),
        );
    }
    if state.cfg.turnstile_secret.is_empty() {
        if payload.captcha_token.is_empty() || payload.captcha_answer.is_empty() {
            problems.insert(
                "captcha".to_string(),
                "Enter the code from the security check.".to_string(),
            );
        }
    } else if payload.turnstile_token.is_empty() {
        problems.insert(
            "captcha".to_string(),
            "Complete the security check and try again.".to_string(),
        );
    }
    if !(2..=120).contains(&rune_len(&payload.name)) {
        problems.insert(
            "name".to_string(),
            "Use between 2 and 120 characters.".to_string(),
        );
    }
    if !valid_email(&payload.email) {
        problems.insert("email".to_string(), "Enter a valid email.".to_string());
    }
    if !valid_algerian_mobile(&payload.phone) {
        problems.insert(
            "phone".to_string(),
            "Use an Algerian mobile number, for example 0550 12 34 56.".to_string(),
        );
    }
    if !(1..=58).contains(&payload.wilaya_code) {
        problems.insert("wilaya_code".to_string(), "Choose a wilaya.".to_string());
    }
    if !(2..=80).contains(&rune_len(&payload.commune_name)) {
        problems.insert("commune_name".to_string(), "Choose a commune.".to_string());
    }
    if payload.delivery_type != "home" && payload.delivery_type != "stopdesk" {
        problems.insert(
            "delivery_type".to_string(),
            "Choose home delivery or a stop desk.".to_string(),
        );
    }
    if payload.delivery_type == "stopdesk" {
        if !(1..=100).contains(&rune_len(&payload.stop_desk_id)) {
            problems.insert(
                "stop_desk_id".to_string(),
                "Choose a stop desk.".to_string(),
            );
        }
    } else if !(5..=200).contains(&rune_len(&payload.address)) {
        problems.insert(
            "address".to_string(),
            "Use between 5 and 200 characters.".to_string(),
        );
    }
    if rune_len(&payload.address) > 200 {
        problems.insert(
            "address".to_string(),
            "Keep the address under 200 characters.".to_string(),
        );
    }
    if rune_len(&payload.notes) > 500 {
        problems.insert(
            "notes".to_string(),
            "Keep the notes under 500 characters.".to_string(),
        );
    }
    let item_count = payload.items.as_ref().map_or(0, Vec::len);
    if item_count == 0 || item_count > 20 {
        problems.insert(
            "items".to_string(),
            "Add between 1 and 20 products.".to_string(),
        );
    }
    if !problems.is_empty() {
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            problems,
        ));
    }

    let lines: Vec<azahar_orderintake::Line> = payload
        .items
        .unwrap_or_default()
        .into_iter()
        .map(|item| azahar_orderintake::Line {
            product_id: item.product_id,
            quantity: item.quantity,
            options: item
                .options
                .unwrap_or_default()
                .into_iter()
                .map(|(key, value)| (key, value.unwrap_or_default()))
                .collect(),
            expected_price_cents: item.expected_price_cents,
        })
        .collect();
    let (lines, item_fields) = azahar_orderintake::build_lines(lines);
    if !item_fields.is_empty() {
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            item_fields,
        ));
    }
    if lines.is_empty() {
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            fields(&[("items", "Add at least one product.")]),
        ));
    }

    let wilaya_code = i32::try_from(payload.wilaya_code).unwrap_or_default();
    let destination = resolve_destination(
        &state,
        wilaya_code,
        &payload.commune_name,
        &payload.delivery_type,
        &payload.stop_desk_id,
    )
    .await?;

    if !state.cfg.turnstile_secret.is_empty() {
        match verify_turnstile(&state.cfg.turnstile_secret, &payload.turnstile_token, &ip).await {
            Err(error) => {
                tracing::warn!(error = %error, "turnstile verification failed");
                return Err(problem(
                    503,
                    "captcha_unavailable",
                    "The security check is unavailable. Try again in a moment.",
                ));
            }
            Ok(false) => {
                return Err(problem_fields(
                    422,
                    "validation_error",
                    "The security check failed.",
                    fields(&[("captcha", "Complete the security check and try again.")]),
                ));
            }
            Ok(true) => {}
        }
    } else {
        let check = captcha.verify(&payload.captcha_token, &payload.captcha_answer, &ip);
        if check != CaptchaCheck::Ok {
            return Err(problem_fields(
                422,
                "validation_error",
                "The security check failed.",
                fields(&[("captcha", captcha_error_message(check))]),
            ));
        }
    }

    let mut customer = azahar_orderintake::Customer {
        name: payload.name,
        email: payload.email,
        phone: payload.phone,
        address: payload.address,
        notes: payload.notes,
        ..azahar_orderintake::Customer::default()
    };
    let mut target = Destination {
        wilaya_code: destination.wilaya.code,
        wilaya_name: destination.wilaya.name_fr.clone(),
        commune_name: destination.commune.clone(),
        delivery_type: payload.delivery_type.clone(),
        ..Destination::default()
    };
    if let Some(desk) = &destination.desk {
        target.stop_desk_id = desk.id.clone();
        target.stop_desk_name = desk.name.clone();
        if customer.address.is_empty() {
            customer.address = desk.address.clone();
        }
    }
    if let Some(user_id) = &session_user
        && let Ok(user) = state.store.user_by_id(user_id).await
    {
        customer.user_id = user.id;
    }

    let token = new_token().map_err(|_| internal_error())?;
    let intake = Intake::new(
        state.store.clone(),
        state.plugins.clone(),
        Arc::new(PhoneHooks::new()) as Arc<dyn azahar_orderintake::HookProvider>,
    );
    let actor_user_id = customer.user_id.clone();
    let result = intake
        .place(azahar_orderintake::Request {
            lines,
            channel: azahar_orderintake::checkout_channel(&payload.terms_version),
            customer,
            destination: target,
            status: "pending".to_string(),
            token_hash: hash_token(&token),
            actor_user_id,
        })
        .await
        .map_err(|error| intake_problem(&error))?;

    Ok(json_response(
        201,
        &json!({"order": to_order(&result.order, &result.items, &token, true)}),
    ))
}

/// `GET /api/checkout/challenge`.
pub async fn challenge(
    State(state): State<AppState>,
    Extension(captcha): Extension<Arc<CaptchaStore>>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
) -> Result<Response, ApiProblem> {
    // Go's `requireChallengeHeader`: a cross-site image cannot set this header,
    // and the check runs before the limiter so a rejected request never spends
    // a visitor's bucket.
    if headers
        .get("x-requested-with")
        .and_then(|value| value.to_str().ok())
        != Some("azahar-web")
    {
        return Err(problem(
            403,
            "forbidden",
            "The request could not be verified.",
        ));
    }
    if let Some(response) = check_limiter(&state.captcha_limiter, &ip) {
        return Ok(response);
    }

    let issued = captcha.issue(&ip).map_err(|_| internal_error())?;
    let mut response = json_response(
        200,
        &json!({
            "token": issued.token,
            "image": issued.image,
            "expires_in": issued.ttl.as_secs(),
        }),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

struct ResolvedDestination {
    wilaya: Wilaya,
    commune: String,
    desk: Option<Desk>,
}

/// `resolveDestination` in `handlers_shipping.go`. The public shipping module
/// keeps its helpers private, so the checkout owns the same logic here.
async fn resolve_destination(
    state: &AppState,
    wilaya_code: i32,
    commune_name: &str,
    delivery_type: &str,
    stop_desk_id: &str,
) -> Result<ResolvedDestination, ApiProblem> {
    let wilaya = match state.shipping.wilaya(wilaya_code).await {
        Ok(wilaya) => wilaya,
        Err(CourierError::UnknownWilaya) => {
            return Err(problem_fields(
                422,
                "validation_error",
                "Check the marked fields.",
                fields(&[("wilaya_code", "Choose a wilaya from the list.")]),
            ));
        }
        Err(_) => {
            return Err(problem(
                503,
                "courier_unavailable",
                "The delivery map is unavailable. Try again in a moment.",
            ));
        }
    };

    let mut result = ResolvedDestination {
        wilaya,
        commune: commune_name.trim().to_string(),
        desk: None,
    };

    if delivery_type == "stopdesk" {
        let desk = match state.shipping.desk(wilaya_code, stop_desk_id).await {
            Ok(desk) => desk,
            Err(CourierError::UnknownDesk) => {
                return Err(problem_fields(
                    422,
                    "validation_error",
                    "Check the marked fields.",
                    fields(&[("stop_desk_id", "Choose a stop desk from the list.")]),
                ));
            }
            Err(_) => {
                return Err(problem(
                    503,
                    "courier_unavailable",
                    "The delivery map is unavailable. Try again in a moment.",
                ));
            }
        };
        if !desk.commune_name.trim().is_empty() {
            result.commune = desk.commune_name.clone();
        }
        result.desk = Some(desk);
    }

    if result.commune.trim().is_empty() {
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            fields(&[("commune_name", "Choose a commune from the list.")]),
        ));
    }

    let canonical = match state.shipping.commune(wilaya_code, &result.commune).await {
        Ok(canonical) => canonical,
        Err(CourierError::UnknownCommune) => {
            return Err(problem_fields(
                422,
                "validation_error",
                "Check the marked fields.",
                fields(&[("commune_name", "Choose a commune from the list.")]),
            ));
        }
        Err(_) => {
            return Err(problem(
                503,
                "courier_unavailable",
                "The delivery map is unavailable. Try again in a moment.",
            ));
        }
    };
    result.commune = canonical;
    Ok(result)
}

fn intake_problem(error: &IntakeError) -> ApiProblem {
    let Some(refusal) = error.problem() else {
        return internal_error();
    };
    match &refusal.fields {
        Some(fields) => problem_fields(
            refusal.status,
            &refusal.code,
            &refusal.message,
            fields.clone(),
        ),
        None => problem(refusal.status, &refusal.code, &refusal.message),
    }
}

// ---------------------------------------------------------------------------
// Rate limiting
// ---------------------------------------------------------------------------

/// The shared limiter check, with Go's 429 body and `Retry-After` header.
///
/// Axum's `from_fn` middleware cannot extract `State<AppState>` on a
/// `Router<AppState>` that has not been given its state value yet, and
/// `checkout_routes()` has no value to pass to `from_fn_with_state`. The
/// handlers call this as their first statement instead, which keeps the same
/// observable behaviour and the same bucket.
pub(crate) fn check_limiter(limiter: &crate::ratelimit::RateLimiter, ip: &str) -> Option<Response> {
    let (allowed, wait) = limiter.allow(ip, Instant::now());
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

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// The two checkout routes. `router.rs` only needs
/// `.merge(checkout_routes())`; the checkout and challenge limiters and the
/// challenge header guard are applied at the top of each handler.
pub fn checkout_routes() -> Router<AppState> {
    checkout_routes_with_captcha(Arc::new(CaptchaStore::new(crate::captcha::CAPTCHA_TTL)))
}

/// Same as [`checkout_routes`] with an owned store, so tests can inject a
/// clock and read issued codes.
pub fn checkout_routes_with_captcha(captcha: Arc<CaptchaStore>) -> Router<AppState> {
    captcha.start_cleanup();
    Router::new()
        .route("/api/checkout", post(checkout))
        .route(CHECKOUT_CHALLENGE_PATH, get(challenge))
        .layer(Extension(captcha))
}
