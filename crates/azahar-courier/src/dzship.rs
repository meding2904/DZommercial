//! The dzship HTTP adapter. It knows the wire contract and nothing about
//! caching, outages or fallbacks; `Client` owns those.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::Method;
use reqwest::header::{ACCEPT, CONTENT_TYPE, RETRY_AFTER, USER_AGENT};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::client::Source;
use crate::options::Options;
use crate::types::{
    APIError, Commune, Desk, Error, OrderRequest, Shipment, Tracking, TrackingEvent, Wilaya,
};

const RESPONSE_LIMIT: usize = 2 << 20;
const USER_AGENT_VALUE: &str = "azahar-store/1.0";

pub(crate) struct DzShip {
    base_url: String,
    courier_key: String,
    credentials: BTreeMap<String, String>,
    from_wilaya: i32,
    http: Result<reqwest::Client, String>,
}

impl DzShip {
    pub(crate) fn new(options: &Options, timeout: Duration) -> Self {
        // The builder only fails if the TLS backend cannot initialise. Keep the
        // configured timeout and redirect policy either way: a failure is
        // surfaced as a typed error on the next request instead of silently
        // falling back to reqwest's defaults.
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| error.to_string());
        Self {
            base_url: options.base_url.trim().trim_end_matches('/').to_owned(),
            courier_key: options.courier_key.trim().to_owned(),
            credentials: options.credentials.clone(),
            from_wilaya: options.from_wilaya,
            http,
        }
    }

    fn credentials_or_none(&self) -> Option<&BTreeMap<String, String>> {
        (!self.credentials.is_empty()).then_some(&self.credentials)
    }

    fn options_payload(&self) -> Option<OptionsPayload> {
        (self.from_wilaya != 0).then_some(OptionsPayload {
            from_wilaya: self.from_wilaya,
        })
    }

    fn create_order_body<'a>(&'a self, request: &'a OrderRequest) -> CreateOrderPayload<'a> {
        let recipient = &request.recipient;
        CreateOrderPayload {
            courier: &self.courier_key,
            credentials: self.credentials_or_none(),
            options: self.options_payload(),
            order: OrderPayload {
                reference: optional(&request.reference),
                recipient: RecipientPayload {
                    full_name: &recipient.full_name,
                    phone: &recipient.phone,
                    phone_alt: optional(&recipient.phone_alt),
                    wilaya_code: recipient.wilaya_code,
                    commune_name: &recipient.commune_name,
                    address_line: optional(&recipient.address_line),
                },
                delivery_type: &request.delivery_type,
                stop_desk_id: optional(&request.stop_desk_id),
                product_list: &request.product_list,
                cod_amount: request.cod_amount,
                free_shipping: request.free_shipping,
                notes: optional(&request.notes),
            },
        }
    }

    fn track_body<'a>(&'a self, key: &'a str, tracking_number: &'a str) -> TrackPayload<'a> {
        TrackPayload {
            courier: key,
            credentials: self.credentials_or_none(),
            options: self.options_payload(),
            tracking_number,
        }
    }

    fn desks_body(&self, wilaya_code: i32) -> DesksPayload<'_> {
        DesksPayload {
            courier: &self.courier_key,
            credentials: self.credentials_or_none(),
            options: self.options_payload(),
            wilaya_code,
        }
    }

    async fn do_json<T, D>(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        decode: D,
    ) -> Result<T, Error>
    where
        D: FnOnce(Value) -> Result<T, String>,
    {
        let http = self
            .http
            .as_ref()
            .map_err(|detail| Error::BuildRequest(detail.clone()))?;
        let url = format!("{}{}", self.base_url, path);
        let url =
            reqwest::Url::parse(&url).map_err(|error| Error::BuildRequest(error.to_string()))?;

        let mut request = http
            .request(method, url)
            .header(ACCEPT, "application/json")
            .header(USER_AGENT, USER_AGENT_VALUE);
        if let Some(body) = body {
            request = request.header(CONTENT_TYPE, "application/json").body(body);
        }

        let response = request
            .send()
            .await
            .map_err(|error| Error::transport(error.to_string()))?;
        let status = response.status();
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let data = read_limited(response).await?;

        if !status.is_success() {
            return Err(api_error(status.as_u16(), retry_after.as_deref(), &data).into());
        }

        let value: Value = serde_json::from_slice(&data)
            .map_err(|error| Error::transport(format!("decode courier response: {error}")))?;
        decode(value)
            .map_err(|detail| Error::transport(format!("decode courier response: {detail}")))
    }
}

#[async_trait]
impl Source for DzShip {
    async fn wilayas(&self) -> Result<Vec<Wilaya>, Error> {
        self.do_json(Method::GET, "/v1/wilayas", None, |value| {
            decode_list(value, "wilaya list", decode_wilaya)
        })
        .await
    }

    async fn communes(&self, wilaya_code: i32) -> Result<Vec<Commune>, Error> {
        let path = format!("/v1/communes?wilaya={wilaya_code}");
        self.do_json(Method::GET, &path, None, |value| {
            decode_list(value, "commune list", decode_commune)
        })
        .await
    }

    async fn desks(&self, wilaya_code: i32) -> Result<Vec<Desk>, Error> {
        let body = serde_json::to_vec(&self.desks_body(wilaya_code))
            .map_err(|error| Error::Encode(error.to_string()))?;
        self.do_json(Method::POST, "/v1/desks", Some(body), |value| {
            decode_list(value, "desk list", decode_desk)
        })
        .await
    }

    async fn create_order(&self, request: OrderRequest) -> Result<Shipment, Error> {
        let body = serde_json::to_vec(&self.create_order_body(&request))
            .map_err(|error| Error::Encode(error.to_string()))?;
        self.do_json(Method::POST, "/v1/orders", Some(body), decode_shipment)
            .await
    }

    async fn track(&self, courier_key: &str, tracking_number: &str) -> Result<Tracking, Error> {
        let key = courier_key.trim();
        let key = if key.is_empty() {
            self.courier_key.as_str()
        } else {
            key
        };
        let body = serde_json::to_vec(&self.track_body(key, tracking_number))
            .map_err(|error| Error::Encode(error.to_string()))?;
        self.do_json(Method::POST, "/v1/track", Some(body), decode_tracking)
            .await
    }
}

async fn read_limited(mut response: reqwest::Response) -> Result<Vec<u8>, Error> {
    let mut data = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| Error::transport(format!("read courier response: {error}")))?
    {
        let remaining = RESPONSE_LIMIT.saturating_sub(data.len());
        if remaining == 0 {
            break;
        }
        let take = chunk.len().min(remaining);
        data.extend_from_slice(&chunk[..take]);
        if take < chunk.len() {
            break;
        }
    }
    Ok(data)
}

/// A JSON object read with Go's `encoding/json` matching rules: keys match
/// case-insensitively, and an absent or `null` field keeps the Go zero value.
/// A wrong value type is a decode error, exactly like `json.Unmarshal`.
struct Fields(Map<String, Value>);

impl Fields {
    fn new(value: Value, what: &str) -> Result<Self, String> {
        match value {
            Value::Object(map) => Ok(Self(map)),
            Value::Null => Ok(Self(Map::new())),
            _ => Err(format!("expected a JSON object for the {what}")),
        }
    }

    fn get(&self, key: &str) -> Option<&Value> {
        if let Some(value) = self.0.get(key) {
            return Some(value);
        }
        self.0
            .iter()
            .rev()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value)
    }

    fn string(&self, key: &str) -> Result<String, String> {
        match self.get(key) {
            None | Some(Value::Null) => Ok(String::new()),
            Some(Value::String(value)) => Ok(value.clone()),
            Some(_) => Err(format!("field {key:?} is not a string")),
        }
    }

    fn i32(&self, key: &str) -> Result<i32, String> {
        match self.get(key) {
            None | Some(Value::Null) => Ok(0),
            Some(Value::Number(value)) => value
                .as_i64()
                .and_then(|value| i32::try_from(value).ok())
                .ok_or_else(|| format!("field {key:?} is not an integer")),
            Some(_) => Err(format!("field {key:?} is not an integer")),
        }
    }

    fn bool(&self, key: &str) -> Result<bool, String> {
        match self.get(key) {
            None | Some(Value::Null) => Ok(false),
            Some(Value::Bool(value)) => Ok(*value),
            Some(_) => Err(format!("field {key:?} is not a boolean")),
        }
    }

    fn array(&self, key: &str) -> Result<&[Value], String> {
        match self.get(key) {
            None | Some(Value::Null) => Ok(&[]),
            Some(Value::Array(values)) => Ok(values),
            Some(_) => Err(format!("field {key:?} is not an array")),
        }
    }

    fn string_map(&self, key: &str) -> Result<BTreeMap<String, String>, String> {
        match self.get(key) {
            None | Some(Value::Null) => Ok(BTreeMap::new()),
            Some(Value::Object(map)) => map
                .iter()
                .map(|(name, value)| match value {
                    Value::String(text) => Ok((name.clone(), text.clone())),
                    Value::Null => Ok((name.clone(), String::new())),
                    _ => Err(format!("field {key:?} has a non-string value")),
                })
                .collect(),
            Some(_) => Err(format!("field {key:?} is not an object")),
        }
    }
}

fn decode_list<T>(
    value: Value,
    what: &str,
    decode: fn(Value) -> Result<T, String>,
) -> Result<Vec<T>, String> {
    match value {
        Value::Array(values) => values.into_iter().map(decode).collect(),
        Value::Null => Ok(Vec::new()),
        _ => Err(format!("expected a JSON array for the {what}")),
    }
}

fn decode_wilaya(value: Value) -> Result<Wilaya, String> {
    let fields = Fields::new(value, "wilaya")?;
    Ok(Wilaya {
        code: fields.i32("code")?,
        name_ar: fields.string("nameAr")?,
        name_fr: fields.string("nameFr")?,
        is_deep_south: fields.bool("isDeepSouth")?,
        commune_count: fields.i32("communeCount")?,
        courier_supported: fields.bool("courierSupported")?,
    })
}

fn decode_commune(value: Value) -> Result<Commune, String> {
    let fields = Fields::new(value, "commune")?;
    Ok(Commune {
        wilaya_code: fields.i32("wilayaCode")?,
        name_fr: fields.string("nameFr")?,
        name_ar: fields.string("nameAr")?,
    })
}

fn decode_desk(value: Value) -> Result<Desk, String> {
    let fields = Fields::new(value, "desk")?;
    Ok(Desk {
        id: fields.string("id")?,
        name: fields.string("name")?,
        wilaya_code: fields.i32("wilayaCode")?,
        commune_name: fields.string("communeName")?,
        address: fields.string("address")?,
    })
}

fn decode_shipment(value: Value) -> Result<Shipment, String> {
    let fields = Fields::new(value, "shipment")?;
    Ok(Shipment {
        tracking_number: fields.string("trackingNumber")?,
        courier_reference: fields.string("courierReference")?,
        status: fields.string("status")?,
        reference: fields.string("reference")?,
    })
}

fn decode_tracking_event(value: Value) -> Result<TrackingEvent, String> {
    let fields = Fields::new(value, "tracking event")?;
    Ok(TrackingEvent {
        status: fields.string("status")?,
        raw_status: fields.string("rawStatus")?,
        timestamp: fields.string("timestamp")?,
    })
}

fn decode_tracking(value: Value) -> Result<Tracking, String> {
    let fields = Fields::new(value, "tracking response")?;
    let events = fields
        .array("events")?
        .iter()
        .cloned()
        .map(decode_tracking_event)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Tracking {
        tracking_number: fields.string("trackingNumber")?,
        status: fields.string("status")?,
        events,
    })
}

#[derive(Default)]
struct EnvelopeError {
    code: String,
    message: String,
    fields: BTreeMap<String, String>,
}

fn decode_envelope(data: &[u8]) -> Option<EnvelopeError> {
    let value = serde_json::from_slice::<Value>(data).ok()?;
    let root = Fields::new(value, "error envelope").ok()?;
    let error = Fields::new(root.get("error")?.clone(), "error").ok()?;
    Some(EnvelopeError {
        code: error.string("code").ok()?,
        message: error.string("message").ok()?,
        fields: error.string_map("fields").ok()?,
    })
}

fn api_error(status: u16, retry_after: Option<&str>, data: &[u8]) -> APIError {
    let mut result = APIError {
        status: i32::from(status),
        code: "courier_error".to_owned(),
        ..APIError::default()
    };

    if let Some(seconds) = retry_after
        .map(str::trim)
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|seconds| *seconds > 0)
    {
        result.retry_after = Duration::from_secs(seconds.unsigned_abs());
    }

    if let Some(envelope) = decode_envelope(data)
        && !envelope.code.is_empty()
    {
        result.code = envelope.code;
        result.message = envelope.message.trim().to_owned();
        result.fields = envelope.fields;
    }

    if result.message.is_empty() {
        let mut message = String::from_utf8_lossy(data).trim().to_owned();
        if message.is_empty() || message.starts_with('<') {
            message = format!("The courier returned HTTP {status}.");
        }
        result.message = message;
    }
    result
}

fn optional(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Serialize)]
struct OptionsPayload {
    #[serde(rename = "fromWilaya")]
    from_wilaya: i32,
}

#[derive(Serialize)]
struct RecipientPayload<'a> {
    #[serde(rename = "fullName")]
    full_name: &'a str,
    phone: &'a str,
    #[serde(rename = "phoneAlt", skip_serializing_if = "Option::is_none")]
    phone_alt: Option<&'a str>,
    #[serde(rename = "wilayaCode")]
    wilaya_code: i32,
    #[serde(rename = "communeName")]
    commune_name: &'a str,
    #[serde(rename = "addressLine", skip_serializing_if = "Option::is_none")]
    address_line: Option<&'a str>,
}

#[derive(Serialize)]
struct OrderPayload<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    reference: Option<&'a str>,
    recipient: RecipientPayload<'a>,
    #[serde(rename = "deliveryType")]
    delivery_type: &'a str,
    #[serde(rename = "stopDeskId", skip_serializing_if = "Option::is_none")]
    stop_desk_id: Option<&'a str>,
    #[serde(rename = "productList")]
    product_list: &'a str,
    #[serde(rename = "codAmount")]
    cod_amount: i64,
    #[serde(rename = "freeShipping", skip_serializing_if = "is_false")]
    free_shipping: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<&'a str>,
}

#[derive(Serialize)]
struct CreateOrderPayload<'a> {
    courier: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    credentials: Option<&'a BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OptionsPayload>,
    order: OrderPayload<'a>,
}

#[derive(Serialize)]
struct TrackPayload<'a> {
    courier: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    credentials: Option<&'a BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OptionsPayload>,
    #[serde(rename = "trackingNumber")]
    tracking_number: &'a str,
}

#[derive(Serialize)]
struct DesksPayload<'a> {
    courier: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    credentials: Option<&'a BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OptionsPayload>,
    #[serde(rename = "wilayaCode")]
    wilaya_code: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter(options: Options) -> DzShip {
        DzShip::new(&options, Duration::from_secs(1))
    }

    #[test]
    fn api_error_reads_the_envelope_status_and_retry_after() {
        let error = api_error(
            422,
            Some(" 7 "),
            br#"{"error":{"code":"invalid_phone","message":" Invalid phone. ","fields":{"recipient.phone":"Use 05, 06 or 07.","empty":null}}}"#,
        );
        assert_eq!(error.status, 422);
        assert_eq!(error.code, "invalid_phone");
        assert_eq!(error.message, "Invalid phone.");
        assert_eq!(
            error.fields.get("recipient.phone").map(String::as_str),
            Some("Use 05, 06 or 07.")
        );
        assert_eq!(error.fields.get("empty").map(String::as_str), Some(""));
        assert_eq!(error.retry_after, Duration::from_secs(7));
    }

    #[test]
    fn api_error_defaults_to_the_body_text_or_the_status() {
        let raw = api_error(400, None, b"  plain failure ");
        assert_eq!(raw.code, "courier_error");
        assert_eq!(raw.message, "plain failure");
        assert_eq!(raw.retry_after, Duration::ZERO);

        let html = api_error(502, Some("0"), b"<html>down</html>");
        assert_eq!(html.message, "The courier returned HTTP 502.");
        assert_eq!(html.retry_after, Duration::ZERO);
    }

    #[test]
    fn api_error_ignores_an_envelope_without_a_code() {
        let error = api_error(500, None, br#"{"error":{"message":"boom"}}"#);
        assert_eq!(error.code, "courier_error");
        assert_eq!(error.message, r#"{"error":{"message":"boom"}}"#);
    }

    #[test]
    fn the_error_envelope_keys_are_matched_case_insensitively() {
        let error = api_error(
            422,
            None,
            br#"{"ERROR":{"Code":"invalid_phone","MESSAGE":" Invalid phone. ","Fields":{"recipient.phone":"Use 05, 06 or 07.","empty":null}}}"#,
        );
        assert_eq!(error.code, "invalid_phone");
        assert_eq!(error.message, "Invalid phone.");
        assert_eq!(
            error.fields.get("recipient.phone").map(String::as_str),
            Some("Use 05, 06 or 07.")
        );
        assert_eq!(error.fields.get("empty").map(String::as_str), Some(""));
    }

    fn decode_payload<T>(payload: &str, decode: fn(Value) -> Result<T, String>) -> T {
        let value: Value = serde_json::from_str(payload).expect("payload json");
        decode(value).expect("decode payload")
    }

    #[test]
    fn shipment_keys_are_matched_case_insensitively() {
        let shipment = decode_payload(
            r#"{"TRACKINGNUMBER":"yal-ABC123","CourierReference":"ref-1","STATUS":"created","Reference":"AZ-2026-0041"}"#,
            decode_shipment,
        );
        assert_eq!(shipment.tracking_number, "yal-ABC123");
        assert_eq!(shipment.courier_reference, "ref-1");
        assert_eq!(shipment.status, "created");
        assert_eq!(shipment.reference, "AZ-2026-0041");

        let partial = decode_payload(r#"{"Status":"created"}"#, decode_shipment);
        assert_eq!(partial.status, "created");
        assert_eq!(partial.tracking_number, "");
        assert_eq!(partial.courier_reference, "");
        assert_eq!(partial.reference, "");
    }

    #[test]
    fn tracking_and_event_keys_are_matched_case_insensitively() {
        let tracking = decode_payload(
            r#"{"TrackingNumber":"yal-ABC123","Status":"in_transit","EVENTS":[{"STATUS":"picked_up","RawStatus":"Ramassé","TIMEStamp":"2026-11-11T10:00:00Z"},{"Status":"in_transit"}]}"#,
            decode_tracking,
        );
        assert_eq!(tracking.tracking_number, "yal-ABC123");
        assert_eq!(tracking.status, "in_transit");
        assert_eq!(tracking.events.len(), 2);
        assert_eq!(tracking.events[0].status, "picked_up");
        assert_eq!(tracking.events[0].raw_status, "Ramassé");
        assert_eq!(tracking.events[0].timestamp, "2026-11-11T10:00:00Z");
        assert_eq!(tracking.events[1].status, "in_transit");
        assert_eq!(tracking.events[1].raw_status, "");
        assert_eq!(tracking.events[1].timestamp, "");
    }

    #[test]
    fn list_keys_are_matched_case_insensitively() {
        let wilayas = decode_payload(
            r#"[{"CODE":16,"NameAr":"الجزائر","namefr":"Alger","IsDeepSouth":false,"communeCount":57,"COURIERSUPPORTED":true}]"#,
            |value| decode_list(value, "wilaya list", decode_wilaya),
        );
        assert_eq!(wilayas.len(), 1);
        assert_eq!(wilayas[0].code, 16);
        assert_eq!(wilayas[0].name_ar, "الجزائر");
        assert_eq!(wilayas[0].name_fr, "Alger");
        assert!(!wilayas[0].is_deep_south);
        assert_eq!(wilayas[0].commune_count, 57);
        assert!(wilayas[0].courier_supported);

        let desks = decode_payload(
            r#"[{"ID":"SBX-16","Name":"Sandbox desk","WilayaCode":16,"COMMUNENAME":"Bab Ezzouar","Address":"Sandbox address"}]"#,
            |value| decode_list(value, "desk list", decode_desk),
        );
        assert_eq!(desks.len(), 1);
        assert_eq!(desks[0].id, "SBX-16");
        assert_eq!(desks[0].wilaya_code, 16);
        assert_eq!(desks[0].commune_name, "Bab Ezzouar");

        let communes = decode_payload(
            r#"[{"WILAYACODE":16,"namefr":"Bab Ezzouar","NAMEAR":"باب الزوار"}]"#,
            |value| decode_list(value, "commune list", decode_commune),
        );
        assert_eq!(communes.len(), 1);
        assert_eq!(communes[0].wilaya_code, 16);
        assert_eq!(communes[0].name_fr, "Bab Ezzouar");
        assert_eq!(communes[0].name_ar, "باب الزوار");

        assert!(
            decode_payload("null", |value| decode_list(
                value,
                "wilaya list",
                decode_wilaya
            ))
            .is_empty()
        );
    }

    #[test]
    fn wrong_payload_shapes_are_decode_errors_like_go() {
        let array: Value = serde_json::from_str("[]").expect("payload json");
        assert!(decode_shipment(array).is_err());

        let wrong_field: Value = serde_json::from_str(r#"{"status":12}"#).expect("payload json");
        assert!(decode_shipment(wrong_field).is_err());

        let object: Value = serde_json::from_str("{}").expect("payload json");
        assert!(decode_list(object, "wilaya list", decode_wilaya).is_err());
    }

    #[test]
    fn null_payloads_keep_the_go_zero_value() {
        let null: Value = serde_json::from_str("null").expect("payload json");
        assert_eq!(
            decode_shipment(null).expect("null shipment"),
            Shipment::default()
        );

        let null: Value = serde_json::from_str(r#"{"events":null}"#).expect("payload json");
        let tracking = decode_tracking(null).expect("null events");
        assert!(tracking.events.is_empty());
    }

    #[test]
    fn create_order_payload_omits_empty_optional_fields() {
        let adapter = adapter(Options::default());
        let request = OrderRequest::default();
        let body = adapter.create_order_body(&request);
        let value = serde_json::to_value(body).expect("serialize");
        assert_eq!(value["courier"], "");
        assert!(value.get("credentials").is_none());
        assert!(value.get("options").is_none());
        assert!(value["order"].get("reference").is_none());
        assert!(value["order"].get("freeShipping").is_none());
        assert!(value["order"].get("stopDeskId").is_none());
        assert!(value["order"].get("notes").is_none());
        assert!(value["order"]["recipient"].get("phoneAlt").is_none());
        assert!(value["order"]["recipient"].get("addressLine").is_none());
        assert_eq!(value["order"]["codAmount"], 0);
    }

    #[test]
    fn create_order_payload_carries_the_full_contract() {
        let mut credentials = BTreeMap::new();
        credentials.insert("apiId".to_owned(), "test-id".to_owned());
        credentials.insert("apiToken".to_owned(), "test-token".to_owned());
        let adapter = adapter(Options {
            courier_key: "yalidine".into(),
            credentials,
            from_wilaya: 16,
            ..Options::default()
        });
        let request = OrderRequest {
            reference: "AZ-2026-0041".into(),
            recipient: crate::types::Recipient {
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
        };
        let value = serde_json::to_value(adapter.create_order_body(&request)).expect("serialize");
        assert_eq!(value["courier"], "yalidine");
        assert_eq!(value["credentials"]["apiId"], "test-id");
        assert_eq!(value["credentials"]["apiToken"], "test-token");
        assert_eq!(value["options"]["fromWilaya"], 16);
        assert_eq!(value["order"]["reference"], "AZ-2026-0041");
        assert_eq!(value["order"]["deliveryType"], "home");
        assert_eq!(value["order"]["stopDeskId"], "SBX-16");
        assert_eq!(value["order"]["codAmount"], 5400);
        assert_eq!(value["order"]["freeShipping"], true);
        assert_eq!(value["order"]["notes"], "Call before delivery");
        assert_eq!(value["order"]["recipient"]["fullName"], "Amina Bouzid");
        assert_eq!(value["order"]["recipient"]["communeName"], "Bab Ezzouar");
        assert_eq!(value["order"]["recipient"]["wilayaCode"], 16);
    }

    #[test]
    fn track_payload_uses_the_given_key() {
        let adapter = adapter(Options {
            courier_key: "yalidine".into(),
            from_wilaya: 16,
            ..Options::default()
        });
        let value =
            serde_json::to_value(adapter.track_body("sandbox", "SBX-1")).expect("serialize");
        assert_eq!(value["courier"], "sandbox");
        assert_eq!(value["trackingNumber"], "SBX-1");
        assert_eq!(value["options"]["fromWilaya"], 16);
    }

    #[test]
    fn desks_payload_carries_the_wilaya() {
        let adapter = adapter(Options {
            courier_key: "sandbox".into(),
            ..Options::default()
        });
        let value = serde_json::to_value(adapter.desks_body(31)).expect("serialize");
        assert_eq!(value["courier"], "sandbox");
        assert_eq!(value["wilayaCode"], 31);
        assert!(value.get("credentials").is_none());
        assert!(value.get("options").is_none());
    }
}
