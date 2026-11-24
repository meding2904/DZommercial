//! Process-wide state shared by every request handler.
//!
//! The state is immutable after the server starts and cheap to clone: one
//! [`Arc`] per process. The plugin registry sits behind a [`RwLock`] because
//! build-time registration mutates it before the router exists, while handlers
//! only read it. The lock is the standard library's: reads never await, so a
//! blocking lock cannot stall a runtime worker.

use std::sync::{Arc, RwLock};

use azahar_config::Config;
use azahar_courier::Courier;
use azahar_plugins::Registry;
use azahar_store::Store;
use chrono::Duration as ChronoDuration;

use crate::ratelimit::RateLimiter;

/// One token bucket per client address for the whole API: 120 requests a
/// minute, matching `newRateLimiter(120, time.Minute)`.
pub const GENERAL_LIMIT: u32 = 120;
pub const GENERAL_WINDOW_SECS: u64 = 60;

/// Credential endpoints: 10 attempts per 15 minutes.
pub const AUTH_LIMIT: u32 = 10;
pub const AUTH_WINDOW_SECS: u64 = 15 * 60;

/// Checkout: 20 attempts per 10 minutes.
pub const CHECKOUT_LIMIT: u32 = 20;
pub const CHECKOUT_WINDOW_SECS: u64 = 10 * 60;

/// Captcha challenge issuance: 30 per 10 minutes.
pub const CAPTCHA_LIMIT: u32 = 30;
pub const CAPTCHA_WINDOW_SECS: u64 = 10 * 60;

/// Admin mutations: 60 per minute.
pub const ADMIN_LIMIT: u32 = 60;
pub const ADMIN_WINDOW_SECS: u64 = 60;

/// Cheap-to-clone handle to the server's shared state.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<AppStateInner>,
}

/// The data behind [`AppState`].
///
/// `shipping` is a trait object: [`azahar_courier::Courier`] is object-safe
/// because `async-trait` boxes its futures, so no enum is needed.
pub struct AppStateInner {
    pub cfg: Config,
    pub store: Arc<Store>,
    pub plugins: Arc<RwLock<Registry>>,
    pub shipping: Arc<dyn Courier>,
    pub general_limiter: RateLimiter,
    pub auth_limiter: RateLimiter,
    pub checkout_limiter: RateLimiter,
    pub captcha_limiter: RateLimiter,
    pub admin_limiter: RateLimiter,
}

impl AppState {
    /// Builds the state once, wiring every limiter with the Go limits.
    #[must_use]
    pub fn new(
        cfg: Config,
        store: Arc<Store>,
        plugins: Registry,
        shipping: Arc<dyn Courier>,
    ) -> Self {
        Self {
            inner: Arc::new(AppStateInner {
                cfg,
                store,
                plugins: Arc::new(RwLock::new(plugins)),
                shipping,
                general_limiter: RateLimiter::new(
                    GENERAL_LIMIT,
                    std::time::Duration::from_secs(GENERAL_WINDOW_SECS),
                ),
                auth_limiter: RateLimiter::new(
                    AUTH_LIMIT,
                    std::time::Duration::from_secs(AUTH_WINDOW_SECS),
                ),
                checkout_limiter: RateLimiter::new(
                    CHECKOUT_LIMIT,
                    std::time::Duration::from_secs(CHECKOUT_WINDOW_SECS),
                ),
                captcha_limiter: RateLimiter::new(
                    CAPTCHA_LIMIT,
                    std::time::Duration::from_secs(CAPTCHA_WINDOW_SECS),
                ),
                admin_limiter: RateLimiter::new(
                    ADMIN_LIMIT,
                    std::time::Duration::from_secs(ADMIN_WINDOW_SECS),
                ),
            }),
        }
    }

    /// The configured customer session lifetime as a `chrono` duration.
    #[must_use]
    pub fn session_ttl(&self) -> ChronoDuration {
        configured_ttl(&self.cfg)
    }
}

impl std::ops::Deref for AppState {
    type Target = AppStateInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// Converts the environment duration to the type `tower-sessions` and
/// `azahar_auth` use. `Config` already rejects negative values.
#[must_use]
pub fn configured_ttl(cfg: &Config) -> ChronoDuration {
    let seconds = i64::try_from(cfg.session_ttl.as_secs()).unwrap_or(i64::MAX);
    ChronoDuration::seconds(seconds.max(0))
}
