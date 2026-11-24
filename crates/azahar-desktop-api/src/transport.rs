//! HTTP transport abstraction.
//!
//! Production uses [`ReqwestTransport`]. Tests inject fakes to exercise
//! network failures and to inspect the exact request without a socket.

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use reqwest::blocking::Client as BlockingClient;
use reqwest::header::{CONTENT_TYPE, SET_COOKIE};
use reqwest::{Method, redirect};

/// One request after URL resolution and header construction.
#[derive(Debug, Clone)]
pub struct TransportRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout: Duration,
    pub max_bytes: u64,
}

/// Raw response returned by a transport.
#[derive(Debug, Clone, Default)]
pub struct TransportResponse {
    pub status: i32,
    pub body: Vec<u8>,
    pub content_type: String,
    pub set_cookies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    ResponseTooLarge,
    Network { message: String, timeout: bool },
}

impl TransportError {
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Network { timeout: true, .. })
    }
}

pub trait Transport: Send + Sync {
    fn execute(&self, request: TransportRequest) -> Result<TransportResponse, TransportError>;
}

/// reqwest on rustls, without redirect following.
pub struct ReqwestTransport {
    client: BlockingClient,
}

impl ReqwestTransport {
    pub fn create() -> Result<Self, String> {
        let client = BlockingClient::builder()
            .redirect(redirect::Policy::none())
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self { client })
    }
}

impl Transport for ReqwestTransport {
    fn execute(&self, request: TransportRequest) -> Result<TransportResponse, TransportError> {
        let method = Method::from_bytes(request.method.as_bytes()).map_err(|error| {
            TransportError::Network {
                message: error.to_string(),
                timeout: false,
            }
        })?;
        let mut builder = self
            .client
            .request(method, &request.url)
            .timeout(request.timeout);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        if let Some(body) = request.body {
            builder = builder.body(body);
        }
        let response = builder.send().map_err(reqwest_error)?;

        let status = response.status().as_u16() as i32;
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let set_cookies = response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok().map(str::to_owned))
            .collect::<Vec<_>>();
        if response
            .content_length()
            .is_some_and(|length| length > request.max_bytes)
        {
            return Err(TransportError::ResponseTooLarge);
        }

        let mut body = Vec::new();
        if let Err(error) = response
            .take(request.max_bytes.saturating_add(1))
            .read_to_end(&mut body)
        {
            return Err(TransportError::Network {
                message: error.to_string(),
                timeout: matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ),
            });
        }
        if body.len() as u64 > request.max_bytes {
            return Err(TransportError::ResponseTooLarge);
        }
        Ok(TransportResponse {
            status,
            body,
            content_type,
            set_cookies,
        })
    }
}

fn reqwest_error(error: reqwest::Error) -> TransportError {
    TransportError::Network {
        message: error.to_string(),
        timeout: error.is_timeout(),
    }
}

impl std::fmt::Debug for ReqwestTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReqwestTransport")
    }
}

/// Convenience for callers that want the default transport behind an `Arc`.
pub fn default_transport() -> Result<Arc<dyn Transport>, String> {
    ReqwestTransport::create().map(|transport| Arc::new(transport) as Arc<dyn Transport>)
}
