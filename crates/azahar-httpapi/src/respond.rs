//! JSON responses and the shared problem envelope.
//!
//! Go's `encoding/json` escapes HTML by default and terminates every response
//! with a newline. Both are observable on the wire, so the same bytes are
//! produced here rather than relying on `axum::Json`.

use std::collections::HashMap;

use axum::body::Body;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiProblem {
    pub status: StatusCode,
    pub code: String,
    pub message: String,
    pub fields: Option<HashMap<String, String>>,
}

impl ApiProblem {
    pub fn new(status: u16, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            code: code.into(),
            message: message.into(),
            fields: None,
        }
    }

    pub fn with_fields(
        status: u16,
        code: impl Into<String>,
        message: impl Into<String>,
        fields: HashMap<String, String>,
    ) -> Self {
        Self {
            status: StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            code: code.into(),
            message: message.into(),
            fields: Some(fields),
        }
    }
}

impl std::fmt::Display for ApiProblem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ApiProblem {}

pub fn problem(status: u16, code: &str, message: &str) -> ApiProblem {
    ApiProblem::new(status, code, message)
}

pub fn problem_fields(
    status: u16,
    code: &str,
    message: &str,
    fields: HashMap<String, String>,
) -> ApiProblem {
    ApiProblem::with_fields(status, code, message, fields)
}

#[derive(Serialize)]
struct ErrorEnvelope<'a> {
    error: ErrorPayload<'a>,
}

#[derive(Serialize)]
struct ErrorPayload<'a> {
    code: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    fields: Option<&'a HashMap<String, String>>,
}

impl IntoResponse for ApiProblem {
    fn into_response(self) -> Response {
        let fields = self.fields.as_ref().filter(|fields| !fields.is_empty());
        let payload = ErrorEnvelope {
            error: ErrorPayload {
                code: &self.code,
                message: &self.message,
                fields,
            },
        };
        json_response(self.status.as_u16(), &payload).into_response()
    }
}

/// Serialise with Go `encoding/json` escaping and a trailing newline.
pub fn encode_json<T: Serialize>(payload: &T) -> String {
    String::from_utf8(encode_json_vec(payload)).unwrap_or_else(|_| "null\n".to_string())
}

/// The byte form used by [`json_response`].
///
/// serde emits UTF-8, so the HTML and line-separator characters can be
/// replaced at the byte level. The common response has none of them: it is
/// serialised once and gets its newline, without a second full-size allocation
/// or a character-by-character pass.
pub fn encode_json_vec<T: Serialize>(payload: &T) -> Vec<u8> {
    let mut raw = serde_json::to_vec(payload).unwrap_or_else(|_| b"null".to_vec());
    let needs_escape = raw.iter().any(|byte| matches!(byte, b'<' | b'>' | b'&'))
        || raw
            .windows(3)
            .any(|window| matches!(window, [0xE2, 0x80, 0xA8 | 0xA9]));
    if !needs_escape {
        raw.push(b'\n');
        return raw;
    }

    let mut out = Vec::with_capacity(raw.len() + 8);
    let mut index = 0;
    while index < raw.len() {
        match raw[index] {
            b'<' => out.extend_from_slice(b"\\u003c"),
            b'>' => out.extend_from_slice(b"\\u003e"),
            b'&' => out.extend_from_slice(b"\\u0026"),
            0xE2 if raw.get(index + 1) == Some(&0x80)
                && matches!(raw.get(index + 2), Some(0xA8 | 0xA9)) =>
            {
                if raw[index + 2] == 0xA8 {
                    out.extend_from_slice(b"\\u2028");
                } else {
                    out.extend_from_slice(b"\\u2029");
                }
                index += 3;
                continue;
            }
            byte => out.push(byte),
        }
        index += 1;
    }
    out.push(b'\n');
    out
}

pub fn json_response<T: Serialize>(status: u16, payload: &T) -> Response {
    let body = encode_json_vec(payload);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        Body::from(body),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_envelope_omits_empty_fields_like_go() {
        let problem = problem(404, "not_found", "Not found.");
        assert_eq!(
            encode_json(&serde_json::json!({
                "error": {"code": problem.code, "message": problem.message}
            })),
            "{\"error\":{\"code\":\"not_found\",\"message\":\"Not found.\"}}\n"
        );
    }

    #[test]
    fn html_characters_are_escaped() {
        let payload = serde_json::json!({"message": "<b>a & b</b>"});
        assert_eq!(
            encode_json(&payload),
            "{\"message\":\"\\u003cb\\u003ea \\u0026 b\\u003c/b\\u003e\"}\n"
        );
        assert_eq!(
            encode_json_vec(&payload),
            b"{\"message\":\"\\u003cb\\u003ea \\u0026 b\\u003c/b\\u003e\"}\n"
        );
    }

    #[test]
    fn line_separators_are_escaped_and_other_unicode_is_untouched() {
        let payload = serde_json::json!({"text": "a\u{2028}b\u{2029}c \u{00e9}\u{1f600}"});
        assert_eq!(
            encode_json(&payload),
            "{\"text\":\"a\\u2028b\\u2029c \u{00e9}\u{1f600}\"}\n"
        );
    }

    #[tokio::test]
    async fn problem_fields_survive_serialisation() {
        let fields = HashMap::from([("email".to_string(), "Enter a valid email.".to_string())]);
        let problem = problem_fields(422, "validation_error", "Check the marked fields.", fields);
        let response = problem.into_response();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert_eq!(
            String::from_utf8(body.to_vec()).expect("utf8"),
            "{\"error\":{\"code\":\"validation_error\",\"message\":\"Check the marked fields.\",\"fields\":{\"email\":\"Enter a valid email.\"}}}\n"
        );
    }
}
