//! Public delivery-map routes: wilayas, communes and stop desks.
//!
//! Ported from `handlers_shipping.go`. Only the read-only map endpoints live
//! here; the admin shipment and tracking routes belong to the admin agent.

use std::collections::HashMap;

use axum::extract::{RawQuery, State};
use axum::response::Response;
use azahar_courier::{Desk, Error as CourierError};
use serde_json::json;

use crate::handlers_catalog::query_param;
use crate::respond::{ApiProblem, json_response, problem, problem_fields};
use crate::state::AppState;

/// `writeDeliveryMapError`: every lookup failure becomes one 503.
fn delivery_map_unavailable() -> ApiProblem {
    problem(
        503,
        "courier_unavailable",
        "The delivery map is unavailable. Try again in a moment.",
    )
}

/// `parseWilayaCode`: trimmed decimal in 1..=58, otherwise the shared 422.
fn parse_wilaya_code(raw: Option<&str>) -> Result<i32, ApiProblem> {
    let value = query_param(raw, "wilaya");
    let code = value.trim().parse::<i32>();
    match code {
        Ok(code) if (1..=58).contains(&code) => Ok(code),
        _ => Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            HashMap::from([(
                "wilaya".to_string(),
                "Choose a wilaya between 1 and 58.".to_string(),
            )]),
        )),
    }
}

/// `GET /api/shipping/wilayas`.
pub async fn wilayas(State(state): State<AppState>) -> Result<Response, ApiProblem> {
    let wilayas = state
        .shipping
        .wilayas()
        .await
        .map_err(|_| delivery_map_unavailable())?;
    Ok(json_response(200, &json!({"wilayas": wilayas})))
}

/// `GET /api/shipping/communes?wilaya=N`.
pub async fn communes(
    State(state): State<AppState>,
    RawQuery(raw): RawQuery,
) -> Result<Response, ApiProblem> {
    let wilaya = parse_wilaya_code(raw.as_deref())?;
    let communes = state
        .shipping
        .communes(wilaya)
        .await
        .map_err(|_| delivery_map_unavailable())?;
    Ok(json_response(200, &json!({"communes": communes})))
}

/// `GET /api/shipping/desks?wilaya=N`.
///
/// A courier 422 means "no desks here", which Go renders as an empty list and
/// a 200; every other failure keeps the shared 503.
pub async fn desks(
    State(state): State<AppState>,
    RawQuery(raw): RawQuery,
) -> Result<Response, ApiProblem> {
    let wilaya = parse_wilaya_code(raw.as_deref())?;
    match state.shipping.desks(wilaya).await {
        Ok(desks) => Ok(json_response(200, &json!({"desks": desks}))),
        Err(CourierError::Api(api)) if api.status == 422 => {
            Ok(json_response(200, &json!({"desks": Vec::<Desk>::new()})))
        }
        Err(_) => Err(delivery_map_unavailable()),
    }
}
