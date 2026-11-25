//! Image downloads, MIME sniffing and the bounded image cache.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::client::{Client, MAX_IMAGE_BYTES};
use crate::request::RequestOptions;
use crate::types::{ApiError, Response};

const ALLOWED_IMAGE_TYPES: [&str; 5] = [
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/gif",
    "image/avif",
];

impl Client {
    /// Renderer-facing bridge call for an API image.
    pub fn image(&self, path: &str) -> Response {
        match self.image_data(path) {
            Ok(data) => Response::success(Some(serde_json::Value::String(data))),
            Err(error) => Response::failure(error),
        }
    }

    pub(crate) fn image_data(&self, path: &str) -> Result<String, ApiError> {
        if !path.starts_with("/api/images/") {
            return Err(invalid_image());
        }
        if let Some(cached) = self.cached_image(path) {
            return Ok(cached);
        }

        let target = self.target_url(path)?;
        let result = match self.execute(RequestOptions {
            method: "GET".to_owned(),
            target: target.clone(),
            payload: None,
            timeout: self.image_timeout(),
            include_origin: true,
            include_csrf: true,
            max_bytes: MAX_IMAGE_BYTES,
        }) {
            Ok(result) => result,
            Err(error) if error.code == "response_too_large" => {
                return Err(response_too_large_image());
            }
            Err(error) => return Err(error),
        };
        if !(200..=299).contains(&result.status) {
            return Err(ApiError::new(
                result.status,
                "image_failed",
                "The image could not be loaded.",
            ));
        }

        let Some(media_type) = parse_media_type(&result.content_type) else {
            return Err(unsupported_image());
        };
        if !ALLOWED_IMAGE_TYPES.contains(&media_type.as_str()) {
            return Err(unsupported_image());
        }

        let value = format!("data:{media_type};base64,{}", STANDARD.encode(&result.body));
        self.store_image(path.to_owned(), value.clone(), target.generation);
        Ok(value)
    }
}

fn invalid_image() -> ApiError {
    ApiError::new(0, "invalid_image", "That image is not served by the API.")
}

fn response_too_large_image() -> ApiError {
    ApiError::new(0, "image_failed", "The image is too large.")
}

fn unsupported_image() -> ApiError {
    ApiError::new(0, "image_failed", "The API returned an unsupported image.")
}

fn is_token_char(byte: u8) -> bool {
    byte > 0x20
        && byte < 0x7f
        && !matches!(
            byte,
            b'(' | b')'
                | b'<'
                | b'>'
                | b'@'
                | b','
                | b';'
                | b':'
                | b'\\'
                | b'"'
                | b'/'
                | b'['
                | b']'
                | b'?'
                | b'='
                | b'{'
                | b'}'
        )
}

/// The decision-relevant subset of `mime.ParseMediaType`.
fn parse_media_type(value: &str) -> Option<String> {
    let mut segments = split_parameters(value);
    let media = segments.next()?.trim().to_ascii_lowercase();
    if media.is_empty() {
        return None;
    }
    let (kind, subtype) = media.split_once('/')?;
    if kind.is_empty()
        || subtype.is_empty()
        || !kind.bytes().all(is_token_char)
        || !subtype.bytes().all(is_token_char)
    {
        return None;
    }
    for parameter in segments {
        let parameter = parameter.trim();
        if parameter.is_empty() {
            continue;
        }
        let (name, raw_value) = parameter.split_once('=')?;
        if name.trim().is_empty() || !name.trim().bytes().all(is_token_char) {
            return None;
        }
        let raw_value = raw_value.trim();
        if raw_value.starts_with('"') {
            if !raw_value.ends_with('"') || raw_value.len() < 2 {
                return None;
            }
            let inner = &raw_value[1..raw_value.len() - 1];
            let mut escaped = false;
            for byte in inner.bytes() {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' || (byte < 0x20 && byte != b'\t') {
                    return None;
                }
            }
            if escaped {
                return None;
            }
        } else if raw_value.is_empty() || !raw_value.bytes().all(is_token_char) {
            return None;
        }
    }
    Some(media)
}

fn split_parameters(value: &str) -> impl Iterator<Item = &str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'"' if index == 0 || bytes[index - 1] != b'\\' => quoted = !quoted,
            b';' if !quoted => {
                parts.push(&value[start..index]);
                start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    parts.push(&value[start..]);
    parts.into_iter()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Client, ClientOptions};

    #[test]
    fn stale_generation_is_not_cached() {
        let temp = tempfile::tempdir().expect("profile");
        let client = Client::with_options(ClientOptions::new(
            "2.0.0-test",
            temp.path().join("profile"),
        ))
        .expect("client");
        let generation = client.lock_state().generation;
        client.clear_session().expect("clear session");
        client.store_image(
            "/api/images/stale".to_owned(),
            "data:image/png;base64,AAAA".to_owned(),
            generation,
        );
        assert!(client.cached_image("/api/images/stale").is_none());

        let current = client.lock_state().generation;
        client.store_image(
            "/api/images/fresh".to_owned(),
            "data:image/png;base64,AAAA".to_owned(),
            current,
        );
        assert!(client.cached_image("/api/images/fresh").is_some());
    }

    #[test]
    fn media_type_parser_matches_go_cases() {
        assert_eq!(parse_media_type("image/png"), Some("image/png".to_owned()));
        assert_eq!(
            parse_media_type("IMAGE/PNG; charset=binary"),
            Some("image/png".to_owned())
        );
        assert_eq!(
            parse_media_type("image/png; note=\"a;b\""),
            Some("image/png".to_owned())
        );
        assert_eq!(parse_media_type("text/html"), Some("text/html".to_owned()));
        assert_eq!(parse_media_type("image/png;"), Some("image/png".to_owned()));
        assert_eq!(parse_media_type(""), None);
        assert_eq!(parse_media_type("image"), None);
        assert_eq!(parse_media_type("image/"), None);
        assert_eq!(parse_media_type("image/png; bare"), None);
        assert_eq!(parse_media_type("image/png; x=\"a\"b\""), None);
        assert_eq!(parse_media_type("image/png extra"), None);
    }
}
