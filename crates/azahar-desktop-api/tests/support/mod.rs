//! Local HTTP mock server for the API client tests.
//!
//! Each connection serves exactly one request. The client opens a fresh
//! connection per call, so keep-alive is not needed.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Clone, Default)]
pub struct MockRequest {
    pub method: String,
    pub path: String,
    pub raw_query: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl MockRequest {
    pub fn header(&self, name: &str) -> String {
        self.headers
            .get(&name.to_ascii_lowercase())
            .cloned()
            .unwrap_or_default()
    }

    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

#[derive(Debug, Clone)]
pub struct MockResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl MockResponse {
    pub fn new(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: vec![("Content-Type".to_owned(), content_type.to_owned())],
            body: body.into(),
        }
    }

    pub fn json(status: u16, body: &str) -> Self {
        Self::new(status, "application/json", body.as_bytes().to_vec())
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    pub fn with_set_cookie(self, value: impl Into<String>) -> Self {
        let value = value.into();
        self.with_header("Set-Cookie", &value)
    }

    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }
}

pub struct MockServer {
    url: String,
    requests: Arc<Mutex<Vec<MockRequest>>>,
}

impl MockServer {
    pub fn start(handler: impl Fn(&MockRequest) -> MockResponse + Send + Sync + 'static) -> Self {
        let handler = Arc::new(handler);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_thread = Arc::clone(&requests);
        let (ready_sender, ready_receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => panic!("mock server runtime: {error}"),
            };
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
                    .await
                    .expect("bind mock server");
                let address = listener.local_addr().expect("mock server address");
                ready_sender.send(address).expect("mock server ready");
                loop {
                    let Ok((socket, _)) = listener.accept().await else {
                        break;
                    };
                    let handler = Arc::clone(&handler);
                    let requests = Arc::clone(&requests_for_thread);
                    tokio::spawn(async move {
                        serve(socket, handler, requests).await;
                    });
                }
            });
        });
        let address = ready_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("mock server startup");
        Self {
            url: format!("http://{address}"),
            requests,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn requests(&self) -> Vec<MockRequest> {
        self.requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

async fn serve(
    mut socket: tokio::net::TcpStream,
    handler: Arc<dyn Fn(&MockRequest) -> MockResponse + Send + Sync>,
    requests: Arc<Mutex<Vec<MockRequest>>>,
) {
    let Some(request) = read_request(&mut socket).await else {
        return;
    };
    {
        let mut recorded = requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        recorded.push(request.clone());
    }
    let response = handler(&request);
    let mut head = format!(
        "HTTP/1.1 {} Mock\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        response.body.len()
    );
    for (name, value) in &response.headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    let _ = socket.write_all(head.as_bytes()).await;
    if request.method != "HEAD" {
        let _ = socket.write_all(&response.body).await;
    }
    let _ = socket.shutdown().await;
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> Option<MockRequest> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 8192];
    let header_end = loop {
        if let Some(index) = find(&buffer, b"\r\n\r\n") {
            break index;
        }
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 || buffer.len() > 64 * 1024 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    };

    let head = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default().to_owned();
    let (path, raw_query) = match target.split_once('?') {
        Some((path, query)) => (path.to_owned(), query.to_owned()),
        None => (target, String::new()),
    };
    let mut headers = BTreeMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let body_start = header_end + 4;
    while buffer.len() < body_start + content_length {
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let body_end = (body_start + content_length).min(buffer.len());
    Some(MockRequest {
        method,
        path,
        raw_query,
        headers,
        body: buffer[body_start..body_end].to_vec(),
    })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
