use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use azahar_desktop_api::{
    Client, Environment, MemorySecretStore, SecretStore, Settings, Transport, TransportError,
    TransportRequest, TransportResponse, WindowBounds,
};
use tempfile::TempDir;

pub struct TestClient {
    pub client: Client,
    pub secrets: Arc<MemorySecretStore>,
    pub temp: TempDir,
    pub profile: PathBuf,
}

pub fn test_client(api_url: &str) -> TestClient {
    client_with(api_url, Environment::default().origin(""), None)
}

pub fn test_client_with_origin(api_url: &str, origin: &str) -> TestClient {
    client_with(api_url, Environment::default().origin(origin), None)
}

pub fn client_with(
    api_url: &str,
    environment: Environment,
    transport: Option<Arc<dyn Transport>>,
) -> TestClient {
    let environment = if api_url.is_empty() {
        environment
    } else {
        environment.api_url(api_url)
    };
    let temp = tempfile::tempdir().expect("temp profile");
    let profile = temp.path().join("profile");
    let secrets = Arc::new(MemorySecretStore::new());
    let mut options = azahar_desktop_api::ClientOptions::new("2.0.0-test", &profile)
        .environment(environment)
        .secret_store(secrets.clone());
    if let Some(transport) = transport {
        options = options.transport(transport);
    }
    let client = Client::with_options(options).expect("client");
    TestClient {
        client,
        secrets,
        temp,
        profile,
    }
}

pub fn write_settings(profile: &Path, settings: &Settings) {
    std::fs::create_dir_all(profile).expect("profile dir");
    std::fs::write(
        profile.join("settings.json"),
        serde_json::to_vec(settings).expect("settings json"),
    )
    .expect("write settings");
}

pub fn read_settings(profile: &Path) -> Settings {
    let data = std::fs::read(profile.join("settings.json")).expect("read settings");
    serde_json::from_slice(&data).expect("decode settings")
}

pub fn window(width: i64, height: i64) -> WindowBounds {
    WindowBounds {
        width,
        height,
        x: 0,
        y: 0,
    }
}

/// Transport that always fails, recording the request URLs it was given.
#[derive(Default)]
pub struct FailingTransport {
    requests: Mutex<Vec<TransportRequest>>,
    timeout: bool,
}

impl FailingTransport {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn timed_out() -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::new(Vec::new()),
            timeout: true,
        })
    }

    pub fn requests(&self) -> Vec<TransportRequest> {
        self.requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl Transport for FailingTransport {
    fn execute(&self, request: TransportRequest) -> Result<TransportResponse, TransportError> {
        self.requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(request);
        Err(TransportError::Network {
            message: "unreachable".to_owned(),
            timeout: self.timeout,
        })
    }
}

/// Transport returning queued responses in order, defaulting to 200 `{}`.
#[derive(Default)]
pub struct QueueTransport {
    responses: Mutex<VecDeque<TransportResponse>>,
    requests: Mutex<Vec<TransportRequest>>,
}

impl QueueTransport {
    pub fn with(responses: Vec<TransportResponse>) -> Arc<Self> {
        Arc::new(Self {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
        })
    }

    pub fn requests(&self) -> Vec<TransportRequest> {
        self.requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl Transport for QueueTransport {
    fn execute(&self, request: TransportRequest) -> Result<TransportResponse, TransportError> {
        self.requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(request);
        let response = self
            .responses
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front()
            .unwrap_or_else(|| TransportResponse {
                status: 200,
                body: b"{}".to_vec(),
                ..TransportResponse::default()
            });
        Ok(response)
    }
}

pub struct DeleteFailingStore {
    inner: MemorySecretStore,
}

impl DeleteFailingStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: MemorySecretStore::new(),
        })
    }
}

impl SecretStore for DeleteFailingStore {
    fn load(&self) -> Result<Option<String>, azahar_desktop_api::SecretError> {
        self.inner.load()
    }

    fn save(&self, value: &str) -> Result<(), azahar_desktop_api::SecretError> {
        self.inner.save(value)
    }

    fn delete(&self) -> Result<(), azahar_desktop_api::SecretError> {
        Err(azahar_desktop_api::SecretError::new("delete failed"))
    }
}

pub struct BlockingStore {
    released: Mutex<bool>,
    condvar: std::sync::Condvar,
}

impl BlockingStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            released: Mutex::new(false),
            condvar: std::sync::Condvar::new(),
        })
    }

    pub fn release(&self) {
        let mut released = self
            .released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *released = true;
        self.condvar.notify_all();
    }

    fn block(&self) {
        let mut released = self
            .released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while !*released {
            released = self
                .condvar
                .wait(released)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }
}

impl SecretStore for BlockingStore {
    fn load(&self) -> Result<Option<String>, azahar_desktop_api::SecretError> {
        self.block();
        Err(azahar_desktop_api::SecretError::new("released"))
    }

    fn save(&self, _value: &str) -> Result<(), azahar_desktop_api::SecretError> {
        self.block();
        Err(azahar_desktop_api::SecretError::new("released"))
    }

    fn delete(&self) -> Result<(), azahar_desktop_api::SecretError> {
        self.block();
        Err(azahar_desktop_api::SecretError::new("released"))
    }
}

pub fn timeout() -> Duration {
    Duration::from_millis(20)
}
