#[allow(dead_code)]
mod common;
#[allow(dead_code)]
mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use azahar_desktop_api::{MAX_IMAGE_BYTES, Response};
use serde_json::Value;
use support::{MockResponse, MockServer};

fn data(response: &Response) -> &str {
    response
        .data
        .as_ref()
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("no data: {response:?}"))
}

#[test]
fn image_loads_and_caches() {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    let server = MockServer::start(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        MockResponse::new(200, "image/png", vec![0x89, 0x50, 0x4e, 0x47])
    });
    let test = common::test_client(server.url());
    let response = test.client.image("/api/images/good.png");
    assert!(response.ok, "{:?}", response.error);
    assert!(data(&response).starts_with("data:image/png;base64,"));
    assert!(test.client.image("/api/images/good.png").ok);
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "image was fetched more than once"
    );
}

#[test]
fn image_allowed_types() {
    let server = MockServer::start(|request| {
        let media = match request.path.as_str() {
            "/api/images/a.png" => "image/png",
            "/api/images/a.jpg" => "image/jpeg",
            "/api/images/a.webp" => "image/webp",
            "/api/images/a.gif" => "image/gif",
            "/api/images/a.avif" => "image/avif",
            "/api/images/a.upper" => "IMAGE/PNG",
            _ => "application/octet-stream",
        };
        MockResponse::new(200, &format!("{media}; charset=binary"), vec![1, 2, 3])
    });
    let test = common::test_client(server.url());
    let cases = [
        ("/api/images/a.png", "image/png"),
        ("/api/images/a.jpg", "image/jpeg"),
        ("/api/images/a.webp", "image/webp"),
        ("/api/images/a.gif", "image/gif"),
        ("/api/images/a.avif", "image/avif"),
        ("/api/images/a.upper", "image/png"),
    ];
    for (path, media) in cases {
        let response = test.client.image(path);
        assert!(response.ok, "{path}: {:?}", response.error);
        assert!(
            data(&response).starts_with(&format!("data:{media};base64,")),
            "{path}"
        );
    }
}

#[test]
fn image_rejects_invalid_requests() {
    let server = MockServer::start(|request| match request.path.as_str() {
        "/api/images/html" => MockResponse::new(200, "text/html", b"<html>".to_vec()),
        "/api/images/svg" => MockResponse::new(200, "image/svg+xml", b"<svg/>".to_vec()),
        "/api/images/missing" => MockResponse::json(
            404,
            r#"{"error":{"code":"not_found","message":"Not found.","fields":{}}}"#,
        ),
        _ => MockResponse::new(200, "image/png", vec![1]),
    });
    let test = common::test_client(server.url());
    let cases = [
        ("/images/good.png", "invalid_image"),
        ("/api/other/image.png", "invalid_image"),
        ("/api/images/../secret", "invalid_path"),
        ("/api/images/%2e%2e/secret", "invalid_path"),
        ("/api/images/x\\y", "invalid_path"),
        ("/api/images/html", "image_failed"),
        ("/api/images/svg", "image_failed"),
        ("/api/images/missing", "image_failed"),
    ];
    for (path, code) in cases {
        let response = test.client.image(path);
        assert!(!response.ok, "{path} was accepted");
        assert_eq!(
            response.error.as_ref().map(|error| error.code.as_str()),
            Some(code),
            "{path}"
        );
    }
}

#[test]
fn image_size_limit() {
    let server = MockServer::start(|_| {
        MockResponse::new(200, "image/png", vec![0_u8; MAX_IMAGE_BYTES as usize + 1])
    });
    let test = common::test_client(server.url());
    let response = test.client.image("/api/images/huge");
    assert!(!response.ok);
    assert_eq!(response.error.expect("error").code, "image_failed");
}

#[test]
fn image_cache_is_bounded() {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    let server = MockServer::start(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        MockResponse::new(200, "image/png", vec![1])
    });
    let test = common::test_client(server.url());
    for index in 0..=azahar_desktop_api::IMAGE_CACHE_SIZE {
        let path = format!("/api/images/{index}");
        assert!(test.client.image(&path).ok, "{path}");
    }
    assert!(test.client.image("/api/images/0").ok, "image reload failed");
    assert_eq!(
        hits.load(Ordering::SeqCst),
        azahar_desktop_api::IMAGE_CACHE_SIZE + 2
    );
}

#[test]
fn image_cache_byte_limit_evicts() {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    let server = MockServer::start(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        MockResponse::new(200, "image/png", vec![1])
    });
    let test = common::test_client(server.url());
    test.client.set_image_cache_byte_limit(50);
    assert!(test.client.image("/api/images/one").ok);
    assert!(test.client.image("/api/images/two").ok);
    assert!(test.client.image("/api/images/two").ok);
    assert_eq!(
        hits.load(Ordering::SeqCst),
        2,
        "second image was not cached"
    );
    assert!(test.client.image("/api/images/one").ok);
    assert_eq!(hits.load(Ordering::SeqCst), 3, "first image stayed cached");
}
