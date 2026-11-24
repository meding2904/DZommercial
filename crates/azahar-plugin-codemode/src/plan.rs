//! Plan types, strict JSON parsing, validation and fingerprints.
//!
//! Port of `backend/internal/plugins/codemode/plan.go`. The parser accepts only
//! the five documented actions, rejects unknown fields and keeps patch objects
//! as raw JSON so a preview echoes exactly what the provider sent.

use std::collections::{BTreeMap, HashMap};
use std::sync::{LazyLock, RwLock};

use azahar_plugins::Registry;
use azahar_productrules as rules;
use azahar_store::products::{CORE_PLUGIN_ID, normalize_plugin_id};
use azahar_store::{Product, Store, StoreError};
use azahar_storefront::Theme;
use regex::Regex;
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::map::Map as JsonMap;
use serde_json::value::RawValue;

/// A raw JSON object: keys are sorted and duplicate keys keep the last value.
type RawObject = BTreeMap<String, Box<RawValue>>;

use crate::json;

/// A plan takes at most ten actions.
pub const MAX_ACTIONS: usize = 10;

static SLUG_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-z0-9]+(?:-[a-z0-9]+)*$").expect("static slug pattern is valid")
});
static PLUGIN_ID_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-z][a-z0-9_-]{1,31}$").expect("static plugin id pattern is valid")
});

const THEME_PATCH_KEYS: [&str; 28] = [
    "brand_name",
    "tagline",
    "description",
    "hero_heading",
    "hero_body",
    "hero_alt",
    "hero_cta",
    "hero_image",
    "statement",
    "shelf_heading",
    "shop_title",
    "house_label",
    "about_heading",
    "about_body",
    "about_image",
    "about_alt",
    "closing_heading",
    "footer_note",
    "footer_credit",
    "contact_email",
    "announcement",
    "concerns_enabled",
    "font_display",
    "font_body",
    "font_scale",
    "radius",
    "light",
    "dark",
];

const PALETTE_PATCH_KEYS: [&str; 6] = ["ground", "surface", "ink", "muted", "accent", "accent_ink"];

/// One strict action of a plan.
#[derive(Debug, Clone, Default)]
pub struct Action {
    pub action_type: String,
    /// Raw patch object, preserved byte-for-byte apart from Go's compaction.
    pub patch: Option<Box<RawValue>>,
    pub product_id: String,
    pub product: Option<ProductDraft>,
    pub order_id: String,
    pub status: String,
}

/// A `product.create` draft. Field names match the Go JSON tags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProductDraft {
    pub name: String,
    pub slug: String,
    pub price_cents: i64,
    pub category_slug: String,
    pub plugin_id: String,
    pub stock: i64,
    pub image: String,
    pub description: String,
    pub attributes: HashMap<String, Vec<String>>,
}

/// A validated `product.update` patch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProductPatch {
    pub name: Option<String>,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub price_cents: Option<i64>,
    pub stock: Option<i64>,
    pub active: Option<bool>,
}

/// A parsed plan.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    pub summary: String,
    pub actions: Vec<Action>,
}

impl PartialEq for Action {
    fn eq(&self, other: &Self) -> bool {
        self.action_type == other.action_type
            && self.patch.as_ref().map(|raw| raw.get()) == other.patch.as_ref().map(|raw| raw.get())
            && self.product_id == other.product_id
            && self.product == other.product
            && self.order_id == other.order_id
            && self.status == other.status
    }
}

/// One row of the plan history response.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PlanHistoryDto {
    pub id: String,
    pub instruction: String,
    pub summary: String,
    pub status: String,
    pub created_at: String,
}

/// The outcome of one applied action.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionResult {
    pub action_type: String,
    pub ok: bool,
    pub message: String,
}

/// A stored preview row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanRecord {
    pub id: String,
    pub actor_user_id: String,
    pub instruction: String,
    pub plan: String,
    pub status: String,
    pub result: String,
    pub baseline: String,
    pub created_at: String,
}

/// A plan the server refuses to apply, with the exact admin-facing message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct InvalidPlanError(pub String);

impl InvalidPlanError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// A reference check can fail because the plan is invalid, or because the
/// database could not answer.
#[derive(Debug, thiserror::Error)]
pub enum ReferenceError {
    #[error(transparent)]
    Invalid(#[from] InvalidPlanError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// A baseline can fail because the plan refers to a row the preview never saw,
/// or because the theme cannot be encoded.
#[derive(Debug, thiserror::Error)]
pub enum BaselineError {
    #[error(transparent)]
    Invalid(#[from] InvalidPlanError),
    #[error("{0}")]
    Encode(String),
}

fn invalid_plan(message: impl Into<String>) -> InvalidPlanError {
    InvalidPlanError::new(message)
}

/// `ids.New("cmp")`.
#[must_use]
pub fn new_plan_id() -> String {
    azahar_ids::new("cmp")
}

pub(crate) fn registry_read<T>(
    registry: Option<&RwLock<Registry>>,
    read: impl FnOnce(&Registry) -> T,
) -> Option<T> {
    registry.map(|lock| {
        let guard = lock
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        read(&guard)
    })
}

// ---------------------------------------------------------------------------
// Plan parsing
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Envelope {
    summary: Option<Box<RawValue>>,
    actions: Option<Box<RawValue>>,
}

impl<'de> Deserialize<'de> for Envelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(EnvelopeVisitor)
    }
}

struct EnvelopeVisitor;

impl<'de> Visitor<'de> for EnvelopeVisitor {
    type Value = Envelope;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a plan object")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Envelope::default())
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut envelope = Envelope::default();
        while let Some(key) = map.next_key::<String>()? {
            match canonical_field(&key, &["summary", "actions"]) {
                Some("summary") => envelope.summary = Some(map.next_value()?),
                Some("actions") => envelope.actions = Some(map.next_value()?),
                _ => {
                    return Err(serde::de::Error::unknown_field(
                        &key,
                        &["summary", "actions"],
                    ));
                }
            }
        }
        Ok(envelope)
    }
}

/// Parses a provider answer or a stored plan. Markdown fences are tolerated.
pub fn parse_plan(raw: &str) -> Result<Plan, String> {
    let mut text = raw.trim();
    if text.starts_with("```") {
        if let Some(newline) = text.find('\n') {
            text = &text[newline + 1..];
        }
        text = text.trim();
        if let Some(stripped) = text.strip_suffix("```") {
            text = stripped;
        }
        text = text.trim();
    }
    if text.is_empty() {
        return Err("empty plan".to_string());
    }

    let mut deserializer = serde_json::Deserializer::from_str(text);
    let envelope =
        Envelope::deserialize(&mut deserializer).map_err(|error| format!("read plan: {error}"))?;
    if deserializer.end().is_err() {
        return Err("plan must be one JSON object".to_string());
    }

    let summary = match envelope.summary {
        None => String::new(),
        Some(raw) => decode_optional_string(raw.get())
            .map_err(|_| "read plan: summary must be a string".to_string())?
            .unwrap_or_default(),
    };
    let raw_actions: Vec<Box<RawValue>> = match envelope.actions {
        None => Vec::new(),
        Some(raw) => decode_actions(raw.get())
            .map_err(|_| "read plan: actions must be a list".to_string())?
            .unwrap_or_default(),
    };

    let mut actions = Vec::with_capacity(raw_actions.len());
    for (index, raw) in raw_actions.iter().enumerate() {
        let action =
            parse_action(raw.get()).map_err(|error| format!("action {}: {error}", index + 1))?;
        actions.push(action);
    }

    Ok(Plan {
        summary: summary.trim().to_string(),
        actions,
    })
}

fn decode_actions(raw: &str) -> Result<Option<Vec<Box<RawValue>>>, ()> {
    serde_json::from_str::<Option<Vec<Box<RawValue>>>>(raw).map_err(|_| ())
}

/// Parses one action object, rejecting unknown keys and wrong field types.
pub fn parse_action(raw: &str) -> Result<Action, String> {
    let fields = raw_object(raw).ok_or_else(|| "action must be a JSON object".to_string())?;

    let action_type = match fields.get("type") {
        None => return Err("action needs a type".to_string()),
        Some(raw) => decode_optional_string(raw.get())
            .map_err(|_| "action needs a type".to_string())?
            .unwrap_or_default(),
    };
    let action_type = action_type.trim().to_string();
    if action_type.is_empty() {
        return Err("action needs a type".to_string());
    }

    let allowed: &[&str] = match action_type.as_str() {
        "theme.update" => &["type", "patch"],
        "product.update" => &["type", "product_id", "patch"],
        "product.archive" => &["type", "product_id"],
        "product.create" => &["type", "product"],
        "order.status" => &["type", "order_id", "status"],
        other => return Err(format!("unknown action type {}", json::quote(other))),
    };
    for key in fields.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("{action_type} does not take {}", json::quote(key)));
        }
    }

    if matches!(action_type.as_str(), "theme.update" | "product.update")
        && fields
            .get("patch")
            .is_none_or(|raw| raw_object(raw.get()).is_none())
    {
        return Err(format!("{action_type} needs a patch object"));
    }
    if action_type == "product.create"
        && fields
            .get("product")
            .is_none_or(|raw| raw_object(raw.get()).is_none())
    {
        return Err("product.create needs a product object".to_string());
    }
    if action_type == "order.status" {
        let order_id = match fields.get("order_id") {
            None => return Err("order.status needs an order id".to_string()),
            Some(raw) => decode_optional_string(raw.get())
                .map_err(|_| "order.status needs an order id".to_string())?
                .unwrap_or_default(),
        };
        if order_id.trim().is_empty() || order_id.len() > 64 {
            return Err("order.status needs an order id".to_string());
        }
        let status = match fields.get("status") {
            None => return Err(order_status_message()),
            Some(raw) => decode_optional_string(raw.get())
                .map_err(|_| order_status_message())?
                .unwrap_or_default(),
        };
        if !azahar_orderstatus::known(status.trim()) {
            return Err(order_status_message());
        }
    }

    let mut action = Action {
        action_type: action_type.clone(),
        ..Action::default()
    };
    match action_type.as_str() {
        "theme.update" => {
            action.patch = fields.get("patch").cloned();
        }
        "product.update" => {
            action.product_id = decode_field_string(&fields, "product_id", &action_type)?;
            action.patch = fields.get("patch").cloned();
        }
        "product.archive" => {
            action.product_id = decode_field_string(&fields, "product_id", &action_type)?;
        }
        "product.create" => {
            let product = fields.get("product").expect("checked above");
            action.product = Some(
                decode_product_draft(product.get())
                    .map_err(|error| format!("read {action_type}: {error}"))?,
            );
        }
        "order.status" => {
            action.order_id = decode_field_string(&fields, "order_id", &action_type)?;
            action.status = decode_field_string(&fields, "status", &action_type)?;
        }
        _ => {}
    }
    Ok(action)
}

fn order_status_message() -> String {
    format!(
        "order.status needs one of {}",
        azahar_orderstatus::STATUSES.join(", ")
    )
}

fn decode_field_string(fields: &RawObject, key: &str, action_type: &str) -> Result<String, String> {
    match fields.get(key) {
        None => Ok(String::new()),
        Some(raw) => decode_optional_string(raw.get())
            .map(|value| value.unwrap_or_default())
            .map_err(|_| format!("read {action_type}: {key} must be a string")),
    }
}

/// Go's `json.Unmarshal` into `map[string]json.RawMessage`, rejecting `null`.
#[must_use]
pub fn raw_object(raw: &str) -> Option<RawObject> {
    if raw.is_empty() || raw == "null" {
        return None;
    }
    serde_json::from_str::<RawObject>(raw).ok()
}

/// Go's `isNull`: the raw text trims to `null`.
#[must_use]
pub fn is_null(raw: &str) -> bool {
    raw.trim() == "null"
}

fn decode_optional_string(raw: &str) -> Result<Option<String>, ()> {
    serde_json::from_str::<Option<String>>(raw).map_err(|_| ())
}

/// Go decoding of a JSON number into `int`/`int64`: only a plain integer
/// literal fits. `serde_json` stores `-0` as a float, so the literal is
/// inspected rather than the parsed number.
fn decode_int(raw: &str) -> Result<i64, ()> {
    let text = raw.trim();
    if text == "null" {
        return Ok(0);
    }
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(());
    }
    text.parse::<i64>().map_err(|_| ())
}

const DRAFT_FIELDS: [&str; 9] = [
    "name",
    "slug",
    "price_cents",
    "category_slug",
    "plugin_id",
    "stock",
    "image",
    "description",
    "attributes",
];

fn decode_product_draft(raw: &str) -> Result<ProductDraft, String> {
    let fields = raw_object(raw).ok_or_else(|| "expected a JSON object".to_string())?;
    for key in fields.keys() {
        if canonical_field(key, &DRAFT_FIELDS).is_none() {
            return Err(format!("unknown field {}", json::quote(key)));
        }
    }

    let mut draft = ProductDraft::default();
    for (key, raw) in &fields {
        let Some(canonical) = canonical_field(key, &DRAFT_FIELDS) else {
            continue;
        };
        match canonical {
            "name" => {
                draft.name = decode_optional_string(raw.get())
                    .map_err(|_| type_error(key))?
                    .unwrap_or_default()
            }
            "slug" => {
                draft.slug = decode_optional_string(raw.get())
                    .map_err(|_| type_error(key))?
                    .unwrap_or_default()
            }
            "price_cents" => {
                draft.price_cents = decode_int(raw.get()).map_err(|_| type_error(key))?
            }
            "category_slug" => {
                draft.category_slug = decode_optional_string(raw.get())
                    .map_err(|_| type_error(key))?
                    .unwrap_or_default();
            }
            "plugin_id" => {
                draft.plugin_id = decode_optional_string(raw.get())
                    .map_err(|_| type_error(key))?
                    .unwrap_or_default();
            }
            "stock" => draft.stock = decode_int(raw.get()).map_err(|_| type_error(key))?,
            "image" => {
                draft.image = decode_optional_string(raw.get())
                    .map_err(|_| type_error(key))?
                    .unwrap_or_default()
            }
            "description" => {
                draft.description = decode_optional_string(raw.get())
                    .map_err(|_| type_error(key))?
                    .unwrap_or_default();
            }
            "attributes" => {
                draft.attributes = decode_attributes(raw.get()).map_err(|_| type_error(key))?
            }
            _ => {}
        }
    }
    Ok(draft)
}

/// Go's struct field matching: an exact tag wins, otherwise an ASCII
/// case-insensitive match is accepted.
fn canonical_field<'a>(key: &str, names: &[&'a str]) -> Option<&'a str> {
    names
        .iter()
        .find(|name| **name == key)
        .or_else(|| names.iter().find(|name| name.eq_ignore_ascii_case(key)))
        .copied()
}

fn type_error(key: &str) -> String {
    format!("{key} has a value of the wrong type")
}

fn decode_attributes(raw: &str) -> Result<HashMap<String, Vec<String>>, ()> {
    let value = serde_json::from_str::<serde_json::Value>(raw).map_err(|_| ())?;
    match value {
        serde_json::Value::Null => Ok(HashMap::new()),
        serde_json::Value::Object(entries) => {
            let mut attributes = HashMap::with_capacity(entries.len());
            for (key, value) in entries {
                let values = match value {
                    serde_json::Value::Null => Vec::new(),
                    serde_json::Value::Array(items) => {
                        let mut list = Vec::with_capacity(items.len());
                        for item in items {
                            match item {
                                serde_json::Value::Null => list.push(String::new()),
                                serde_json::Value::String(text) => list.push(text),
                                _ => return Err(()),
                            }
                        }
                        list
                    }
                    _ => return Err(()),
                };
                attributes.insert(key, values);
            }
            Ok(attributes)
        }
        _ => Err(()),
    }
}

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

/// Go `json.Marshal(plan)` with patch objects compacted and HTML-escaped.
#[must_use]
pub fn encode_plan(plan: &Plan) -> String {
    let mut object = json::Object::new();
    object.string_field("summary", &plan.summary);
    object.raw_field("actions", &encode_actions(&plan.actions));
    object.finish()
}

fn encode_actions(actions: &[Action]) -> String {
    let mut out = String::from("[");
    for (index, action) in actions.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&encode_action(action));
    }
    out.push(']');
    out
}

pub fn encode_action(action: &Action) -> String {
    let mut object = json::Object::new();
    object.string_field("type", &action.action_type);
    if let Some(patch) = &action.patch {
        object.raw_field("patch", &json::compact(patch.get()));
    }
    if !action.product_id.is_empty() {
        object.string_field("product_id", &action.product_id);
    }
    if let Some(product) = &action.product {
        object.raw_field("product", &encode_product_draft(product));
    }
    if !action.order_id.is_empty() {
        object.string_field("order_id", &action.order_id);
    }
    if !action.status.is_empty() {
        object.string_field("status", &action.status);
    }
    object.finish()
}

pub fn encode_product_draft(draft: &ProductDraft) -> String {
    let mut object = json::Object::new();
    object.string_field("name", &draft.name);
    object.string_field("slug", &draft.slug);
    object.int_field("price_cents", draft.price_cents);
    object.string_field("category_slug", &draft.category_slug);
    object.string_field("plugin_id", &draft.plugin_id);
    object.int_field("stock", draft.stock);
    object.string_field("image", &draft.image);
    object.string_field("description", &draft.description);
    if !draft.attributes.is_empty() {
        let sorted: BTreeMap<&String, &Vec<String>> = draft.attributes.iter().collect();
        let mut attributes = String::from("{");
        for (index, (key, values)) in sorted.iter().enumerate() {
            if index > 0 {
                attributes.push(',');
            }
            attributes.push_str(&json::string(key));
            attributes.push_str(":[");
            for (value_index, value) in values.iter().enumerate() {
                if value_index > 0 {
                    attributes.push(',');
                }
                attributes.push_str(&json::string(value));
            }
            attributes.push(']');
        }
        attributes.push('}');
        object.raw_field("attributes", &attributes);
    }
    object.finish()
}

/// The preview/apply plan envelope. Go marshals the map with sorted keys, so
/// the field order is `actions`, `created_at`, `id`, `status`, `summary`.
#[must_use]
pub fn plan_dto(id: &str, plan: &Plan, status: &str, created_at: &str) -> String {
    let mut object = json::Object::new();
    object.raw_field("actions", &encode_actions(&plan.actions));
    object.string_field("created_at", created_at);
    object.string_field("id", id);
    object.string_field("status", status);
    object.string_field("summary", &plan.summary);
    object.finish()
}

/// The stored `result` column: `{"results":[...]}`.
#[must_use]
pub fn encode_results(results: &[ActionResult]) -> String {
    let mut object = json::Object::new();
    object.raw_field("results", &encode_result_list(results));
    object.finish()
}

/// The `results` array used by both the stored column and the HTTP response.
#[must_use]
pub fn encode_result_list(results: &[ActionResult]) -> String {
    let mut out = String::from("[");
    for (index, result) in results.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let mut object = json::Object::new();
        object.string_field("type", &result.action_type);
        object.bool_field("ok", result.ok);
        object.string_field("message", &result.message);
        out.push_str(&object.finish());
    }
    out.push(']');
    out
}

// ---------------------------------------------------------------------------
// Shape validation
// ---------------------------------------------------------------------------

/// Validates every action against the current theme, applying theme patches in
/// order. A `product.create` draft is trimmed and normalised in place, exactly
/// like the Go pointer receiver.
pub fn validate_plan_shape(
    plan: &mut Plan,
    theme: &Theme,
    registry: Option<&RwLock<Registry>>,
) -> Result<Theme, InvalidPlanError> {
    let mut theme = theme.clone();
    let summary = plan.summary.trim();
    let length = summary.chars().count();
    if !(1..=300).contains(&length) {
        return Err(invalid_plan("the plan summary must be 1 to 300 characters"));
    }
    if !rules::plain_text(summary) {
        return Err(invalid_plan("the plan summary must be plain text"));
    }
    if plan.actions.is_empty() {
        return Err(invalid_plan(
            "the instruction cannot be planned with the available actions",
        ));
    }
    if plan.actions.len() > MAX_ACTIONS {
        return Err(invalid_plan(format!(
            "a plan takes at most {MAX_ACTIONS} actions"
        )));
    }

    for index in 0..plan.actions.len() {
        let prefix = format!("action {}", index + 1);
        let action = &mut plan.actions[index];
        match action.action_type.as_str() {
            "theme.update" => {
                if !action.product_id.is_empty() || action.product.is_some() {
                    return Err(invalid_plan(format!(
                        "{prefix}: theme.update takes only a patch"
                    )));
                }
                theme = apply_theme_patch(&theme, action.patch.as_deref())
                    .map_err(|error| invalid_plan(format!("{prefix}: {error}")))?;
            }
            "product.update" => {
                if action.product_id.trim().is_empty() {
                    return Err(invalid_plan(format!("{prefix}: choose a product id")));
                }
                parse_product_patch(action.patch.as_deref())
                    .map_err(|error| invalid_plan(format!("{prefix}: {error}")))?;
            }
            "product.archive" => {
                if action.product_id.trim().is_empty() {
                    return Err(invalid_plan(format!("{prefix}: choose a product id")));
                }
                if action.patch.is_some() || action.product.is_some() {
                    return Err(invalid_plan(format!(
                        "{prefix}: product.archive takes only a product id"
                    )));
                }
            }
            "product.create" => {
                if !action.product_id.is_empty() || action.patch.is_some() {
                    return Err(invalid_plan(format!(
                        "{prefix}: product.create takes only a product"
                    )));
                }
                if let Some(draft) = action.product.as_mut() {
                    validate_product_draft(draft, registry)
                        .map_err(|error| invalid_plan(format!("{prefix}: {error}")))?;
                } else {
                    return Err(invalid_plan(format!(
                        "{prefix}: choose the product to create"
                    )));
                }
            }
            "order.status" => {
                if !action.product_id.is_empty()
                    || action.product.is_some()
                    || action.patch.is_some()
                {
                    return Err(invalid_plan(format!(
                        "{prefix}: order.status takes only an order id and status"
                    )));
                }
                if action.order_id.trim().is_empty() {
                    return Err(invalid_plan(format!("{prefix}: choose an order id")));
                }
                if !azahar_orderstatus::known(&action.status) {
                    return Err(invalid_plan(format!(
                        "{prefix}: choose a known order status"
                    )));
                }
            }
            other => {
                return Err(invalid_plan(format!(
                    "{prefix}: unknown action type {}",
                    json::quote(other)
                )));
            }
        }
    }
    Ok(theme)
}

// ---------------------------------------------------------------------------
// Theme patches
// ---------------------------------------------------------------------------

/// The theme as a JSON object without `updated_at`.
pub fn theme_object(theme: &Theme) -> Result<JsonMap<String, serde_json::Value>, String> {
    let font_scale = serde_json::Number::from_f64(theme.font_scale)
        .ok_or_else(|| float_error(theme.font_scale))?;
    let mut object = JsonMap::new();
    let strings = [
        ("brand_name", &theme.brand_name),
        ("tagline", &theme.tagline),
        ("description", &theme.description),
        ("hero_heading", &theme.hero_heading),
        ("hero_body", &theme.hero_body),
        ("hero_alt", &theme.hero_alt),
        ("hero_cta", &theme.hero_cta),
        ("hero_image", &theme.hero_image),
        ("statement", &theme.statement),
        ("shelf_heading", &theme.shelf_heading),
        ("shop_title", &theme.shop_title),
        ("house_label", &theme.house_label),
        ("about_heading", &theme.about_heading),
        ("about_body", &theme.about_body),
        ("about_image", &theme.about_image),
        ("about_alt", &theme.about_alt),
        ("closing_heading", &theme.closing_heading),
        ("footer_note", &theme.footer_note),
        ("footer_credit", &theme.footer_credit),
        ("contact_email", &theme.contact_email),
        ("announcement", &theme.announcement),
        ("font_display", &theme.font_display),
        ("font_body", &theme.font_body),
    ];
    for (key, value) in strings {
        object.insert(key.to_string(), serde_json::Value::String(value.clone()));
    }
    object.insert(
        "concerns_enabled".to_string(),
        serde_json::Value::Bool(theme.concerns_enabled),
    );
    object.insert(
        "font_scale".to_string(),
        serde_json::Value::Number(font_scale),
    );
    object.insert("radius".to_string(), serde_json::Value::from(theme.radius));
    object.insert("light".to_string(), palette_value(&theme.light));
    object.insert("dark".to_string(), palette_value(&theme.dark));
    Ok(object)
}

fn palette_value(palette: &azahar_storefront::Palette) -> serde_json::Value {
    let mut object = JsonMap::new();
    for (key, value) in [
        ("ground", &palette.ground),
        ("surface", &palette.surface),
        ("ink", &palette.ink),
        ("muted", &palette.muted),
        ("accent", &palette.accent),
        ("accent_ink", &palette.accent_ink),
    ] {
        object.insert(key.to_string(), serde_json::Value::String(value.clone()));
    }
    serde_json::Value::Object(object)
}

fn float_error(value: f64) -> String {
    if value.is_nan() {
        "json: unsupported value: NaN".to_string()
    } else {
        let rendered = if value > 0.0 { "+Inf" } else { "-Inf" };
        format!("json: unsupported value: {rendered}")
    }
}

/// Applies an allowlisted theme patch and validates the resulting theme.
pub fn apply_theme_patch(theme: &Theme, raw: Option<&RawValue>) -> Result<Theme, String> {
    let patch = raw
        .and_then(|raw| raw_object(raw.get()))
        .ok_or_else(|| "theme.update needs a patch object".to_string())?;
    if patch.is_empty() {
        return Err("theme.update needs at least one field".to_string());
    }
    for (key, value) in &patch {
        if !THEME_PATCH_KEYS.contains(&key.as_str()) {
            return Err(format!("{} is not a theme field", json::quote(key)));
        }
        if key == "light" || key == "dark" {
            let palette = raw_object(value.get())
                .ok_or_else(|| format!("{} must be an object", json::quote(key)))?;
            if palette.is_empty() {
                return Err(format!("{} needs at least one colour", json::quote(key)));
            }
            for (colour, colour_value) in &palette {
                if !PALETTE_PATCH_KEYS.contains(&colour.as_str()) {
                    return Err(format!(
                        "{} is not a colour in {}",
                        json::quote(colour),
                        json::quote(key)
                    ));
                }
                if is_null(colour_value.get()) {
                    return Err(format!("{key}.{colour} cannot be empty"));
                }
            }
            continue;
        }
        if is_null(value.get()) {
            return Err(format!("{} cannot be empty", json::quote(key)));
        }
    }

    let mut current = theme_object(theme)?;
    for (key, value) in &patch {
        if key == "light" || key == "dark" {
            let source = raw_object(value.get()).expect("validated above");
            if !current.contains_key(key) {
                current.insert(key.clone(), serde_json::Value::Object(JsonMap::new()));
            }
            let merged = current
                .get_mut(key)
                .and_then(serde_json::Value::as_object_mut)
                .expect("just ensured the palette object");
            for (colour, colour_value) in source {
                let parsed = serde_json::from_str::<serde_json::Value>(colour_value.get())
                    .map_err(|_| "the theme patch has a value of the wrong type".to_string())?;
                merged.insert(colour, parsed);
            }
            continue;
        }
        let parsed = serde_json::from_str::<serde_json::Value>(value.get())
            .map_err(|_| "the theme patch has a value of the wrong type".to_string())?;
        current.insert(key.clone(), parsed);
    }

    let encoded = serde_json::to_string(&serde_json::Value::Object(current))
        .map_err(|error| error.to_string())?;
    let mut next = azahar_storefront::parse(&encoded)
        .map_err(|_| "the theme patch has a value of the wrong type".to_string())?;
    next.updated_at = String::new();
    if let Some(message) = first_field_message(&azahar_storefront::validate(&next)) {
        return Err(message);
    }
    Ok(next)
}

// ---------------------------------------------------------------------------
// Product patches and drafts
// ---------------------------------------------------------------------------

/// Parses and validates a `product.update` patch.
pub fn parse_product_patch(raw: Option<&RawValue>) -> Result<ProductPatch, String> {
    let fields = raw
        .and_then(|raw| raw_object(raw.get()))
        .ok_or_else(|| "product.update needs a patch object".to_string())?;
    if fields.is_empty() {
        return Err("product.update needs at least one field".to_string());
    }
    for (key, value) in &fields {
        if !matches!(
            key.as_str(),
            "name" | "subtitle" | "description" | "price_cents" | "stock" | "active"
        ) {
            return Err(format!(
                "{} is not an editable product field",
                json::quote(key)
            ));
        }
        if is_null(value.get()) {
            return Err(format!("{} cannot be empty", json::quote(key)));
        }
    }

    let mut patch = ProductPatch::default();
    for (key, raw) in &fields {
        match key.as_str() {
            "name" => {
                patch.name = Some(
                    decode_optional_string(raw.get())
                        .map_err(|_| wrong_patch_type())?
                        .unwrap_or_default(),
                );
            }
            "subtitle" => {
                patch.subtitle = Some(
                    decode_optional_string(raw.get())
                        .map_err(|_| wrong_patch_type())?
                        .unwrap_or_default(),
                );
            }
            "description" => {
                patch.description = Some(
                    decode_optional_string(raw.get())
                        .map_err(|_| wrong_patch_type())?
                        .unwrap_or_default(),
                );
            }
            "price_cents" => {
                patch.price_cents = Some(decode_int(raw.get()).map_err(|_| wrong_patch_type())?);
            }
            "stock" => {
                patch.stock = Some(decode_int(raw.get()).map_err(|_| wrong_patch_type())?);
            }
            "active" => {
                patch.active =
                    Some(serde_json::from_str::<bool>(raw.get()).map_err(|_| wrong_patch_type())?);
            }
            _ => {}
        }
    }

    if let Some(name) = &patch.name {
        let name = name.trim().to_string();
        if !rules::valid_name(&name) {
            return Err("the product name must be 2 to 120 characters".to_string());
        }
        if !rules::plain_text(&name) {
            return Err("the product name must be plain text".to_string());
        }
        patch.name = Some(name);
    }
    if let Some(subtitle) = &patch.subtitle {
        let subtitle = subtitle.trim().to_string();
        if subtitle.chars().count() > rules::MAX_SUBTITLE_RUNES {
            return Err("the subtitle must stay under 161 characters".to_string());
        }
        patch.subtitle = Some(subtitle);
    }
    if let Some(description) = &patch.description {
        let description = description.trim().to_string();
        if description.chars().count() > rules::MAX_DESCRIPTION_RUNES {
            return Err("the description must stay under 4001 characters".to_string());
        }
        patch.description = Some(description);
    }
    if let Some(price) = patch.price_cents
        && !rules::valid_price(price)
    {
        return Err(format!(
            "the price must be between 1 and {} cents",
            rules::MAX_PRICE_CENTS
        ));
    }
    if let Some(stock) = patch.stock
        && !rules::valid_stock(stock)
    {
        return Err(format!(
            "the stock must be between 0 and {}",
            rules::MAX_STOCK
        ));
    }
    Ok(patch)
}

fn wrong_patch_type() -> String {
    "product patch has a value of the wrong type".to_string()
}

/// Trims, validates and normalises a draft in place, using the installed
/// product types for plugin drafts.
pub fn validate_product_draft(
    product: &mut ProductDraft,
    registry: Option<&RwLock<Registry>>,
) -> Result<(), String> {
    product.name = product.name.trim().to_string();
    if !rules::valid_name(&product.name) {
        return Err("the product name must be 2 to 120 characters".to_string());
    }
    if !rules::plain_text(&product.name) {
        return Err("the product name must be plain text".to_string());
    }
    product.slug = product.slug.trim().to_string();
    if product.slug.is_empty() || product.slug.len() > 120 || !SLUG_PATTERN.is_match(&product.slug)
    {
        return Err("the product slug must use lowercase letters, numbers and hyphens".to_string());
    }
    product.category_slug = product.category_slug.trim().to_string();
    if product.category_slug.is_empty() || !SLUG_PATTERN.is_match(&product.category_slug) {
        return Err("choose a valid category slug".to_string());
    }
    product.plugin_id = product.plugin_id.trim().to_string();
    if !PLUGIN_ID_PATTERN.is_match(&product.plugin_id) {
        return Err("choose a valid plugin id".to_string());
    }
    if !rules::valid_price(product.price_cents) {
        return Err(format!(
            "the price must be between 1 and {} cents",
            rules::MAX_PRICE_CENTS
        ));
    }
    if !rules::valid_stock(product.stock) {
        return Err(format!(
            "the stock must be between 0 and {}",
            rules::MAX_STOCK
        ));
    }
    if !rules::valid_image_path(product.image.trim()) {
        return Err("use an uploaded /api/images/ or /images/ path for the image".to_string());
    }
    product.description = product.description.trim().to_string();
    if product.description.chars().count() > rules::MAX_DESCRIPTION_RUNES {
        return Err("the description must stay under 4001 characters".to_string());
    }

    product.attributes = normalize_attributes(&product.attributes)?;

    let plugin_id = normalize_plugin_id(&product.plugin_id);
    product.plugin_id = plugin_id.clone();
    if plugin_id == CORE_PLUGIN_ID {
        if !product.attributes.is_empty() {
            return Err("a core product does not take plugin options".to_string());
        }
        return Ok(());
    }

    let Some(plugin) = registry_read(registry, |registry| registry.get(&plugin_id)).flatten()
    else {
        return Err(if registry.is_none() {
            "the installed plugin list is unavailable".to_string()
        } else {
            "choose an installed plugin".to_string()
        });
    };
    let Some(product_type) = plugin.manifest().product_type else {
        return Err("that plugin does not define a product type".to_string());
    };
    validate_product_attributes(&product_type, &product.attributes)
}

/// Trims attribute keys and values and reports the two structural problems.
pub fn normalize_attributes(
    attributes: &HashMap<String, Vec<String>>,
) -> Result<HashMap<String, Vec<String>>, String> {
    let (normalized, problems) = rules::normalize_attributes(attributes);
    for problem in problems {
        match problem.kind {
            rules::Kind::DuplicateOptionName => {
                return Err("option names must be unique".to_string());
            }
            rules::Kind::EmptyOptionName => {
                return Err("option names cannot be empty".to_string());
            }
            _ => {}
        }
    }
    Ok(normalized)
}

/// Maps product-rule problems to the admin-facing messages, first problem wins.
pub fn validate_product_attributes(
    product_type: &azahar_plugins::ProductType,
    attributes: &HashMap<String, Vec<String>>,
) -> Result<(), String> {
    let Some(problem) = rules::validate_attributes(product_type, attributes)
        .into_iter()
        .next()
    else {
        return Ok(());
    };
    let label = go_lower(&problem.label);
    match problem.kind {
        rules::Kind::MissingRequired => Err(format!("add at least one {label} value")),
        rules::Kind::SingleValue => Err(format!("{label} takes one value")),
        rules::Kind::InvalidText => Err(format!(
            "{label} must be plain text under {} characters",
            problem.limit + 1
        )),
        rules::Kind::TooManyValues => Err(format!("{label} takes at most 12 values")),
        rules::Kind::InvalidValue => Err(format!(
            "{label} values must be plain text under 81 characters"
        )),
        rules::Kind::DuplicateValue => Err(format!("{label} values must be unique")),
        _ => Err("remove options this product type does not define".to_string()),
    }
}

/// Go `strings.ToLower` uses the one-rune simple mapping.
fn go_lower(value: &str) -> String {
    value
        .chars()
        .map(|character| character.to_lowercase().next().unwrap_or(character))
        .collect()
}

// ---------------------------------------------------------------------------
// References and baselines
// ---------------------------------------------------------------------------

/// Checks that every referenced product, category and order exists and that
/// the order move is allowed.
pub async fn validate_plan_references(store: &Store, plan: &Plan) -> Result<(), ReferenceError> {
    for (index, action) in plan.actions.iter().enumerate() {
        let prefix = format!("action {}", index + 1);
        match action.action_type.as_str() {
            "product.update" | "product.archive" => {
                match store.product_by_id(&action.product_id, false).await {
                    Ok(_) => {}
                    Err(StoreError::NotFound) => {
                        return Err(invalid_plan(format!(
                            "{prefix}: product {} does not exist",
                            json::quote(&action.product_id)
                        ))
                        .into());
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            "product.create" => {
                let Some(product) = &action.product else {
                    return Err(
                        invalid_plan(format!("{prefix}: choose the product to create")).into(),
                    );
                };
                let category = match store.category_by_slug(&product.category_slug).await {
                    Ok(category) => category,
                    Err(StoreError::NotFound) => {
                        return Err(invalid_plan(format!(
                            "{prefix}: category {} does not exist",
                            json::quote(&product.category_slug)
                        ))
                        .into());
                    }
                    Err(error) => return Err(error.into()),
                };
                if category.plugin_id != normalize_plugin_id(&product.plugin_id) {
                    return Err(invalid_plan(format!(
                        "{prefix}: category {} belongs to another product type",
                        json::quote(&product.category_slug)
                    ))
                    .into());
                }
                match store.product_by_slug(&product.slug, false).await {
                    Ok(_) => {
                        return Err(invalid_plan(format!(
                            "{prefix}: slug {} is already in use",
                            json::quote(&product.slug)
                        ))
                        .into());
                    }
                    Err(StoreError::NotFound) => {}
                    Err(error) => return Err(error.into()),
                }
            }
            "order.status" => {
                let order = match store.order_by_id(&action.order_id).await {
                    Ok(order) => order,
                    Err(StoreError::NotFound) => {
                        return Err(invalid_plan(format!(
                            "{prefix}: order {} does not exist",
                            json::quote(&action.order_id)
                        ))
                        .into());
                    }
                    Err(error) => return Err(error.into()),
                };
                if !azahar_orderstatus::can_transition(&order.status, &action.status) {
                    return Err(invalid_plan(format!(
                        "{prefix}: order {} cannot move from {} to {}",
                        order.number, order.status, action.status
                    ))
                    .into());
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// The fields a code-mode product action can write, in Go struct order.
#[derive(Serialize)]
struct ProductBaselineFields<'a> {
    name: &'a str,
    subtitle: &'a str,
    description: &'a str,
    price_cents: i64,
    stock: i32,
    active: bool,
}

/// Fingerprint of exactly the fields a product action can change.
#[must_use]
pub fn product_fingerprint(product: &Product) -> String {
    let fields = ProductBaselineFields {
        name: &product.name,
        subtitle: &product.subtitle,
        description: &product.description,
        price_cents: product.price_cents,
        stock: product.stock,
        active: product.active,
    };
    // Go's `json.Marshal` sorts no struct fields, but escapes HTML.
    let mut object = json::Object::new();
    object.string_field("name", fields.name);
    object.string_field("subtitle", fields.subtitle);
    object.string_field("description", fields.description);
    object.int_field("price_cents", fields.price_cents);
    object.int_field("stock", i64::from(fields.stock));
    object.bool_field("active", fields.active);
    object.finish()
}

/// Records the exact values the model saw so apply can detect concurrent edits.
pub fn plan_baseline(
    plan: &Plan,
    products: &[Product],
    orders: &[crate::prompt::PromptOrder],
    theme: &Theme,
) -> Result<String, BaselineError> {
    let product_snapshots: HashMap<&str, String> = products
        .iter()
        .map(|product| (product.id.as_str(), product_fingerprint(product)))
        .collect();
    let order_snapshots: HashMap<&str, &str> = orders
        .iter()
        .map(|order| (order.id.as_str(), order.status.as_str()))
        .collect();
    let theme_json = azahar_storefront::encode(theme)
        .map_err(|error| BaselineError::Encode(error.to_string()))?;

    let mut baseline = BTreeMap::new();
    for action in &plan.actions {
        match action.action_type.as_str() {
            "product.update" | "product.archive" => {
                let Some(value) = product_snapshots.get(action.product_id.as_str()) else {
                    return Err(invalid_plan(format!(
                        "product {} was not in this preview; preview the change again",
                        json::quote(&action.product_id)
                    ))
                    .into());
                };
                baseline.insert(format!("product:{}", action.product_id), value.clone());
            }
            "order.status" => {
                let Some(value) = order_snapshots.get(action.order_id.as_str()) else {
                    return Err(invalid_plan(format!(
                        "order {} was not in this preview; preview the change again",
                        json::quote(&action.order_id)
                    ))
                    .into());
                };
                baseline.insert(format!("order:{}", action.order_id), (*value).to_string());
            }
            "theme.update" => {
                baseline.insert("theme".to_string(), theme_json.clone());
            }
            _ => {}
        }
    }
    Ok(json::string_map(&baseline))
}

/// The first field problem in sorted-key order: `key: message`.
#[must_use]
pub fn first_field_message(fields: &BTreeMap<String, String>) -> Option<String> {
    fields
        .iter()
        .next()
        .map(|(key, message)| format!("{key}: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use azahar_storefront::Theme;

    fn raw(text: &str) -> Box<RawValue> {
        RawValue::from_string(text.to_string()).expect("valid raw")
    }

    #[test]
    fn parses_every_action_kind() {
        let plan = parse_plan(
            r#"{
                "summary":"Change things.",
                "actions":[
                    {"type":"theme.update","patch":{"hero_heading":"A shorter shelf."}},
                    {"type":"product.update","product_id":"prd_1","patch":{"price_cents":4200}},
                    {"type":"product.archive","product_id":"prd_2"},
                    {"type":"product.create","product":{"name":"Rose balm","slug":"rose-balm","price_cents":3200,"category_slug":"body","plugin_id":"oils","stock":4,"image":"/images/rose.webp","description":"A balm.","attributes":{"size":["50 ml"]}}},
                    {"type":"order.status","order_id":"ord_1","status":"shipped"}
                ]
            }"#,
        )
        .expect("plan");
        assert_eq!(plan.actions.len(), 5);
        assert_eq!(
            plan.actions[3].product.as_ref().expect("draft").slug,
            "rose-balm"
        );
    }

    #[test]
    fn parses_fenced_plan_and_rejects_trailing_data() {
        assert!(parse_plan("```json\n{\"summary\":\"A.\",\"actions\":[]}\n```").is_ok());
        assert!(parse_plan("{\"summary\":\"A.\",\"actions\":[]} {}").is_err());
        assert!(parse_plan("").is_err());
        assert!(parse_plan("null").is_ok());
    }

    #[test]
    fn rejects_unknown_and_mistyped_fields() {
        for raw in [
            r#"{"summary":"A.","actions":[{"type":"order.cancel","order_id":"o1"}]}"#,
            r#"{"summary":"A.","actions":[{"type":"theme.update","patch":{},"product_id":"p"}]}"#,
            r#"{"summary":"A.","actions":[{"type":"product.update","product_id":"p"}]}"#,
            r#"{"summary":"A.","actions":[],"code":"rm -rf"}"#,
            r#"[]"#,
            r#"{"summary":"A.","actions":[{"type":"order.status","order_id":"o","status":"lost"}]}"#,
            r#"{"summary":"A.","actions":[{"type":"order.status","order_id":"o","status":"shipped","price_cents":1}]}"#,
        ] {
            assert!(parse_plan(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn nulls_and_wrong_types_follow_go() {
        assert!(parse_plan(r#"{"summary":null,"actions":null}"#).is_ok());
        assert!(parse_plan(r#"{"summary":1,"actions":[]}"#).is_err());
        assert!(parse_plan(r#"{"summary":"A.","actions":[null]}"#).is_err());
        assert!(parse_plan(r#"{"summary":"A.","actions":[{"type":null}]}"#).is_err());
        // product_id null decodes as the empty string and fails shape
        // validation, not parsing.
        let plan = parse_plan(
            r#"{"summary":"A.","actions":[{"type":"product.archive","product_id":null}]}"#,
        )
        .expect("parse");
        assert_eq!(plan.actions[0].product_id, "");
    }

    #[test]
    fn round_trips_the_encoded_plan() {
        let plan = parse_plan(
            r#"{"summary":"A.","actions":[{"type":"theme.update","patch":{"radius":4,"radius":6}},{"type":"product.archive","product_id":"prd_1"}]}"#,
        )
        .expect("plan");
        let encoded = encode_plan(&plan);
        assert_eq!(
            encoded,
            r#"{"summary":"A.","actions":[{"type":"theme.update","patch":{"radius":4,"radius":6}},{"type":"product.archive","product_id":"prd_1"}]}"#
        );
        assert_eq!(parse_plan(&encoded).expect("again"), plan);
    }

    #[test]
    fn theme_patch_allowlist_and_validation() {
        let theme = Theme::default();
        let next = apply_theme_patch(
            &theme,
            Some(&raw(
                r##"{"hero_heading":"A shorter shelf.","light":{"accent":"#123456"}}"##,
            )),
        )
        .expect("patch");
        assert_eq!(next.hero_heading, "A shorter shelf.");
        assert_eq!(next.light.accent, "#123456");

        for patch in [
            r#"{"css":"body{}"}"#,
            r#"{"updated_at":"2026-01-01T00:00:00Z"}"#,
            r##"{"light":{"background":"#ffffff"}}"##,
            r#"{"hero_heading":null}"#,
            r#"{"hero_heading":""}"#,
            r#"{"font_scale":4}"#,
            r#"{"contact_email":"not-an-email"}"#,
            r#"{"radius":"4"}"#,
        ] {
            assert!(
                apply_theme_patch(&Theme::default(), Some(&raw(patch))).is_err(),
                "{patch}"
            );
        }
        assert_eq!(
            apply_theme_patch(&theme, None).expect_err("missing"),
            "theme.update needs a patch object"
        );
    }

    #[test]
    fn product_patch_validation() {
        let patch = parse_product_patch(Some(&raw(
            r#"{"price_cents":4200,"stock":9,"active":false,"name":"New name","subtitle":"Short","description":"Long enough."}"#,
        )))
        .expect("patch");
        assert_eq!(patch.price_cents, Some(4200));
        assert_eq!(patch.active, Some(false));
        assert_eq!(patch.name.as_deref(), Some("New name"));

        for patch in [
            r#"{}"#,
            r#"{"price_cents":0}"#,
            r#"{"price_cents":100000000}"#,
            r#"{"stock":-1}"#,
            r#"{"stock":100001}"#,
            r#"{"name":"x"}"#,
            r#"{"slug":"new-slug"}"#,
            r#"{"price_cents":1.5}"#,
            r#"{"price_cents":1e2}"#,
            r#"{"active":null}"#,
        ] {
            assert!(parse_product_patch(Some(&raw(patch))).is_err(), "{patch}");
        }
    }

    #[test]
    fn shape_validation_messages_are_exact() {
        let mut plan = Plan {
            summary: String::new(),
            actions: vec![Action {
                action_type: "product.archive".to_string(),
                product_id: "prd_1".to_string(),
                ..Action::default()
            }],
        };
        assert_eq!(
            validate_plan_shape(&mut plan, &Theme::default(), None)
                .expect_err("summary")
                .to_string(),
            "the plan summary must be 1 to 300 characters"
        );

        let mut plan = Plan {
            summary: "Change.".to_string(),
            actions: vec![],
        };
        assert_eq!(
            validate_plan_shape(&mut plan, &Theme::default(), None)
                .expect_err("actions")
                .to_string(),
            "the instruction cannot be planned with the available actions"
        );

        let mut plan = Plan {
            summary: "Change.".to_string(),
            actions: vec![Action {
                action_type: "theme.update".to_string(),
                patch: Some(raw(r#"{"css":"x"}"#)),
                ..Action::default()
            }],
        };
        assert_eq!(
            validate_plan_shape(&mut plan, &Theme::default(), None)
                .expect_err("patch")
                .to_string(),
            "action 1: \"css\" is not a theme field"
        );
    }

    #[test]
    fn fingerprints_and_baselines_are_stable() {
        let product = Product {
            id: "prd_1".to_string(),
            name: "A <name>".to_string(),
            subtitle: "Sub".to_string(),
            description: "Desc".to_string(),
            price_cents: 4200,
            stock: 9,
            active: true,
            ..Product::default()
        };
        assert_eq!(
            product_fingerprint(&product),
            r#"{"name":"A \u003cname\u003e","subtitle":"Sub","description":"Desc","price_cents":4200,"stock":9,"active":true}"#
        );

        let order = crate::prompt::PromptOrder {
            id: "ord_1".to_string(),
            number: "AZ-1".to_string(),
            status: "pending".to_string(),
            updated_at: String::new(),
        };
        let plan = Plan {
            summary: "A.".to_string(),
            actions: vec![
                Action {
                    action_type: "product.update".to_string(),
                    product_id: "prd_1".to_string(),
                    ..Action::default()
                },
                Action {
                    action_type: "order.status".to_string(),
                    order_id: "ord_1".to_string(),
                    status: "confirmed".to_string(),
                    ..Action::default()
                },
            ],
        };
        let baseline =
            plan_baseline(&plan, &[product], &[order], &Theme::default()).expect("baseline");
        let value: serde_json::Value = serde_json::from_str(&baseline).expect("json");
        assert!(value.get("product:prd_1").is_some());
        assert_eq!(
            value.get("order:ord_1").and_then(serde_json::Value::as_str),
            Some("pending")
        );
    }

    #[test]
    fn results_encode_empty_as_an_array() {
        assert_eq!(encode_results(&[]), r#"{"results":[]}"#);
        assert_eq!(
            encode_results(&[ActionResult {
                action_type: "product.archive".to_string(),
                ok: false,
                message: "no".to_string(),
            }]),
            r#"{"results":[{"type":"product.archive","ok":false,"message":"no"}]}"#
        );
    }

    #[test]
    fn integers_follow_go_number_acceptance() {
        assert_eq!(decode_int("-0"), Ok(0));
        assert_eq!(decode_int("100"), Ok(100));
        assert_eq!(decode_int("null"), Ok(0));
        assert!(decode_int("1.0").is_err());
        assert!(decode_int("1e2").is_err());
        assert!(decode_int("9223372036854775808").is_err());
        assert!(decode_int("\"1\"").is_err());
    }

    #[test]
    fn first_field_message_uses_the_sorted_first_key() {
        let fields = BTreeMap::from([
            ("zeta".to_string(), "Z".to_string()),
            ("alpha".to_string(), "A".to_string()),
        ]);
        assert_eq!(first_field_message(&fields).as_deref(), Some("alpha: A"));
    }
}
