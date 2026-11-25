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
    let raw = serde_json::to_string(payload).unwrap_or_else(|_| "null".to_string());
    let mut out = String::with_capacity(raw.len() + 1);
    for character in raw.chars() {
        match character {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            other => out.push(other),
        }
    }
    out.push('\n');
    out
}

pub fn json_response<T: Serialize>(status: u16, payload: &T) -> Response {
    let body = encode_json(payload);
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
