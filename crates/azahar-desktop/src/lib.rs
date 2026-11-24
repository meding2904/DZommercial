#![forbid(unsafe_code)]

//! Tauri 2 replacement for the Wails host in `desktop/main.go`.
//!
//! The renderer in `desktop/frontend` is unchanged. A Tauri initialization
//! script recreates `window.go.main.App` on top of `window.__TAURI__.core.invoke`
//! and maps the Wails positional arguments to the camelCase command arguments
//! Tauri expects. Rust errors reject with their message string, so the existing
//! `bridge.js` still wraps them in an `Error`.
//!
//! The API client lives in `azahar-desktop-api`; this crate owns only the host:
//! profile directory, window lifecycle, monitor handling, security headers and
//! the eight renderer-facing calls.

mod app;
mod bridge;
mod commands;
mod security;
mod window;

use std::process::ExitCode;
use std::sync::Arc;

use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};

pub use app::{
    App, DEFAULT_VERSION, Error, INVALID_EXTERNAL_URL, ProfileError, profile_directory,
    profile_directory_from, validate_external_url, version,
};
pub use bridge::{BRIDGE_SCRIPT, WAILS_METHODS};
pub use security::{
    CONTENT_SECURITY_POLICY, SECURITY_HEADERS, apply_security_headers, security_headers,
};
pub use window::{
    BACKGROUND, DEFAULT_HEIGHT, DEFAULT_WIDTH, MIN_HEIGHT, MIN_WIDTH, MonitorGeometry,
    WINDOW_TITLE, clamp_window_size, coordinate_to_logical, is_app_url, logical_inner_size,
    logical_window_bounds, monitor_geometry, physical_to_logical, screen_position_allowed,
};

/// Best-effort check for a session bus on Linux. The single-instance plugin
/// panics if zbus cannot connect, while the Wails host merely loses the lock.
#[cfg(target_os = "linux")]
fn session_bus_available() -> bool {
    if let Some(address) = std::env::var_os("DBUS_SESSION_BUS_ADDRESS")
        && !address.is_empty()
        && address.as_os_str() != std::ffi::OsStr::new("disabled:")
    {
        return true;
    }
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .is_some_and(|directory| directory.join("bus").exists())
}

#[cfg(not(target_os = "linux"))]
fn session_bus_available() -> bool {
    true
}

/// Port of `desktop.main`. Setup failures are printed by the binary and produce
/// a non-zero exit; Tauri owns the event loop after `build` succeeds.
pub fn run() -> Result<(), Error> {
    let app = Arc::new(App::new(version())?);
    let setup_app = Arc::clone(&app);
    let event_app = Arc::clone(&app);
    let exit_app = Arc::clone(&app);

    let single_instance = tauri_plugin_single_instance::init(|handle, _argv, _cwd| {
        if let Some(window) = handle.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    });

    let builder = tauri::Builder::default();
    // tauri-plugin-single-instance 2.3 unwraps its D-Bus session setup. Wails
    // logged and kept running when the session had no bus, so only install the
    // plugin when a bus is plausibly present; otherwise accept two instances
    // rather than panic during setup.
    let builder = if session_bus_available() {
        builder.plugin(single_instance)
    } else {
        eprintln!("azahar-admin: no D-Bus session bus; the single-instance lock is disabled.");
        builder
    };

    let tauri_app = builder
        .plugin(tauri_plugin_opener::init())
        .manage(Arc::clone(&app))
        .invoke_handler(tauri::generate_handler![
            commands::info,
            commands::health,
            commands::request,
            commands::image,
            commands::update_settings,
            commands::clear_session,
            commands::firewall_guide,
            commands::open_external,
        ])
        .setup(move |tauri_app| {
            setup_app.startup(tauri_app.handle().clone());
            let bounds = setup_app.window_bounds();
            let page_app = Arc::clone(&setup_app);

            WebviewWindowBuilder::new(tauri_app, "main", WebviewUrl::App("index.html".into()))
                .title(WINDOW_TITLE)
                .inner_size(bounds.width as f64, bounds.height as f64)
                .min_inner_size(MIN_WIDTH, MIN_HEIGHT)
                .background_color(BACKGROUND)
                .disable_drag_drop_handler()
                .initialization_script(BRIDGE_SCRIPT)
                .on_navigation(is_app_url)
                .on_new_window(|_url, _features| NewWindowResponse::Deny)
                .on_web_resource_request(security_headers)
                .on_page_load(move |window, payload| {
                    if payload.event() == PageLoadEvent::Finished && is_app_url(payload.url()) {
                        page_app.dom_ready(&window);
                    }
                })
                .build()?;

            Ok(())
        })
        .on_window_event(move |window, event| {
            if window.label() == "main" && matches!(event, WindowEvent::CloseRequested { .. }) {
                event_app.before_close(window);
            }
        })
        .build(tauri::generate_context!())?;

    tauri_app.run(move |_handle, event| {
        if matches!(event, RunEvent::Exit) {
            exit_app.clear_context();
        }
    });
    Ok(())
}

/// Binary entry point: a clear one-line error and a non-zero exit on failure.
pub fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("azahar-admin: {error}");
            ExitCode::FAILURE
        }
    }
}
