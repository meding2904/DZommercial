//! In-process read cache, ported from `backend/internal/httpapi/cache.go`.
//!
//! The Go cache is per server: 512 entries, 32 MiB total and 256 KiB per body.
//! Only successful GET responses are stored, keyed by the raw request URI, and
//! every unsafe request clears the whole map. The TTLs are deliberately short
//! (2-5 seconds) so a checkout still re-reads stock and price.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::{Body, HttpBody as _, to_bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::middleware::{REQUEST_ID_HEADER, RequestId};

pub const READ_CACHE_MAX_ENTRIES: usize = 512;
pub const READ_CACHE_MAX_BODY: usize = 256 << 10;
pub const READ_CACHE_MAX_BYTES: usize = 32 << 20;

/// One cached response. Headers are stored without `X-Request-Id` and `Date`,
/// exactly like the Go entry.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
    pub expires: Instant,
}

#[derive(Debug)]
struct Inner {
    entries: HashMap<String, CacheEntry>,
    max_entries: usize,
    max_bytes: usize,
    bytes: usize,
}

/// A bounded, expiry-ordered response cache.
#[derive(Debug)]
pub struct ReadCache {
    inner: Mutex<Inner>,
}

impl Default for ReadCache {
    fn default() -> Self {
        Self::new(READ_CACHE_MAX_ENTRIES)
    }
}

impl ReadCache {
    /// `newReadCache`: a non-positive maximum selects the Go default.
    #[must_use]
    pub fn new(max_entries: usize) -> Self {
        Self::with_limits(max_entries, READ_CACHE_MAX_BYTES)
    }

    /// The production limits, with the byte ceiling parameterised for tests.
    #[must_use]
    pub fn with_limits(max_entries: usize, max_bytes: usize) -> Self {
        let max_entries = if max_entries == 0 {
            READ_CACHE_MAX_ENTRIES
        } else {
            max_entries
        };
        Self {
            inner: Mutex::new(Inner {
                entries: HashMap::new(),
                max_entries,
                max_bytes,
                bytes: 0,
            }),
        }
    }

    /// Looks up a live entry. An expired entry is removed and reported missing.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<CacheEntry> {
        self.get_at(key, Instant::now())
    }

    /// Explicit-clock lookup used by the TTL tests.
    #[must_use]
    pub fn get_at(&self, key: &str, now: Instant) -> Option<CacheEntry> {
        let mut inner = self.lock();
        let entry = inner.entries.get(key)?;
        if entry.expires <= now {
            let removed = inner.entries.remove(key).expect("entry exists");
            inner.bytes -= removed.body.len();
            return None;
        }
        Some(entry.clone())
    }

    /// Inserts or replaces one entry, evicting the earliest expiry first.
    pub fn put(&self, key: String, entry: CacheEntry) {
        let mut inner = self.lock();
        if let Some(previous) = inner.entries.remove(&key) {
            inner.bytes -= previous.body.len();
        }
        let size = entry.body.len();
        while !inner.entries.is_empty()
            && (inner.entries.len() >= inner.max_entries || inner.bytes + size > inner.max_bytes)
        {
            let Some(oldest_key) = inner
                .entries
                .iter()
                .min_by_key(|(_, candidate)| candidate.expires)
                .map(|(candidate_key, _)| candidate_key.clone())
            else {
                break;
            };
            let removed = inner.entries.remove(&oldest_key).expect("entry exists");
            inner.bytes -= removed.body.len();
        }
        inner.bytes += size;
        inner.entries.insert(key, entry);
    }

    /// Drops every entry; called before every write request.
    pub fn clear(&self) {
        let mut inner = self.lock();
        inner.entries.clear();
        inner.bytes = 0;
    }

    /// Number of live entries, including not-yet-purged expired rows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// The cache handle shared by the invalidation layer and every route layer.
#[derive(Clone, Debug)]
pub struct CacheHandle(pub Arc<ReadCache>);

impl CacheHandle {
    #[must_use]
    pub fn new(cache: Arc<ReadCache>) -> Self {
        Self(cache)
    }
}

/// One cached route's middleware state: the shared cache and its TTL.
#[derive(Clone, Debug)]
pub struct CacheState {
    pub cache: Arc<ReadCache>,
    pub ttl: Duration,
}

impl CacheState {
    #[must_use]
    pub fn new(cache: Arc<ReadCache>, ttl: Duration) -> Self {
        Self { cache, ttl }
    }
}

/// `cacheRead(ttl)`: serve a stored 200 for a GET, otherwise record one.
pub async fn cache_read(State(state): State<CacheState>, request: Request, next: Next) -> Response {
    if request.method() != Method::GET {
        return next.run(request).await;
    }

    let key = request
        .uri()
        .path_and_query()
        .map(|value| value.as_str().to_string())
        .unwrap_or_else(|| request.uri().path().to_string());

    if let Some(entry) = state.cache.get(&key) {
        let mut response = Response::new(Body::from(entry.body));
        *response.status_mut() = entry.status;
        *response.headers_mut() = entry.headers;
        if let Some(RequestId(id)) = request.extensions().get::<RequestId>()
            && let Ok(value) = HeaderValue::from_str(id)
        {
            response.headers_mut().insert(REQUEST_ID_HEADER, value);
        }
        return response;
    }

    let response = next.run(request).await;
    if response.status() != StatusCode::OK {
        return response;
    }

    // A response must announce its size before it is buffered, or a large body
    // is read into memory only to be rejected afterwards. `Content-Length` is
    // authoritative when set, but axum adds it after this middleware runs, so
    // the body's exact size hint stands in for it. When neither is available
    // the size cannot be bounded, and the body is streamed to the client
    // untouched.
    let declared = match response.headers().get(header::CONTENT_LENGTH) {
        Some(value) => value
            .to_str()
            .ok()
            .and_then(|value| value.parse::<usize>().ok()),
        None => response
            .body()
            .size_hint()
            .exact()
            .and_then(|value| usize::try_from(value).ok()),
    };
    let Some(declared) = declared else {
        return response;
    };
    if declared > READ_CACHE_MAX_BODY {
        return response;
    }

    let (mut parts, body) = response.into_parts();
    let Ok(body) = to_bytes(body, READ_CACHE_MAX_BODY.saturating_add(1)).await else {
        // The handler's body could not be collected: never cache a truncated
        // response.
        return crate::middleware::internal_error().into_response();
    };

    if body.len() > READ_CACHE_MAX_BODY {
        return Response::from_parts(parts, Body::from(body));
    }

    parts.headers.remove(REQUEST_ID_HEADER);
    parts.headers.remove(header::DATE);
    state.cache.put(
        key,
        CacheEntry {
            status: parts.status,
            headers: parts.headers.clone(),
            body: body.to_vec(),
            expires: Instant::now() + state.ttl,
        },
    );
    Response::from_parts(parts, Body::from(body))
}

/// `invalidateReadCacheOnWrite`: every POST/PUT/PATCH/DELETE clears the cache
/// before the handler runs, so the next read fills from the database.
pub async fn invalidate_read_cache_on_write(
    State(state): State<CacheHandle>,
    request: Request,
    next: Next,
) -> Response {
    if matches!(
        *request.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    ) {
        state.0.clear();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::Router;
    use axum::routing::{get, post};
    use tower::ServiceExt;

    fn entry(body: &str, expires: Instant) -> CacheEntry {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
        CacheEntry {
            status: StatusCode::OK,
            headers,
            body: body.as_bytes().to_vec(),
            expires,
        }
    }

    #[test]
    fn expired_entries_are_removed_and_reported_missing() {
        let cache = ReadCache::new(8);
        let now = Instant::now();
        cache.put("a".to_string(), entry("one", now + Duration::from_secs(5)));

        assert!(cache.get_at("a", now).is_some());
        assert!(cache.get_at("a", now + Duration::from_secs(5)).is_none());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn eviction_drops_the_earliest_expiry() {
        let cache = ReadCache::new(2);
        let now = Instant::now();
        cache.put("a".to_string(), entry("a", now + Duration::from_secs(10)));
        cache.put("b".to_string(), entry("b", now + Duration::from_secs(20)));
        cache.put("c".to_string(), entry("c", now + Duration::from_secs(30)));

        assert_eq!(cache.len(), 2);
        assert!(cache.get_at("a", now).is_none());
        assert!(cache.get_at("b", now).is_some());
        assert!(cache.get_at("c", now).is_some());
    }

    #[test]
    fn replacing_a_key_refunds_its_bytes() {
        let cache = ReadCache::with_limits(8, 10);
        let now = Instant::now();
        cache.put(
            "a".to_string(),
            entry("12345", now + Duration::from_secs(10)),
        );
        cache.put(
            "a".to_string(),
            entry("12345", now + Duration::from_secs(20)),
        );
        cache.put(
            "b".to_string(),
            entry("12345", now + Duration::from_secs(30)),
        );

        assert_eq!(cache.len(), 2, "the replacement frees the old bytes");
    }

    #[test]
    fn byte_overflow_evicts_until_the_entry_fits() {
        let cache = ReadCache::with_limits(8, 10);
        let now = Instant::now();
        cache.put(
            "a".to_string(),
            entry("123456", now + Duration::from_secs(10)),
        );
        cache.put(
            "b".to_string(),
            entry("123456", now + Duration::from_secs(20)),
        );

        assert_eq!(cache.len(), 1);
        assert!(cache.get_at("a", now).is_none());
        assert!(cache.get_at("b", now).is_some());
    }

    #[tokio::test]
    async fn repeated_gets_are_served_from_the_cache() {
        let cache = Arc::new(ReadCache::new(8));
        let hits = Arc::new(AtomicUsize::new(0));
        let app = source_router(cache.clone(), hits.clone());

        let first = send_text(app.clone(), "/source").await;
        let second = send_text(app.clone(), "/source").await;

        assert_eq!(first, "body 1");
        assert_eq!(second, "body 1");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_write_clears_the_cache() {
        let cache = Arc::new(ReadCache::new(8));
        let hits = Arc::new(AtomicUsize::new(0));
        let app = source_router(cache.clone(), hits.clone());

        assert_eq!(send_text(app.clone(), "/source").await, "body 1");

        let write = Request::builder()
            .method(Method::POST)
            .uri("/write")
            .body(Body::empty())
            .expect("request");
        app.clone().oneshot(write).await.expect("write response");

        assert_eq!(send_text(app.clone(), "/source").await, "body 2");
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn bodies_over_the_per_body_limit_are_not_cached() {
        let cache = Arc::new(ReadCache::new(8));
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let app = Router::new().route(
            "/big",
            get(move || {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    "x".repeat(READ_CACHE_MAX_BODY + 1)
                }
            })
            .layer(axum::middleware::from_fn_with_state(
                CacheState::new(cache.clone(), Duration::from_secs(30)),
                cache_read,
            )),
        );

        for _ in 0..2 {
            let request = Request::builder()
                .uri("/big")
                .body(Body::empty())
                .expect("request");
            let response = app.clone().oneshot(request).await.expect("response");
            assert_eq!(response.status(), StatusCode::OK);
        }
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn large_bodies_are_passed_through_without_being_cached() {
        let cache = Arc::new(ReadCache::new(8));
        let app = caching_route("/large", get(|| async { "x".repeat(300 << 10) }), &cache);

        let request = Request::builder()
            .uri("/large")
            .body(Body::empty())
            .expect("request");
        let response = app.oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let length = (300 << 10).to_string();
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_LENGTH)
                .and_then(|value| value.to_str().ok()),
            Some(length.as_str())
        );
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert_eq!(body.len(), 300 << 10);
        assert_eq!(cache.len(), 0);
    }

    #[tokio::test]
    async fn bodies_without_a_declared_length_are_passed_through_and_not_cached() {
        let cache = Arc::new(ReadCache::new(8));
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let app = caching_route(
            "/streamed",
            get(move || {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    let source = Body::from("streamed body");
                    Response::new(Body::from_stream(source.into_data_stream()))
                }
            }),
            &cache,
        );

        for _ in 0..2 {
            let request = Request::builder()
                .uri("/streamed")
                .body(Body::empty())
                .expect("request");
            let response = app.clone().oneshot(request).await.expect("response");
            assert_eq!(response.status(), StatusCode::OK);
            assert!(response.headers().get(header::CONTENT_LENGTH).is_none());
            let body = to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body");
            assert_eq!(body.as_ref(), b"streamed body");
        }
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        assert_eq!(cache.len(), 0);
    }

    #[tokio::test]
    async fn small_bodies_are_still_cached() {
        let cache = Arc::new(ReadCache::new(8));
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let app = caching_route(
            "/small",
            get(move || {
                let counter = counter.clone();
                async move {
                    let value = counter.fetch_add(1, Ordering::SeqCst) + 1;
                    let body = format!("body {value}");
                    Response::builder()
                        .header(header::CONTENT_LENGTH, body.len().to_string())
                        .body(Body::from(body))
                        .expect("response")
                }
            }),
            &cache,
        );

        assert_eq!(send_text(app.clone(), "/small").await, "body 1");
        assert_eq!(send_text(app.clone(), "/small").await, "body 1");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(cache.len(), 1);
    }

    /// A router with one cache-read route, so the pass-through tests can build
    /// their own handler without repeating the layer.
    fn caching_route(
        uri: &str,
        handler: axum::routing::MethodRouter,
        cache: &Arc<ReadCache>,
    ) -> Router {
        Router::new().route(
            uri,
            handler.layer(axum::middleware::from_fn_with_state(
                CacheState::new(cache.clone(), Duration::from_secs(30)),
                cache_read,
            )),
        )
    }

    fn source_router(cache: Arc<ReadCache>, hits: Arc<AtomicUsize>) -> Router {
        let counter = hits.clone();
        Router::new()
            .route(
                "/source",
                get(move || {
                    let counter = counter.clone();
                    async move {
                        let value = counter.fetch_add(1, Ordering::SeqCst) + 1;
                        format!("body {value}")
                    }
                })
                .layer(axum::middleware::from_fn_with_state(
                    CacheState::new(cache.clone(), Duration::from_secs(30)),
                    cache_read,
                )),
            )
            .route("/write", post(|| async { "written" }))
            .layer(axum::middleware::from_fn_with_state(
                CacheHandle::new(cache),
                invalidate_read_cache_on_write,
            ))
    }

    async fn send_text(app: Router, uri: &str) -> String {
        let request = Request::builder()
            .uri(uri)
            .body(Body::empty())
            .expect("request");
        let response = app.oneshot(request).await.expect("response");
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        String::from_utf8(body.to_vec()).expect("utf8")
    }
}
