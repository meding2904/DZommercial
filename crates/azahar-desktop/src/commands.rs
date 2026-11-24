//! Tauri commands exposed to the Wails compatibility shim.
//!
//! Every network or process call goes through `spawn_blocking`; the managed
//! `Arc<App>` or `Arc<Client>` is cloned first. `State` is a shared reference,
//! not a lock, so no client or manager lock is held across a suspension point.
//! `Info` and `FirewallGuide` stay synchronous because they only read memory.

use std::sync::Arc;

use serde_json::Value;
use tauri::State;

use azahar_desktop_api::{ApiError, FirewallGuide, Health, Info, Response, SettingsResult};

use crate::App;

const BRIDGE_FAILED: &str = "The app bridge did not answer.";

fn join_error(error: tauri::Error) -> String {
    format!("{BRIDGE_FAILED} {error}")
}

#[tauri::command]
pub(crate) fn info(state: State<'_, Arc<App>>) -> Info {
    state.info()
}

#[tauri::command]
pub(crate) async fn health(state: State<'_, Arc<App>>) -> Result<Health, String> {
    let client = state.client();
    match tauri::async_runtime::spawn_blocking(move || client.health()).await {
        Ok(health) => Ok(health),
        Err(error) => Ok(Health {
            ok: false,
            status: 0,
            message: join_error(error),
            ..Health::default()
        }),
    }
}

#[tauri::command]
pub(crate) async fn request(
    state: State<'_, Arc<App>>,
    method: String,
    path: String,
    body: Option<Value>,
) -> Result<Response, String> {
    let client = state.client();
    match tauri::async_runtime::spawn_blocking(move || {
        client.request(&method, &path, body.as_ref())
    })
    .await
    {
        Ok(response) => Ok(response),
        Err(error) => Ok(Response::failure(ApiError::new(
            0,
            "bridge_failed",
            join_error(error),
        ))),
    }
}

#[tauri::command]
pub(crate) async fn image(state: State<'_, Arc<App>>, path: String) -> Result<Response, String> {
    let client = state.client();
    match tauri::async_runtime::spawn_blocking(move || client.image(&path)).await {
        Ok(response) => Ok(response),
        Err(error) => Ok(Response::failure(ApiError::new(
            0,
            "bridge_failed",
            join_error(error),
        ))),
    }
}

#[tauri::command]
pub(crate) async fn update_settings(
    state: State<'_, Arc<App>>,
    api_url: String,
    origin: String,
    allow_network: bool,
) -> Result<SettingsResult, String> {
    let client = state.client();
    tauri::async_runtime::spawn_blocking(move || {
        client.update_settings(&api_url, &origin, allow_network)
    })
    .await
    .map_err(join_error)?
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn clear_session(state: State<'_, Arc<App>>) -> Result<(), String> {
    let client = state.client();
    tauri::async_runtime::spawn_blocking(move || client.clear_session())
        .await
        .map_err(join_error)?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn firewall_guide(state: State<'_, Arc<App>>) -> FirewallGuide {
    state.firewall_guide()
}

#[tauri::command]
pub(crate) async fn open_external(state: State<'_, Arc<App>>, url: String) -> Result<(), String> {
    let app = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || app.open_external(&url))
        .await
        .map_err(join_error)?
        .map_err(|message| message.to_owned())
}
