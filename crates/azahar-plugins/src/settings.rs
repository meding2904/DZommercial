//! Encrypted plugin settings and the form merge/mask helpers.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use azahar_secrets::SecretBox;
use azahar_store::{Store, StoreError, Timestamp};
use serde::Deserializer;
use serde::de::{MapAccess, Visitor};

use crate::Manifest;

/// The single message the Go layer returns for every undecryptable value.
pub const UNREADABLE_MESSAGE: &str =
    "plugin settings cannot be decrypted with the current PLUGIN_SECRET_KEY";

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("{UNREADABLE_MESSAGE}")]
    Unreadable,
    #[error("encode plugin settings: {0}")]
    Encode(String),
    #[error("encrypt plugin settings: {0}")]
    Encrypt(String),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Runtime services a plugin handler may use.
///
/// The Go `Env` also carries a logger, database handle, actor and request-id
/// callbacks. Those belong to the HTTP layer; the port keeps only what the
/// settings and AI-facing plugins actually read.
pub struct Env {
    pub store: Arc<Store>,
    pub secret_key: String,
    pub ai_base_url: String,
    pub ai_timeout: Duration,
    pub is_development: bool,
}

impl Env {
    #[must_use]
    pub fn new(store: Arc<Store>, secret_key: impl Into<String>) -> Self {
        Self {
            store,
            secret_key: secret_key.into(),
            ai_base_url: String::new(),
            ai_timeout: Duration::ZERO,
            is_development: false,
        }
    }

    /// Stored settings, or an empty map when the plugin never saved any.
    pub async fn settings(
        &self,
        plugin_id: &str,
    ) -> Result<HashMap<String, String>, SettingsError> {
        self.settings_with_time(plugin_id)
            .await
            .map(|(values, _)| values)
    }

    /// Stored settings with the row timestamp. A missing row is the Go zero
    /// time, represented here by `None`.
    pub async fn settings_with_time(
        &self,
        plugin_id: &str,
    ) -> Result<(HashMap<String, String>, Timestamp), SettingsError> {
        let (raw, updated_at) = match self.store.plugin_settings(plugin_id).await {
            Ok(value) => value,
            Err(StoreError::NotFound) => return Ok((HashMap::new(), None)),
            Err(error) => return Err(error.into()),
        };
        let plain = SecretBox::new(&self.secret_key)
            .decrypt(&raw)
            .map_err(|_| SettingsError::Unreadable)?;
        // Go's decoder turns a null value into "", accepts null at the top
        // level as an empty map, and fails on every other non-string value.
        // Deserialising entry by entry also keeps that failure when a bad
        // duplicate key is overwritten later.
        let mut deserializer = serde_json::Deserializer::from_slice(&plain);
        let values = deserializer
            .deserialize_any(SettingsMap)
            .map_err(|_| SettingsError::Unreadable)?;
        deserializer.end().map_err(|_| SettingsError::Unreadable)?;
        Ok((values, updated_at))
    }

    /// Encrypts the settings map for storage in `plugin_settings.settings`.
    pub fn encode_settings(
        &self,
        values: &HashMap<String, String>,
    ) -> Result<String, SettingsError> {
        let plain =
            serde_json::to_vec(values).map_err(|error| SettingsError::Encode(error.to_string()))?;
        SecretBox::new(&self.secret_key)
            .encrypt(&plain)
            .map_err(|error| SettingsError::Encrypt(error.to_string()))
    }
}

/// Deserialises the plaintext settings blob exactly like
/// `json.Unmarshal(plain, &map[string]string{})`.
struct SettingsMap;

impl<'de> Visitor<'de> for SettingsMap {
    type Value = HashMap<String, String>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON object of plugin settings")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(HashMap::new())
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = HashMap::with_capacity(map.size_hint().unwrap_or(0));
        while let Some((key, value)) = map.next_entry::<String, Option<String>>()? {
            values.insert(key, value.unwrap_or_default());
        }
        Ok(values)
    }
}

/// Applies a submitted settings form to the stored values.
///
/// A blank secret keeps the stored secret, so the browser never needs the old
/// key back. Unknown incoming keys and schema violations land in the field
/// error map with the exact Go wording.
#[must_use]
pub fn merge_settings(
    manifest: &Manifest,
    existing: &HashMap<String, String>,
    incoming: &HashMap<String, String>,
) -> (HashMap<String, String>, HashMap<String, String>) {
    let mut merged = existing.clone();
    let mut fields = HashMap::new();
    let mut known = HashSet::with_capacity(manifest.settings.len());

    for field in &manifest.settings {
        known.insert(field.key.as_str());
        let (value, mut present) = match incoming.get(&field.key) {
            Some(value) => (value.trim().to_string(), true),
            None => (String::new(), false),
        };
        if field.field_type == "secret" && present && value.is_empty() {
            present = false;
        }
        if !present {
            let stored_empty = merged
                .get(&field.key)
                .is_none_or(|stored| stored.trim().is_empty());
            if field.required && stored_empty {
                fields.insert(field.key.clone(), "This setting is required.".to_string());
            }
            continue;
        }

        match field.field_type.as_str() {
            "text" | "secret" => {
                let max = if field.max_length <= 0 {
                    if field.field_type == "secret" {
                        512
                    } else {
                        300
                    }
                } else {
                    field.max_length
                };
                if i32::try_from(value.chars().count()).unwrap_or(i32::MAX) > max {
                    fields.insert(
                        field.key.clone(),
                        format!("Keep this under {} characters.", max + 1),
                    );
                    continue;
                }
            }
            "number" => {
                if !is_go_number(&value) {
                    fields.insert(field.key.clone(), "Enter a number.".to_string());
                    continue;
                }
            }
            "boolean" => {
                if value != "true" && value != "false" {
                    fields.insert(field.key.clone(), "Choose true or false.".to_string());
                    continue;
                }
            }
            "select" => {
                let valid = field.options.iter().any(|option| option.value == value);
                if !valid {
                    fields.insert(
                        field.key.clone(),
                        "Choose one of the listed options.".to_string(),
                    );
                    continue;
                }
            }
            _ => {}
        }
        merged.insert(field.key.clone(), value);
    }

    for key in incoming.keys() {
        if !known.contains(key.as_str()) {
            fields.insert(key.clone(), "This plugin has no such setting.".to_string());
        }
    }
    (merged, fields)
}

/// Removes secret values from the response and reports which ones hold a
/// value. The secret itself never leaves the server.
#[must_use]
pub fn mask_settings(
    manifest: &Manifest,
    values: &HashMap<String, String>,
) -> (HashMap<String, String>, Vec<String>) {
    let mut masked = HashMap::with_capacity(manifest.settings.len());
    let mut secrets_set = Vec::new();
    for field in &manifest.settings {
        let stored = values.get(&field.key).map(String::as_str).unwrap_or("");
        let value = if stored.is_empty() {
            field.default.clone()
        } else {
            stored.to_string()
        };
        if field.field_type == "secret" {
            if !stored.is_empty() {
                secrets_set.push(field.key.clone());
            }
            masked.insert(field.key.clone(), String::new());
            continue;
        }
        masked.insert(field.key.clone(), value);
    }
    (masked, secrets_set)
}

/// True when every required setting holds a non-blank value. Defaults do not
/// count as stored values, matching the Go helper.
#[must_use]
pub fn settings_complete(manifest: &Manifest, values: &HashMap<String, String>) -> bool {
    manifest.settings.iter().all(|field| {
        !field.required
            || values
                .get(&field.key)
                .is_some_and(|value| !value.trim().is_empty())
    })
}

/// Mirrors `strconv.ParseFloat` acceptance for a 64-bit float: the string is
/// accepted exactly when Go returns no error (syntax valid and no overflow).
fn is_go_number(value: &str) -> bool {
    if is_go_special(value) {
        return true;
    }
    match read_go_float(value) {
        Some(GoFloat::Decimal(text)) => text.parse::<f64>().is_ok_and(f64::is_finite),
        Some(GoFloat::Hex {
            mantissa,
            exponent,
            truncated,
        }) => !hex_float_overflows(mantissa, exponent, truncated),
        None => false,
    }
}

/// Go's `special`: `NaN` takes no sign, `Inf` and `Infinity` do.
fn is_go_special(value: &str) -> bool {
    if value.eq_ignore_ascii_case("nan") {
        return true;
    }
    let rest = value
        .strip_prefix('+')
        .or_else(|| value.strip_prefix('-'))
        .unwrap_or(value);
    rest.eq_ignore_ascii_case("inf") || rest.eq_ignore_ascii_case("infinity")
}

enum GoFloat {
    Decimal(String),
    Hex {
        mantissa: u64,
        exponent: i64,
        truncated: bool,
    },
}

/// Port of `strconv.readFloat`: parses the mantissa and exponent exactly as the
/// Go scanner does, including its underscore handling and truncation flags.
fn read_go_float(value: &str) -> Option<GoFloat> {
    let bytes = value.as_bytes();
    let mut index = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));

    let mut hex = false;
    let mut base = 10_u64;
    let mut exponent_character = b'e';
    if index + 2 < bytes.len()
        && bytes[index] == b'0'
        && bytes[index + 1].eq_ignore_ascii_case(&b'x')
    {
        hex = true;
        base = 16;
        exponent_character = b'p';
        index += 2;
    }

    let max_mantissa_digits = if hex { 16 } else { 19 };
    let mut saw_dot = false;
    let mut saw_digits = false;
    let mut underscores = false;
    let mut digits = 0_i64;
    let mut mantissa_digits = 0_i64;
    let mut decimal_point = 0_i64;
    let mut mantissa = 0_u64;
    let mut truncated = false;

    while index < bytes.len() {
        let character = bytes[index];
        match character {
            b'_' => {
                underscores = true;
                index += 1;
            }
            b'.' => {
                if saw_dot {
                    break;
                }
                saw_dot = true;
                decimal_point = digits;
                index += 1;
            }
            b'0'..=b'9' => {
                saw_digits = true;
                if character == b'0' && digits == 0 {
                    decimal_point -= 1;
                    index += 1;
                    continue;
                }
                digits += 1;
                if mantissa_digits < max_mantissa_digits {
                    mantissa = mantissa * base + u64::from(character - b'0');
                    mantissa_digits += 1;
                } else if character != b'0' {
                    truncated = true;
                }
                index += 1;
            }
            _ if hex && matches!(character.to_ascii_lowercase(), b'a'..=b'f') => {
                saw_digits = true;
                digits += 1;
                let digit = u64::from(character.to_ascii_lowercase() - b'a' + 10);
                if mantissa_digits < max_mantissa_digits {
                    mantissa = mantissa * 16 + digit;
                    mantissa_digits += 1;
                } else {
                    truncated = true;
                }
                index += 1;
            }
            _ => break,
        }
    }

    if !saw_digits {
        return None;
    }
    if !saw_dot {
        decimal_point = digits;
    }
    if hex {
        decimal_point *= 4;
        mantissa_digits *= 4;
    }

    if index < bytes.len() && bytes[index].to_ascii_lowercase() == exponent_character {
        index += 1;
        if index >= bytes.len() {
            return None;
        }
        let mut sign = 1_i64;
        match bytes[index] {
            b'+' => index += 1,
            b'-' => {
                index += 1;
                sign = -1;
            }
            _ => {}
        }
        if index >= bytes.len() || !bytes[index].is_ascii_digit() {
            return None;
        }
        let mut exponent = 0_i64;
        while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'_') {
            if bytes[index] == b'_' {
                underscores = true;
                index += 1;
                continue;
            }
            if exponent < 10_000 {
                exponent = exponent * 10 + i64::from(bytes[index] - b'0');
            }
            index += 1;
        }
        decimal_point += exponent * sign;
    } else if hex {
        return None;
    }

    if index != bytes.len() {
        return None;
    }
    if underscores && !underscore_ok(&value[..index]) {
        return None;
    }

    if hex {
        let exponent = if mantissa == 0 {
            0
        } else {
            decimal_point - mantissa_digits
        };
        Some(GoFloat::Hex {
            mantissa,
            exponent,
            truncated,
        })
    } else {
        let cleaned = value
            .chars()
            .filter(|character| *character != '_')
            .collect();
        Some(GoFloat::Decimal(cleaned))
    }
}

/// Port of `strconv.underscoreOK`: an underscore must sit between digits, or
/// between a base prefix and a digit.
fn underscore_ok(value: &str) -> bool {
    let rest = value
        .strip_prefix('+')
        .or_else(|| value.strip_prefix('-'))
        .unwrap_or(value);
    let bytes = rest.as_bytes();
    let mut index = 0;
    let mut saw_digit = false;
    let mut saw_underscore = false;
    let mut hex = false;
    if bytes.len() >= 2
        && bytes[0] == b'0'
        && matches!(bytes[1].to_ascii_lowercase(), b'b' | b'o' | b'x')
    {
        index = 2;
        saw_digit = true;
        hex = bytes[1].eq_ignore_ascii_case(&b'x');
    }
    while index < bytes.len() {
        let character = bytes[index];
        let digit = character.is_ascii_digit()
            || (hex && matches!(character.to_ascii_lowercase(), b'a'..=b'f'));
        if digit {
            saw_digit = true;
            saw_underscore = false;
        } else if character == b'_' {
            if !saw_digit {
                return false;
            }
            saw_digit = false;
            saw_underscore = true;
        } else {
            if saw_underscore {
                return false;
            }
            saw_digit = false;
            saw_underscore = false;
        }
        index += 1;
    }
    !saw_underscore
}

/// Port of `strconv.atofHex`'s range decision. `ErrRange` is reported exactly
/// when the rounded exponent exceeds the float64 maximum.
fn hex_float_overflows(mut mantissa: u64, mut exponent: i64, truncated: bool) -> bool {
    const MANTISSA_BITS: u32 = 52;
    // Go's float64info stores the bias as -1023.
    const BIAS: i64 = -1023;
    const MAX_EXPONENT: i64 = (1 << 11) + BIAS - 2;
    const MIN_EXPONENT: i64 = BIAS + 1;

    exponent += i64::from(MANTISSA_BITS);
    while mantissa != 0 && mantissa >> (MANTISSA_BITS + 2) == 0 {
        mantissa <<= 1;
        exponent -= 1;
    }
    if truncated {
        mantissa |= 1;
    }
    while mantissa >> (1 + MANTISSA_BITS + 2) != 0 {
        mantissa = (mantissa >> 1) | (mantissa & 1);
        exponent += 1;
    }
    while mantissa > 1 && exponent < MIN_EXPONENT - 2 {
        mantissa = (mantissa >> 1) | (mantissa & 1);
        exponent += 1;
    }

    let mut round = mantissa & 3;
    mantissa >>= 2;
    round |= mantissa & 1;
    exponent += 2;
    if round == 3 {
        mantissa += 1;
        if mantissa == 1 << (1 + MANTISSA_BITS) {
            mantissa >>= 1;
            exponent += 1;
        }
    }
    if mantissa >> MANTISSA_BITS == 0 {
        exponent = BIAS;
    }
    exponent > MAX_EXPONENT
}
