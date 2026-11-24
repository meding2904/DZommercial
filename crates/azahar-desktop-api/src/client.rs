//! The desktop API client: profile resolution, settings persistence, secret
//! store scoping and the image cache.

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use serde::Deserialize;
use serde_json::Value;

use crate::environment::Environment;
use crate::session::{
    FileSecretStore, SecretStore, StoredCookie, clean_path, default_secret_store, device_hash,
    session_scope, valid_cookie_value, write_private_file,
};
use crate::transport::{Transport, default_transport};
use crate::types::{ClientError, Info, Settings, SettingsResult, WindowBounds, go_platform};
use crate::urls::{
    DEFAULT_API_URL, FALLBACK_ORIGIN, api_host, consent_host_for_update, is_loopback_host,
    normalize_api_url, normalize_host, normalize_origin, settings_consent_allowed,
};

pub const DEFAULT_WINDOW_WIDTH: i64 = 1380;
pub const DEFAULT_WINDOW_HEIGHT: i64 = 900;
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
pub const DEFAULT_IMAGE_TIMEOUT: Duration = Duration::from_secs(15);
pub const DEFAULT_IMAGE_CACHE_BYTES: i64 = 64 << 20;
pub const IMAGE_CACHE_SIZE: usize = 40;
pub const MAX_RESPONSE_BYTES: u64 = 16 << 20;
pub const MAX_IMAGE_BYTES: u64 = 8 << 20;
pub const MAX_REQUEST_BODY_BYTES: usize = 2 << 20;

pub type SecretStoreFactory = Arc<dyn Fn(&str) -> Option<Arc<dyn SecretStore>> + Send + Sync>;

pub(crate) struct State {
    pub settings: Settings,
    pub resolved_origin: String,
    pub session: StoredCookie,
    pub csrf: StoredCookie,
    pub generation: u64,
    pub image_cache: HashMap<String, String>,
    pub image_order: VecDeque<String>,
    pub image_cache_bytes: i64,
    pub image_cache_byte_limit: i64,
    pub timeout: Duration,
    pub image_timeout: Duration,
}

/// Options for constructing a client.
pub struct ClientOptions {
    pub version: String,
    pub profile_dir: PathBuf,
    pub environment: Environment,
    secret_store_factory: Option<SecretStoreFactory>,
    pub transport: Option<Arc<dyn Transport>>,
}

impl ClientOptions {
    pub fn new(version: impl Into<String>, profile_dir: impl Into<PathBuf>) -> Self {
        Self {
            version: version.into(),
            profile_dir: profile_dir.into(),
            environment: Environment::default(),
            secret_store_factory: None,
            transport: None,
        }
    }

    pub fn environment(mut self, environment: Environment) -> Self {
        self.environment = environment;
        self
    }

    /// Use one store for every API URL.
    pub fn secret_store(self, store: Arc<dyn SecretStore>) -> Self {
        self.secret_store_factory(move |_| Some(Arc::clone(&store)))
    }

    /// Choose a store per normalized API URL.
    pub fn secret_store_factory(
        mut self,
        factory: impl Fn(&str) -> Option<Arc<dyn SecretStore>> + Send + Sync + 'static,
    ) -> Self {
        self.secret_store_factory = Some(Arc::new(factory));
        self
    }

    pub fn transport(mut self, transport: Arc<dyn Transport>) -> Self {
        self.transport = Some(transport);
        self
    }
}

/// Desktop API client.
pub struct Client {
    pub(crate) version: String,
    pub(crate) profile_dir: PathBuf,
    pub(crate) settings_path: PathBuf,
    pub(crate) user_agent: String,
    pub(crate) transport: Arc<dyn Transport>,
    pub(crate) secrets_factory: SecretStoreFactory,
    pub(crate) state: Mutex<State>,
    pub(crate) secrets: Mutex<Option<Arc<dyn SecretStore>>>,
    secret_mu: Mutex<()>,
}

impl Client {
    pub fn new(
        version: impl Into<String>,
        profile_dir: impl Into<PathBuf>,
    ) -> Result<Self, ClientError> {
        let options =
            ClientOptions::new(version, profile_dir).environment(Environment::from_process());
        Self::with_options(options)
    }

    pub fn with_environment(
        version: impl Into<String>,
        profile_dir: impl Into<PathBuf>,
        environment: Environment,
    ) -> Result<Self, ClientError> {
        Self::with_options(ClientOptions::new(version, profile_dir).environment(environment))
    }

    pub fn with_secret_store(
        version: impl Into<String>,
        profile_dir: impl Into<PathBuf>,
        store: Arc<dyn SecretStore>,
    ) -> Result<Self, ClientError> {
        Self::with_environment_and_secret_store(version, profile_dir, Environment::default(), store)
    }

    pub fn with_environment_and_secret_store(
        version: impl Into<String>,
        profile_dir: impl Into<PathBuf>,
        environment: Environment,
        store: Arc<dyn SecretStore>,
    ) -> Result<Self, ClientError> {
        Self::with_options(
            ClientOptions::new(version, profile_dir)
                .environment(environment)
                .secret_store(store),
        )
    }

    pub fn with_options(options: ClientOptions) -> Result<Self, ClientError> {
        if options.profile_dir.to_string_lossy().trim().is_empty() {
            return Err(ClientError::ProfileDirectoryRequired);
        }
        let profile_dir = std::path::absolute(&options.profile_dir)?;
        let profile_dir = PathBuf::from(clean_path(&profile_dir));
        fs::create_dir_all(&profile_dir)?;
        let _ = set_mode(&profile_dir, 0o700);

        let environment = options.environment;
        let profile_for_factory = profile_dir.clone();
        let environment_for_factory = environment.clone();
        let secrets_factory: SecretStoreFactory =
            options.secret_store_factory.unwrap_or_else(|| {
                Arc::new(move |api_url: &str| {
                    Some(default_secret_store(
                        &profile_for_factory,
                        api_url,
                        &environment_for_factory,
                    ))
                })
            });

        let transport = match options.transport {
            Some(transport) => transport,
            None => default_transport().map_err(ClientError::Message)?,
        };

        let version = options.version;
        let user_agent = format!("AzaharDesktop/{} ({})", version, device_hash(&profile_dir));
        let client = Self {
            version,
            settings_path: profile_dir.join("settings.json"),
            user_agent,
            transport,
            secrets_factory,
            state: Mutex::new(State {
                settings: Settings {
                    api_url: DEFAULT_API_URL.to_owned(),
                    ..Settings::default()
                },
                resolved_origin: String::new(),
                session: StoredCookie::default(),
                csrf: StoredCookie::default(),
                generation: 0,
                image_cache: HashMap::new(),
                image_order: VecDeque::new(),
                image_cache_bytes: 0,
                image_cache_byte_limit: DEFAULT_IMAGE_CACHE_BYTES,
                timeout: DEFAULT_REQUEST_TIMEOUT,
                image_timeout: DEFAULT_IMAGE_TIMEOUT,
            }),
            secrets: Mutex::new(None),
            secret_mu: Mutex::new(()),
            profile_dir,
        };
        client.load_settings(&environment);
        let api_url = client.lock_state().settings.api_url.clone();
        let secrets = client.secrets_for(&api_url);
        *client.lock_secrets() = Some(secrets);
        client.load_session();
        Ok(client)
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn profile_dir(&self) -> &Path {
        &self.profile_dir
    }

    pub fn set_request_timeout(&self, timeout: Duration) {
        self.lock_state().timeout = timeout;
    }

    pub fn set_image_timeout(&self, timeout: Duration) {
        self.lock_state().image_timeout = timeout;
    }

    pub fn set_image_cache_byte_limit(&self, limit: i64) {
        self.lock_state().image_cache_byte_limit = limit;
    }

    pub fn info(&self) -> Info {
        let state = self.lock_state();
        let settings = &state.settings;
        let resolved = if settings.origin.is_empty() {
            state.resolved_origin.clone()
        } else {
            settings.origin.clone()
        };
        Info {
            version: self.version.clone(),
            platform: go_platform().to_owned(),
            api_url: settings.api_url.clone(),
            origin: settings.origin.clone(),
            resolved_origin: resolved,
            network_allowed: settings_consent_allowed(settings),
        }
    }

    pub fn window_bounds(&self) -> WindowBounds {
        let mut bounds = self.lock_state().settings.window;
        if bounds.width <= 0 {
            bounds.width = DEFAULT_WINDOW_WIDTH;
        }
        if bounds.height <= 0 {
            bounds.height = DEFAULT_WINDOW_HEIGHT;
        }
        bounds
    }

    pub fn save_window_bounds(&self, mut bounds: WindowBounds) {
        if bounds.width <= 0 {
            bounds.width = DEFAULT_WINDOW_WIDTH;
        }
        if bounds.height <= 0 {
            bounds.height = DEFAULT_WINDOW_HEIGHT;
        }
        let mut state = self.lock_state();
        state.settings.window = bounds;
        let settings = state.settings.clone();
        let _ = save_settings(&self.settings_path, &settings);
    }

    pub fn update_settings(
        &self,
        api_url: &str,
        origin: &str,
        allow_network: bool,
    ) -> Result<SettingsResult, ClientError> {
        let normalized_url = normalize_api_url(api_url)?;
        let normalized_origin = normalize_origin(origin)?;

        let (url_changed, old_secrets) = {
            let mut state = self.lock_state();
            let previous = state.settings.clone();
            let desired_host = consent_host_for_update(&previous, &normalized_url, allow_network)?;
            if previous.api_url == normalized_url
                && previous.origin == normalized_origin
                && previous.network_allowed_host == desired_host
            {
                let resolved = if previous.origin.is_empty() {
                    state.resolved_origin.clone()
                } else {
                    previous.origin.clone()
                };
                drop(state);
                return Ok(self.settings_result(&resolved));
            }

            let url_changed = previous.api_url != normalized_url;
            let origin_changed = previous.origin != normalized_origin;
            let old_secrets = if url_changed {
                self.lock_secrets().clone()
            } else {
                None
            };
            let mut next = previous;
            next.api_url = normalized_url.clone();
            next.origin = normalized_origin.clone();
            next.network_allowed = allow_network;
            next.network_allowed_host = desired_host;

            if save_settings(&self.settings_path, &next).is_err() {
                return Err(ClientError::SettingsNotSaved);
            }

            state.settings = next;
            if url_changed {
                state.resolved_origin.clear();
                state.session = StoredCookie::default();
                state.csrf = StoredCookie::default();
                clear_image_cache(&mut state);
                let replacement = self.secrets_for(&normalized_url);
                *self.lock_secrets() = Some(replacement);
            }
            if url_changed || origin_changed {
                state.generation += 1;
            }
            (url_changed, old_secrets)
        };

        let mut warning = String::new();
        if url_changed
            && let Some(store) = old_secrets
            && self.delete_stored_session(&store).is_err()
        {
            warning =
                "The old server's saved session could not be removed. It is scoped to the old server."
                    .to_owned();
        }

        let mut result = self.settings_result(&self.resolve_origin(true));
        result.warning = warning;
        Ok(result)
    }

    fn settings_result(&self, resolved: &str) -> SettingsResult {
        let state = self.lock_state();
        let settings = state.settings.clone();
        let mut resolved = resolved.to_owned();
        if resolved.is_empty() {
            resolved = settings.origin.clone();
        }
        if resolved.is_empty() {
            resolved = state.resolved_origin.clone();
        }
        SettingsResult {
            api_url: settings.api_url.clone(),
            origin: settings.origin.clone(),
            resolved_origin: resolved,
            network_allowed: settings_consent_allowed(&settings),
            warning: String::new(),
        }
    }

    pub(crate) fn resolve_origin(&self, force: bool) -> String {
        let (origin, resolved, api_url) = {
            let state = self.lock_state();
            (
                state.settings.origin.clone(),
                state.resolved_origin.clone(),
                state.settings.api_url.clone(),
            )
        };
        if !origin.is_empty() {
            return origin;
        }
        if !resolved.is_empty() && !force {
            return resolved;
        }

        self.health();

        let mut state = self.lock_state();
        if !state.settings.origin.is_empty() {
            return state.settings.origin.clone();
        }
        if state.settings.api_url != api_url {
            return state.resolved_origin.clone();
        }
        if !state.resolved_origin.is_empty() {
            return state.resolved_origin.clone();
        }
        if api_host(&state.settings.api_url).is_ok_and(|host| is_loopback_host(&host)) {
            state.resolved_origin = FALLBACK_ORIGIN.to_owned();
        }
        state.resolved_origin.clone()
    }

    pub(crate) fn request_timeout(&self) -> Duration {
        self.lock_state().timeout
    }

    pub(crate) fn image_timeout(&self) -> Duration {
        self.lock_state().image_timeout
    }

    pub(crate) fn active_csrf(&self) -> String {
        let state = self.lock_state();
        if state.csrf.active(SystemTime::now()) {
            state.csrf.value.clone()
        } else {
            String::new()
        }
    }

    pub(crate) fn store_csrf(&self, token: &str, generation: u64) {
        if !valid_cookie_value(token) {
            return;
        }
        let mut state = self.lock_state();
        if state.generation == generation && !state.csrf.active(SystemTime::now()) {
            state.csrf = StoredCookie {
                value: token.to_owned(),
                expires: None,
            };
        }
    }

    pub(crate) fn clear_csrf(&self) {
        self.lock_state().csrf = StoredCookie::default();
    }

    /// Current in-memory settings. Diagnostics and tests.
    #[doc(hidden)]
    pub fn settings_snapshot(&self) -> Settings {
        self.lock_state().settings.clone()
    }

    /// Seed a CSRF token without a round trip. Test and embedding hook.
    #[doc(hidden)]
    pub fn seed_csrf(&self, token: &str) {
        self.lock_state().csrf = StoredCookie {
            value: token.to_owned(),
            expires: None,
        };
    }

    /// Seed a session cookie without a round trip. Test and embedding hook.
    #[doc(hidden)]
    pub fn seed_session(&self, token: &str) {
        self.lock_state().session = StoredCookie {
            value: token.to_owned(),
            expires: None,
        };
    }

    /// In-memory session and CSRF cookie values, for diagnostics and tests.
    pub fn session_state(&self) -> (String, String) {
        let state = self.lock_state();
        (state.session.value.clone(), state.csrf.value.clone())
    }

    pub fn clear_session(&self) -> Result<(), ClientError> {
        let store = {
            let mut state = self.lock_state();
            state.generation += 1;
            state.session = StoredCookie::default();
            state.csrf = StoredCookie::default();
            clear_image_cache(&mut state);
            self.lock_secrets().clone()
        };
        if let Some(store) = store {
            self.delete_stored_session(&store)?;
        }
        Ok(())
    }

    pub(crate) fn secrets_for(&self, api_url: &str) -> Arc<dyn SecretStore> {
        if let Some(store) = (self.secrets_factory)(api_url) {
            return store;
        }
        Arc::new(FileSecretStore::new(self.profile_dir.join(format!(
            "{}.json",
            session_scope(&self.profile_dir, api_url)
        ))))
    }

    pub(crate) fn current_secrets(&self) -> Option<Arc<dyn SecretStore>> {
        self.lock_secrets().clone()
    }

    pub(crate) fn persist_session(&self, value: &str) {
        let _guard = self
            .secret_mu
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (current, store) = {
            let state = self.lock_state();
            (state.session.value.clone(), self.lock_secrets().clone())
        };
        let Some(store) = store else {
            return;
        };
        if current != value {
            return;
        }
        if value.is_empty() {
            let _ = store.delete();
        } else {
            let _ = store.save(value);
        }
    }

    fn delete_stored_session(&self, store: &Arc<dyn SecretStore>) -> Result<(), ClientError> {
        let _guard = self
            .secret_mu
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        store.delete().map_err(ClientError::Secret)
    }

    fn load_session(&self) {
        let Some(store) = self.current_secrets() else {
            return;
        };
        let Ok(Some(value)) = store.load() else {
            return;
        };
        if !valid_cookie_value(&value) {
            return;
        }
        self.lock_state().session = StoredCookie {
            value,
            expires: None,
        };
    }

    pub(crate) fn store_image(&self, key: String, value: String, generation: u64) {
        let size = value.len() as i64;
        let mut state = self.lock_state();
        let limit = if state.image_cache_byte_limit <= 0 {
            DEFAULT_IMAGE_CACHE_BYTES
        } else {
            state.image_cache_byte_limit
        };
        if state.generation != generation || size > limit {
            return;
        }
        if let Some(old) = state.image_cache.get(&key) {
            state.image_cache_bytes -= old.len() as i64;
        } else {
            state.image_order.push_back(key.clone());
        }
        state.image_cache.insert(key, value);
        state.image_cache_bytes += size;

        while (state.image_cache.len() > IMAGE_CACHE_SIZE || state.image_cache_bytes > limit)
            && !state.image_order.is_empty()
        {
            let Some(oldest) = state.image_order.pop_front() else {
                break;
            };
            if let Some(old) = state.image_cache.remove(&oldest) {
                state.image_cache_bytes -= old.len() as i64;
            }
        }
    }

    pub(crate) fn cached_image(&self, path: &str) -> Option<String> {
        self.lock_state().image_cache.get(path).cloned()
    }

    pub(crate) fn lock_state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn lock_secrets(&self) -> std::sync::MutexGuard<'_, Option<Arc<dyn SecretStore>>> {
        self.secrets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn load_settings(&self, environment: &Environment) {
        let mut next = Settings {
            api_url: DEFAULT_API_URL.to_owned(),
            ..Settings::default()
        };
        let mut imported = false;
        match fs::read(&self.settings_path) {
            Ok(data) => {
                let _ = set_mode(&self.settings_path, 0o600);
                if let Ok(Value::Object(fields)) = serde_json::from_slice::<Value>(&data) {
                    decode_settings(&fields, &mut next);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if let Some(legacy) = load_legacy_settings(environment) {
                    next = legacy;
                    imported = true;
                }
            }
            Err(_) => {}
        }
        if next.api_url.is_empty() {
            next.api_url = DEFAULT_API_URL.to_owned();
        }

        if let Some(value) = environment
            .api_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            && let Ok(normalized) = normalize_api_url(value)
            && api_host(&normalized).is_ok()
        {
            next.api_url = normalized;
        }
        if let Some(value) = environment.origin.as_deref()
            && let Ok(normalized) = normalize_origin(value)
        {
            next.origin = normalized;
        }

        let mut host = String::new();
        match normalize_api_url(&next.api_url) {
            Ok(normalized) => match api_host(&normalized) {
                Ok(parsed) => {
                    next.api_url = normalized;
                    host = parsed;
                }
                Err(_) => next.api_url = DEFAULT_API_URL.to_owned(),
            },
            Err(_) => next.api_url = DEFAULT_API_URL.to_owned(),
        }
        if host.is_empty() {
            host = api_host(&next.api_url).unwrap_or_default();
        }
        next.origin = normalize_origin(&next.origin).unwrap_or_default();
        next.network_allowed_host = normalize_host(&next.network_allowed_host);
        if is_loopback_host(&host) || !host.eq_ignore_ascii_case(&next.network_allowed_host) {
            next.network_allowed_host.clear();
        }

        if imported {
            let _ = save_settings(&self.settings_path, &next);
        }
        self.lock_state().settings = next;
    }
}

fn decode_settings(fields: &serde_json::Map<String, Value>, next: &mut Settings) {
    if let Some(value) = decode_string(fields.get("apiUrl")) {
        next.api_url = value;
    }
    if let Some(value) = decode_string(fields.get("origin")) {
        next.origin = value;
    }
    if let Some(value) = decode_bool(fields.get("networkAllowed")) {
        next.network_allowed = value;
    }
    if let Some(value) = decode_string(fields.get("networkAllowedHost")) {
        next.network_allowed_host = value;
    }
    if let Some(Value::Object(window)) = fields.get("window") {
        if let Some(value) = decode_i64(window.get("width")) {
            next.window.width = value;
        }
        if let Some(value) = decode_i64(window.get("height")) {
            next.window.height = value;
        }
        if let Some(value) = decode_i64(window.get("x")) {
            next.window.x = value;
        }
        if let Some(value) = decode_i64(window.get("y")) {
            next.window.y = value;
        }
    }
}

fn decode_string(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(value)) => Some(value.clone()),
        _ => None,
    }
}

fn decode_bool(value: Option<&Value>) -> Option<bool> {
    match value {
        Some(Value::Bool(value)) => Some(*value),
        _ => None,
    }
}

fn decode_i64(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(number)) => number.as_i64(),
        _ => None,
    }
}

#[derive(Deserialize)]
struct LegacySettings {
    #[serde(rename = "apiUrl")]
    api_url: String,
    #[serde(default)]
    origin: String,
    #[serde(default)]
    window: WindowBounds,
}

fn load_legacy_settings(environment: &Environment) -> Option<Settings> {
    let root = environment.user_config_dir()?;
    let data = fs::read(root.join("Azahar Admin").join("settings.json")).ok()?;
    let legacy: LegacySettings = serde_json::from_slice(&data).ok()?;
    if legacy.api_url.trim().is_empty() {
        return None;
    }
    Some(Settings {
        api_url: legacy.api_url,
        origin: legacy.origin,
        network_allowed: false,
        network_allowed_host: String::new(),
        window: legacy.window,
    })
}

fn save_settings(path: &Path, settings: &Settings) -> Result<(), ClientError> {
    let data = serde_json::to_vec_pretty(settings)
        .map_err(|error| ClientError::Message(error.to_string()))?;
    write_private_file(path, &data)?;
    Ok(())
}

pub(crate) fn clear_image_cache(state: &mut State) {
    state.image_cache.clear();
    state.image_order.clear();
    state.image_cache_bytes = 0;
}

fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

impl std::fmt::Debug for Client {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Client")
            .field("version", &self.version)
            .field("profile_dir", &self.profile_dir)
            .finish_non_exhaustive()
    }
}
