//! Data transfer objects, JSON field names and error envelopes for the
//! desktop API client.
//!
//! The field names mirror the Go structs in `desktop/internal/api/types.go`
//! exactly; the Wails bridge and the renderer depend on them.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Bounds of the desktop window, persisted in `settings.json`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowBounds {
    pub width: i64,
    pub height: i64,
    pub x: i64,
    pub y: i64,
}

/// Persisted desktop settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub api_url: String,
    pub origin: String,
    pub network_allowed: bool,
    pub network_allowed_host: String,
    pub window: WindowBounds,
}

/// Build metadata reported to the renderer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub version: String,
    pub platform: String,
    pub api_url: String,
    pub origin: String,
    pub resolved_origin: String,
    pub network_allowed: bool,
}

/// Result of a health probe.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub ok: bool,
    pub status: i32,
    pub env: String,
    pub origin: String,
    pub server_time: String,
    pub message: String,
}

/// API error envelope, also returned to the renderer as `Response.error`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{message}")]
pub struct ApiError {
    pub status: i32,
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
}

impl ApiError {
    pub fn new(status: i32, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
            fields: BTreeMap::new(),
        }
    }

    pub fn validation(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(0, code, message)
    }
}

/// Bridge response wrapping a decoded JSON value or an API error.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
}

impl Response {
    pub fn success(data: Option<serde_json::Value>) -> Self {
        Self {
            ok: true,
            data,
            error: None,
        }
    }

    pub fn failure(error: ApiError) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(error),
        }
    }
}

/// Outcome of `update_settings`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsResult {
    pub api_url: String,
    pub origin: String,
    pub resolved_origin: String,
    pub network_allowed: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub warning: String,
}

/// Platform-specific firewall instructions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirewallGuide {
    pub platform: String,
    pub title: String,
    pub intro: String,
    pub steps: Vec<String>,
    pub note: String,
}

impl ClientError {
    pub fn api(&self) -> Option<&ApiError> {
        match self {
            Self::Api(error) => Some(error),
            _ => None,
        }
    }
}

/// Failure of a secret store implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{message}")]
pub struct SecretError {
    pub message: String,
    /// True when the store did not answer before its timeout.
    pub timeout: bool,
}

impl SecretError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            timeout: false,
        }
    }

    pub fn timed_out() -> Self {
        Self {
            message: "secret store timed out".to_owned(),
            timeout: true,
        }
    }
}

/// Error returned by fallible client entry points.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error("profile directory is required")]
    ProfileDirectoryRequired,
    #[error("The settings could not be saved. Check that the profile folder is writable.")]
    SettingsNotSaved,
    #[error(transparent)]
    Secret(#[from] SecretError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Message(String),
}

/// `runtime.GOOS` spelling of the current platform.
pub fn go_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    }
}
