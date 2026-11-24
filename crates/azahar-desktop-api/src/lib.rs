//! Rust port of `desktop/internal/api`.
//!
//! The crate keeps the Go client's observable behaviour: URL and origin
//! canonicalisation, the host consent policy, the `/api/` path guard, CSRF
//! cookie/token handling, the session secret store (OS keyring with a scoped
//! file fallback), settings persistence and the bounded image cache.
//!
//! The Wails host in `desktop/main.go` is not ported here; this crate is the
//! layer a Rust desktop shell would call.

mod client;
mod environment;
mod firewall;
mod health;
mod image;
mod request;
mod session;
mod transport;
mod types;
mod urls;

pub use client::{
    Client, ClientOptions, DEFAULT_IMAGE_CACHE_BYTES, DEFAULT_IMAGE_TIMEOUT,
    DEFAULT_REQUEST_TIMEOUT, DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH, IMAGE_CACHE_SIZE,
    MAX_IMAGE_BYTES, MAX_REQUEST_BODY_BYTES, MAX_RESPONSE_BYTES, SecretStoreFactory,
};
pub use environment::Environment;
pub use firewall::firewall_guide;
pub use request::{HeaderInput, build_headers, is_timeout};
pub use session::{
    CSRF_COOKIE_NAME, FallbackSecretStore, FileSecretStore, KEYRING_SERVICE, KEYRING_TIMEOUT,
    KeyringSecretStore, MemorySecretStore, SESSION_COOKIE_NAME, SecretStore, StoredCookie,
    TimeoutSecretStore, clean_path, cookie_from_set_cookie, default_secret_store, device_hash,
    env_enabled, session_scope, valid_cookie_value, write_private_file,
};
pub use transport::{
    ReqwestTransport, Transport, TransportError, TransportRequest, TransportResponse,
    default_transport,
};
pub use types::{
    ApiError, ClientError, FirewallGuide, Health, Info, Response, SecretError, Settings,
    SettingsResult, WindowBounds, go_platform,
};

/// Pure helpers used by the differential harness.
#[doc(hidden)]
pub mod harness {
    pub use crate::request::{
        HttpResult, decode_json, encode_request_body, error_code, error_from_result,
    };
}
pub use urls::{
    ApiPath, DEFAULT_API_URL, FALLBACK_ORIGIN, MAX_API_PATH_LENGTH, ParsedUrl, TargetUrl,
    UrlParseError, api_host, canonical_origin, consent_host_for_update, contains_control, hostname,
    invalid_api_url, invalid_origin, invalid_path, is_csrf_endpoint, is_local_host,
    is_loopback_host, network_consent_error, normalize_api_url, normalize_host, normalize_origin,
    parse_url, port, settings_consent_allowed, target_url_with, validate_api_path,
    validate_server_address,
};
