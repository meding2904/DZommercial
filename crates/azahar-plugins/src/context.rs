//! HTTP-facing context helpers the Go plugin SDK exposes on `*plugins.Context`.
//!
//! The Rust server renders responses in `azahar-httpapi`; this module keeps the
//! SDK-level behaviour (JSON escaping, error envelopes, plugin-scoped settings
//! and body decoding) testable without depending on axum. That dependency
//! direction is what lets plugin crates consume the SDK without a cycle.

use std::collections::HashMap;
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::{Env, Problem, SettingsError};

/// The subset of a request context a plugin receives from the host.
#[derive(Clone)]
pub struct Context {
    pub env: Option<Arc<Env>>,
    pub plugin_id: String,
    pub user_id: String,
    pub user_role: String,
}

impl Context {
    pub fn is_admin(&self) -> bool {
        self.user_role == "admin"
    }

    /// Go `Context.JSON`: status, content type and the shared JSON encoding.
    pub fn json<T: Serialize>(&self, status: u16, payload: &T) -> (u16, &'static str, String) {
        (
            status,
            "application/json; charset=utf-8",
            write_json(Some(payload)),
        )
    }

    pub fn error(&self, status: u16, code: &str, message: &str) -> Problem {
        Problem::new(status, code, message, None)
    }

    pub fn problem(
        &self,
        status: u16,
        code: &str,
        message: &str,
        fields: Option<HashMap<String, String>>,
    ) -> Problem {
        Problem::new(status, code, message, fields)
    }

    /// Plugin-scoped settings, decrypted with the host key.
    pub async fn settings(&self) -> Result<HashMap<String, String>, SettingsError> {
        match self.env.as_ref() {
            Some(env) => env.settings(&self.plugin_id).await,
            None => Err(SettingsError::Unreadable),
        }
    }
}

/// Go `writeJSON`: `null`/`None` writes no body; otherwise Go's html-escaped
/// JSON plus the encoder's trailing newline.
pub fn write_json<T: Serialize>(payload: Option<&T>) -> String {
    let Some(payload) = payload else {
        return String::new();
    };
    let raw = serde_json::to_string(payload).unwrap_or_else(|_| "null".to_string());
    format!("{}\n", escape_json(&raw))
}

fn escape_json(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
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
    out
}

/// Go `plugins.DecodeBody`: accepts any `application/json*` content type, caps
/// the body at 1 MiB, and reports the SDK's 422/413/415 problems.
///
/// Unknown fields are rejected only when `T` declares `deny_unknown_fields`;
/// Rust cannot inspect arbitrary struct fields at runtime. Every in-tree caller
/// uses such a payload type.
pub fn decode_body<T: DeserializeOwned + Default>(
    content_type: &str,
    body: &[u8],
    fields: &[&str],
) -> Result<T, Problem> {
    if !content_type
        .trim()
        .to_ascii_lowercase()
        .starts_with("application/json")
    {
        return Err(Problem::new(
            415,
            "unsupported_media_type",
            "Send the request body as application/json.",
            None,
        ));
    }
    if body.len() > 1 << 20 {
        return Err(Problem::new(
            413,
            "payload_too_large",
            "The request body is too large.",
            None,
        ));
    }
    if String::from_utf8_lossy(body).trim().is_empty() {
        return Err(Problem::new(
            422,
            "validation_error",
            "Check the marked fields.",
            Some(HashMap::from([(
                "body".to_string(),
                "Send a JSON object.".to_string(),
            )])),
        ));
    }

    let mut probe = serde_json::Deserializer::from_slice(body);
    if serde_json::Value::deserialize(&mut probe).is_err() {
        return Err(Problem::new(
            422,
            "invalid_json",
            "The request body does not match the expected shape.",
            None,
        ));
    }
    if probe.end().is_err() {
        return Err(Problem::new(
            422,
            "invalid_json",
            "Send one JSON object.",
            None,
        ));
    }

    let pairs = match parse_pairs(body) {
        Ok(pairs) => pairs,
        Err(()) => {
            return Err(Problem::new(
                422,
                "invalid_json",
                "The request body does not match the expected shape.",
                None,
            ));
        }
    };
    let Some(pairs) = pairs else {
        return Ok(T::default());
    };

    // encoding/json matches struct fields case-insensitively and applies
    // duplicate keys in document order, so the last matching spelling wins.
    let mut normalized = serde_json::Map::new();
    for (key, value) in pairs {
        let Some(field) = fields.iter().find(|field| field.eq_ignore_ascii_case(&key)) else {
            return Err(Problem::new(
                422,
                "invalid_json",
                "The request body does not match the expected shape.",
                None,
            ));
        };
        normalized.insert((*field).to_string(), value);
    }
    serde_json::from_value(serde_json::Value::Object(normalized)).map_err(|_| {
        Problem::new(
            422,
            "invalid_json",
            "The request body does not match the expected shape.",
            None,
        )
    })
}

/// Top-level `null` is Go's zero value; any other non-object is a shape error.
fn parse_pairs(body: &[u8]) -> Result<Option<Vec<(String, serde_json::Value)>>, ()> {
    struct Pairs(Vec<(String, serde_json::Value)>);

    impl<'de> Deserialize<'de> for Pairs {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct Visitor;
            impl<'de> serde::de::Visitor<'de> for Visitor {
                type Value = Option<Vec<(String, serde_json::Value)>>;
                fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    formatter.write_str("a JSON object")
                }
                fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                    Ok(None)
                }
                fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                    Ok(None)
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(
                    self,
                    mut map: A,
                ) -> Result<Self::Value, A::Error> {
                    let mut pairs = Vec::new();
                    while let Some(key) = map.next_key::<String>()? {
                        let value = map.next_value::<serde_json::Value>()?;
                        pairs.push((key, value));
                    }
                    Ok(Some(pairs))
                }
            }
            struct Wrapper(Option<Vec<(String, serde_json::Value)>>);
            impl<'de> Deserialize<'de> for Wrapper {
                fn deserialize<D: serde::Deserializer<'de>>(
                    deserializer: D,
                ) -> Result<Self, D::Error> {
                    deserializer.deserialize_any(Visitor).map(Wrapper)
                }
            }
            let Pairs(pairs) = Pairs(Vec::new());
            let _ = pairs;
            let wrapper = Wrapper::deserialize(deserializer)?;
            Ok(Pairs(wrapper.0.unwrap_or_default()))
        }
    }

    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let parsed = Pairs::deserialize(&mut deserializer).map_err(|_| ())?;
    if deserializer.end().is_err() {
        return Err(());
    }
    // A non-object top level (array/number/string) has no pairs; distinguish it
    // from null by re-checking the first non-space byte.
    let trimmed = body.iter().find(|byte| !byte.is_ascii_whitespace());
    if trimmed == Some(&b'n') {
        return Ok(None);
    }
    if trimmed != Some(&b'{') {
        return Err(());
    }
    Ok(Some(parsed.0))
}

/// The SDK's error envelope, matching the HTTP layer's omission of empty
/// fields. Used by `Context` callers that need bytes rather than a writer.
pub fn error_body(problem: &Problem) -> String {
    let code = serde_json::to_string(&problem.code).unwrap_or_else(|_| "\"\"".to_string());
    let message = serde_json::to_string(&problem.message).unwrap_or_else(|_| "\"\"".to_string());
    let mut out = format!("{{\"error\":{{\"code\":{code},\"message\":{message}");
    if let Some(fields) = problem.fields.as_ref().filter(|fields| !fields.is_empty()) {
        let sorted: std::collections::BTreeMap<_, _> = fields.iter().collect();
        let fields = serde_json::to_string(&sorted).unwrap_or_else(|_| "{}".to_string());
        out.push_str(",\"fields\":");
        out.push_str(&fields);
    }
    out.push_str("}}\n");
    escape_json(&out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Default, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct Payload {
        name: String,
    }

    #[test]
    fn clipboard_json_escapes_like_go() {
        assert_eq!(
            write_json(Some(&serde_json::json!({"name": "<a & b>"}))),
            "{\"name\":\"\\u003ca \\u0026 b\\u003e\"}\n"
        );
        let none: Option<&serde_json::Value> = None;
        assert_eq!(write_json(none), "");
    }

    #[test]
    fn body_decoder_matches_go_statuses() {
        let problem = decode_body::<Payload>("text/plain", b"{}", &["name"]).unwrap_err();
        assert_eq!(
            (problem.status, problem.code.as_str()),
            (415, "unsupported_media_type")
        );
        let problem = decode_body::<Payload>("application/json", b"  ", &["name"]).unwrap_err();
        assert_eq!(
            (problem.status, problem.code.as_str()),
            (422, "validation_error")
        );
        assert!(
            problem
                .fields
                .as_ref()
                .is_some_and(|fields| fields.get("body").is_some())
        );
        let problem = decode_body::<Payload>("application/json; charset=utf-8", b"[]", &["name"])
            .unwrap_err();
        assert_eq!(
            (problem.status, problem.code.as_str()),
            (422, "invalid_json")
        );
        let problem =
            decode_body::<Payload>("application/json", br#"{"name":"a","x":1}"#, &["name"])
                .unwrap_err();
        assert_eq!(
            (problem.status, problem.code.as_str()),
            (422, "invalid_json")
        );
        let problem = decode_body::<Payload>("application/json", br#"{"name":"a"} {}"#, &["name"])
            .unwrap_err();
        assert_eq!(problem.message, "Send one JSON object.");
        assert_eq!(
            decode_body::<Payload>("APPLICATION/JSON", br#"{"name":"a"}"#, &["name"]).unwrap(),
            Payload {
                name: "a".to_string()
            }
        );
        assert_eq!(
            decode_body::<Payload>("application/json", b"null", &["name"]).unwrap(),
            Payload::default()
        );
        assert_eq!(
            decode_body::<Payload>("application/json", br#"{"name":"a","name":"b"}"#, &["name"])
                .unwrap(),
            Payload {
                name: "b".to_string()
            }
        );
    }
}
