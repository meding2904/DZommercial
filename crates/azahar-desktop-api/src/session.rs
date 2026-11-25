//! Cookie state and secret storage: file, OS keyring, timeout and fallback.
//!
//! The keyring is reached through the maintained `keyring` crate and hidden
//! behind [`SecretStore`] so tests and the differential harness use an
//! in-memory implementation.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::environment::Environment;
use crate::types::SecretError;
use crate::urls::normalize_api_url;

pub const SESSION_COOKIE_NAME: &str = "azahar_session";
pub const CSRF_COOKIE_NAME: &str = "azahar_csrf";
pub const KEYRING_SERVICE: &str = "azahar-admin";
pub const KEYRING_TIMEOUT: Duration = Duration::from_millis(1500);

/// Persistent storage for the session cookie.
pub trait SecretStore: Send + Sync {
    fn load(&self) -> Result<Option<String>, SecretError>;
    fn save(&self, value: &str) -> Result<(), SecretError>;
    fn delete(&self) -> Result<(), SecretError>;
}

/// In-memory store used by tests and the differential harness.
#[derive(Debug, Default)]
pub struct MemorySecretStore {
    state: Mutex<MemoryState>,
}

#[derive(Debug, Default)]
struct MemoryState {
    value: Option<String>,
    fail: bool,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn failing() -> Self {
        Self {
            state: Mutex::new(MemoryState {
                value: None,
                fail: true,
            }),
        }
    }

    pub fn stored(&self) -> Option<String> {
        self.lock().value.clone()
    }

    pub fn set_failure(&self, fail: bool) {
        self.lock().fail = fail;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, MemoryState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl SecretStore for MemorySecretStore {
    fn load(&self) -> Result<Option<String>, SecretError> {
        let state = self.lock();
        if state.fail {
            return Err(SecretError::new("keyring unavailable"));
        }
        Ok(state.value.clone())
    }

    fn save(&self, value: &str) -> Result<(), SecretError> {
        let mut state = self.lock();
        if state.fail {
            return Err(SecretError::new("keyring unavailable"));
        }
        state.value = Some(value.to_owned());
        Ok(())
    }

    fn delete(&self) -> Result<(), SecretError> {
        let mut state = self.lock();
        if state.fail {
            return Err(SecretError::new("keyring unavailable"));
        }
        state.value = None;
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct SessionFile {
    #[serde(rename = "azahar_session")]
    session: String,
}

/// Decode the session file like Go's struct unmarshal: the tag
/// `azahar_session` matches case-insensitively, null leaves the zero value and
/// any other type rejects the document.
fn session_file_value(data: &[u8]) -> Option<String> {
    let Ok(serde_json::Value::Object(fields)) = serde_json::from_slice(data) else {
        return None;
    };
    let mut session = String::new();
    for (key, value) in &fields {
        if !key.eq_ignore_ascii_case("azahar_session") {
            continue;
        }
        match value {
            serde_json::Value::String(value) => session = value.clone(),
            serde_json::Value::Null => {}
            _ => return None,
        }
    }
    Some(session)
}

/// JSON file holding only the session cookie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSecretStore {
    pub path: PathBuf,
}

impl FileSecretStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl SecretStore for FileSecretStore {
    fn load(&self) -> Result<Option<String>, SecretError> {
        let data = match fs::read(&self.path) {
            Ok(data) => data,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(SecretError::new(error.to_string())),
        };
        let _ = set_mode(&self.path, 0o600);
        let Some(session) = session_file_value(&data) else {
            return Ok(None);
        };
        if !valid_cookie_value(&session) {
            return Ok(None);
        }
        Ok(Some(session))
    }

    fn save(&self, value: &str) -> Result<(), SecretError> {
        let data = serde_json::to_vec(&SessionFile {
            session: value.to_owned(),
        })
        .map_err(|error| SecretError::new(error.to_string()))?;
        write_private_file(&self.path, &data).map_err(|error| SecretError::new(error.to_string()))
    }

    fn delete(&self) -> Result<(), SecretError> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(SecretError::new(error.to_string())),
        }
    }
}

/// Store backed by the operating system keyring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyringSecretStore {
    pub account: String,
}

impl KeyringSecretStore {
    pub fn new(account: impl Into<String>) -> Self {
        Self {
            account: account.into(),
        }
    }

    fn entry(&self) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(KEYRING_SERVICE, &self.account)
            .map_err(|error| SecretError::new(error.to_string()))
    }
}

impl SecretStore for KeyringSecretStore {
    fn load(&self) -> Result<Option<String>, SecretError> {
        match self.entry()?.get_password() {
            Ok(value) if value.is_empty() => Ok(None),
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(SecretError::new(error.to_string())),
        }
    }

    fn save(&self, value: &str) -> Result<(), SecretError> {
        self.entry()?
            .set_password(value)
            .map_err(|error| SecretError::new(error.to_string()))
    }

    fn delete(&self) -> Result<(), SecretError> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(SecretError::new(error.to_string())),
        }
    }
}

/// Bound the time a platform keyring call may take.
///
/// A thread is spawned per operation because the keyring API has no deadline.
/// The first timeout marks the store unhealthy and every later load or save
/// fails immediately instead of spawning another abandoned thread. The process
/// therefore stays on the file fallback until restart: a keyring that recovers
/// later cannot resurrect an older credential over the newer file value.
pub struct TimeoutSecretStore {
    store: Arc<dyn SecretStore>,
    timeout: Duration,
    healthy: AtomicBool,
}

impl TimeoutSecretStore {
    pub fn new(store: Arc<dyn SecretStore>, timeout: Duration) -> Self {
        Self {
            store,
            timeout,
            healthy: AtomicBool::new(true),
        }
    }

    #[must_use]
    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::SeqCst)
    }

    /// Run `operation` on a worker thread, bounded by the configured timeout.
    ///
    /// An unhealthy store is skipped unless `force` is set, so a hung keyring
    /// costs one abandoned thread per process instead of one per call.
    fn run<T: Send + 'static>(
        &self,
        force: bool,
        operation: impl FnOnce(Arc<dyn SecretStore>) -> T + Send + 'static,
    ) -> Result<T, SecretError> {
        if !force && !self.is_healthy() {
            return Err(SecretError::timed_out());
        }
        let store = Arc::clone(&self.store);
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(operation(store));
        });
        match receiver.recv_timeout(self.timeout) {
            Ok(value) => Ok(value),
            Err(RecvTimeoutError::Timeout) => {
                self.healthy.store(false, Ordering::SeqCst);
                Err(SecretError::timed_out())
            }
            Err(RecvTimeoutError::Disconnected) => {
                Err(SecretError::new("secret store is not configured"))
            }
        }
    }
}

impl SecretStore for TimeoutSecretStore {
    fn load(&self) -> Result<Option<String>, SecretError> {
        self.run(false, |store| store.load())?
    }

    fn save(&self, value: &str) -> Result<(), SecretError> {
        let value = value.to_owned();
        self.run(false, move |store| store.save(&value))?
    }

    fn delete(&self) -> Result<(), SecretError> {
        // Sign-out must still ask the keyring to remove the credential, and
        // must not report success when that attempt times out.
        self.run(true, |store| store.delete())?
    }
}

/// Try the keyring first, then the scoped file.
pub struct FallbackSecretStore {
    primary: Option<Arc<dyn SecretStore>>,
    fallback: Arc<dyn SecretStore>,
}

impl FallbackSecretStore {
    pub fn new(primary: Option<Arc<dyn SecretStore>>, fallback: Arc<dyn SecretStore>) -> Self {
        Self { primary, fallback }
    }
}

impl SecretStore for FallbackSecretStore {
    fn load(&self) -> Result<Option<String>, SecretError> {
        if let Some(primary) = &self.primary
            && let Ok(Some(value)) = primary.load()
        {
            return Ok(Some(value));
        }
        self.fallback.load()
    }

    fn save(&self, value: &str) -> Result<(), SecretError> {
        if let Some(primary) = &self.primary
            && primary.save(value).is_ok()
        {
            let _ = self.fallback.delete();
            return Ok(());
        }
        self.fallback.save(value)
    }

    fn delete(&self) -> Result<(), SecretError> {
        let primary_error = self.primary.as_ref().and_then(|store| store.delete().err());
        self.fallback.delete()?;
        match primary_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

/// `AZAHAR_SESSION_BACKEND=file` or `AZAHAR_DISABLE_KEYRING` selects the file.
pub fn env_enabled(value: Option<&str>) -> bool {
    match value.map(|value| value.trim().to_ascii_lowercase()) {
        Some(value) => !matches!(value.as_str(), "" | "0" | "false" | "no" | "off"),
        None => false,
    }
}

pub fn session_scope(profile_dir: &Path, api_url: &str) -> String {
    let api_url = normalize_api_url(api_url).unwrap_or_else(|_| api_url.to_owned());
    let cleaned = clean_path(profile_dir);
    let mut hasher = Sha256::new();
    hasher.update(cleaned.as_bytes());
    hasher.update(b"\n");
    hasher.update(api_url.as_bytes());
    let digest = hasher.finalize();
    format!("session-{}", hex::encode(&digest[..8]))
}

pub(crate) fn default_secret_backend_is_file(environment: &Environment) -> bool {
    let backend_is_file = environment
        .session_backend
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("file"));
    backend_is_file || env_enabled(environment.disable_keyring.as_deref())
}

pub fn default_secret_store(
    profile_dir: &Path,
    api_url: &str,
    environment: &Environment,
) -> Arc<dyn SecretStore> {
    let scope = session_scope(profile_dir, api_url);
    let file: Arc<dyn SecretStore> = Arc::new(FileSecretStore::new(
        profile_dir.join(format!("{scope}.json")),
    ));

    if default_secret_backend_is_file(environment) {
        return file;
    }

    let primary: Arc<dyn SecretStore> = Arc::new(TimeoutSecretStore::new(
        Arc::new(KeyringSecretStore::new(scope)),
        KEYRING_TIMEOUT,
    ));
    Arc::new(FallbackSecretStore::new(Some(primary), file))
}

/// Cookie value or CSRF token accepted by the session layer.
pub fn valid_cookie_value(value: &str) -> bool {
    if value.is_empty() || value.len() > 4096 {
        return false;
    }
    value
        .bytes()
        .all(|byte| (0x21..=0x7e).contains(&byte) && !matches!(byte, b';' | b'"' | b'\\' | b','))
}

/// A session or CSRF cookie with an optional absolute expiry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoredCookie {
    pub value: String,
    pub expires: Option<SystemTime>,
}

impl StoredCookie {
    pub fn active(&self, now: SystemTime) -> bool {
        !self.value.is_empty() && self.expires.is_none_or(|expires| now < expires)
    }
}

/// Parse one `Set-Cookie` header with the rules of `cookieFromSetCookie`.
pub fn cookie_from_set_cookie(header: &str, now: SystemTime) -> Option<StoredCookie> {
    let cookie = cookie::Cookie::parse(header.to_owned()).ok()?;
    cookie_from_parsed(&cookie, now)
}

/// Apply the stored-cookie rules to an already parsed cookie.
pub fn cookie_from_parsed(cookie: &cookie::Cookie<'_>, now: SystemTime) -> Option<StoredCookie> {
    let value = cookie.value().to_owned();
    if value.is_empty() || !valid_cookie_value(&value) {
        return None;
    }
    let max_age = cookie.max_age();
    let expires = if let Some(max_age) = max_age {
        let seconds = max_age.whole_seconds();
        if seconds <= 0 {
            return None;
        }
        Some(now.checked_add(Duration::from_secs(seconds as u64))?)
    } else {
        cookie.expires_datetime().and_then(|expires| {
            let seconds = expires.unix_timestamp();
            if seconds < 0 {
                SystemTime::UNIX_EPOCH.checked_sub(Duration::from_secs(seconds.unsigned_abs()))
            } else {
                SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(seconds as u64))
            }
        })
    };
    if expires.is_some_and(|expires| expires <= now) {
        return None;
    }
    Some(StoredCookie { value, expires })
}

fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
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

/// Go's `filepath.Clean`, used for scope and profile paths.
pub fn clean_path(path: &Path) -> String {
    let mut prefix = String::new();
    let rooted = path.is_absolute();
    let mut parts: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(value) => {
                prefix = value.as_os_str().to_string_lossy().into_owned();
            }
            std::path::Component::RootDir => {}
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => match parts.last() {
                Some(last) if last != ".." => {
                    parts.pop();
                }
                _ if !rooted => parts.push("..".to_owned()),
                _ => {}
            },
            std::path::Component::Normal(value) => {
                parts.push(value.to_string_lossy().into_owned());
            }
        }
    }

    let separator = std::path::MAIN_SEPARATOR;
    let mut result = prefix;
    if rooted {
        result.push(separator);
    }
    result.push_str(&parts.join(&separator.to_string()));
    if result.is_empty() {
        ".".to_owned()
    } else {
        result
    }
}

/// `deviceHash`: the first twelve hex characters of sha256(profile dir).
pub fn device_hash(profile_dir: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(profile_dir.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    hex::encode(digest)[..12].to_owned()
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Atomically write a 0600 file inside a 0700 directory.
pub fn write_private_file(path: &Path, data: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
    let _ = set_mode(dir, 0o700);

    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "settings.json".to_owned());
    let mut temp_path = None;
    let mut file = None;
    for _ in 0..32 {
        let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = dir.join(format!(".{name}.{}.{count}", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&candidate) {
            Ok(opened) => {
                temp_path = Some(candidate);
                file = Some(opened);
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    let (temp_path, mut file) = match (temp_path, file) {
        (Some(path), Some(file)) => (path, file),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "could not create a temporary settings file",
            ));
        }
    };

    let written = file
        .write_all(data)
        .and_then(|()| file.sync_all())
        .and_then(|()| file.flush());
    drop(file);
    if let Err(error) = written {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    #[cfg(windows)]
    {
        if path.exists() {
            let _ = fs::remove_file(path);
        }
    }
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    Ok(())
}
