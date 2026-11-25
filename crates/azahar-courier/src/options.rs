//! Construction options and the default policy values they imply.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};

pub type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;
pub type FallbackCallback = Arc<dyn Fn(&str) + Send + Sync>;

pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(12);
pub(crate) const SANDBOX_MAX_TIMEOUT: Duration = Duration::from_secs(4);
pub(crate) const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
pub(crate) const FALLBACK_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
pub(crate) const OFFLINE_WINDOW: Duration = Duration::from_secs(2 * 60);

#[derive(Clone, Default)]
pub struct Options {
    pub base_url: String,
    pub courier_key: String,
    /// Sorted so the encoded request body matches Go's `encoding/json` map
    /// ordering.
    pub credentials: BTreeMap<String, String>,
    pub from_wilaya: i32,
    /// Zero means the default of 12 seconds.
    pub timeout: Duration,
    /// Zero means the default of 24 hours.
    pub cache_ttl: Duration,
    pub allow_local_sandbox: bool,
    pub on_fallback: Option<FallbackCallback>,
}

pub(crate) struct Settings {
    pub timeout: Duration,
    pub ttl: Duration,
    pub local_sandbox: bool,
}

impl Settings {
    pub(crate) fn from_options(options: &Options) -> Self {
        let mut timeout = if options.timeout.is_zero() {
            DEFAULT_TIMEOUT
        } else {
            options.timeout
        };
        let local_sandbox = options.allow_local_sandbox && options.courier_key.trim() == "sandbox";
        if local_sandbox && timeout > SANDBOX_MAX_TIMEOUT {
            timeout = SANDBOX_MAX_TIMEOUT;
        }
        let ttl = if options.cache_ttl.is_zero() {
            DEFAULT_CACHE_TTL
        } else {
            options.cache_ttl
        };
        Self {
            timeout,
            ttl,
            local_sandbox,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_twelve_seconds_and_one_day() {
        let settings = Settings::from_options(&Options::default());
        assert_eq!(settings.timeout, Duration::from_secs(12));
        assert_eq!(settings.ttl, Duration::from_secs(24 * 60 * 60));
        assert!(!settings.local_sandbox);
    }

    #[test]
    fn sandbox_clamps_the_timeout_to_four_seconds() {
        let options = Options {
            courier_key: "sandbox".into(),
            allow_local_sandbox: true,
            ..Options::default()
        };
        let settings = Settings::from_options(&options);
        assert!(settings.local_sandbox);
        assert_eq!(settings.timeout, Duration::from_secs(4));
    }

    #[test]
    fn a_short_timeout_is_not_stretched_by_the_sandbox_clamp() {
        let options = Options {
            courier_key: " sandbox ".into(),
            allow_local_sandbox: true,
            timeout: Duration::from_secs(2),
            ..Options::default()
        };
        let settings = Settings::from_options(&options);
        assert_eq!(settings.timeout, Duration::from_secs(2));
    }

    #[test]
    fn a_real_courier_keeps_the_configured_timeout() {
        let options = Options {
            courier_key: "yalidine".into(),
            allow_local_sandbox: true,
            timeout: Duration::from_secs(30),
            ..Options::default()
        };
        let settings = Settings::from_options(&options);
        assert!(!settings.local_sandbox);
        assert_eq!(settings.timeout, Duration::from_secs(30));
    }

    #[test]
    fn zero_timeouts_use_the_defaults() {
        let options = Options {
            timeout: Duration::ZERO,
            cache_ttl: Duration::ZERO,
            ..Options::default()
        };
        let settings = Settings::from_options(&options);
        assert_eq!(settings.timeout, DEFAULT_TIMEOUT);
        assert_eq!(settings.ttl, DEFAULT_CACHE_TTL);
    }

    #[test]
    fn a_custom_cache_ttl_is_kept() {
        let options = Options {
            cache_ttl: Duration::from_secs(60),
            ..Options::default()
        };
        let settings = Settings::from_options(&options);
        assert_eq!(settings.ttl, Duration::from_secs(60));
    }
}
