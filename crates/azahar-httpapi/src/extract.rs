//! JSON request decoding and client address resolution.
//!
//! [`decode_json`] is the drop-in port of Go's `decodeJSON[T]`: the same
//! content-type gate, the same 1 MiB ceiling and the same problem envelope for
//! every failure. Payload structs should use `#[serde(default)]` and
//! `#[serde(deny_unknown_fields)]` to reproduce Go's zero-value and
//! `DisallowUnknownFields` behaviour.

use std::collections::HashMap;
use std::error::Error as _;
use std::net::SocketAddr;

use axum::body::to_bytes;
use axum::extract::{FromRequest, Request};
use axum::http::{HeaderMap, header};
use http_body_util::LengthLimitError;
use mime::Mime;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::respond::{ApiProblem, problem, problem_fields};

/// Request bodies are capped at 1 MiB, matching the Go middleware.
pub const BODY_LIMIT: usize = 1 << 20;

/// Raw top-level fields, kept for handlers that support partial updates.
pub type RawFields = HashMap<String, Value>;

/// A decoded JSON payload plus its untouched top-level fields.
#[derive(Debug, Clone)]
pub struct JsonBody<T> {
    pub payload: T,
    pub raw: RawFields,
}

impl<S, T> FromRequest<S> for JsonBody<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiProblem;

    async fn from_request(request: Request, _state: &S) -> Result<Self, Self::Rejection> {
        let (payload, raw) = decode_json::<T>(request).await?;
        Ok(Self { payload, raw })
    }
}

/// Decodes one JSON object into `T` and returns the raw field map.
///
/// Failure modes, in the Go order:
/// 415 for a non-JSON content type, 413 past the body limit, 422 with a
/// `body` field for an empty body, 422 for invalid JSON or a non-object, 422
/// for an unknown field, 422 for a shape mismatch, and 422 for trailing data.
pub async fn decode_json<T>(request: Request) -> Result<(T, RawFields), ApiProblem>
where
    T: DeserializeOwned,
{
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !is_json_media_type(content_type) {
        return Err(problem(
            415,
            "unsupported_media_type",
            "Send the request body as application/json.",
        ));
    }

    let bytes = match to_bytes(request.into_body(), BODY_LIMIT).await {
        Ok(bytes) => bytes,
        Err(err) => {
            if err
                .source()
                .is_some_and(|source| source.is::<LengthLimitError>())
            {
                return Err(problem(
                    413,
                    "payload_too_large",
                    "The request body is too large.",
                ));
            }
            return Err(problem(
                400,
                "invalid_json",
                "The request body could not be read.",
            ));
        }
    };
    let body = bytes.as_ref();
    if std::str::from_utf8(body).is_ok_and(|text| text.trim().is_empty()) {
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            HashMap::from([("body".to_string(), "Send a JSON object.".to_string())]),
        ));
    }

    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let raw = match RawFields::deserialize(&mut deserializer) {
        Ok(raw) => raw,
        Err(_) => {
            return Err(problem(
                422,
                "invalid_json",
                "The request body is not valid JSON.",
            ));
        }
    };
    if deserializer.end().is_err() {
        return Err(problem(422, "invalid_json", "Send one JSON object."));
    }

    // Decode the typed payload from the deduplicated field map. Go's map
    // unmarshal keeps the last duplicate key and never rejects one; decoding
    // the raw bytes directly would make serde fail on "duplicate field".
    let deduplicated = match serde_json::to_vec(&raw) {
        Ok(bytes) => bytes,
        Err(_) => {
            return Err(problem(
                422,
                "invalid_json",
                "The request body does not match the expected shape.",
            ));
        }
    };
    match serde_json::from_slice::<T>(&deduplicated) {
        Ok(payload) => Ok((payload, raw)),
        Err(err) if err.to_string().contains("unknown field") => Err(problem(
            422,
            "validation_error",
            "The request body contains an unknown field.",
        )),
        Err(_) => Err(problem(
            422,
            "invalid_json",
            "The request body does not match the expected shape.",
        )),
    }
}

/// Go's `mime.ParseMediaType` followed by an exact `application/json`
/// comparison. The media type is case-insensitive; parameters such as
/// `charset=utf-8` are accepted. Go tolerates spaces around the parameter
/// separators, so they are trimmed before handing the value to `mime`.
fn is_json_media_type(value: &str) -> bool {
    let normalized = value
        .split(';')
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(";");
    normalized.parse::<Mime>().is_ok_and(|mime| {
        media_type_eq(mime.type_().as_str(), "application")
            && media_type_eq(mime.subtype().as_str(), "json")
    })
}

fn media_type_eq(value: &str, expected: &str) -> bool {
    value.eq_ignore_ascii_case(expected)
}

/// Whether a connected peer is a local reverse proxy, and so may speak for the
/// client in `X-Forwarded-For`. Public peers are the client themselves: their
/// header would be a spoofed address for the rate limiters and the captcha.
fn is_trusted_proxy_peer(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(address) => {
            address.is_loopback() || address.is_private() || address.is_link_local()
        }
        std::net::IpAddr::V6(address) => {
            address.is_loopback() || address.is_unique_local() || address.is_unicast_link_local()
        }
    }
}

/// Resolves the client address exactly like Go's `clientIP`.
///
/// With `trust_proxy`, the first `X-Forwarded-For` entry wins when it is not
/// empty and the connection itself came from a local reverse proxy (or from a
/// caller with no peer information, which is how oneshot tests connect).
/// Otherwise the peer address loses its port (the text after the last `:`) and
/// any IPv6 brackets.
#[must_use]
pub fn client_ip(headers: &HeaderMap, peer: Option<SocketAddr>, trust_proxy: bool) -> String {
    let forwarded_from_proxy =
        trust_proxy && peer.is_none_or(|address| is_trusted_proxy_peer(address.ip()));
    if forwarded_from_proxy
        && let Some(forwarded) = headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
    {
        let first = forwarded.split(',').next().unwrap_or_default().trim();
        if !first.is_empty() {
            return first.to_string();
        }
    }

    let mut host = peer.map(|address| address.to_string()).unwrap_or_default();
    if let Some(index) = host.rfind(':')
        && index > 0
    {
        host.truncate(index);
    }
    host.trim_matches(['[', ']']).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_type_accepts_parameters_and_case() {
        assert!(is_json_media_type("application/json"));
        assert!(is_json_media_type("application/json; charset=utf-8"));
        assert!(is_json_media_type(" Application/JSON ; Charset=UTF-8 "));
        assert!(!is_json_media_type("text/json"));
        assert!(!is_json_media_type("application/jsonx"));
        assert!(!is_json_media_type(""));
        assert!(!is_json_media_type("application"));
    }

    #[test]
    fn forwarded_for_uses_the_first_entry() {
        let mut headers = HeaderMap::new();
        headers.append(
            "x-forwarded-for",
            "203.0.113.7, 70.41.3.18, 150.172.238.178".parse().unwrap(),
        );
        headers.append("x-forwarded-for", "10.0.0.1".parse().unwrap());
        assert_eq!(
            client_ip(&headers, None, true),
            "203.0.113.7",
            "the first value of the first header wins"
        );
        assert_eq!(client_ip(&headers, None, false), "");
    }

    #[test]
    fn peer_address_loses_port_and_brackets() {
        let headers = HeaderMap::new();
        let ipv4: SocketAddr = "127.0.0.1:54321".parse().unwrap();
        let ipv6: SocketAddr = "[::1]:54321".parse().unwrap();
        assert_eq!(client_ip(&headers, Some(ipv4), false), "127.0.0.1");
        assert_eq!(client_ip(&headers, Some(ipv6), false), "::1");
    }

    fn forwarded_headers(value: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", value.parse().unwrap());
        headers
    }

    #[test]
    fn a_local_proxy_peer_may_forward_the_client_address() {
        for peer in [
            "127.0.0.1:4000",
            "10.1.2.3:4000",
            "192.168.0.9:4000",
            "169.254.1.1:4000",
            "[::1]:4000",
            "[fd00::1]:4000",
            "[fe80::1]:4000",
        ] {
            let peer: SocketAddr = peer.parse().expect("peer");
            assert_eq!(
                client_ip(
                    &forwarded_headers("203.0.113.7, 70.41.3.18"),
                    Some(peer),
                    true
                ),
                "203.0.113.7",
                "peer {peer} is a local proxy"
            );
        }
    }

    #[test]
    fn a_public_peer_cannot_forward_a_client_address() {
        for peer in ["198.51.100.4:4000", "[2001:db8::1]:4000"] {
            let peer: SocketAddr = peer.parse().expect("peer");
            assert_eq!(
                client_ip(&forwarded_headers("203.0.113.7"), Some(peer), true),
                peer.ip().to_string(),
                "peer {peer} is the client, not a proxy"
            );
        }
    }

    #[test]
    fn a_missing_peer_still_honours_the_header() {
        assert_eq!(
            client_ip(&forwarded_headers("203.0.113.7"), None, true),
            "203.0.113.7"
        );
        assert_eq!(
            client_ip(&forwarded_headers("203.0.113.7"), None, false),
            ""
        );
    }
}
