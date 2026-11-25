//! Admin shipment creation and tracking refresh.
//!
//! Port of `handleCreateShipment` and `handleRefreshTracking` in
//! `handlers_shipping.go`. Both routes are staff mutations: each handler first
//! runs the same guard chain (signed-in user, staff role) and shared admin
//! mutation limiter as the Go `/admin` group.

use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use azahar_courier::{APIError, Error as CourierError, OrderRequest, Recipient};
use azahar_ids::new as new_id;
use azahar_store::{AuditEntry, CourierEvent, Order, OrderItem, StoreError, User};
use chrono::Utc;
use serde_json::{Value, json};

use crate::dto::to_order;
use crate::handlers_checkout::{ClientIp, check_limiter};
use crate::middleware::{forbidden, internal_error, unauthorized};
use crate::respond::{ApiProblem, encode_json, json_response, problem, problem_fields};
use crate::session_layer::SessionUser;
use crate::state::AppState;
use crate::validate::{normalized_algerian_mobile, rune_len, truncate_runes};

/// The Go `requireUser` + `requireStaff` chain: 401 without a valid session,
/// 403 for a signed-in customer.
async fn staff_user(state: &AppState, session_user: Option<&str>) -> Result<User, ApiProblem> {
    let Some(user_id) = session_user else {
        return Err(unauthorized());
    };
    match state.store.user_by_id(user_id).await {
        Ok(user) if user.role == "admin" || user.role == "employee" => Ok(user),
        Ok(_) => Err(forbidden()),
        Err(StoreError::NotFound) => Err(unauthorized()),
        Err(_) => Err(internal_error()),
    }
}

/// `POST /api/admin/orders/{id}/shipment`.
pub async fn create_shipment(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ClientIp(ip): ClientIp,
    SessionUser(session_user): SessionUser,
) -> Response {
    let current = match staff_user(&state, session_user.as_deref()).await {
        Ok(current) => current,
        Err(problem) => return problem.into_response(),
    };
    if let Some(response) = check_limiter(&state.admin_limiter, &ip) {
        return response;
    }

    let mut order = match state.store.order_by_id(&id).await {
        Ok(order) => order,
        Err(StoreError::NotFound) => {
            return problem(404, "not_found", "Order not found.").into_response();
        }
        Err(_) => return internal_error().into_response(),
    };
    if !order.tracking_number.is_empty() {
        return problem(409, "conflict", "This order already has a shipment.").into_response();
    }
    if order.status == "cancelled" || order.status == "delivered" {
        return problem(
            409,
            "invalid_transition",
            "This order is closed and cannot be shipped.",
        )
        .into_response();
    }

    let delivery_type = if order.delivery_type.is_empty() {
        "home".to_string()
    } else {
        order.delivery_type.clone()
    };
    let mut problems = std::collections::HashMap::new();
    if !(1..=58).contains(&order.wilaya_code) {
        problems.insert(
            "delivery".to_string(),
            "This order has no wilaya.".to_string(),
        );
    }
    if order.commune_name.trim().is_empty() {
        problems.insert(
            "commune_name".to_string(),
            "This order has no commune.".to_string(),
        );
    }
    if delivery_type == "stopdesk" {
        if order.stop_desk_id.trim().is_empty() {
            problems.insert(
                "stop_desk_id".to_string(),
                "This order has no stop desk.".to_string(),
            );
        }
    } else if rune_len(&order.address) < 5 {
        problems.insert(
            "address".to_string(),
            "This order has no deliverable address.".to_string(),
        );
    }
    if !problems.is_empty() {
        return problem_fields(
            422,
            "validation_error",
            "This order needs delivery details first.",
            problems,
        )
        .into_response();
    }

    let Some(phone) = normalized_algerian_mobile(&order.phone) else {
        return problem_fields(
            422,
            "validation_error",
            "This order needs a valid phone first.",
            std::collections::HashMap::from([(
                "phone".to_string(),
                "Use an Algerian mobile number.".to_string(),
            )]),
        )
        .into_response();
    };

    let items = match state.store.order_items(&order.id).await {
        Ok(items) => items,
        Err(_) => return internal_error().into_response(),
    };

    let address_line = if delivery_type == "stopdesk" {
        String::new()
    } else {
        order.address.clone()
    };
    let request = OrderRequest {
        reference: order.number.clone(),
        recipient: Recipient {
            full_name: order.name.clone(),
            phone,
            phone_alt: String::new(),
            wilaya_code: order.wilaya_code,
            commune_name: order.commune_name.clone(),
            address_line,
        },
        delivery_type: delivery_type.clone(),
        stop_desk_id: order.stop_desk_id.clone(),
        product_list: order_product_list(&items),
        // The COD amount already contains the store's delivery charge. Tell the
        // courier not to add its own fee on top of the amount we collect.
        cod_amount: order.total_cents / 100,
        free_shipping: true,
        notes: truncate_runes(&order.notes, 255),
    };
    let shipment = match state.shipping.create_order(request).await {
        Ok(shipment) => shipment,
        Err(error) => return courier_error_response(&error),
    };

    let status = {
        let trimmed = shipment.status.trim();
        if trimmed.is_empty() {
            "created".to_string()
        } else {
            trimmed.to_string()
        }
    };
    let now = Utc::now();
    let detail = audit_detail(&json!({
        "courier": state.cfg.courier_key,
        "tracking_number": shipment.tracking_number,
    }));

    if let Err(problem) = store_shipment(
        &state,
        &order,
        &shipment.tracking_number,
        &status,
        now,
        &current.id,
        detail,
    )
    .await
    {
        return problem.into_response();
    }

    order.courier_key = state.cfg.courier_key.clone();
    order.tracking_number = shipment.tracking_number;
    order.courier_status = status;
    order.courier_events = Vec::new();
    order.courier_synced_at = Some(now);
    order.updated_at = Some(now);

    json_response(200, &json!({"order": to_order(&order, &items, "", true)}))
}

async fn store_shipment(
    state: &AppState,
    order: &Order,
    tracking_number: &str,
    status: &str,
    now: chrono::DateTime<Utc>,
    actor_user_id: &str,
    detail: String,
) -> Result<(), ApiProblem> {
    let tx = match state.store.begin_write().await {
        Ok(tx) => tx,
        Err(_) => return Err(internal_error()),
    };
    if let Err(error) = state
        .store
        .attach_shipment(
            tx.conn(),
            &order.id,
            &state.cfg.courier_key,
            tracking_number,
            status,
            &[],
            Some(now),
            now,
        )
        .await
    {
        let _ = tx.rollback().await;
        return Err(match error {
            StoreError::NotFound => problem(409, "conflict", "This order already has a shipment."),
            _ => internal_error(),
        });
    }
    if state
        .store
        .insert_audit(
            tx.conn(),
            &AuditEntry {
                id: new_id("aud"),
                actor_user_id: actor_user_id.to_string(),
                action: "order.shipment".to_string(),
                entity_type: "order".to_string(),
                entity_id: order.id.clone(),
                detail,
                created_at: Some(now),
            },
        )
        .await
        .is_err()
    {
        let _ = tx.rollback().await;
        return Err(internal_error());
    }
    if tx.commit().await.is_err() {
        return Err(internal_error());
    }
    Ok(())
}

/// `POST /api/admin/orders/{id}/tracking`.
pub async fn refresh_tracking(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ClientIp(ip): ClientIp,
    SessionUser(session_user): SessionUser,
) -> Response {
    let current = match staff_user(&state, session_user.as_deref()).await {
        Ok(current) => current,
        Err(problem) => return problem.into_response(),
    };
    if let Some(response) = check_limiter(&state.admin_limiter, &ip) {
        return response;
    }

    let mut order = match state.store.order_by_id(&id).await {
        Ok(order) => order,
        Err(StoreError::NotFound) => {
            return problem(404, "not_found", "Order not found.").into_response();
        }
        Err(_) => return internal_error().into_response(),
    };
    if order.tracking_number.is_empty() {
        return problem(409, "conflict", "This order has no shipment yet.").into_response();
    }

    let tracking = match state
        .shipping
        .track(&order.courier_key, &order.tracking_number)
        .await
    {
        Ok(tracking) => tracking,
        Err(error) => return courier_error_response(&error),
    };

    let events: Vec<CourierEvent> = tracking
        .events
        .iter()
        .map(|event| CourierEvent {
            status: event.status.clone(),
            raw_status: event.raw_status.clone(),
            timestamp: event.timestamp.clone(),
        })
        .collect();
    let status = {
        let trimmed = tracking.status.trim();
        if trimmed.is_empty() {
            "unknown".to_string()
        } else {
            trimmed.to_string()
        }
    };
    let now = Utc::now();
    let detail = audit_detail(&json!({"status": status}));

    if let Err(problem) =
        store_tracking(&state, &order, &status, &events, now, &current.id, detail).await
    {
        return problem.into_response();
    }

    let items = match state.store.order_items(&order.id).await {
        Ok(items) => items,
        Err(_) => return internal_error().into_response(),
    };
    order.courier_status = status;
    order.courier_events = events;
    order.courier_synced_at = Some(now);
    order.updated_at = Some(now);
    json_response(200, &json!({"order": to_order(&order, &items, "", true)}))
}

async fn store_tracking(
    state: &AppState,
    order: &Order,
    status: &str,
    events: &[CourierEvent],
    now: chrono::DateTime<Utc>,
    actor_user_id: &str,
    detail: String,
) -> Result<(), ApiProblem> {
    let tx = match state.store.begin_write().await {
        Ok(tx) => tx,
        Err(_) => return Err(internal_error()),
    };
    if state
        .store
        .update_tracking(tx.conn(), &order.id, status, events, Some(now), now)
        .await
        .is_err()
    {
        let _ = tx.rollback().await;
        return Err(internal_error());
    }
    if state
        .store
        .insert_audit(
            tx.conn(),
            &AuditEntry {
                id: new_id("aud"),
                actor_user_id: actor_user_id.to_string(),
                action: "order.tracking".to_string(),
                entity_type: "order".to_string(),
                entity_id: order.id.clone(),
                detail,
                created_at: Some(now),
            },
        )
        .await
        .is_err()
    {
        let _ = tx.rollback().await;
        return Err(internal_error());
    }
    if tx.commit().await.is_err() {
        return Err(internal_error());
    }
    Ok(())
}

/// Go's `orderProductList`: `name xN` joined by commas, capped at 500 runes.
fn order_product_list(items: &[OrderItem]) -> String {
    let parts: Vec<String> = items
        .iter()
        .map(|item| format!("{} x{}", item.name, item.quantity))
        .collect();
    truncate_runes(&parts.join(", "), 500)
}

/// Go's `audit_detail` helper: `json.Marshal` output without the response
/// trailing newline.
fn audit_detail(value: &Value) -> String {
    let encoded = encode_json(value);
    encoded.strip_suffix('\n').unwrap_or(&encoded).to_string()
}

/// `writeCourierError`: the one error policy both shipment routes share.
fn courier_error_response(error: &CourierError) -> Response {
    let CourierError::Api(api) = error else {
        tracing::warn!(error = %error, "courier request failed");
        return problem(
            502,
            "courier_error",
            "The courier service is unavailable. Try again in a moment.",
        )
        .into_response();
    };

    if api.status == 429 {
        let seconds = retry_after_seconds(api);
        let mut response = problem(
            429,
            "courier_rate_limited",
            "The courier service is busy. Try again shortly.",
        )
        .into_response();
        if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        return response;
    }

    let mut message = truncate_runes(api.message.trim(), 300);
    if message.is_empty() {
        message = "The courier service rejected the request.".to_string();
    }
    if api.status == 400 || api.status == 422 {
        let fields: std::collections::HashMap<String, String> = api
            .fields
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        return problem_fields(422, "courier_error", &message, fields).into_response();
    }
    problem(502, "courier_error", &message).into_response()
}

fn retry_after_seconds(api: &APIError) -> u64 {
    let seconds = api.retry_after.as_secs();
    seconds.max(1)
}

/// The two staff shipment routes. `router.rs` only needs
/// `.merge(shipment_routes())`; each handler runs the staff guard chain and the
/// shared admin mutation limiter.
pub fn shipment_routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/orders/{id}/shipment", post(create_shipment))
        .route("/api/admin/orders/{id}/tracking", post(refresh_tracking))
}
