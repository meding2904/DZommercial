//! Process environment captured once per client.
//!
//! Go reads the environment through `os.Getenv` at load time. Rust tests cannot
//! mutate the process environment without `unsafe` in edition 2024, so the
//! values are captured into this plain struct. Production callers use
//! [`Environment::from_process`]; tests build one explicitly.

use std::env;
use std::path::PathBuf;

/// Environment inputs used while loading settings and the secret store.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Environment {
    pub api_url: Option<String>,
    pub origin: Option<String>,
    pub session_backend: Option<String>,
    pub disable_keyring: Option<String>,
    pub config_home: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub app_data: Option<PathBuf>,
}

impl Environment {
    pub fn from_process() -> Self {
        Self {
            api_url: env::var_os("AZAHAR_API_URL")
                .map(|value| value.to_string_lossy().into_owned()),
            origin: env::var_os("AZAHAR_ORIGIN").map(|value| value.to_string_lossy().into_owned()),
            session_backend: env::var_os("AZAHAR_SESSION_BACKEND")
                .map(|value| value.to_string_lossy().into_owned()),
            disable_keyring: env::var_os("AZAHAR_DISABLE_KEYRING")
                .map(|value| value.to_string_lossy().into_owned()),
            config_home: env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            home: env::var_os("HOME").map(PathBuf::from),
            app_data: env::var_os("APPDATA").map(PathBuf::from),
        }
    }

    pub fn api_url(mut self, value: impl Into<String>) -> Self {
        self.api_url = Some(value.into());
        self
    }

    pub fn origin(mut self, value: impl Into<String>) -> Self {
        self.origin = Some(value.into());
        self
    }

    pub fn session_backend(mut self, value: impl Into<String>) -> Self {
        self.session_backend = Some(value.into());
        self
    }

    pub fn disable_keyring(mut self, value: impl Into<String>) -> Self {
        self.disable_keyring = Some(value.into());
        self
    }

    pub fn config_home(mut self, value: impl Into<PathBuf>) -> Self {
        self.config_home = Some(value.into());
        self
    }

    pub fn home(mut self, value: impl Into<PathBuf>) -> Self {
        self.home = Some(value.into());
        self
    }

    /// `os.UserConfigDir`, including its absolute-path rule for XDG.
    pub fn user_config_dir(&self) -> Option<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            return self
                .app_data
                .clone()
                .filter(|path| !path.as_os_str().is_empty());
        }
        #[cfg(target_os = "macos")]
        {
            let home = self.home.clone()?;
            return Some(home.join("Library").join("Application Support"));
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            if let Some(config) = &self.config_home {
                return if config.is_absolute() {
                    Some(config.clone())
                } else {
                    None
                };
            }
            let home = self.home.clone()?;
            Some(home.join(".config"))
        }
    }
}
