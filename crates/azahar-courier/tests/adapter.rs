//! Wire-contract tests against a throwaway local HTTP server. The real courier
//! service is never contacted.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use azahar_courier::{Client, Courier, Error, Options, OrderRequest, Recipient};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::time::timeout;

#[derive(Debug, Default)]
struct CapturedRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> CapturedRequest {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];

    loop {
        if let Some(end) = find(&buffer, b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buffer[..end]);
            let mut lines = head.split("\r\n");
            let request_line = lines.next().unwrap_or_default();
            let mut parts = request_line.split_whitespace();
            let method = parts.next().unwrap_or_default().to_owned();
            let path = parts.next().unwrap_or_default().to_owned();

            let mut headers = HashMap::new();
            for line in lines {
                if let Some((name, value)) = line.split_once(':') {
                    headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
                }
            }

            let content_length = headers
                .get("content-length")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            let body_start = end + 4;
            while buffer.len() < body_start + content_length {
                let read = socket.read(&mut chunk).await.unwrap_or(0);
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);
            }

            let body_end = (body_start + content_length).min(buffer.len());
            return CapturedRequest {
                method,
                path,
                headers,
                body: buffer[body_start..body_end].to_vec(),
            };
        }

        let read = socket.read(&mut chunk).await.unwrap_or(0);
        if read == 0 || buffer.len() > 64 * 1024 {
            return CapturedRequest::default();
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}

async fn serve_once(
    status: u16,
    headers: &[(&str, &str)],
    body: &'static str,
) -> (String, oneshot::Receiver<CapturedRequest>) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind test server");
    let address = listener.local_addr().expect("test server address");
    let (sender, receiver) = oneshot::channel();
    let headers: Vec<(String, String)> = headers
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();

    tokio::spawn(async move {
        let accepted = timeout(Duration::from_secs(5), listener.accept()).await;
        let Ok(Ok((mut socket, _))) = accepted else {
            return;
        };
        let request = read_request(&mut socket).await;
        let mut response = format!(
            "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        for (name, value) in &headers {
            response.push_str(name);
            response.push_str(": ");
            response.push_str(value);
            response.push_str("\r\n");
        }
        response.push_str("\r\n");
        response.push_str(body);
        let _ = socket.write_all(response.as_bytes()).await;
        let _ = sender.send(request);
    });

    (format!("http://{address}"), receiver)
}

fn client(base_url: String, courier_key: &str) -> Client {
    let mut credentials = BTreeMap::new();
    credentials.insert("apiId".to_owned(), "test-id".to_owned());
    credentials.insert("apiToken".to_owned(), "test-token".to_owned());
    Client::new(Options {
        base_url,
        courier_key: courier_key.to_owned(),
        credentials,
        from_wilaya: 16,
        timeout: Duration::from_secs(5),
        ..Options::default()
    })
}

fn order_request() -> OrderRequest {
    OrderRequest {
        reference: "AZ-2026-0041".into(),
        recipient: Recipient {
            full_name: "Amina Bouzid".into(),
            phone: "0551234567".into(),
            phone_alt: "0661234567".into(),
            wilaya_code: 16,
            commune_name: "Bab Ezzouar".into(),
            address_line: "12 rue des Oliviers".into(),
        },
        delivery_type: "home".into(),
        stop_desk_id: "SBX-16".into(),
        product_list: "Orange blossom cleansing oil x1".into(),
        cod_amount: 5400,
        free_shipping: true,
        notes: "Call before delivery".into(),
    }
}

#[tokio::test]
async fn create_order_sends_the_wire_contract_and_maps_the_response() {
    let (base_url, captured) = serve_once(
        200,
        &[("Content-Type", "application/json")],
        r#"{"trackingNumber":"yal-ABC123","courierReference":"ref-1","status":"created","reference":"AZ-2026-0041"}"#,
    )
    .await;
    let client = client(base_url, "yalidine");

    let shipment = client
        .create_order(order_request())
        .await
        .expect("create order");
    assert_eq!(shipment.tracking_number, "yal-ABC123");
    assert_eq!(shipment.courier_reference, "ref-1");
    assert_eq!(shipment.status, "created");
    assert_eq!(shipment.reference, "AZ-2026-0041");

    let request = captured.await.expect("captured request");
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/v1/orders");
    assert_eq!(
        request.headers.get("accept").map(String::as_str),
        Some("application/json")
    );
    assert_eq!(
        request.headers.get("user-agent").map(String::as_str),
        Some("azahar-store/1.0")
    );
    assert_eq!(
        request.headers.get("content-type").map(String::as_str),
        Some("application/json")
    );

    // Semantic body check: serde does not HTML-escape like `json.Marshal`, so
    // the bytes can differ while any JSON server decodes the same document.
    let body: Value = serde_json::from_slice(&request.body).expect("request json");
    assert_eq!(body["courier"], "yalidine");
    assert_eq!(body["credentials"]["apiId"], "test-id");
    assert_eq!(body["credentials"]["apiToken"], "test-token");
    assert_eq!(body["options"]["fromWilaya"], 16);
    assert_eq!(body["order"]["reference"], "AZ-2026-0041");
    assert_eq!(body["order"]["deliveryType"], "home");
    assert_eq!(body["order"]["stopDeskId"], "SBX-16");
    assert_eq!(
        body["order"]["productList"],
        "Orange blossom cleansing oil x1"
    );
    assert_eq!(body["order"]["codAmount"], 5400);
    assert_eq!(body["order"]["freeShipping"], true);
    assert_eq!(body["order"]["notes"], "Call before delivery");
    assert_eq!(body["order"]["recipient"]["fullName"], "Amina Bouzid");
    assert_eq!(body["order"]["recipient"]["phone"], "0551234567");
    assert_eq!(body["order"]["recipient"]["phoneAlt"], "0661234567");
    assert_eq!(body["order"]["recipient"]["wilayaCode"], 16);
    assert_eq!(body["order"]["recipient"]["communeName"], "Bab Ezzouar");
    assert_eq!(
        body["order"]["recipient"]["addressLine"],
        "12 rue des Oliviers"
    );
}

#[tokio::test]
async fn response_keys_are_matched_case_insensitively() {
    let (base_url, _captured) = serve_once(
        200,
        &[("Content-Type", "application/json")],
        r#"{"TRACKINGNUMBER":"yal-ABC123","CourierReference":"ref-1","STATUS":"created","Reference":"AZ-2026-0041"}"#,
    )
    .await;
    let client = client(base_url, "yalidine");

    let shipment = client
        .create_order(order_request())
        .await
        .expect("create order");
    assert_eq!(shipment.tracking_number, "yal-ABC123");
    assert_eq!(shipment.courier_reference, "ref-1");
    assert_eq!(shipment.status, "created");
    assert_eq!(shipment.reference, "AZ-2026-0041");
}

#[tokio::test]
async fn the_http_error_envelope_is_preserved() {
    let (base_url, _captured) = serve_once(
        422,
        &[("Content-Type", "application/json")],
        r#"{"error":{"code":"invalid_phone","message":" Invalid phone. ","fields":{"recipient.phone":"Use 05, 06 or 07."}}}"#,
    )
    .await;
    let client = client(base_url, "yalidine");

    let error = client
        .create_order(OrderRequest::default())
        .await
        .expect_err("422");
    let api = error.api().expect("api error");
    assert_eq!(api.status, 422);
    assert_eq!(api.code, "invalid_phone");
    assert_eq!(api.message, "Invalid phone.");
    assert_eq!(
        api.fields.get("recipient.phone").map(String::as_str),
        Some("Use 05, 06 or 07.")
    );
    assert_eq!(error.to_string(), "Invalid phone.");
}

#[tokio::test]
async fn retry_after_is_parsed_from_the_header() {
    let (base_url, _captured) = serve_once(
        429,
        &[("Content-Type", "application/json"), ("Retry-After", "7")],
        r#"{"error":{"code":"rate_limited","message":"Slow down."}}"#,
    )
    .await;
    let client = client(base_url, "yalidine");

    let error = client
        .create_order(OrderRequest::default())
        .await
        .expect_err("429");
    let api = error.api().expect("api error");
    assert_eq!(api.status, 429);
    assert_eq!(api.retry_after, Duration::from_secs(7));
}

#[tokio::test]
async fn redirects_are_returned_instead_of_followed() {
    let (base_url, captured) =
        serve_once(302, &[("Location", "http://127.0.0.1:1/elsewhere")], "").await;
    let client = client(base_url, "yalidine");

    let error = client
        .create_order(OrderRequest::default())
        .await
        .expect_err("302");
    let api = error.api().expect("api error");
    assert_eq!(api.status, 302);
    assert_eq!(api.message, "The courier returned HTTP 302.");

    let request = captured.await.expect("captured request");
    assert_eq!(request.method, "POST");
}

#[tokio::test]
async fn desks_and_tracking_use_their_paths_and_payloads() {
    let (desks_url, desks_captured) = serve_once(
        200,
        &[("Content-Type", "application/json")],
        r#"[{"id":"SBX-16","name":"Sandbox desk","wilayaCode":16,"communeName":"Bab Ezzouar"}]"#,
    )
    .await;
    let desks_client = client(desks_url, "sandbox");
    let desks = desks_client.desks(16).await.expect("desks");
    assert_eq!(desks.len(), 1);
    assert_eq!(desks[0].id, "SBX-16");

    let desks_request = desks_captured.await.expect("desks request");
    assert_eq!(desks_request.method, "POST");
    assert_eq!(desks_request.path, "/v1/desks");
    let desks_body: Value = serde_json::from_slice(&desks_request.body).expect("desks json");
    assert_eq!(desks_body["courier"], "sandbox");
    assert_eq!(desks_body["options"]["fromWilaya"], 16);
    assert_eq!(desks_body["wilayaCode"], 16);

    let (track_url, track_captured) = serve_once(
        200,
        &[("Content-Type", "application/json")],
        r#"{"trackingNumber":"yal-ABC123","status":"in_transit","events":[{"status":"in_transit","rawStatus":"En transit","timestamp":"2026-11-11T10:00:00Z"}]}"#,
    )
    .await;
    let track_client = client(track_url, "yalidine");
    let tracking = track_client
        .track("", "yal-ABC123")
        .await
        .expect("tracking");
    assert_eq!(tracking.status, "in_transit");
    assert_eq!(tracking.events.len(), 1);
    assert_eq!(tracking.events[0].raw_status, "En transit");
    assert_eq!(tracking.events[0].timestamp, "2026-11-11T10:00:00Z");

    let track_request = track_captured.await.expect("track request");
    assert_eq!(track_request.method, "POST");
    assert_eq!(track_request.path, "/v1/track");
    let track_body: Value = serde_json::from_slice(&track_request.body).expect("track json");
    assert_eq!(track_body["courier"], "yalidine");
    assert_eq!(track_body["trackingNumber"], "yal-ABC123");
    assert_eq!(track_body["credentials"]["apiId"], "test-id");
}

#[tokio::test]
async fn communes_use_a_get_query_and_null_lists_become_empty() {
    let (base_url, captured) = serve_once(200, &[], "null").await;
    let client = client(base_url, "yalidine");

    let communes = client.communes(16).await.expect("communes");
    assert!(communes.is_empty());

    let request = captured.await.expect("captured request");
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/v1/communes?wilaya=16");
    assert!(request.body.is_empty());
}

#[tokio::test]
async fn a_non_json_success_body_is_a_transport_error() {
    let (base_url, _captured) = serve_once(200, &[], "<html>not json</html>").await;
    let client = client(base_url, "yalidine");

    let error = client
        .create_order(OrderRequest::default())
        .await
        .expect_err("decode failure");
    assert!(error.is_transport(), "error = {error}");
    assert!(
        error
            .to_string()
            .starts_with("courier transport: decode courier response:"),
        "error = {error}"
    );
    assert!(!matches!(error, Error::Api(_)));
}
