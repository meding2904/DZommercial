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
    use std::collections::HashMap;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::{Value, json};

    use crate::client::Client;
    use crate::environment::Environment;
    use crate::session::StoredCookie;
    use crate::types::{ApiError, Settings};

    pub use crate::request::{
        HttpResult, decode_json, encode_request_body, error_code, error_from_result,
    };

    pub fn decode_setting(document: &[u8], key: &str, kind: &str) -> (Value, bool) {
        crate::client::decode_setting_value(document, key, kind)
    }

    pub fn save_settings(path: &Path, settings: &Settings) -> Result<(), String> {
        crate::client::save_settings(path, settings).map_err(|error| error.to_string())
    }

    pub fn load_legacy_settings(environment: &Environment) -> Option<Settings> {
        crate::client::load_legacy_settings(environment)
    }

    pub fn ensure_origin(client: &Client) -> Result<(), ApiError> {
        client.ensure_origin()
    }

    pub fn ensure_csrf(client: &Client) -> Result<(), ApiError> {
        client.ensure_csrf()
    }

    pub fn resolve_origin(client: &Client, force: bool) -> String {
        client.resolve_origin(force)
    }

    pub fn seed_session(client: &Client, value: &str, expires: Option<SystemTime>) {
        client.lock_state().session = StoredCookie {
            value: value.to_owned(),
            expires,
        };
    }

    pub fn seed_csrf(client: &Client, value: &str, expires: Option<SystemTime>) {
        client.lock_state().csrf = StoredCookie {
            value: value.to_owned(),
            expires,
        };
    }

    pub fn active_csrf(client: &Client) -> String {
        client.active_csrf()
    }

    pub fn store_csrf(client: &Client, token: &str, generation: u64) {
        client.store_csrf(token, generation);
    }

    pub fn clear_csrf(client: &Client) {
        client.clear_csrf();
    }

    pub fn persist_session(client: &Client, value: &str) {
        client.persist_session(value);
    }

    pub fn delete_stored_session(client: &Client) -> Result<(), String> {
        match client.current_secrets() {
            Some(store) => client
                .delete_stored_session(&store)
                .map_err(|error| error.to_string()),
            None => Ok(()),
        }
    }

    pub fn load_session(client: &Client) {
        client.load_session();
    }

    pub fn absorb_cookies(client: &Client, set_cookies: &[String], generation: u64) {
        client.absorb_cookies(set_cookies, generation);
    }

    pub fn image_cache(client: &Client) -> (HashMap<String, String>, Vec<String>, i64) {
        let state = client.lock_state();
        (
            state.image_cache.clone(),
            state.image_order.iter().cloned().collect(),
            state.image_cache_bytes,
        )
    }

    pub fn store_image(client: &Client, key: &str, value: &str, generation: u64) {
        client.store_image(key.to_owned(), value.to_owned(), generation);
    }

    pub fn clear_image_cache(client: &Client) {
        crate::client::clear_image_cache(&mut client.lock_state());
    }

    pub fn default_secret_store_kind(
        _profile: &Path,
        _api_url: &str,
        environment: &Environment,
    ) -> &'static str {
        if crate::session::default_secret_backend_is_file(environment) {
            "file"
        } else {
            "fallback"
        }
    }

    pub fn generation(client: &Client) -> u64 {
        client.lock_state().generation
    }

    pub fn client_state(client: &Client) -> Value {
        let state = client.lock_state();
        let now = SystemTime::now();
        json!({
            "settings": state.settings,
            "resolved_origin": state.resolved_origin,
            "generation": state.generation,
            "session": state.session.value,
            "session_expires_unix": expires_unix(state.session.expires),
            "csrf": state.csrf.value,
            "csrf_expires_unix": expires_unix(state.csrf.expires),
            "active_csrf": if state.csrf.active(now) { state.csrf.value.clone() } else { String::new() },
        })
    }

    fn expires_unix(value: Option<SystemTime>) -> i64 {
        match value {
            None => 0,
            Some(time) => match time.duration_since(UNIX_EPOCH) {
                Ok(duration) => duration.as_secs() as i64,
                Err(error) => -(error.duration().as_secs() as i64),
            },
        }
    }
}
pub use urls::{
    ApiPath, DEFAULT_API_URL, FALLBACK_ORIGIN, MAX_API_PATH_LENGTH, ParsedUrl, TargetUrl,
    UrlParseError, api_host, canonical_origin, consent_host_for_update, contains_control, hostname,
    invalid_api_url, invalid_origin, invalid_path, is_csrf_endpoint, is_local_host,
    is_loopback_host, network_consent_error, normalize_api_url, normalize_host, normalize_origin,
    parse_url, port, settings_consent_allowed, target_url_with, validate_api_path,
    validate_server_address,
};
