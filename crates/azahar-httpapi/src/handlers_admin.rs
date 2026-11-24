//! Staff and administrator routes: orders, customers, team, preferences and
//! the counter sale.
//!
//! Ported from `handlers_admin.go` and `handlers_admin_sales.go`. Every write
//! here joins its audit row to the same store transaction, exactly like the Go
//! handlers.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, RawQuery, State};
use axum::response::Response;
use azahar_auth::{hash_token, new_token};
use azahar_ids::new as new_id;
use azahar_orderintake::{Intake, IntakeError};
use azahar_orderstatus::known as known_status;
use azahar_plugin_phone::PhoneHooks;
use azahar_store::orders::AdminOrderFilter;
use azahar_store::{AuditEntry, StoreError};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;

use crate::dto::{CustomerDto, TeamMemberDto, to_order};
use crate::handlers_catalog::query_param;
use crate::middleware::{CurrentUser, internal_error};
use crate::respond::{ApiProblem, encode_json, json_response, problem, problem_fields};
use crate::state::AppState;
use crate::validate::{fields, rune_len, truncate_runes, valid_algerian_mobile};

/// Go's `knownPreset`: the four names the preferences form offers.
const THEME_PRESETS: [&str; 4] = ["plaster", "cellar", "terracotta", "lagoon"];

fn known_preset(preset: &str) -> bool {
    THEME_PRESETS.contains(&preset)
}

/// Serialises an audit `detail` the way `encoding/json` does for Go's
/// `json.Marshal`, including HTML escaping and without a trailing newline.
pub(crate) fn audit_detail(value: &serde_json::Value) -> String {
    let encoded = encode_json(value);
    encoded
        .strip_suffix('\n')
        .unwrap_or(encoded.as_str())
        .to_string()
}

fn intake(state: &AppState) -> Intake {
    Intake::new(
        state.store.clone(),
        state.plugins.clone(),
        Arc::new(PhoneHooks::new()),
    )
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
// Orders
// ---------------------------------------------------------------------------

/// `GET /api/admin/orders`.
pub async fn orders(
    State(state): State<AppState>,
    RawQuery(raw): RawQuery,
) -> Result<Response, ApiProblem> {
    let status = query_param(raw.as_deref(), "status").trim().to_string();
    if !status.is_empty() && !known_status(&status) {
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            fields(&[("status", "Unknown order status.")]),
        ));
    }
    let search = truncate_runes(query_param(raw.as_deref(), "q").trim(), 120);

    let orders = state
        .store
        .admin_orders(&AdminOrderFilter {
            status,
            query: search,
            limit: 0,
        })
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
            to_order(order, &items, "", true)
        })
        .collect();
    Ok(json_response(200, &json!({"orders": result})))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OrderStatusPayload {
    pub status: Option<String>,
}

/// `PATCH /api/admin/orders/{id}/status`.
pub async fn order_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    CurrentUser(current): CurrentUser,
    body: crate::extract::JsonBody<OrderStatusPayload>,
) -> Result<Response, ApiProblem> {
    let status = body.payload.status.unwrap_or_default().trim().to_string();
    let changed = intake(&state)
        .change_status(azahar_orderintake::Change {
            order_id: id,
            to: status,
            actor_user_id: current.id.clone(),
            actor_role: current.role.clone(),
        })
        .await
        .map_err(|error| intake_problem(&error))?;
    Ok(json_response(
        200,
        &json!({"order": to_order(&changed.order, &changed.items, "", true)}),
    ))
}

// ---------------------------------------------------------------------------
// Customers
// ---------------------------------------------------------------------------

/// `GET /api/admin/customers`.
pub async fn customers(State(state): State<AppState>) -> Result<Response, ApiProblem> {
    let customers = state
        .store
        .list_customers()
        .await
        .map_err(|_| internal_error())?;
    let result: Vec<CustomerDto> = customers.iter().map(CustomerDto::from).collect();
    Ok(json_response(200, &json!({"customers": result})))
}

// ---------------------------------------------------------------------------
// Team
// ---------------------------------------------------------------------------

/// `GET /api/admin/team`.
pub async fn team(State(state): State<AppState>) -> Result<Response, ApiProblem> {
    let users = state
        .store
        .list_users()
        .await
        .map_err(|_| internal_error())?;
    let result: Vec<TeamMemberDto> = users.iter().map(TeamMemberDto::from).collect();
    Ok(json_response(200, &json!({"users": result})))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TeamRolePayload {
    pub role: Option<String>,
}

/// `PATCH /api/admin/team/{id}`.
pub async fn update_team_role(
    State(state): State<AppState>,
    Path(id): Path<String>,
    CurrentUser(current): CurrentUser,
    body: crate::extract::JsonBody<TeamRolePayload>,
) -> Result<Response, ApiProblem> {
    let role = body.payload.role.unwrap_or_default().trim().to_string();
    if role != "admin" && role != "employee" && role != "customer" {
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            fields(&[("role", "Choose admin, employee or customer.")]),
        ));
    }
    let target = match state.store.user_by_id(&id).await {
        Ok(user) => user,
        Err(StoreError::NotFound) => {
            return Err(problem(404, "not_found", "User not found."));
        }
        Err(_) => return Err(internal_error()),
    };
    if target.id == current.id && role != "admin" {
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            fields(&[("role", "You cannot remove your own administrator role.")]),
        ));
    }

    let now = Utc::now();
    let detail = audit_detail(&json!({"from": target.role, "to": role}));
    let tx = state
        .store
        .begin_write()
        .await
        .map_err(|_| internal_error())?;
    if target.role == "admin" && role != "admin" {
        let admins = match state.store.count_admins(tx.conn()).await {
            Ok(admins) => admins,
            Err(_) => {
                let _ = tx.rollback().await;
                return Err(internal_error());
            }
        };
        if admins <= 1 {
            let _ = tx.rollback().await;
            return Err(problem_fields(
                409,
                "last_admin",
                "Keep at least one administrator.",
                fields(&[("role", "Promote another administrator first.")]),
            ));
        }
    }
    if state
        .store
        .update_user_role(tx.conn(), &target.id, &role, now)
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
                actor_user_id: current.id.clone(),
                action: "user.role".to_string(),
                entity_type: "user".to_string(),
                entity_id: target.id.clone(),
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
    tx.commit().await.map_err(|_| internal_error())?;

    let mut target = target;
    target.role = role;
    target.updated_at = Some(now);
    Ok(json_response(
        200,
        &json!({"user": TeamMemberDto::from(&target)}),
    ))
}

// ---------------------------------------------------------------------------
// Preferences
// ---------------------------------------------------------------------------

/// `GET /api/admin/preferences`.
pub async fn get_preferences(CurrentUser(current): CurrentUser) -> Response {
    let mut preset = current.theme_preset.clone();
    if !known_preset(&preset) {
        preset = "cellar".to_string();
    }
    json_response(200, &json!({"theme_preset": preset}))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PreferencesPayload {
    pub theme_preset: Option<String>,
}

/// `PUT /api/admin/preferences`.
pub async fn put_preferences(
    State(state): State<AppState>,
    CurrentUser(current): CurrentUser,
    body: crate::extract::JsonBody<PreferencesPayload>,
) -> Result<Response, ApiProblem> {
    let preset = body
        .payload
        .theme_preset
        .unwrap_or_default()
        .trim()
        .to_string();
    if !known_preset(&preset) {
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            fields(&[(
                "theme_preset",
                "Choose plaster, cellar, terracotta or lagoon.",
            )]),
        ));
    }

    let now = Utc::now();
    let detail = audit_detail(&json!({"theme_preset": preset}));
    let tx = state
        .store
        .begin_write()
        .await
        .map_err(|_| internal_error())?;
    if state
        .store
        .update_theme(tx.conn(), &current.id, &preset, now)
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
                actor_user_id: current.id.clone(),
                action: "preferences.update".to_string(),
                entity_type: "user".to_string(),
                entity_id: current.id.clone(),
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
    tx.commit().await.map_err(|_| internal_error())?;
    Ok(json_response(200, &json!({"theme_preset": preset})))
}

// ---------------------------------------------------------------------------
// Counter sale
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CounterSalePayload {
    pub items: Option<Vec<CounterSaleLine>>,
    pub customer_name: Option<String>,
    pub phone: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CounterSaleLine {
    pub product_id: Option<String>,
    pub quantity: Option<i64>,
    /// Go decodes a JSON null map value into the empty string.
    pub options: Option<HashMap<String, Option<String>>>,
}

/// `POST /api/admin/sales`.
pub async fn counter_sale(
    State(state): State<AppState>,
    CurrentUser(current): CurrentUser,
    body: crate::extract::JsonBody<CounterSalePayload>,
) -> Result<Response, ApiProblem> {
    let payload = body.payload;
    let customer_name = payload.customer_name.unwrap_or_default().trim().to_string();
    let phone = payload.phone.unwrap_or_default().trim().to_string();
    let note = payload.note.unwrap_or_default().trim().to_string();

    let mut problems = HashMap::new();
    if !customer_name.is_empty() && !(2..=120).contains(&rune_len(&customer_name)) {
        problems.insert(
            "customer_name".to_string(),
            "Use between 2 and 120 characters, or leave it empty.".to_string(),
        );
    }
    if !phone.is_empty() && !valid_algerian_mobile(&phone) {
        problems.insert(
            "phone".to_string(),
            "Use an Algerian mobile number, for example 0550 12 34 56.".to_string(),
        );
    }
    if rune_len(&note) > 500 {
        problems.insert(
            "note".to_string(),
            "Keep the note under 500 characters.".to_string(),
        );
    }
    let payload_items = payload.items.unwrap_or_default();
    if payload_items.is_empty() || payload_items.len() > 20 {
        problems.insert(
            "items".to_string(),
            "Add between 1 and 20 products.".to_string(),
        );
    }

    let items: Vec<azahar_orderintake::Line> = payload_items
        .into_iter()
        .map(|item| azahar_orderintake::Line {
            product_id: item.product_id.unwrap_or_default(),
            quantity: item.quantity.unwrap_or_default(),
            options: item
                .options
                .unwrap_or_default()
                .into_iter()
                .map(|(key, value)| (key, value.unwrap_or_default()))
                .collect(),
            expected_price_cents: None,
        })
        .collect();
    let (lines, item_fields) = azahar_orderintake::build_lines(items);
    problems.extend(item_fields);
    if !problems.is_empty() {
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            problems,
        ));
    }

    let name = if customer_name.is_empty() {
        "Counter sale".to_string()
    } else {
        customer_name
    };
    let token = new_token().map_err(|_| internal_error())?;
    let result = intake(&state)
        .place(azahar_orderintake::Request {
            lines,
            channel: azahar_orderintake::counter_channel(),
            customer: azahar_orderintake::Customer {
                name,
                phone,
                notes: note,
                ..azahar_orderintake::Customer::default()
            },
            destination: azahar_orderintake::Destination {
                delivery_type: "pickup".to_string(),
                ..azahar_orderintake::Destination::default()
            },
            status: "pending".to_string(),
            token_hash: hash_token(&token),
            actor_user_id: current.id.clone(),
        })
        .await
        .map_err(|error| intake_problem(&error))?;

    Ok(json_response(
        201,
        &json!({"order": to_order(&result.order, &result.items, &token, true)}),
    ))
}
