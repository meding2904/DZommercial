//! Window geometry helpers and the Tauri monitor translation.
//!
//! Wails reports both window bounds and screen sizes in logical pixels. Tauri
//! reports physical pixels and a per-monitor (or per-window) scale factor, so
//! every value crossing that boundary is converted here. The persisted
//! `settings.json` keeps Wails' logical units: a profile copied from the Go
//! host opens at the same place and size.

use tauri::window::Color;
use tauri::{Monitor, WebviewWindow};

use azahar_desktop_api::WindowBounds;

pub const WINDOW_TITLE: &str = "Azahar admin";
pub const DEFAULT_WIDTH: f64 = 1380.0;
pub const DEFAULT_HEIGHT: f64 = 900.0;
pub const MIN_WIDTH: f64 = 1080.0;
pub const MIN_HEIGHT: f64 = 720.0;
/// The Go host's `options.RGBA{R: 246, G: 240, B: 235, A: 1}`.
pub const BACKGROUND: Color = Color(246, 240, 235, 255);

/// A monitor size in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorGeometry {
    pub width: i64,
    pub height: i64,
}

/// Convert a physical dimension to the logical units Wails used.
///
/// Tauri's scale factor is physical/logical. A non-finite, zero or negative
/// factor is rejected rather than producing a bogus window size.
pub fn physical_to_logical(value: u32, scale: f64) -> Option<i64> {
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let logical = (f64::from(value) / scale).round();
    if !logical.is_finite() || logical <= 0.0 {
        return None;
    }
    Some(logical as i64)
}

/// Convert a physical coordinate to a logical one. Coordinates may be negative
/// on a multi-monitor desktop.
pub fn coordinate_to_logical(value: i32, scale: f64) -> Option<i64> {
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let logical = (f64::from(value) / scale).round();
    if !logical.is_finite() {
        return None;
    }
    Some(logical as i64)
}

/// Logical size of a Tauri monitor.
pub fn monitor_geometry(monitor: &Monitor) -> Option<MonitorGeometry> {
    let size = monitor.size();
    Some(MonitorGeometry {
        width: physical_to_logical(size.width, monitor.scale_factor())?,
        height: physical_to_logical(size.height, monitor.scale_factor())?,
    })
}

/// Port of `desktop.screenPositionAllowed`.
///
/// Wails' screen list has no monitor origins, so the Go check only asks whether
/// a saved position is non-negative and inside the current (else primary)
/// screen. Tauri exposes origins, but the coarse rule is kept: dropping a
/// position on a second monitor is the documented Wails trade-off and changing
/// it would move windows the Go host would have left alone.
pub fn screen_position_allowed(
    x: i64,
    y: i64,
    current: Option<MonitorGeometry>,
    primary: Option<MonitorGeometry>,
) -> bool {
    if x < 0 || y < 0 {
        return false;
    }
    let target = match current.or(primary) {
        Some(target) => target,
        None => return true,
    };
    if target.width <= 0 || target.height <= 0 {
        return true;
    }
    x <= target.width && y <= target.height
}

/// Port of `desktop.clampWindowSize`.
///
/// Returns the input unchanged when Wails would have returned early (no screen,
/// no window, or a non-positive dimension), otherwise the component-wise
/// minimum. The caller applies the result only when it changed.
pub fn clamp_window_size(
    width: i64,
    height: i64,
    screen_width: i64,
    screen_height: i64,
) -> (i64, i64) {
    if width <= 0 || height <= 0 || screen_width <= 0 || screen_height <= 0 {
        return (width, height);
    }
    (width.min(screen_width), height.min(screen_height))
}

/// Whether navigation stays inside the bundled app.
///
/// Tauri serves the renderer from `tauri://localhost` on Linux and macOS and
/// `http(s)://tauri.localhost` on Windows and Android. The renderer is a hash
/// router, so its own hash changes keep the same URL and stay allowed.
pub fn is_app_url(url: &url::Url) -> bool {
    match url.scheme() {
        "tauri" => url.host_str() == Some("localhost"),
        "http" | "https" => url.host_str() == Some("tauri.localhost"),
        _ => false,
    }
}

/// Read the window size and position from Tauri, in Wails' logical units.
pub fn logical_window_bounds(window: &tauri::Window) -> Option<WindowBounds> {
    let size = window.inner_size().ok()?;
    let scale = window.scale_factor().ok()?;
    let position = window
        .outer_position()
        .unwrap_or(tauri::PhysicalPosition::new(0, 0));

    let width = physical_to_logical(size.width, scale)?;
    let height = physical_to_logical(size.height, scale)?;
    let x = coordinate_to_logical(position.x, scale)?;
    let y = coordinate_to_logical(position.y, scale)?;
    Some(WindowBounds {
        width,
        height,
        x,
        y,
    })
}

/// Logical inner size of a live Tauri window.
pub fn logical_inner_size(window: &WebviewWindow) -> Option<(i64, i64)> {
    let size = window.inner_size().ok()?;
    let scale = window
        .scale_factor()
        .ok()
        .filter(|scale| scale.is_finite() && *scale > 0.0)?;
    Some((
        physical_to_logical(size.width, scale)?,
        physical_to_logical(size.height, scale)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(width: i64, height: i64) -> Option<MonitorGeometry> {
        Some(MonitorGeometry { width, height })
    }

    #[test]
    fn window_defaults_match_the_wails_options() {
        assert_eq!(WINDOW_TITLE, "Azahar admin");
        assert_eq!((DEFAULT_WIDTH, DEFAULT_HEIGHT), (1380.0, 900.0));
        assert_eq!((MIN_WIDTH, MIN_HEIGHT), (1080.0, 720.0));
        assert_eq!(BACKGROUND, Color(246, 240, 235, 255));
    }

    #[test]
    fn physical_sizes_become_logical() {
        assert_eq!(physical_to_logical(3840, 2.0), Some(1920));
        assert_eq!(physical_to_logical(1366, 1.0), Some(1366));
        assert_eq!(physical_to_logical(1600, 1.25), Some(1280));
        assert_eq!(physical_to_logical(0, 1.0), None);
        assert_eq!(physical_to_logical(100, 0.0), None);
        assert_eq!(physical_to_logical(100, -1.0), None);
        assert_eq!(physical_to_logical(100, f64::NAN), None);
        assert_eq!(physical_to_logical(100, f64::INFINITY), None);
    }

    #[test]
    fn coordinates_keep_their_sign() {
        assert_eq!(coordinate_to_logical(-1920, 1.0), Some(-1920));
        assert_eq!(coordinate_to_logical(-1920, 2.0), Some(-960));
        assert_eq!(coordinate_to_logical(0, 1.0), Some(0));
        assert_eq!(coordinate_to_logical(10, 0.0), None);
    }

    #[test]
    fn saved_position_must_be_non_negative() {
        assert!(!screen_position_allowed(-1, 0, monitor(1920, 1080), None));
        assert!(!screen_position_allowed(0, -1, monitor(1920, 1080), None));
        assert!(screen_position_allowed(0, 0, monitor(1920, 1080), None));
    }

    #[test]
    fn saved_position_uses_current_then_primary() {
        let current = monitor(1600, 900);
        let primary = monitor(3840, 2160);
        assert!(screen_position_allowed(1600, 900, current, primary));
        assert!(!screen_position_allowed(1601, 900, current, primary));
        assert!(screen_position_allowed(3840, 2160, None, primary));
        assert!(!screen_position_allowed(3841, 0, None, primary));
    }

    #[test]
    fn missing_or_empty_monitor_allows_the_position() {
        assert!(screen_position_allowed(i64::MAX, i64::MAX, None, None));
        assert!(screen_position_allowed(5000, 5000, monitor(0, 0), None));
        assert!(screen_position_allowed(5000, 5000, monitor(-1, 1080), None));
    }

    #[test]
    fn clamp_matches_the_go_minimum() {
        assert_eq!(clamp_window_size(1380, 900, 1920, 1080), (1380, 900));
        assert_eq!(clamp_window_size(2400, 1400, 1920, 1080), (1920, 1080));
        assert_eq!(clamp_window_size(2400, 900, 1920, 1080), (1920, 900));
        assert_eq!(clamp_window_size(0, 900, 1920, 1080), (0, 900));
        assert_eq!(clamp_window_size(1380, 900, 0, 0), (1380, 900));
    }

    #[test]
    fn only_the_bundled_origin_is_navigable() {
        for raw in [
            "tauri://localhost/index.html",
            "tauri://localhost/#/ledger",
            "http://tauri.localhost/",
            "https://tauri.localhost/index.html#/orders",
        ] {
            assert!(is_app_url(&url::Url::parse(raw).unwrap()), "{raw}");
        }
        for raw in [
            "https://example.com/",
            "http://localhost:5173/",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "about:blank",
        ] {
            assert!(!is_app_url(&url::Url::parse(raw).unwrap()), "{raw}");
        }
    }
}
