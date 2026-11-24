//! Request construction, CSRF handling, response decoding and error envelopes.

use std::collections::BTreeMap;
use std::fmt;
use std::time::SystemTime;

use serde::Deserializer;
use serde::de::{IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::Value;

use crate::client::{Client, MAX_REQUEST_BODY_BYTES, MAX_RESPONSE_BYTES};
use crate::session::{
    CSRF_COOKIE_NAME, SESSION_COOKIE_NAME, StoredCookie, cookie_from_parsed, valid_cookie_value,
};
use crate::transport::{TransportError, TransportRequest};
use crate::types::{ApiError, Response};
use crate::urls::{TargetUrl, is_csrf_endpoint, target_url_with};

const ALLOWED_METHODS: [&str; 6] = ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE"];

pub(crate) struct RequestOptions {
    pub method: String,
    pub target: TargetUrl,
    pub payload: Option<Vec<u8>>,
    pub timeout: std::time::Duration,
    pub include_origin: bool,
    pub include_csrf: bool,
    pub max_bytes: u64,
}

pub struct HttpResult {
    pub status: i32,
    pub body: Vec<u8>,
    pub content_type: String,
}

/// Inputs of `requestHeadersLocked`, separated so the oracle can compare it.
pub struct HeaderInput<'a> {
    pub user_agent: &'a str,
    pub origin: &'a str,
    pub resolved_origin: &'a str,
    pub session: &'a StoredCookie,
    pub csrf: &'a StoredCookie,
    pub now: SystemTime,
    pub include_origin: bool,
    pub include_csrf: bool,
    pub has_body: bool,
}

pub fn build_headers(input: &HeaderInput<'_>) -> Vec<(String, String)> {
    let mut headers = vec![
        ("Accept".to_owned(), "application/json".to_owned()),
        ("User-Agent".to_owned(), input.user_agent.to_owned()),
        ("X-Requested-With".to_owned(), "azahar-web".to_owned()),
    ];

    let mut cookies = Vec::new();
    if input.session.active(input.now) {
        cookies.push(format!("{SESSION_COOKIE_NAME}={}", input.session.value));
    }
    if input.csrf.active(input.now) {
        cookies.push(format!("{CSRF_COOKIE_NAME}={}", input.csrf.value));
        if input.include_csrf {
            headers.push(("X-Csrf-Token".to_owned(), input.csrf.value.clone()));
        }
    }
    if !cookies.is_empty() {
        headers.push(("Cookie".to_owned(), cookies.join("; ")));
    }
    if input.include_origin {
        let origin = if input.origin.is_empty() {
            input.resolved_origin
        } else {
            input.origin
        };
        if !origin.is_empty() {
            headers.push(("Origin".to_owned(), origin.to_owned()));
        }
    }
    if input.has_body {
        headers.push(("Content-Type".to_owned(), "application/json".to_owned()));
    }
    headers
}

impl Client {
    /// Renderer-facing bridge call. Errors are returned inside the response.
    pub fn request(&self, method: &str, path: &str, body: Option<&Value>) -> Response {
        if is_csrf_endpoint(path) {
            return Response::failure(crate::urls::invalid_path());
        }
        match self.request_value(method, path, body) {
            Ok(data) => Response::success(data),
            Err(error) => Response::failure(error),
        }
    }

    pub(crate) fn request_value(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Option<Value>, ApiError> {
        let method = method.trim().to_ascii_uppercase();
        let method = if method.is_empty() {
            "GET".to_owned()
        } else {
            method
        };
        if !ALLOWED_METHODS.contains(&method.as_str()) {
            return Err(ApiError::new(
                0,
                "invalid_method",
                "That request method is not allowed.",
            ));
        }

        let target = self.target_url(path)?;
        let payload = encode_request_body(&method, body)?;
        let safe = method == "GET" || method == "HEAD";
        if !safe {
            self.ensure_origin()?;
            self.ensure_csrf()?;
        }

        let mut result = self.execute(RequestOptions {
            method: method.clone(),
            target: target.clone(),
            payload: payload.clone(),
            timeout: self.request_timeout(),
            include_origin: true,
            include_csrf: true,
            max_bytes: MAX_RESPONSE_BYTES,
        })?;

        if !safe && result.status == 403 && error_code(&result.body) == "csrf_failed" {
            let retry = self.target_url(path)?;
            if retry.generation != target.generation {
                return Err(settings_changed_error());
            }
            self.clear_csrf();
            self.ensure_csrf()?;
            result = self.execute(RequestOptions {
                method: method.clone(),
                target: retry,
                payload,
                timeout: self.request_timeout(),
                include_origin: true,
                include_csrf: true,
                max_bytes: MAX_RESPONSE_BYTES,
            })?;
        }

        if !(200..=299).contains(&result.status) {
            return Err(error_from_result(&result));
        }
        Ok(decode_json(&result.body))
    }

    pub(crate) fn target_url(&self, raw: &str) -> Result<TargetUrl, ApiError> {
        let (api_url, allowed_host, generation) = {
            let state = self.lock_state();
            (
                state.settings.api_url.clone(),
                state.settings.network_allowed_host.clone(),
                state.generation,
            )
        };
        target_url_with(&api_url, &allowed_host, generation, raw)
    }

    pub(crate) fn execute(&self, options: RequestOptions) -> Result<HttpResult, ApiError> {
        let now = SystemTime::now();
        let headers = {
            let state = self.lock_state();
            if state.generation != options.target.generation {
                return Err(settings_changed_error());
            }
            build_headers(&HeaderInput {
                user_agent: &self.user_agent,
                origin: &state.settings.origin,
                resolved_origin: &state.resolved_origin,
                session: &state.session,
                csrf: &state.csrf,
                now,
                include_origin: options.include_origin,
                include_csrf: options.include_csrf,
                has_body: options.payload.is_some(),
            })
        };

        let request = TransportRequest {
            method: options.method,
            url: options.target.url,
            headers,
            body: options.payload,
            timeout: options.timeout,
            max_bytes: options.max_bytes,
        };
        match self.transport.execute(request) {
            Ok(response) => {
                self.absorb_cookies(&response.set_cookies, options.target.generation);
                Ok(HttpResult {
                    status: response.status,
                    body: response.body,
                    content_type: response.content_type,
                })
            }
            Err(TransportError::ResponseTooLarge) => Err(response_too_large()),
            Err(TransportError::Network { timeout, .. }) => Err(network_failure(timeout)),
        }
    }

    pub(crate) fn absorb_cookies(&self, set_cookies: &[String], generation: u64) {
        let now = SystemTime::now();
        let mut session_changed = false;
        let session_value;

        {
            let mut state = self.lock_state();
            if generation != state.generation {
                return;
            }
            for header in set_cookies {
                let Ok(cookie) = cookie::Cookie::parse(header.clone()) else {
                    continue;
                };
                let next = cookie_from_parsed(&cookie, now).unwrap_or_default();
                match cookie.name() {
                    SESSION_COOKIE_NAME => {
                        if state.session != next {
                            state.session = next;
                            session_changed = true;
                        }
                    }
                    CSRF_COOKIE_NAME => state.csrf = next,
                    _ => {}
                }
            }
            session_value = state.session.value.clone();
        }

        if session_changed {
            self.persist_session(&session_value);
        }
    }

    pub(crate) fn ensure_csrf(&self) -> Result<(), ApiError> {
        if !self.active_csrf().is_empty() {
            return Ok(());
        }
        let target = self.target_url("/api/csrf")?;
        let result = self.execute(RequestOptions {
            method: "GET".to_owned(),
            target: target.clone(),
            payload: None,
            timeout: self.request_timeout(),
            include_origin: true,
            include_csrf: false,
            max_bytes: MAX_RESPONSE_BYTES,
        })?;
        if !(200..=299).contains(&result.status) {
            return Err(ApiError::new(
                result.status,
                "csrf_unavailable",
                "The API did not issue a security token. Try again.",
            ));
        }
        if !self.active_csrf().is_empty() {
            return Ok(());
        }

        if let Ok(Value::Object(body)) = serde_json::from_slice::<Value>(&result.body)
            && let Some(Value::String(token)) = body.get("csrf_token")
        {
            let token = token.trim();
            if valid_cookie_value(token) {
                self.store_csrf(token, target.generation);
                return Ok(());
            }
        }
        Err(ApiError::new(
            result.status,
            "csrf_unavailable",
            "The API did not issue a security token. Try again.",
        ))
    }
}

pub fn encode_request_body(
    method: &str,
    body: Option<&Value>,
) -> Result<Option<Vec<u8>>, ApiError> {
    if method == "GET" || method == "HEAD" {
        return Ok(None);
    }
    match body {
        Some(body) => {
            let payload = go_json_encode(body)?;
            if payload.len() > MAX_REQUEST_BODY_BYTES {
                return Err(ApiError::new(
                    0,
                    "payload_too_large",
                    "The request body is too large.",
                ));
            }
            Ok(Some(payload))
        }
        None => Ok(Some(b"{}".to_vec())),
    }
}

/// `json.Marshal` compatibility: escape HTML characters and the JS line
/// separators that Go escapes by default.
fn go_json_encode(value: &Value) -> Result<Vec<u8>, ApiError> {
    let data = serde_json::to_vec(value)
        .map_err(|_| ApiError::new(0, "invalid_body", "The request body could not be sent."))?;
    let text = String::from_utf8(data)
        .map_err(|_| ApiError::new(0, "invalid_body", "The request body could not be sent."))?;
    let text = text
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    Ok(text.into_bytes())
}

/// A JSON value with object members kept in document order.
///
/// `encoding/json` matches struct fields case-insensitively and lets a later
/// duplicate overwrite an earlier one. `serde_json::Map` loses both order and
/// duplicate case variants, so the error envelope and the code probe decode
/// into this ordered tree instead. Scalars and arrays only need their kind, so
/// they collapse to `Other`.
enum OrderedValue {
    Null,
    String(String),
    Object(Vec<(String, OrderedValue)>),
    Other,
}

impl<'de> serde::Deserialize<'de> for OrderedValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ValueVisitor;

        impl<'de> Visitor<'de> for ValueVisitor {
            type Value = OrderedValue;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON value")
            }

            fn visit_bool<E: serde::de::Error>(self, _value: bool) -> Result<Self::Value, E> {
                Ok(OrderedValue::Other)
            }

            fn visit_i64<E: serde::de::Error>(self, _value: i64) -> Result<Self::Value, E> {
                Ok(OrderedValue::Other)
            }

            fn visit_u64<E: serde::de::Error>(self, _value: u64) -> Result<Self::Value, E> {
                Ok(OrderedValue::Other)
            }

            fn visit_f64<E: serde::de::Error>(self, _value: f64) -> Result<Self::Value, E> {
                Ok(OrderedValue::Other)
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(OrderedValue::String(value.to_owned()))
            }

            fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(OrderedValue::String(value))
            }

            fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(OrderedValue::Null)
            }

            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(OrderedValue::Null)
            }

            fn visit_some<D: Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<Self::Value, D::Error> {
                deserializer.deserialize_any(ValueVisitor)
            }

            fn visit_newtype_struct<D: Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<Self::Value, D::Error> {
                deserializer.deserialize_any(ValueVisitor)
            }

            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                while sequence.next_element::<IgnoredAny>()?.is_some() {}
                Ok(OrderedValue::Other)
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut members = Vec::new();
                while let Some((key, value)) = map.next_entry::<String, OrderedValue>()? {
                    members.push((key, value));
                }
                Ok(OrderedValue::Object(members))
            }
        }

        deserializer.deserialize_any(ValueVisitor)
    }
}

/// The members of a top-level JSON object, or an empty list for JSON `null`
/// (which Go decodes into a zero struct). Anything else is not an envelope.
fn ordered_members(data: &[u8]) -> Option<Vec<(String, OrderedValue)>> {
    match serde_json::from_slice::<OrderedValue>(data).ok()? {
        OrderedValue::Object(members) => Some(members),
        OrderedValue::Null => Some(Vec::new()),
        _ => None,
    }
}

#[derive(Default)]
struct ErrorParts {
    code: Option<String>,
    message: Option<String>,
    fields: Option<BTreeMap<String, String>>,
}

fn ordered_error_parts(body: &[u8]) -> Option<(bool, ErrorParts)> {
    let members = ordered_members(body)?;
    let mut parts = ErrorParts::default();
    let mut present = false;
    for (key, value) in members {
        if !key.eq_ignore_ascii_case("error") {
            continue;
        }
        let error_members = match value {
            OrderedValue::Null => {
                parts = ErrorParts::default();
                present = false;
                continue;
            }
            OrderedValue::Object(error_members) => error_members,
            _ => return None,
        };
        present = true;
        for (key, value) in error_members {
            if key.eq_ignore_ascii_case("code") {
                match value {
                    OrderedValue::String(value) => parts.code = Some(value),
                    OrderedValue::Null => {}
                    _ => return None,
                }
            } else if key.eq_ignore_ascii_case("message") {
                match value {
                    OrderedValue::String(value) => parts.message = Some(value),
                    OrderedValue::Null => {}
                    _ => return None,
                }
            } else if key.eq_ignore_ascii_case("fields") {
                match value {
                    OrderedValue::Null => parts.fields = None,
                    OrderedValue::Object(entries) => {
                        let target = parts.fields.get_or_insert_with(BTreeMap::new);
                        for (key, value) in entries {
                            match value {
                                OrderedValue::String(value) => {
                                    target.insert(key, value);
                                }
                                OrderedValue::Null => {
                                    target.insert(key, String::new());
                                }
                                _ => return None,
                            }
                        }
                    }
                    _ => return None,
                }
            }
        }
    }
    Some((present, parts))
}

pub fn error_from_result(result: &HttpResult) -> ApiError {
    let Some((present, parts)) = ordered_error_parts(&result.body) else {
        return default_api_error(result.status);
    };
    if !present {
        return default_api_error(result.status);
    }
    let code = parts
        .code
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "request_failed".to_owned());
    let message = parts
        .message
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "The API returned an error. Try again.".to_owned());
    ApiError {
        status: result.status,
        code,
        message,
        fields: parts.fields.unwrap_or_default(),
    }
}

fn default_api_error(status: i32) -> ApiError {
    ApiError {
        status,
        code: "request_failed".to_owned(),
        message: "The API returned an error. Try again.".to_owned(),
        fields: BTreeMap::new(),
    }
}

pub fn error_code(body: &[u8]) -> String {
    let Some(members) = ordered_members(body) else {
        return String::new();
    };
    let mut present = false;
    let mut code = String::new();
    for (key, value) in members {
        if !key.eq_ignore_ascii_case("error") {
            continue;
        }
        let error_members = match value {
            OrderedValue::Null => {
                present = false;
                code.clear();
                continue;
            }
            OrderedValue::Object(error_members) => error_members,
            _ => return String::new(),
        };
        present = true;
        for (key, value) in error_members {
            if !key.eq_ignore_ascii_case("code") {
                continue;
            }
            match value {
                OrderedValue::String(value) => code = value,
                OrderedValue::Null => {}
                _ => return String::new(),
            }
        }
    }
    if present { code } else { String::new() }
}

pub fn decode_json(body: &[u8]) -> Option<Value> {
    if body.is_empty() {
        return None;
    }
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Null) | Err(_) => None,
        Ok(value) => Some(value),
    }
}

pub(crate) fn response_too_large() -> ApiError {
    ApiError::new(0, "response_too_large", "The API response was too large.")
}

pub(crate) fn settings_changed_error() -> ApiError {
    ApiError::new(
        0,
        "settings_changed",
        "The server settings changed. Try again.",
    )
}

pub(crate) fn network_failure(timeout: bool) -> ApiError {
    let message = if timeout {
        "The API did not answer in time. Check the service and try again."
    } else {
        "The API did not answer. Check the address and that the service is running."
    };
    ApiError::new(0, "network_error", message)
}

/// Backwards-compatible alias for `isTimeout`.
pub fn is_timeout(error: &TransportError) -> bool {
    error.is_timeout()
}
