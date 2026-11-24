//! Health probing and request-origin resolution.

use serde_json::Value;

use crate::client::{Client, MAX_RESPONSE_BYTES};
use crate::request::{RequestOptions, settings_changed_error};
use crate::types::{ApiError, Health};
use crate::urls::{FALLBACK_ORIGIN, api_host, is_loopback_host, normalize_origin};

impl Client {
    pub(crate) fn ensure_origin(&self) -> Result<(), ApiError> {
        let (has_origin, generation, api_url) = {
            let state = self.lock_state();
            (
                !state.settings.origin.is_empty() || !state.resolved_origin.is_empty(),
                state.generation,
                state.settings.api_url.clone(),
            )
        };
        if has_origin {
            return Ok(());
        }
        let host = api_host(&api_url)?;

        self.health();

        let mut state = self.lock_state();
        if state.generation != generation {
            return Err(settings_changed_error());
        }
        if !state.settings.origin.is_empty() || !state.resolved_origin.is_empty() {
            return Ok(());
        }
        if is_loopback_host(&host) {
            state.resolved_origin = FALLBACK_ORIGIN.to_owned();
            return Ok(());
        }
        Err(ApiError::new(
            0,
            "origin_unavailable",
            "Enter the request origin in Connection before connecting to this server.",
        ))
    }

    pub fn health(&self) -> Health {
        let target = match self.target_url("/api/health") {
            Ok(target) => target,
            Err(error) => {
                return Health {
                    ok: false,
                    status: 0,
                    message: error.message,
                    ..Health::default()
                };
            }
        };

        let result = match self.execute(RequestOptions {
            method: "GET".to_owned(),
            target: target.clone(),
            payload: None,
            timeout: self.request_timeout(),
            include_origin: false,
            include_csrf: false,
            max_bytes: MAX_RESPONSE_BYTES,
        }) {
            Ok(result) => result,
            Err(error) => {
                return Health {
                    ok: false,
                    status: 0,
                    message: error.message,
                    ..Health::default()
                };
            }
        };
        if !(200..=299).contains(&result.status) {
            return Health {
                ok: false,
                status: result.status,
                message: "The API answered, but not with a healthy status.".to_owned(),
                ..Health::default()
            };
        }

        let body = parse_health_body(&result.body);
        if let Ok(origin) = normalize_origin(&body.origin)
            && !origin.is_empty()
        {
            let mut state = self.lock_state();
            if state.generation == target.generation {
                state.resolved_origin = origin;
            }
        }

        let state = self.lock_state();
        let origin = if state.settings.origin.is_empty() {
            state.resolved_origin.clone()
        } else {
            state.settings.origin.clone()
        };
        Health {
            ok: true,
            status: result.status,
            env: body.env,
            origin,
            server_time: body.server_time,
            message: String::new(),
        }
    }
}

#[derive(Default)]
struct HealthBody {
    origin: String,
    env: String,
    server_time: String,
}

/// Lenient decoding: a wrong-typed field is skipped, later fields still apply.
fn parse_health_body(body: &[u8]) -> HealthBody {
    let mut parsed = HealthBody::default();
    let Ok(Value::Object(fields)) = serde_json::from_slice::<Value>(body) else {
        return parsed;
    };
    if let Some(Value::String(value)) = fields.get("origin") {
        parsed.origin = value.clone();
    }
    if let Some(Value::String(value)) = fields.get("env") {
        parsed.env = value.clone();
    }
    if let Some(Value::String(value)) = fields.get("server_time") {
        parsed.server_time = value.clone();
    }
    parsed
}
