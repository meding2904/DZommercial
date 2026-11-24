//! Customer order reads: the public token link and the signed-in order list.
//!
//! Ported from `handleOrderByToken` in `handlers_checkout.go` and
//! `handleAccountOrders` in `handlers_auth.go`.

use axum::extract::{Path, State};
use axum::response::Response;
use azahar_auth::hash_token;
use azahar_store::StoreError;
use serde_json::json;
use subtle::ConstantTimeEq;

use crate::dto::to_order;
use crate::middleware::{CurrentUser, internal_error};
use crate::respond::{ApiProblem, json_response, problem};
use crate::state::AppState;

/// The shared 404 of both order reads.
fn order_not_found() -> ApiProblem {
    problem(404, "not_found", "Order not found.")
}

/// `GET /api/orders/{token}`: the customer's link to one order.
pub async fn by_token(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Response, ApiProblem> {
    if token.is_empty() || token.len() > 200 {
        return Err(order_not_found());
    }

    let hash = hash_token(&token);
    let order = state
        .store
        .order_by_token_hash(&hash)
        .await
        .map_err(|err| match err {
            StoreError::NotFound => order_not_found(),
            _ => internal_error(),
        })?;

    if !bool::from(order.token_hash.as_bytes().ct_eq(hash.as_bytes())) {
        return Err(order_not_found());
    }

    let items = state
        .store
        .order_items(&order.id)
        .await
        .map_err(|_| internal_error())?;
    // Go passes an empty token and includes the address on this route.
    Ok(json_response(
        200,
        &json!({"order": to_order(&order, &items, "", true)}),
    ))
}

/// `GET /api/account/orders`: the signed-in customer's orders, newest first.
pub async fn account_orders(
    State(state): State<AppState>,
    CurrentUser(current): CurrentUser,
) -> Result<Response, ApiProblem> {
    let orders = state
        .store
        .orders_by_user(&current.id)
        .await
        .map_err(|_| internal_error())?;
    let ids: Vec<String> = orders.iter().map(|order| order.id.clone()).collect();
    let mut items_by_order = state
        .store
        .order_items_by_order_ids(&ids)
        .await
        .map_err(|_| internal_error())?;

    let result: Vec<_> = orders
        .iter()
        .map(|order| {
            let items = items_by_order.remove(&order.id).unwrap_or_default();
            // The account list never exposes the delivery address or token.
            to_order(order, &items, "", false)
        })
        .collect();
    Ok(json_response(200, &json!({"orders": result})))
}
