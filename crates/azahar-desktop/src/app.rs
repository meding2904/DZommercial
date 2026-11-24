//! The ported `desktop` package: profile resolution, the client handle, the
//! window lifecycle and the renderer-facing methods.
//!
//! Mapping from Go:
//!
//! | Go | Rust |
//! | --- | --- |
//! | `context.Context` passed to every lifecycle hook | `AppHandle` stored by `startup` |
//! | `WailsRuntime.Window*` on that context | `WebviewWindow` / `Window` from the hook |
//! | `App.ctxMu` | `Mutex<Option<AppHandle>>` |
//! | `api.Client` pointer | `Arc<Client>` shared with blocking commands |

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use tauri::{AppHandle, WebviewWindow, Window};

use azahar_desktop_api::{
    Client, ClientError, FirewallGuide, Health, Info, Response, SettingsResult, WindowBounds,
};
use serde_json::Value;
use url::Url;

use crate::window;

pub const DEFAULT_VERSION: &str = "2.0.0";
pub const INVALID_EXTERNAL_URL: &str = "only http and https links can be opened";

/// The `-ldflags -X main.version=...` equivalent: the build-time
/// `AZAHAR_VERSION` environment variable, else 2.0.0.
pub fn version() -> &'static str {
    match option_env!("AZAHAR_VERSION") {
        Some(value) if !value.is_empty() => value,
        _ => DEFAULT_VERSION,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("the profile directory could not be located: no user config directory")]
    ConfigDirectoryUnavailable,
    #[error("the profile directory could not be resolved: {0}")]
    Resolve(#[source] std::io::Error),
    #[error("the profile directory could not be created: {0}")]
    Create(#[source] std::io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Profile(#[from] ProfileError),
    #[error(transparent)]
    Client(#[from] ClientError),
    #[error(transparent)]
    Tauri(#[from] tauri::Error),
}

/// Port of `desktop.profileDirectory`.
///
/// `AZAHAR_PROFILE_DIR` is made absolute and wins. Otherwise the profile is
/// `<user config dir>/azahar-admin`. The directory is created (or tightened) to
/// 0700. `Client::new` repeats the same check, exactly like the Go client.
pub fn profile_directory() -> Result<PathBuf, ProfileError> {
    let override_dir = std::env::var_os("AZAHAR_PROFILE_DIR");
    profile_directory_from(override_dir.as_deref(), dirs::config_dir().as_deref())
}

/// Test seam for `profile_directory`.
pub fn profile_directory_from(
    override_dir: Option<&OsStr>,
    config_root: Option<&Path>,
) -> Result<PathBuf, ProfileError> {
    let directory = profile_path_from(override_dir, config_root)?;
    create_private_directory(&directory)?;
    Ok(directory)
}

fn profile_path_from(
    override_dir: Option<&OsStr>,
    config_root: Option<&Path>,
) -> Result<PathBuf, ProfileError> {
    match override_dir.filter(|value| !value.is_empty()) {
        Some(value) => std::path::absolute(Path::new(value)).map_err(ProfileError::Resolve),
        None => {
            let root = config_root.ok_or(ProfileError::ConfigDirectoryUnavailable)?;
            Ok(root.join("azahar-admin"))
        }
    }
}

fn create_private_directory(directory: &Path) -> Result<(), ProfileError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(directory).map_err(ProfileError::Create)?;
        // Go ignores the chmod error on an existing directory; do the same so
        // a profile on a filesystem without POSIX modes still opens.
        let _ = fs::set_permissions(directory, fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(directory).map_err(ProfileError::Create)?;
    }
    Ok(())
}

/// The desktop host. Tauri manages one `Arc<App>` as state.
pub struct App {
    client: Arc<Client>,
    handle: Mutex<Option<AppHandle>>,
}

impl App {
    /// Port of `desktop.NewApp`.
    pub fn new(version: impl Into<String>) -> Result<Self, Error> {
        let profile_dir = profile_directory()?;
        let client = Client::new(version, profile_dir)?;
        Ok(Self {
            client: Arc::new(client),
            handle: Mutex::new(None),
        })
    }

    /// Port of `desktop.startup`: store the Tauri app handle that replaces the
    /// Go `context.Context` held behind `ctxMu`.
    pub fn startup(&self, handle: AppHandle) {
        *self.lock_handle() = Some(handle);
    }

    /// Port of `desktop.context`. Every window operation in the Go host went
    /// through this context; here the equivalent is the `AppHandle`, and the
    /// live window is passed to `dom_ready` and `before_close` by Tauri.
    pub fn context(&self) -> Option<AppHandle> {
        self.lock_handle().clone()
    }

    /// Break the `App` -> `AppHandle` -> managed `App` reference cycle before
    /// Tauri tears the manager down.
    pub fn clear_context(&self) {
        *self.lock_handle() = None;
    }

    fn lock_handle(&self) -> MutexGuard<'_, Option<AppHandle>> {
        self.handle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Port of `desktop.windowBounds`.
    pub fn window_bounds(&self) -> WindowBounds {
        self.client.window_bounds()
    }

    /// Port of `desktop.screenPositionAllowed` with Tauri monitors.
    pub fn screen_position_allowed(&self, window: &WebviewWindow, bounds: WindowBounds) -> bool {
        let current = window.current_monitor().ok().flatten();
        let primary = window.primary_monitor().ok().flatten();
        window::screen_position_allowed(
            bounds.x,
            bounds.y,
            current.as_ref().and_then(window::monitor_geometry),
            primary.as_ref().and_then(window::monitor_geometry),
        )
    }

    /// Port of `desktop.clampWindowSize`. Wails compared logical sizes; Tauri
    /// reports the window and monitor in physical pixels, so both sides are
    /// converted with their own scale factor before the minimum is applied.
    pub fn clamp_window_size(&self, window: &WebviewWindow) {
        let Some(monitor) = window.current_monitor().ok().flatten() else {
            return;
        };
        let Some(screen) = window::monitor_geometry(&monitor) else {
            return;
        };
        if screen.width <= 0 || screen.height <= 0 {
            return;
        }
        let Some((width, height)) = window::logical_inner_size(window) else {
            return;
        };
        let (next_width, next_height) =
            window::clamp_window_size(width, height, screen.width, screen.height);
        if next_width != width || next_height != height {
            let _ = window.set_size(tauri::LogicalSize::new(
                next_width as f64,
                next_height as f64,
            ));
        }
    }

    /// Port of `desktop.domReady`: restore a saved position only when the
    /// coarse screen check allows it, then clamp the size to the monitor.
    pub fn dom_ready(&self, window: &WebviewWindow) {
        let bounds = self.window_bounds();
        if (bounds.x != 0 || bounds.y != 0) && self.screen_position_allowed(window, bounds) {
            let _ = window.set_position(tauri::LogicalPosition::new(
                bounds.x as f64,
                bounds.y as f64,
            ));
        }
        self.clamp_window_size(window);
    }

    /// Port of `desktop.beforeClose`. Maximised and minimised windows keep the
    /// previous bounds, exactly like the Go host. Nothing calls
    /// `prevent_close`, so quit is never blocked.
    pub fn before_close(&self, window: &Window) {
        if window.is_maximized().unwrap_or(false) || window.is_minimized().unwrap_or(false) {
            return;
        }
        let Some(bounds) = window::logical_window_bounds(window) else {
            return;
        };
        if bounds.width <= 0 || bounds.height <= 0 {
            return;
        }
        self.client.save_window_bounds(bounds);
    }

    /// Port of `desktop.Info`.
    pub fn info(&self) -> Info {
        self.client.info()
    }

    /// Port of `desktop.Health`.
    pub fn health(&self) -> Health {
        self.client.health()
    }

    /// Port of `desktop.Request`.
    pub fn request(&self, method: &str, path: &str, body: Option<&Value>) -> Response {
        self.client.request(method, path, body)
    }

    /// Port of `desktop.Image`.
    pub fn image(&self, path: &str) -> Response {
        self.client.image(path)
    }

    /// Port of `desktop.UpdateSettings`.
    pub fn update_settings(
        &self,
        api_url: &str,
        origin: &str,
        allow_network: bool,
    ) -> Result<SettingsResult, ClientError> {
        self.client.update_settings(api_url, origin, allow_network)
    }

    /// Port of `desktop.ClearSession`.
    pub fn clear_session(&self) -> Result<(), ClientError> {
        self.client.clear_session()
    }

    /// Port of `desktop.FirewallGuide`.
    pub fn firewall_guide(&self) -> FirewallGuide {
        self.client.firewall_guide()
    }

    /// Port of `desktop.OpenExternal`.
    ///
    /// The URL checks are the Go ones: http(s), a non-empty host and no
    /// userinfo. A missing Tauri context returns the same error the Go host
    /// returned before startup. Opening a valid URL is fire-and-forget like
    /// `wailsRuntime.BrowserOpenURL`; a process-launch failure is logged but
    /// not surfaced as a bridge rejection.
    pub fn open_external(&self, raw_url: &str) -> Result<(), &'static str> {
        let target = validate_external_url(raw_url)?;
        if self.context().is_none() {
            return Err(INVALID_EXTERNAL_URL);
        }
        if let Err(error) = tauri_plugin_opener::open_url(target, None::<&str>) {
            eprintln!("azahar-admin: could not open the system browser: {error}");
        }
        Ok(())
    }

    /// The `Arc<Client>` the blocking command wrappers move to a worker thread.
    pub(crate) fn client(&self) -> Arc<Client> {
        Arc::clone(&self.client)
    }
}

/// Validate an `OpenExternal` target and return the normalised URL.
pub fn validate_external_url(raw_url: &str) -> Result<String, &'static str> {
    let (scheme, _) = raw_url.split_once("://").ok_or(INVALID_EXTERNAL_URL)?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(INVALID_EXTERNAL_URL);
    }

    let parsed = Url::parse(raw_url).map_err(|_| INVALID_EXTERNAL_URL)?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(INVALID_EXTERNAL_URL);
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return Err(INVALID_EXTERNAL_URL);
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(INVALID_EXTERNAL_URL);
    }
    // `Url` normalises `https://@example.com` to an empty username, which the
    // Go check (`parsed.User != nil`) still rejects. Any literal `@` in the
    // authority means userinfo was present.
    if authority_has_userinfo(raw_url) {
        return Err(INVALID_EXTERNAL_URL);
    }
    Ok(parsed.to_string())
}

fn authority_has_userinfo(raw_url: &str) -> bool {
    let Some((_, rest)) = raw_url.split_once("://") else {
        return false;
    };
    rest.split(['/', '?', '#'])
        .next()
        .is_some_and(|authority| authority.contains('@'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_uses_the_config_directory_by_default() {
        let temp = tempfile::tempdir().unwrap();
        let dir = profile_directory_from(None, Some(temp.path())).unwrap();
        assert_eq!(dir, temp.path().join("azahar-admin"));
        assert!(dir.is_dir());
    }

    #[test]
    fn profile_override_wins_and_is_absolute() {
        let temp = tempfile::tempdir().unwrap();
        let override_dir = temp.path().join("custom-profile");
        let config_root = temp.path().join("config");
        let dir =
            profile_directory_from(Some(override_dir.as_os_str()), Some(&config_root)).unwrap();
        assert_eq!(dir, override_dir);
        assert!(dir.is_dir());
    }

    #[test]
    fn a_relative_override_is_made_absolute_without_creating_it() {
        let resolved = profile_path_from(Some(OsStr::new("relative-profile")), None).unwrap();
        assert!(resolved.is_absolute());
        assert!(resolved.ends_with("relative-profile"));
    }

    #[test]
    fn an_empty_override_falls_back_to_the_config_directory() {
        let temp = tempfile::tempdir().unwrap();
        let dir = profile_directory_from(Some(OsStr::new("")), Some(temp.path())).unwrap();
        assert_eq!(dir, temp.path().join("azahar-admin"));
    }

    #[test]
    fn a_missing_config_directory_is_an_error() {
        let error = profile_directory_from(None, None).unwrap_err();
        assert!(matches!(error, ProfileError::ConfigDirectoryUnavailable));
    }

    #[cfg(unix)]
    #[test]
    fn the_profile_is_created_with_mode_0700() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let dir = profile_directory_from(None, Some(temp.path())).unwrap();
        let mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn version_defaults_to_two_point_zero() {
        let expected = option_env!("AZAHAR_VERSION")
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_VERSION);
        assert_eq!(version(), expected);
        assert!(!version().is_empty());
    }

    #[test]
    fn external_urls_must_be_http_and_credential_free() {
        for raw in [
            "",
            "mailto:admin@example.com",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ftp://example.com/file",
            "https://user:pass@example.com",
            "https://user@example.com",
            "https://@example.com",
            "https://",
            "https://:443",
            "http:example.com",
            "//example.com",
            "https://exa mple.com",
            "https://example.com\\@evil.example",
        ] {
            assert!(validate_external_url(raw).is_err(), "{raw:?} was accepted");
        }
    }

    #[test]
    fn external_urls_are_normalised() {
        let cases = [
            ("https://example.com", "https://example.com/"),
            (
                "https://EXAMPLE.com/Path?q=1#top",
                "https://example.com/Path?q=1#top",
            ),
            ("http://127.0.0.1:8080/", "http://127.0.0.1:8080/"),
            ("https://[::1]/admin", "https://[::1]/admin"),
        ];
        for (raw, expected) in cases {
            assert_eq!(validate_external_url(raw).unwrap(), expected, "{raw}");
        }
    }

    #[test]
    fn the_open_external_error_is_the_go_message() {
        assert_eq!(
            validate_external_url("file:///etc/passwd").unwrap_err(),
            "only http and https links can be opened"
        );
    }
}
