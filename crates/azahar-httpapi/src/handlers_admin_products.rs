//! Admin product writes: list, create, update and archive.
//!
//! Ported from `handlers_admin_products.go` and `product_options.go`. The
//! validation order, field messages and audit details follow the Go handlers
//! line by line, including the partial-patch filtering that lets a legacy row
//! accept a stock-only change.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use azahar_ids::new as new_id;
use azahar_plugins::ProductType;
use azahar_productrules as rules;
use azahar_store::pricing::{
    SALE_END_MISSING, SALE_END_NOT_AFTER_START, SALE_PRICE_INVALID, SALE_PRICE_MISSING,
    SALE_START_MISSING, SaleWindow,
};
use azahar_store::{AuditEntry, Product, StoreError, normalize_plugin_id, parse_time};
use chrono::Utc;
use serde::Deserialize;
use serde::de::Deserializer;
use serde_json::json;

use crate::dto::to_product_async;
use crate::extract::{JsonBody, RawFields};
use crate::handlers_admin::audit_detail;
use crate::middleware::{CurrentUser, internal_error};
use crate::respond::{ApiProblem, json_response, problem, problem_fields};
use crate::state::AppState;
use crate::validate::{fields as field_map, rune_len, trim_all, valid_slug};

// ---------------------------------------------------------------------------
// Go-compatible scalar decoding
// ---------------------------------------------------------------------------

/// A Go string field: JSON `null` decodes to the zero string without error.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct GoText(String);

impl<'de> Deserialize<'de> for GoText {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(
            Option::<String>::deserialize(deserializer)?.unwrap_or_default(),
        ))
    }
}

/// A Go `int`/`int64` field: JSON `null` leaves the zero value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct GoInt(i64);

impl<'de> Deserialize<'de> for GoInt {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(
            Option::<i64>::deserialize(deserializer)?.unwrap_or_default(),
        ))
    }
}

/// A Go `bool` field: JSON `null` leaves `false`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct GoBool(bool);

impl<'de> Deserialize<'de> for GoBool {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(
            Option::<bool>::deserialize(deserializer)?.unwrap_or_default(),
        ))
    }
}

fn texts(values: Option<Vec<GoText>>) -> Vec<String> {
    values
        .unwrap_or_default()
        .into_iter()
        .map(|value| value.0)
        .collect()
}

fn attribute_map(
    values: Option<HashMap<String, Option<Vec<GoText>>>>,
) -> HashMap<String, Vec<String>> {
    values
        .unwrap_or_default()
        .into_iter()
        .map(|(key, value)| (key, texts(value)))
        .collect()
}

// ---------------------------------------------------------------------------
// Payloads
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProductCreatePayload {
    name: GoText,
    slug: GoText,
    subtitle: GoText,
    description: GoText,
    price_cents: GoInt,
    compare_at_cents: Option<GoInt>,
    sale_price_cents: Option<GoInt>,
    sale_starts_at: GoText,
    sale_ends_at: GoText,
    size: GoText,
    category_slug: GoText,
    concerns: Option<Vec<GoText>>,
    ingredients: Option<Vec<GoText>>,
    how_to: GoText,
    image: GoText,
    gallery: Option<Vec<GoText>>,
    stock: GoInt,
    featured: GoBool,
    active: Option<bool>,
    plugin_id: GoText,
    attributes: Option<HashMap<String, Option<Vec<GoText>>>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProductPatch {
    name: Option<String>,
    slug: Option<String>,
    subtitle: Option<String>,
    description: Option<String>,
    price_cents: Option<i64>,
    compare_at_cents: Option<i64>,
    sale_price_cents: Option<i64>,
    sale_starts_at: Option<String>,
    sale_ends_at: Option<String>,
    size: Option<String>,
    category_slug: Option<String>,
    concerns: Option<Vec<GoText>>,
    ingredients: Option<Vec<GoText>>,
    how_to: Option<String>,
    image: Option<String>,
    gallery: Option<Vec<GoText>>,
    stock: Option<i64>,
    featured: Option<bool>,
    active: Option<bool>,
    plugin_id: Option<String>,
    attributes: Option<HashMap<String, Option<Vec<GoText>>>>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /api/admin/products`.
pub async fn list(State(state): State<AppState>) -> Result<Response, ApiProblem> {
    let products = state
        .store
        .list_products(&azahar_store::ProductFilter {
            include_inactive: true,
            limit: 500,
            ..azahar_store::ProductFilter::default()
        })
        .await
        .map_err(|_| internal_error())?;
    let registry = state
        .plugins
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(json_response(
        200,
        &json!({"products": crate::dto::to_products(&registry, &products, true)}),
    ))
}

/// `POST /api/admin/products`.
pub async fn create(
    State(state): State<AppState>,
    CurrentUser(current): CurrentUser,
    body: JsonBody<ProductCreatePayload>,
) -> Result<Response, ApiProblem> {
    let payload = body.payload;
    let now = Utc::now();
    let (sale_price, sale_starts, sale_ends, sale_fields) = parse_sale_window(
        payload.sale_price_cents.map(|value| value.0),
        &payload.sale_starts_at.0,
        &payload.sale_ends_at.0,
    );
    let raw_attributes = attribute_map(payload.attributes);
    let mut product = Product {
        id: new_id("prd"),
        name: payload.name.0.trim().to_string(),
        slug: payload.slug.0.trim().to_string(),
        subtitle: payload.subtitle.0.trim().to_string(),
        description: payload.description.0.trim().to_string(),
        price_cents: payload.price_cents.0,
        compare_at_cents: payload.compare_at_cents.map(|value| value.0),
        sale_price_cents: sale_price,
        sale_starts_at: sale_starts,
        sale_ends_at: sale_ends,
        currency: "DZD".to_string(),
        size: payload.size.0.trim().to_string(),
        concerns: trim_all(&texts(payload.concerns)),
        ingredients: trim_all(&texts(payload.ingredients)),
        how_to: payload.how_to.0.trim().to_string(),
        image: payload.image.0.trim().to_string(),
        gallery: trim_all(&texts(payload.gallery)),
        stock: i32::try_from(payload.stock.0).unwrap_or(if payload.stock.0 < 0 {
            i32::MIN
        } else {
            i32::MAX
        }),
        featured: payload.featured.0,
        active: true,
        plugin_id: normalize_plugin_id(payload.plugin_id.0.trim()),
        attributes: normalize_attributes(&raw_attributes),
        created_at: Some(now),
        updated_at: Some(now),
        ..Product::default()
    };
    if let Some(active) = payload.active {
        product.active = active;
    }

    let mut fields = validate_product(&product);
    fields.extend(sale_fields);
    fields.extend(validate_product_attributes(
        &state,
        &product.plugin_id,
        &raw_attributes,
    ));
    match state
        .store
        .category_by_slug(payload.category_slug.0.trim())
        .await
    {
        Ok(category) => {
            if category.plugin_id != normalize_plugin_id(&product.plugin_id) {
                fields.insert(
                    "category_slug".to_string(),
                    "Choose a category that belongs to this product type.".to_string(),
                );
            } else {
                product.category = category;
            }
        }
        Err(StoreError::NotFound) => {
            fields.insert(
                "category_slug".to_string(),
                "Choose a category.".to_string(),
            );
        }
        Err(_) => return Err(internal_error()),
    }
    if !fields.is_empty() {
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            fields,
        ));
    }

    if slug_taken(&state, &product.slug, "").await? {
        return Err(slug_conflict());
    }

    let detail = audit_detail(&json!({"slug": product.slug}));
    let tx = state
        .store
        .begin_write()
        .await
        .map_err(|_| internal_error())?;
    let written = state.store.create_product(tx.conn(), &product).await;
    let audited = match written {
        Ok(()) => {
            state
                .store
                .insert_audit(
                    tx.conn(),
                    &AuditEntry {
                        id: new_id("aud"),
                        actor_user_id: current.id.clone(),
                        action: "product.create".to_string(),
                        entity_type: "product".to_string(),
                        entity_id: product.id.clone(),
                        detail,
                        created_at: Some(now),
                    },
                )
                .await
        }
        Err(error) => Err(error),
    };
    match audited {
        Ok(()) => tx.commit().await.map_err(|_| internal_error())?,
        Err(error) => {
            let _ = tx.rollback().await;
            if error.is_unique_violation() {
                return Err(slug_conflict());
            }
            return Err(internal_error());
        }
    }

    Ok(json_response(
        201,
        &json!({"product": to_product_async(&state, &product, true).await}),
    ))
}

/// `PATCH /api/admin/products/{id}`.
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    CurrentUser(current): CurrentUser,
    body: Result<JsonBody<ProductPatch>, ApiProblem>,
) -> Result<Response, ApiProblem> {
    // Go loads the product before it reads the body, so a missing id wins over
    // a malformed patch.
    let existing = match state.store.product_by_id(&id, false).await {
        Ok(product) => product,
        Err(StoreError::NotFound) => {
            return Err(problem(404, "not_found", "Product not found."));
        }
        Err(_) => return Err(internal_error()),
    };
    let body = body?;
    let patch = body.payload;
    let raw = &body.raw;
    let mut product = existing;
    let mut fields: HashMap<String, String> = HashMap::new();

    macro_rules! patch_text {
        ($field:ident, $key:literal) => {
            if raw.contains_key($key) {
                match &patch.$field {
                    None => {
                        fields.insert($key.to_string(), "This field cannot be empty.".to_string());
                    }
                    Some(value) => product.$field = value.trim().to_string(),
                }
            }
        };
    }

    patch_text!(name, "name");
    patch_text!(slug, "slug");
    patch_text!(subtitle, "subtitle");
    patch_text!(description, "description");
    patch_text!(size, "size");
    patch_text!(how_to, "how_to");
    patch_text!(image, "image");

    if raw.contains_key("price_cents") {
        match patch.price_cents {
            None => {
                fields.insert(
                    "price_cents".to_string(),
                    "This field cannot be empty.".to_string(),
                );
            }
            Some(value) => product.price_cents = value,
        }
    }
    if raw.contains_key("compare_at_cents") {
        product.compare_at_cents = patch.compare_at_cents;
    }
    if raw.contains_key("sale_price_cents") {
        match patch.sale_price_cents {
            None | Some(0) => product.sale_price_cents = None,
            Some(value) => {
                product.sale_price_cents = Some(value);
                if !product.sale_window().price_positive() {
                    fields.insert(
                        "sale_price_cents".to_string(),
                        "Enter a sale price above 0.".to_string(),
                    );
                }
            }
        }
    }
    if raw.contains_key("sale_starts_at") {
        match patch.sale_starts_at.as_deref().map(str::trim) {
            None | Some("") => product.sale_starts_at = None,
            Some(text) => match parse_time(text) {
                Some(parsed) => product.sale_starts_at = Some(parsed),
                None => {
                    fields.insert(
                        "sale_starts_at".to_string(),
                        "Use a valid date and time.".to_string(),
                    );
                }
            },
        }
    }
    if raw.contains_key("sale_ends_at") {
        match patch.sale_ends_at.as_deref().map(str::trim) {
            None | Some("") => product.sale_ends_at = None,
            Some(text) => match parse_time(text) {
                Some(parsed) => product.sale_ends_at = Some(parsed),
                None => {
                    fields.insert(
                        "sale_ends_at".to_string(),
                        "Use a valid date and time.".to_string(),
                    );
                }
            },
        }
    }
    if product.sale_price_cents.is_none() {
        product.sale_starts_at = None;
        product.sale_ends_at = None;
    }
    if raw.contains_key("plugin_id") {
        match &patch.plugin_id {
            None => {
                fields.insert(
                    "plugin_id".to_string(),
                    "This field cannot be empty.".to_string(),
                );
            }
            Some(value) => product.plugin_id = normalize_plugin_id(value.trim()),
        }
    }
    if raw.contains_key("category_slug") {
        match &patch.category_slug {
            None => {
                fields.insert(
                    "category_slug".to_string(),
                    "This field cannot be empty.".to_string(),
                );
            }
            Some(value) => match state.store.category_by_slug(value.trim()).await {
                Ok(category) => {
                    if category.plugin_id != normalize_plugin_id(&product.plugin_id) {
                        fields.insert(
                            "category_slug".to_string(),
                            "Choose a category that belongs to this product type.".to_string(),
                        );
                    } else {
                        product.category = category;
                    }
                }
                Err(StoreError::NotFound) => {
                    fields.insert(
                        "category_slug".to_string(),
                        "Choose a category.".to_string(),
                    );
                }
                Err(_) => return Err(internal_error()),
            },
        }
    }
    if raw.contains_key("concerns") {
        match &patch.concerns {
            None => {
                fields.insert(
                    "concerns".to_string(),
                    "This field cannot be empty.".to_string(),
                );
            }
            Some(values) => product.concerns = trim_all(&texts(Some(values.clone()))),
        }
    }
    if raw.contains_key("ingredients") {
        match &patch.ingredients {
            None => {
                fields.insert(
                    "ingredients".to_string(),
                    "This field cannot be empty.".to_string(),
                );
            }
            Some(values) => product.ingredients = trim_all(&texts(Some(values.clone()))),
        }
    }
    if raw.contains_key("gallery") {
        match &patch.gallery {
            None => {
                fields.insert(
                    "gallery".to_string(),
                    "This field cannot be empty.".to_string(),
                );
            }
            Some(values) => product.gallery = trim_all(&texts(Some(values.clone()))),
        }
    }
    if raw.contains_key("stock") {
        match patch.stock {
            None => {
                fields.insert(
                    "stock".to_string(),
                    "This field cannot be empty.".to_string(),
                );
            }
            Some(value) => {
                product.stock =
                    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX });
            }
        }
    }
    if raw.contains_key("featured") {
        match patch.featured {
            None => {
                fields.insert(
                    "featured".to_string(),
                    "This field cannot be empty.".to_string(),
                );
            }
            Some(value) => product.featured = value,
        }
    }
    if raw.contains_key("active") {
        match patch.active {
            None => {
                fields.insert(
                    "active".to_string(),
                    "This field cannot be empty.".to_string(),
                );
            }
            Some(value) => product.active = value,
        }
    }

    let mut attributes_for_validation = product.attributes.clone();
    if raw.contains_key("attributes") {
        match &patch.attributes {
            None => {
                fields.insert(
                    "attributes".to_string(),
                    "This field cannot be empty.".to_string(),
                );
            }
            Some(values) => {
                let raw_attributes = attribute_map(Some(values.clone()));
                attributes_for_validation = raw_attributes.clone();
                product.attributes = normalize_attributes(&raw_attributes);
            }
        }
    }
    if !fields.is_empty() {
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            fields,
        ));
    }

    fields.extend(filter_field_problems(validate_product(&product), raw));
    if has_any(raw, &["category_slug", "plugin_id"])
        && (product.category.id.is_empty()
            || product.category.plugin_id != normalize_plugin_id(&product.plugin_id))
    {
        fields.insert(
            "category_slug".to_string(),
            "Choose a category that belongs to this product type.".to_string(),
        );
    }
    if has_any(raw, &["attributes", "plugin_id"]) {
        fields.extend(validate_product_attributes(
            &state,
            &product.plugin_id,
            &attributes_for_validation,
        ));
    }
    if !fields.is_empty() {
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            fields,
        ));
    }

    if slug_taken(&state, &product.slug, &product.id).await? {
        return Err(slug_conflict());
    }

    let now = Utc::now();
    product.updated_at = Some(now);
    let mut keys: Vec<&String> = raw.keys().collect();
    keys.sort();
    let detail = audit_detail(&json!({"slug": product.slug, "fields": keys}));
    let tx = state
        .store
        .begin_write()
        .await
        .map_err(|_| internal_error())?;
    let written = state.store.update_product(tx.conn(), &product).await;
    let audited = match written {
        Ok(()) => {
            state
                .store
                .insert_audit(
                    tx.conn(),
                    &AuditEntry {
                        id: new_id("aud"),
                        actor_user_id: current.id.clone(),
                        action: "product.update".to_string(),
                        entity_type: "product".to_string(),
                        entity_id: product.id.clone(),
                        detail,
                        created_at: Some(now),
                    },
                )
                .await
        }
        Err(error) => Err(error),
    };
    match audited {
        Ok(()) => tx.commit().await.map_err(|_| internal_error())?,
        Err(error) => {
            let _ = tx.rollback().await;
            if error.is_unique_violation() {
                return Err(slug_conflict());
            }
            return Err(internal_error());
        }
    }

    Ok(json_response(
        200,
        &json!({"product": to_product_async(&state, &product, true).await}),
    ))
}

/// `DELETE /api/admin/products/{id}`.
pub async fn archive(
    State(state): State<AppState>,
    Path(id): Path<String>,
    CurrentUser(current): CurrentUser,
) -> Result<Response, ApiProblem> {
    let product = match state.store.product_by_id(&id, false).await {
        Ok(product) => product,
        Err(StoreError::NotFound) => {
            return Err(problem(404, "not_found", "Product not found."));
        }
        Err(_) => return Err(internal_error()),
    };

    let now = Utc::now();
    let detail = audit_detail(&json!({"slug": product.slug}));
    let tx = state
        .store
        .begin_write()
        .await
        .map_err(|_| internal_error())?;
    let archived = state
        .store
        .set_product_active(tx.conn(), &product.id, false, now)
        .await;
    let audited = match archived {
        Ok(()) => {
            state
                .store
                .insert_audit(
                    tx.conn(),
                    &AuditEntry {
                        id: new_id("aud"),
                        actor_user_id: current.id.clone(),
                        action: "product.archive".to_string(),
                        entity_type: "product".to_string(),
                        entity_id: product.id.clone(),
                        detail,
                        created_at: Some(now),
                    },
                )
                .await
        }
        Err(error) => Err(error),
    };
    match audited {
        Ok(()) => tx.commit().await.map_err(|_| internal_error())?,
        Err(_) => {
            let _ = tx.rollback().await;
            return Err(internal_error());
        }
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn slug_conflict() -> ApiProblem {
    problem_fields(
        409,
        "conflict",
        "That slug is already in use.",
        field_map(&[("slug", "This slug is already in use.")]),
    )
}

async fn slug_taken(state: &AppState, slug: &str, except_id: &str) -> Result<bool, ApiProblem> {
    match state.store.product_by_slug(slug, false).await {
        Ok(product) => Ok(product.id != except_id),
        Err(StoreError::NotFound) => Ok(false),
        Err(_) => Err(internal_error()),
    }
}

fn parse_sale_window(
    sale_price: Option<i64>,
    starts: &str,
    ends: &str,
) -> (
    Option<i64>,
    azahar_store::Timestamp,
    azahar_store::Timestamp,
    HashMap<String, String>,
) {
    let mut fields = HashMap::new();
    let starts = starts.trim();
    let ends = ends.trim();
    let Some(sale_price) = sale_price else {
        if !starts.is_empty() || !ends.is_empty() {
            fields.insert(
                "sale_price_cents".to_string(),
                "Add a sale price to use a sale window.".to_string(),
            );
        }
        return (None, None, None, fields);
    };
    if sale_price == 0 {
        return (None, None, None, fields);
    }
    let window = SaleWindow {
        price_cents: Some(sale_price),
        starts_at: None,
        ends_at: None,
    };
    if !window.price_positive() {
        fields.insert(
            "sale_price_cents".to_string(),
            "Enter a sale price above 0.".to_string(),
        );
        return (None, None, None, fields);
    }
    let start = parse_time(starts);
    if start.is_none() {
        fields.insert(
            "sale_starts_at".to_string(),
            "Choose when the sale starts.".to_string(),
        );
    }
    let end = parse_time(ends);
    if end.is_none() {
        fields.insert(
            "sale_ends_at".to_string(),
            "Choose when the sale ends.".to_string(),
        );
    }
    let window = SaleWindow {
        price_cents: Some(sale_price),
        starts_at: start,
        ends_at: end,
    };
    if start.is_some() && end.is_some() && !window.ends_after_starts() {
        fields.insert(
            "sale_ends_at".to_string(),
            "The sale must end after it starts.".to_string(),
        );
    }
    (Some(sale_price), start, end, fields)
}

fn has_any(raw: &RawFields, keys: &[&str]) -> bool {
    keys.iter().any(|key| raw.contains_key(*key))
}

fn filter_field_problems(
    problems: HashMap<String, String>,
    raw: &RawFields,
) -> HashMap<String, String> {
    let sale_touched = has_any(raw, &["sale_price_cents", "sale_starts_at", "sale_ends_at"]);
    problems
        .into_iter()
        .filter(|(key, _)| {
            has_any(raw, &[key.as_str()]) || (key.starts_with("sale_") && sale_touched)
        })
        .collect()
}

pub(crate) fn normalize_attributes(
    attributes: &HashMap<String, Vec<String>>,
) -> HashMap<String, Vec<String>> {
    rules::normalize_attributes(attributes).0
}

fn validate_product_attributes(
    state: &AppState,
    plugin_id: &str,
    attributes: &HashMap<String, Vec<String>>,
) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    let plugin_id = plugin_id.trim();
    let plugin_id = if plugin_id.is_empty() {
        azahar_store::CORE_PLUGIN_ID
    } else {
        plugin_id
    };
    if plugin_id == azahar_store::CORE_PLUGIN_ID {
        if !attributes.is_empty() {
            fields.insert(
                "attributes".to_string(),
                "A core product does not take plugin options.".to_string(),
            );
        }
        return fields;
    }
    let registry = state
        .plugins
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(plugin) = registry.get(plugin_id) else {
        fields.insert(
            "plugin_id".to_string(),
            "Choose an installed plugin.".to_string(),
        );
        return fields;
    };
    let Some(product_type) = plugin.manifest().product_type else {
        fields.insert(
            "plugin_id".to_string(),
            "That plugin does not define a product type.".to_string(),
        );
        return fields;
    };
    product_option_fields(&product_type, attributes)
}

fn product_option_fields(
    product_type: &ProductType,
    attributes: &HashMap<String, Vec<String>>,
) -> HashMap<String, String> {
    let (normalized, key_problems) = rules::normalize_attributes(attributes);
    let mut fields = attribute_fields(&rules::validate_attributes(product_type, &normalized));
    for (key, message) in option_key_fields(&key_problems) {
        fields.insert(key, message);
    }
    fields
}

fn option_key_fields(problems: &[rules::Problem]) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    for problem in problems {
        match problem.kind {
            rules::Kind::EmptyOptionName => {
                fields.insert(
                    "attributes".to_string(),
                    "Give every option a name.".to_string(),
                );
            }
            rules::Kind::DuplicateOptionName => {
                fields.insert(
                    format!("attributes.{}", problem.field),
                    "Each option name can appear once.".to_string(),
                );
            }
            _ => {}
        }
    }
    fields
}

fn attribute_fields(problems: &[rules::Problem]) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    for problem in problems {
        if problem.kind == rules::Kind::UnknownField {
            fields.insert(
                "attributes".to_string(),
                "Remove options this product type does not define.".to_string(),
            );
            continue;
        }
        let key = format!("attributes.{}", problem.field);
        match problem.kind {
            rules::Kind::MissingRequired => {
                fields.insert(
                    key,
                    format!("Add at least one {} value.", problem.label.to_lowercase()),
                );
            }
            rules::Kind::SingleValue => {
                fields.insert(key, "Use one value for this field.".to_string());
            }
            rules::Kind::InvalidText => {
                fields.insert(
                    key,
                    format!("Use plain text under {} characters.", problem.limit + 1),
                );
            }
            rules::Kind::TooManyValues => {
                fields.insert(key, "Use at most 12 values.".to_string());
            }
            rules::Kind::InvalidValue => {
                fields.insert(key, "Use plain values under 80 characters.".to_string());
            }
            rules::Kind::DuplicateValue => {
                fields.insert(key, "Each value can appear once.".to_string());
            }
            rules::Kind::UnknownField
            | rules::Kind::EmptyOptionName
            | rules::Kind::DuplicateOptionName => {}
        }
    }
    fields
}

fn validate_product(product: &Product) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    if !rules::valid_name(&product.name) {
        fields.insert(
            "name".to_string(),
            "Use between 2 and 120 characters.".to_string(),
        );
    }
    if !valid_slug(&product.slug) {
        fields.insert(
            "slug".to_string(),
            "Use lowercase letters, numbers and hyphens.".to_string(),
        );
    }
    if rune_len(&product.subtitle) > rules::MAX_SUBTITLE_RUNES {
        fields.insert(
            "subtitle".to_string(),
            "Keep the subtitle under 160 characters.".to_string(),
        );
    }
    if rune_len(&product.description) > rules::MAX_DESCRIPTION_RUNES {
        fields.insert(
            "description".to_string(),
            "Keep the description under 4000 characters.".to_string(),
        );
    }
    if !rules::valid_price(product.price_cents) {
        fields.insert(
            "price_cents".to_string(),
            "Enter a price above 0 and below 100000000.".to_string(),
        );
    }
    if let Some(compare_at) = product.compare_at_cents
        && !rules::valid_price(compare_at)
    {
        fields.insert(
            "compare_at_cents".to_string(),
            "Enter a compare-at price above 0, or leave it empty.".to_string(),
        );
    }
    for problem in product.sale_window().problems(product.price_cents) {
        match problem {
            SALE_PRICE_INVALID => {
                fields.insert(
                    "sale_price_cents".to_string(),
                    "Enter a sale price above 0 and below the regular price.".to_string(),
                );
            }
            SALE_PRICE_MISSING => {
                fields.insert(
                    "sale_price_cents".to_string(),
                    "Add a sale price to use a sale window.".to_string(),
                );
            }
            SALE_START_MISSING => {
                fields.insert(
                    "sale_starts_at".to_string(),
                    "Set when the sale starts.".to_string(),
                );
            }
            SALE_END_MISSING => {
                fields.insert(
                    "sale_ends_at".to_string(),
                    "Set when the sale ends.".to_string(),
                );
            }
            SALE_END_NOT_AFTER_START => {
                fields.insert(
                    "sale_ends_at".to_string(),
                    "The sale must end after it starts.".to_string(),
                );
            }
            _ => {}
        }
    }
    if rune_len(&product.size) > 60 {
        fields.insert(
            "size".to_string(),
            "Keep the size under 60 characters.".to_string(),
        );
    }
    if product.concerns.len() > 12 {
        fields.insert(
            "concerns".to_string(),
            "Use at most 12 concerns.".to_string(),
        );
    } else if product.concerns.iter().any(|value| rune_len(value) > 80) {
        fields.insert(
            "concerns".to_string(),
            "Keep each concern under 80 characters.".to_string(),
        );
    }
    if product.ingredients.len() > 12 {
        fields.insert(
            "ingredients".to_string(),
            "Use at most 12 ingredients.".to_string(),
        );
    } else if product
        .ingredients
        .iter()
        .any(|value| rune_len(value) > 120)
    {
        fields.insert(
            "ingredients".to_string(),
            "Keep each ingredient under 120 characters.".to_string(),
        );
    }
    if rune_len(&product.how_to) > 1000 {
        fields.insert(
            "how_to".to_string(),
            "Keep the instructions under 1000 characters.".to_string(),
        );
    }
    if !rules::valid_image_path(&product.image) {
        fields.insert("image".to_string(), "Use an uploaded image.".to_string());
    }
    if product.gallery.len() > 12 {
        fields.insert("gallery".to_string(), "Use at most 12 images.".to_string());
    } else if product
        .gallery
        .iter()
        .any(|image| !rules::valid_image_path(image))
    {
        fields.insert(
            "gallery".to_string(),
            "Each image must be an uploaded image.".to_string(),
        );
    }
    if !rules::valid_stock(i64::from(product.stock)) {
        fields.insert(
            "stock".to_string(),
            "Use a stock count between 0 and 100000.".to_string(),
        );
    }
    fields
}
