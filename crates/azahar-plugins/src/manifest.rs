//! Manifest data a plugin declares to the core.
//!
//! Field names and omission rules follow the Go JSON tags in
//! `backend/internal/plugins/plugin.go` exactly, because the HTTP layer serves
//! these structs to the admin UI.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OptionField {
    pub key: String,
    pub label: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub required: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub help: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProductType {
    pub label: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub summary: String,
    /// Go's `Fields` may be nil (marshals as `null`) or an empty non-nil slice
    /// (marshals as `[]`). A Rust `Vec` cannot distinguish the two, so an empty
    /// vector always serialises as `null`, the Go zero value. No in-tree
    /// plugin ships an explicitly empty non-nil field list.
    #[serde(
        serialize_with = "serialise_fields",
        deserialize_with = "deserialise_fields"
    )]
    pub fields: Vec<OptionField>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingField {
    pub key: String,
    pub label: String,
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub help: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub placeholder: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<SettingOption>,
    pub required: bool,
    #[serde(skip_serializing_if = "is_zero_i32")]
    pub max_length: i32,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub default: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Category {
    pub slug: String,
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(skip_serializing_if = "is_zero_i32")]
    pub sort_order: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StorefrontPage {
    pub path: String,
    pub label: String,
    #[serde(skip_serializing_if = "is_zero_i32")]
    pub sort_order: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub author: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_type: Option<ProductType>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<Category>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub storefront_pages: Vec<StorefrontPage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<SettingField>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub admin_path: String,
}

/// A route a plugin wants mounted under `/api/plugins/<id>`.
///
/// The Go struct carries a `func(*Context)` handler. HTTP types stay in the
/// HTTP crate, so this port carries the registration metadata only. The HTTP
/// layer owns a per-plugin dispatch table keyed by `(method, pattern, admin)`
/// and looks the handler up there when it mounts the route.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Route {
    #[serde(alias = "method")]
    pub method: String,
    #[serde(alias = "pattern")]
    pub pattern: String,
    #[serde(alias = "admin")]
    pub admin: bool,
}

/// One database migration contributed by a plugin.
///
/// Go has no JSON tags on this struct, so the wire form keeps the exported
/// field names, including the all-caps `SQL`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Migration {
    #[serde(rename = "Version", alias = "version")]
    pub version: String,
    #[serde(rename = "SQL", alias = "sql")]
    pub sql: String,
}

fn is_zero_i32(value: &i32) -> bool {
    *value == 0
}

// Go's `Fields` has no omitempty: a nil slice marshals as null, an empty one as
// []. The zero-value manifest is the nil case, so keep that wire shape.
fn serialise_fields<S>(fields: &[OptionField], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    if fields.is_empty() {
        serializer.serialize_none()
    } else {
        fields.serialize(serializer)
    }
}

fn deserialise_fields<'de, D>(deserializer: D) -> Result<Vec<OptionField>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<Vec<OptionField>>::deserialize(deserializer).map(Option::unwrap_or_default)
}
