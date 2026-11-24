//! Explicit dispatch for the plugin routes the server ships.
//!
//! The Go registry mounts each plugin's `Route` metadata. The Rust port keeps
//! routing in `azahar-httpapi` and maps `(plugin id, method, pattern)` to the
//! handler below, so the plugin crates stay free of axum types.

use std::collections::HashMap;

use axum::Router;
use axum::body::to_bytes;
use axum::extract::{Path, Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::{from_fn, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get};
use serde::Deserialize;

use azahar_plugin_pcbuilder::{PC_PARTS_PLUGIN_ID, build_parts_response};
use azahar_plugin_phone::{CreateDevicePayload, PhoneService};
use azahar_plugins::Problem as PluginProblem;
use azahar_store::ProductFilter;

use crate::middleware::{admin_mutation_limit, require_admin, require_staff, require_user};
use crate::respond::{ApiProblem, json_response};
use crate::state::AppState;

const MAX_PARTS: usize = 500;

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct CreateDeviceRequest {
    product_id: String,
    config: String,
    color: String,
    imei: String,
}

fn plugin_problem(problem: PluginProblem) -> ApiProblem {
    ApiProblem {
        status: StatusCode::from_u16(problem.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        code: problem.code,
        message: problem.message,
        fields: problem.fields,
    }
}

fn internal_problem() -> ApiProblem {
    ApiProblem::new(500, "internal_error", "Something went wrong.")
}

fn plugin_enabled(state: &AppState, plugin_id: &str) -> Result<(), ApiProblem> {
    let registry = state
        .plugins
        .read()
        .unwrap_or_else(|poison| poison.into_inner());
    if registry.effective_enabled(plugin_id) {
        Ok(())
    } else {
        Err(ApiProblem::new(404, "not_found", "Plugin not found."))
    }
}

/// `GET /api/plugins/pcbuilder/parts`.
async fn parts(State(state): State<AppState>) -> Result<Response, ApiProblem> {
    plugin_enabled(&state, "pcbuilder")?;
    let categories = state
        .store
        .categories()
        .await
        .map_err(|_| internal_problem())?;
    let mut products = Vec::new();
    for category in &categories {
        if category.plugin_id != PC_PARTS_PLUGIN_ID {
            continue;
        }
        let items = state
            .store
            .list_products(&ProductFilter {
                category: category.slug.clone(),
                limit: MAX_PARTS as i32,
                ..ProductFilter::default()
            })
            .await
            .map_err(|_| internal_problem())?;
        products.extend(items);
    }
    Ok(json_response(
        200,
        &build_parts_response(&categories, &products),
    ))
}

/// `GET /api/plugins/phone/devices[?product_id=]`.
async fn phone_devices(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiProblem> {
    plugin_enabled(&state, "phone")?;
    let product_id = query.get("product_id").cloned().unwrap_or_default();
    let response = PhoneService::new()
        .list_devices(state.store.conn(), &product_id)
        .await
        .map_err(plugin_problem)?;
    Ok(json_response(200, &response))
}

/// `POST /api/plugins/phone/devices`.
async fn phone_create_device(
    State(state): State<AppState>,
    request: Request,
) -> Result<Response, ApiProblem> {
    plugin_enabled(&state, "phone")?;
    let payload = decode_phone_body(request).await?;
    let device = PhoneService::new()
        .create_device(
            state.store.conn(),
            &CreateDevicePayload {
                product_id: payload.product_id,
                config: payload.config,
                color: payload.color,
                imei: payload.imei,
            },
        )
        .await
        .map_err(plugin_problem)?;
    Ok(json_response(201, &serde_json::json!({ "device": device })))
}

/// `DELETE /api/plugins/phone/devices/{id}`.
async fn phone_delete_device(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiProblem> {
    plugin_enabled(&state, "phone")?;
    PhoneService::new()
        .delete_device(state.store.conn(), &id)
        .await
        .map_err(plugin_problem)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `GET /api/plugins/phone/orders/{order_id}/devices`.
async fn phone_order_devices(
    State(state): State<AppState>,
    Path(order_id): Path<String>,
) -> Result<Response, ApiProblem> {
    plugin_enabled(&state, "phone")?;
    let response = PhoneService::new()
        .order_devices(state.store.conn(), &order_id)
        .await
        .map_err(plugin_problem)?;
    Ok(json_response(200, &response))
}

/// The public plugin route(s), outside the admin group.
pub fn public_plugin_routes() -> Router<AppState> {
    Router::new().route("/api/plugins/pcbuilder/parts", get(parts))
}

/// The admin plugin routes, with the same guard chain as Go's mounted admin
/// routes.
pub fn admin_plugin_routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/api/plugins/phone/admin/devices",
            get(phone_devices).post(phone_create_device),
        )
        .route(
            "/api/plugins/phone/admin/devices/{id}",
            delete(phone_delete_device),
        )
        .route(
            "/api/plugins/phone/admin/orders/{orderID}/devices",
            get(phone_order_devices),
        )
        .route_layer(from_fn_with_state(state.clone(), admin_mutation_limit))
        .route_layer(from_fn(require_admin))
        .route_layer(from_fn(require_staff))
        .route_layer(from_fn_with_state(state, require_user))
}

/// Explicit replacement for Go's `mountPluginRoutes`: every shipped plugin's
/// routes are merged here, with the admin routes already carrying their guard
/// chain.
pub fn mount_plugin_routes(state: AppState) -> Router<AppState> {
    public_plugin_routes().merge(admin_plugin_routes(state))
}

/// The plugin-enabled gate Go applies in `pluginHandler` before dispatch.
pub fn plugin_handler(state: &AppState, plugin_id: &str) -> Result<(), ApiProblem> {
    plugin_enabled(state, plugin_id)
}

/// Go `plugins.DecodeBody` for the phone route: `application/json*`, 1 MiB,
/// case-insensitive keys, null -> zero value, unknown fields refused.
async fn decode_phone_body(request: Request) -> Result<CreateDeviceRequest, ApiProblem> {
    let content_type = request
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if !content_type.starts_with("application/json") {
        return Err(ApiProblem::new(
            415,
            "unsupported_media_type",
            "Send the request body as application/json.",
        ));
    }
    let bytes = to_bytes(request.into_body(), 1 << 20)
        .await
        .map_err(|_| ApiProblem::new(413, "payload_too_large", "The request body is too large."))?;
    if String::from_utf8_lossy(&bytes).trim().is_empty() {
        return Err(ApiProblem::with_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            HashMap::from([("body".to_string(), "Send a JSON object.".to_string())]),
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| shape_problem())?;
    if value.is_null() {
        return Ok(CreateDeviceRequest::default());
    }
    let Some(object) = value.as_object() else {
        return Err(shape_problem());
    };
    let mut payload = CreateDeviceRequest::default();
    for (key, item) in object {
        let target = match key.to_ascii_lowercase().as_str() {
            "product_id" => &mut payload.product_id,
            "config" => &mut payload.config,
            "color" => &mut payload.color,
            "imei" => &mut payload.imei,
            _ => return Err(shape_problem()),
        };
        *target = match item {
            serde_json::Value::Null => String::new(),
            serde_json::Value::String(value) => value.clone(),
            _ => return Err(shape_problem()),
        };
    }
    Ok(payload)
}

fn shape_problem() -> ApiProblem {
    ApiProblem::new(
        422,
        "invalid_json",
        "The request body does not match the expected shape.",
    )
}
