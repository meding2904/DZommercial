//! Shared store models and the JSON/time encoding helpers used by every table.
//!
//! Times are `Option`: Go's `time.Time` zero value means "not set" throughout
//! the store, and None preserves that without inventing a sentinel instant.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Duration, SecondsFormat, TimeZone, Utc};
use serde::{Deserialize, Deserializer, Serialize};

pub type Timestamp = Option<DateTime<Utc>>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct User {
    pub id: String,
    pub name: String,
    pub email: String,
    pub password_hash: String,
    pub role: String,
    pub theme_preset: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub user_id: String,
    pub token_hash: String,
    pub user_agent: String,
    pub ip: String,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Category {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub sort_order: i32,
    pub plugin_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Product {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub subtitle: String,
    pub description: String,
    pub price_cents: i64,
    pub compare_at_cents: Option<i64>,
    pub sale_price_cents: Option<i64>,
    pub sale_starts_at: Timestamp,
    pub sale_ends_at: Timestamp,
    pub currency: String,
    pub size: String,
    pub category: Category,
    pub concerns: Vec<String>,
    pub ingredients: Vec<String>,
    pub how_to: String,
    pub image: String,
    pub gallery: Vec<String>,
    pub stock: i32,
    pub featured: bool,
    pub active: bool,
    pub plugin_id: String,
    pub attributes: HashMap<String, Vec<String>>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrderItem {
    pub id: String,
    pub order_id: String,
    pub product_id: String,
    pub slug: String,
    pub name: String,
    pub image: String,
    pub unit_price_cents: i64,
    pub quantity: i32,
    pub line_total_cents: i64,
    pub options: HashMap<String, String>,
    pub options_label: String,
    pub plugin_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CourierEvent {
    #[serde(default, deserialize_with = "nullable_string")]
    pub status: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub raw_status: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub timestamp: String,
}

/// Go's `encoding/json` leaves a `null` field at its zero value instead of
/// failing the document.
fn nullable_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Order {
    pub id: String,
    pub number: String,
    pub user_id: Option<String>,
    pub name: String,
    pub email: String,
    pub phone: String,
    pub address: String,
    pub city: String,
    pub notes: String,
    pub subtotal_cents: i64,
    pub shipping_cents: i64,
    pub total_cents: i64,
    pub currency: String,
    pub status: String,
    pub token_hash: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,

    pub wilaya_code: i32,
    pub wilaya_name: String,
    pub commune_name: String,
    pub delivery_type: String,
    pub stop_desk_id: String,
    pub stop_desk_name: String,
    pub courier_key: String,
    pub tracking_number: String,
    pub courier_status: String,
    pub courier_events: Vec<CourierEvent>,
    pub courier_synced_at: Timestamp,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CustomerRow {
    pub id: String,
    pub name: String,
    pub email: String,
    pub orders_count: i32,
    pub total_spent_cents: i64,
    pub last_order_at: Timestamp,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditEntry {
    pub id: String,
    pub actor_user_id: String,
    pub action: String,
    pub entity_type: String,
    pub entity_id: String,
    pub detail: String,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProductImage {
    pub id: String,
    pub content_type: String,
    pub data: Vec<u8>,
    pub created_at: Timestamp,
}

/// Go `time.Time` zero value as `FormatTime` renders it.
pub const ZERO_TIME: &str = "0001-01-01T00:00:00Z";

/// Go `time.Format(time.RFC3339)`: second precision, `Z` for UTC.
pub fn format_time(value: &DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub fn format_optional_time(value: Option<&DateTime<Utc>>) -> String {
    value.map(format_time).unwrap_or_default()
}

/// Formats a required column. An unset Rust timestamp is Go's zero time, not
/// "now": the Go store writes `0001-01-01T00:00:00Z` for a zero `time.Time`.
pub fn format_time_or_zero(value: Option<&DateTime<Utc>>) -> String {
    value
        .map(format_time)
        .unwrap_or_else(|| ZERO_TIME.to_string())
}

/// Accepts RFC3339 with or without fractional seconds, like the Go parser.
///
/// Go's `time.Parse` treats the `T` and `Z` as literals, so lowercase forms that
/// chrono would otherwise accept are rejected here too. Leap seconds are
/// rejected: Go's layout parses the seconds field as 0..=59. Numeric offsets
/// follow Go's range check (`hour <= 24`, `minute <= 60`) and are normalised by
/// hand because chrono rejects `+24:00` and `+01:60`.
pub fn parse_time(value: &str) -> Timestamp {
    if value.is_empty() {
        return None;
    }
    let bytes = value.as_bytes();
    if value.len() < 20 || bytes.get(10) != Some(&b'T') {
        return None;
    }
    if bytes.get(17) == Some(&b'6') && bytes.get(18) == Some(&b'0') {
        return None;
    }
    // Go accepts a comma as the fractional separator; chrono only accepts '.'.
    let mut value = std::borrow::Cow::Borrowed(value);
    if bytes.get(19) == Some(&b',') && bytes.get(20).is_some_and(u8::is_ascii_digit) {
        let (whole, rest) = value.split_at(19);
        value = std::borrow::Cow::Owned(format!("{whole}.{}", &rest[1..]));
    }
    let value = value.as_ref();
    if value.ends_with('z') {
        return None;
    }
    if value.ends_with('Z') {
        return DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|parsed| parsed.with_timezone(&Utc));
    }

    // Numeric offset: `±HH:MM`, exactly like the `Z07:00` layout.
    let body = value.get(..value.len() - 6)?;
    let offset = value.get(value.len() - 6..)?;
    let offset = offset.as_bytes();
    if offset[0] != b'+' && offset[0] != b'-' {
        return None;
    }
    if offset[3] != b':' {
        return None;
    }
    let hour = two_digits(offset, 1)?;
    let minute = two_digits(offset, 4)?;
    if hour > 24 || minute > 60 {
        return None;
    }
    let negative = offset[0] == b'-';
    let wall_clock = DateTime::parse_from_rfc3339(&format!("{body}+00:00"))
        .ok()?
        .with_timezone(&Utc);
    let offset = Duration::seconds((hour * 60 + minute) * 60);
    Some(if negative {
        wall_clock + offset
    } else {
        wall_clock - offset
    })
}

/// Two ASCII digits at `index`. Go's offset parser requires the fixed width.
fn two_digits(bytes: &[u8], index: usize) -> Option<i64> {
    let high = *bytes.get(index)?;
    let low = *bytes.get(index + 1)?;
    if !high.is_ascii_digit() || !low.is_ascii_digit() {
        return None;
    }
    Some(i64::from(high - b'0') * 10 + i64::from(low - b'0'))
}

/// Go `NullTime`: empty, unparseable or Go's zero time becomes None. The
/// stored text for a zero time parses successfully, but Go checks `IsZero`
/// before returning the pointer.
pub fn null_time(value: &str) -> Timestamp {
    parse_time(value).filter(|parsed| *parsed != zero_time())
}

fn zero_time() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(1, 1, 1, 0, 0, 0)
        .single()
        .expect("year 1 is a valid instant")
}

pub fn encode_json(values: &[String]) -> String {
    escape_html(serde_json::to_string(values).unwrap_or_else(|_| "[]".to_string()))
}

/// Go's `encoding/json` escapes `<`, `>`, `&`, U+2028 and U+2029 so a JSON
/// column is safe to embed in HTML or script contexts.
fn escape_html(encoded: String) -> String {
    if !encoded.contains(['<', '>', '&', '\u{2028}', '\u{2029}']) {
        return encoded;
    }
    encoded
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

pub fn decode_json(value: &str) -> Vec<String> {
    decode_strings(value).unwrap_or_default()
}

/// Like Go, a `null` element keeps its zero value: `["a",null]` is
/// `["a",""]`, not a decode failure.
fn decode_strings(value: &str) -> Option<Vec<String>> {
    if value.is_empty() {
        return Some(Vec::new());
    }
    let parsed: Option<Vec<Option<String>>> = serde_json::from_str(value).ok()?;
    Some(parsed?.into_iter().map(Option::unwrap_or_default).collect())
}

pub fn encode_string_list_map(values: &HashMap<String, Vec<String>>) -> String {
    let sorted: BTreeMap<&String, &Vec<String>> = values.iter().collect();
    escape_html(serde_json::to_string(&sorted).unwrap_or_else(|_| "{}".to_string()))
}

pub fn decode_string_list_map(value: &str) -> HashMap<String, Vec<String>> {
    let Some(parsed) = decode_nullable_map::<Vec<Option<String>>>(value) else {
        return HashMap::new();
    };
    parsed
        .into_iter()
        .map(|(key, items)| {
            (
                key,
                items
                    .unwrap_or_default()
                    .into_iter()
                    .map(Option::unwrap_or_default)
                    .collect(),
            )
        })
        .collect()
}

pub fn encode_string_map(values: &HashMap<String, String>) -> String {
    let sorted: BTreeMap<&String, &String> = values.iter().collect();
    escape_html(serde_json::to_string(&sorted).unwrap_or_else(|_| "{}".to_string()))
}

pub fn decode_string_map(value: &str) -> HashMap<String, String> {
    let Some(parsed) = decode_nullable_map::<String>(value) else {
        return HashMap::new();
    };
    parsed
        .into_iter()
        .map(|(key, value)| (key, value.unwrap_or_default()))
        .collect()
}

/// A `null` document and an unparseable one both decode as an empty map.
fn decode_nullable_map<V>(value: &str) -> Option<HashMap<String, Option<V>>>
where
    V: serde::de::DeserializeOwned,
{
    if value.is_empty() {
        return Some(HashMap::new());
    }
    let parsed: Option<HashMap<String, Option<V>>> = serde_json::from_str(value).ok()?;
    parsed
}

pub fn encode_courier_events(events: &[CourierEvent]) -> String {
    escape_html(serde_json::to_string(events).unwrap_or_else(|_| "[]".to_string()))
}

pub fn decode_courier_events(value: &str) -> Vec<CourierEvent> {
    if value.is_empty() {
        return Vec::new();
    }
    let parsed: Option<Vec<Option<CourierEvent>>> = match serde_json::from_str(value) {
        Ok(parsed) => parsed,
        Err(_) => return Vec::new(),
    };
    parsed
        .unwrap_or_default()
        .into_iter()
        .map(Option::unwrap_or_default)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
            .unwrap()
    }

    #[test]
    fn parse_time_matches_go_rfc3339_literals() {
        assert!(parse_time("2026-11-24T16:00:00Z").is_some());
        assert!(parse_time("2026-11-24T16:00:00.123456789Z").is_some());
        assert!(parse_time("2026-11-24T16:00:00+01:00").is_some());
        assert!(parse_time("2026-11-24t16:00:00Z").is_none());
        assert!(parse_time("2026-11-24T16:00:00z").is_none());
        assert!(parse_time("").is_none());
        assert!(parse_time("not a time").is_none());
    }

    #[test]
    fn parse_time_rejects_leap_seconds_like_go() {
        assert!(parse_time("2016-12-31T23:59:60Z").is_none());
        assert!(parse_time("2016-12-31T23:59:60+01:00").is_none());
        assert!(parse_time("2016-12-31T23:59:60.5Z").is_none());
        assert!(parse_time("2016-12-31T23:59:59Z").is_some());
    }

    #[test]
    fn parse_time_accepts_go_offset_range_and_normalises() {
        assert_eq!(
            parse_time("2026-11-24T16:00:00+24:00"),
            Some(utc(2026, 11, 23, 16, 0, 0))
        );
        assert_eq!(
            parse_time("2026-11-24T16:00:00-24:00"),
            Some(utc(2026, 11, 25, 16, 0, 0))
        );
        assert_eq!(
            parse_time("2026-11-24T16:00:00+01:60"),
            Some(utc(2026, 11, 24, 14, 0, 0))
        );
        assert_eq!(
            parse_time("2026-11-24T16:00:00+24:60"),
            Some(utc(2026, 11, 23, 15, 0, 0))
        );
        assert!(parse_time("2026-11-24T16:00:00+25:00").is_none());
        assert!(parse_time("2026-11-24T16:00:00+24:61").is_none());
        assert!(parse_time("2026-11-24T16:00:00+1:00").is_none());
        assert_eq!(
            parse_time("2026-11-24T16:00:00-00:00"),
            Some(utc(2026, 11, 24, 16, 0, 0))
        );
    }

    #[test]
    fn parse_time_accepts_go_comma_fraction() {
        assert_eq!(
            parse_time("2026-11-24T16:00:00,5Z"),
            Some(utc(2026, 11, 24, 16, 0, 0) + Duration::milliseconds(500))
        );
        assert_eq!(
            parse_time("2026-11-24T16:00:00,5+01:00"),
            Some(utc(2026, 11, 24, 15, 0, 0) + Duration::milliseconds(500))
        );
    }

    #[test]
    fn null_time_treats_go_zero_time_as_unset() {
        assert_eq!(null_time(""), None);
        assert_eq!(null_time("0001-01-01T00:00:00Z"), None);
        assert_eq!(null_time("not a time"), None);
        assert_eq!(
            null_time("2026-11-24T16:00:00Z").map(|value| format_time(&value)),
            Some("2026-11-24T16:00:00Z".to_string())
        );
    }

    #[test]
    fn format_time_or_zero_matches_go_zero_time() {
        assert_eq!(format_time_or_zero(None), "0001-01-01T00:00:00Z");
        assert_eq!(
            format_time_or_zero(Some(&utc(2026, 11, 24, 16, 0, 0))),
            "2026-11-24T16:00:00Z"
        );
        // The zero instant seen through a required column round-trips as the
        // same text because the seconds formatter drops the fraction.
        assert_eq!(
            format_time_or_zero(parse_time(ZERO_TIME).as_ref()),
            ZERO_TIME
        );
    }

    #[test]
    fn encode_maps_sorts_keys_and_escapes_html() {
        let mut attributes = HashMap::new();
        attributes.insert("size".to_string(), vec!["<m>&".to_string()]);
        attributes.insert("color".to_string(), vec!["\u{2028}\u{2029}".to_string()]);
        assert_eq!(
            encode_string_list_map(&attributes),
            "{\"color\":[\"\\u2028\\u2029\"],\"size\":[\"\\u003cm\\u003e\\u0026\"]}"
        );

        let mut options = HashMap::new();
        options.insert("b".to_string(), "2".to_string());
        options.insert("a".to_string(), "<1>".to_string());
        assert_eq!(
            encode_string_map(&options),
            "{\"a\":\"\\u003c1\\u003e\",\"b\":\"2\"}"
        );

        assert_eq!(
            encode_json(&["<script>".to_string()]),
            "[\"\\u003cscript\\u003e\"]"
        );
    }

    #[test]
    fn decode_json_keeps_null_elements_like_go() {
        assert_eq!(
            decode_json("[\"a\",null]"),
            vec!["a".to_string(), String::new()]
        );
        assert_eq!(decode_json("[null]"), vec![String::new()]);
        assert!(decode_json("null").is_empty());
        assert!(decode_json("").is_empty());
        assert!(decode_json("not json").is_empty());
        assert!(decode_json("[1]").is_empty());
    }

    #[test]
    fn decode_string_maps_keep_null_values_like_go() {
        let decoded = decode_string_map("{\"k\":null,\"a\":\"b\"}");
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded.get("k").map(String::as_str), Some(""));
        assert_eq!(decoded.get("a").map(String::as_str), Some("b"));
        assert!(decode_string_map("null").is_empty());
        assert!(decode_string_map("{\"k\":1}").is_empty());
        assert!(decode_string_map("").is_empty());
    }

    #[test]
    fn decode_string_list_maps_keep_null_values_like_go() {
        let decoded = decode_string_list_map("{\"k\":null,\"a\":[\"x\",null]}");
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded.get("k"), Some(&Vec::new()));
        assert_eq!(
            decoded.get("a"),
            Some(&vec!["x".to_string(), String::new()])
        );
        assert!(decode_string_list_map("null").is_empty());
        assert!(decode_string_list_map("{\"k\":[1]}").is_empty());
        assert!(decode_string_list_map("").is_empty());
    }

    #[test]
    fn decode_courier_events_keeps_null_values_like_go() {
        let decoded = decode_courier_events(
            "[null,{\"status\":null,\"rawStatus\":\"r\",\"timestamp\":\"t\"}]",
        );
        assert_eq!(
            decoded,
            vec![
                CourierEvent::default(),
                CourierEvent {
                    status: String::new(),
                    raw_status: "r".to_string(),
                    timestamp: "t".to_string(),
                },
            ]
        );
        assert!(decode_courier_events("null").is_empty());
        assert!(decode_courier_events("").is_empty());
        assert!(decode_courier_events("[1]").is_empty());
    }
}
