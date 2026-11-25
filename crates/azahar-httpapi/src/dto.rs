//! Response shapes for the HTTP API.
//!
//! Field order and omission follow the Go structs in `dto.go` and
//! `handlers_framework.go` so the JSON is interchangeable with the oracle.

use std::collections::BTreeMap;

use azahar_plugins::{Manifest, OptionField, ProductType, StorefrontPage};
use azahar_store::{Category, CustomerRow, Order, OrderItem, Product, User};
use chrono::Utc;
use serde::{Serialize, Serializer};
use serde_json::Value;

use azahar_store::{CORE_PLUGIN_ID, format_optional_time, format_time, format_time_or_zero};

/// The public user shape: `toUser` in `dto.go`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UserDto {
    pub id: String,
    pub name: String,
    pub email: String,
    pub role: String,
}

impl From<&User> for UserDto {
    fn from(user: &User) -> Self {
        Self {
            id: user.id.clone(),
            name: user.name.clone(),
            email: user.email.clone(),
            role: user.role.clone(),
        }
    }
}

impl From<User> for UserDto {
    fn from(user: User) -> Self {
        Self::from(&user)
    }
}

/// Go's `toUser`.
#[must_use]
pub fn to_user(user: &User) -> UserDto {
    UserDto::from(user)
}

/// `{"user": ...}` is the body of every auth response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UserEnvelope {
    pub user: UserDto,
}

impl From<&User> for UserEnvelope {
    fn from(user: &User) -> Self {
        Self {
            user: UserDto::from(user),
        }
    }
}

/// One catalogue category: `categoryDTO` in `dto.go`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CategoryDto {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub plugin_id: String,
}

impl From<&Category> for CategoryDto {
    fn from(category: &Category) -> Self {
        Self {
            slug: category.slug.clone(),
            name: category.name.clone(),
            description: category.description.clone(),
            plugin_id: category.plugin_id.clone(),
        }
    }
}

/// The category embedded in a product: `categoryRefDTO`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CategoryRefDto {
    pub slug: String,
    pub name: String,
    pub plugin_id: String,
}

/// A product's option schema.
///
/// Go reads `ProductType.Fields` straight into the response. The core path and
/// an unknown plugin return an explicitly empty slice, so `[]`; a known
/// product type carries its slice, whose Go zero value is `null`. The plugin
/// crate's own `ProductType` serialiser already renders an empty field list as
/// `null`, so the two cases can be represented with `Option`:
/// `Some(vec![])` is `[]` and `None` is `null`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OptionSchema(pub Option<Vec<OptionField>>);

impl Serialize for OptionSchema {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match &self.0 {
            None => serializer.serialize_none(),
            Some(fields) => fields.serialize(serializer),
        }
    }
}

/// The public product shape: `productDTO` in `dto.go`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProductDto {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub subtitle: String,
    pub description: String,
    pub price_cents: i64,
    pub compare_at_cents: Option<i64>,
    pub sale_price_cents: Option<i64>,
    pub sale_starts_at: String,
    pub sale_ends_at: String,
    pub effective_price_cents: i64,
    pub on_sale: bool,
    pub currency: String,
    pub size: String,
    pub category: CategoryRefDto,
    pub concerns: Vec<String>,
    pub ingredients: Vec<String>,
    pub how_to: String,
    pub image: String,
    pub gallery: Vec<String>,
    pub stock: i32,
    pub featured: bool,
    pub active: bool,
    pub plugin_id: String,
    pub plugin_name: String,
    pub plugin_summary: String,
    pub option_schema: OptionSchema,
    pub attributes: BTreeMap<String, Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_data: Option<BTreeMap<String, Value>>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub created_at: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub updated_at: String,
}

/// Go's `productDTOFromStore`, including the non-nil list/map normalisation.
#[must_use]
pub fn product_dto_from_store(product: &Product, with_timestamps: bool) -> ProductDto {
    let now = Utc::now();
    ProductDto {
        id: product.id.clone(),
        slug: product.slug.clone(),
        name: product.name.clone(),
        subtitle: product.subtitle.clone(),
        description: product.description.clone(),
        price_cents: product.price_cents,
        compare_at_cents: product.compare_at_cents,
        sale_price_cents: product.sale_price_cents,
        sale_starts_at: format_optional_time(product.sale_starts_at.as_ref()),
        sale_ends_at: format_optional_time(product.sale_ends_at.as_ref()),
        effective_price_cents: product.effective_price_cents(now),
        on_sale: product.sale_active(now),
        currency: product.currency.clone(),
        size: product.size.clone(),
        category: CategoryRefDto {
            slug: product.category.slug.clone(),
            name: product.category.name.clone(),
            plugin_id: product.category.plugin_id.clone(),
        },
        concerns: product.concerns.clone(),
        ingredients: product.ingredients.clone(),
        how_to: product.how_to.clone(),
        image: product.image.clone(),
        gallery: product.gallery.clone(),
        stock: product.stock,
        featured: product.featured,
        active: product.active,
        plugin_id: azahar_store::normalize_plugin_id(&product.plugin_id),
        plugin_name: String::new(),
        plugin_summary: String::new(),
        option_schema: OptionSchema::default(),
        attributes: product.attributes.clone().into_iter().collect(),
        plugin_data: None,
        created_at: if with_timestamps {
            format_time_or_zero(product.created_at.as_ref())
        } else {
            String::new()
        },
        updated_at: if with_timestamps {
            format_time_or_zero(product.updated_at.as_ref())
        } else {
            String::new()
        },
    }
}

/// Go's `decorateProduct`: option schema plus plugin name and summary.
#[must_use]
pub fn decorate_product(
    registry: &azahar_plugins::Registry,
    product: &Product,
    mut dto: ProductDto,
) -> ProductDto {
    dto.option_schema = option_schema(registry, product);
    dto.plugin_name = "Core".to_string();
    if let Some(plugin) = registry.get(&dto.plugin_id) {
        let manifest = plugin.manifest();
        dto.plugin_name = manifest.name;
        if let Some(product_type) = manifest.product_type {
            dto.plugin_summary = product_type.summary;
        }
    }
    dto
}

/// The detail view: decorated plus plugin enrichment for non-core products.
#[must_use]
pub fn to_product(
    registry: &azahar_plugins::Registry,
    product: &Product,
    with_timestamps: bool,
) -> ProductDto {
    let mut dto = decorate_product(
        registry,
        product,
        product_dto_from_store(product, with_timestamps),
    );
    if dto.plugin_id != CORE_PLUGIN_ID {
        dto.plugin_data = registry
            .enrich_product(product)
            .filter(|data| !data.is_empty())
            .map(|data| data.into_iter().collect());
    }
    dto
}

/// Detail view that also runs async plugin enrichment (phone IMEI counts).
/// The registry guard is dropped before the database call so a slow query
/// cannot stall plugin administration.
pub async fn to_product_async(
    state: &crate::state::AppState,
    product: &Product,
    with_timestamps: bool,
) -> ProductDto {
    let mut dto = {
        let registry = state
            .plugins
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        to_product(&registry, product, with_timestamps)
    };
    if dto.plugin_id == "phone"
        && dto.plugin_data.is_none()
        && let Ok(data) = azahar_plugin_phone::PhoneService::new()
            .product_enrichment(state.store.conn(), &product.id)
            .await
        && !data.is_empty()
    {
        dto.plugin_data = Some(data.into_iter().collect());
    }
    dto
}

/// List view: decorated but no per-product plugin counts.
#[must_use]
pub fn to_products(
    registry: &azahar_plugins::Registry,
    products: &[Product],
    with_timestamps: bool,
) -> Vec<ProductDto> {
    products
        .iter()
        .map(|product| {
            decorate_product(
                registry,
                product,
                product_dto_from_store(product, with_timestamps),
            )
        })
        .collect()
}

/// `orderintake.OptionSchema`, inlined because the crate's public API does not
/// expose it yet.
fn option_schema(registry: &azahar_plugins::Registry, product: &Product) -> OptionSchema {
    if product.plugin_id.is_empty() || product.plugin_id == CORE_PLUGIN_ID {
        return OptionSchema(Some(Vec::new()));
    }
    match registry.product_type(&product.plugin_id) {
        None => OptionSchema(Some(Vec::new())),
        Some(ProductType { fields, .. }) => {
            if fields.is_empty() {
                OptionSchema(None)
            } else {
                OptionSchema(Some(fields))
            }
        }
    }
}

/// The public plugin manifest: `publicPluginDTO` in `handlers_framework.go`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublicPluginDto {
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
    pub categories: Vec<azahar_plugins::Category>,
    #[serde(rename = "storefront_pages", skip_serializing_if = "Vec::is_empty")]
    pub pages: Vec<StorefrontPage>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub admin_path: String,
    pub enabled: bool,
}

impl PublicPluginDto {
    /// Builds the DTO from one manifest plus its effective state.
    #[must_use]
    pub fn from_manifest(manifest: &azahar_plugins::Manifest, enabled: bool) -> Self {
        Self {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            description: manifest.description.clone(),
            author: manifest.author.clone(),
            dependencies: manifest.dependencies.clone(),
            product_type: manifest.product_type.clone(),
            categories: manifest.categories.clone(),
            pages: manifest.storefront_pages.clone(),
            admin_path: manifest.admin_path.clone(),
            enabled,
        }
    }
}

/// One order line: `orderItemDTO` in `dto.go`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OrderItemDto {
    pub id: String,
    pub product_id: String,
    pub slug: String,
    pub name: String,
    pub image: String,
    pub unit_price_cents: i64,
    pub quantity: i32,
    pub line_total_cents: i64,
    pub options: BTreeMap<String, String>,
    pub options_label: String,
    pub plugin_id: String,
}

/// The customer block embedded in an order: `orderCustomerDTO`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OrderCustomerDto {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub name: String,
    pub email: String,
    pub phone: String,
}

/// The public order shape: `orderDTO` in `dto.go`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OrderDto {
    pub id: String,
    pub number: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub token: String,
    pub status: String,
    pub name: String,
    pub email: String,
    pub phone: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub address: String,
    pub city: String,
    pub notes: String,
    #[serde(skip_serializing_if = "is_zero_i32")]
    pub wilaya_code: i32,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub wilaya_name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub commune_name: String,
    pub delivery_type: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub stop_desk_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub stop_desk_name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub courier_key: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub tracking_number: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub courier_status: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub courier_events: Vec<azahar_store::CourierEvent>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub courier_synced_at: String,
    pub subtotal_cents: i64,
    pub shipping_cents: i64,
    pub total_cents: i64,
    pub currency: String,
    pub items: Vec<OrderItemDto>,
    pub customer: OrderCustomerDto,
    pub created_at: String,
    pub updated_at: String,
}

/// Go's `toOrder`: the token is whatever the caller passes (the by-token route
/// passes an empty string), and the address is only present when allowed.
#[must_use]
pub fn to_order(
    order: &Order,
    items: &[OrderItem],
    token: &str,
    include_address: bool,
) -> OrderDto {
    OrderDto {
        id: order.id.clone(),
        number: order.number.clone(),
        token: token.to_string(),
        status: order.status.clone(),
        name: order.name.clone(),
        email: order.email.clone(),
        phone: order.phone.clone(),
        address: if include_address {
            order.address.clone()
        } else {
            String::new()
        },
        city: order.city.clone(),
        notes: order.notes.clone(),
        wilaya_code: order.wilaya_code,
        wilaya_name: order.wilaya_name.clone(),
        commune_name: order.commune_name.clone(),
        delivery_type: order.delivery_type.clone(),
        stop_desk_id: order.stop_desk_id.clone(),
        stop_desk_name: order.stop_desk_name.clone(),
        courier_key: order.courier_key.clone(),
        tracking_number: order.tracking_number.clone(),
        courier_status: order.courier_status.clone(),
        courier_events: order.courier_events.clone(),
        courier_synced_at: format_optional_time(order.courier_synced_at.as_ref()),
        subtotal_cents: order.subtotal_cents,
        shipping_cents: order.shipping_cents,
        total_cents: order.total_cents,
        currency: order.currency.clone(),
        items: to_order_items(items),
        customer: OrderCustomerDto {
            id: order.user_id.clone().unwrap_or_default(),
            name: order.name.clone(),
            email: order.email.clone(),
            phone: order.phone.clone(),
        },
        created_at: format_time_or_zero(order.created_at.as_ref()),
        updated_at: format_time_or_zero(order.updated_at.as_ref()),
    }
}

/// Go's `toOrderItems`; a nil list is rendered as `[]`, never `null`.
#[must_use]
pub fn to_order_items(items: &[OrderItem]) -> Vec<OrderItemDto> {
    items
        .iter()
        .map(|item| OrderItemDto {
            id: item.id.clone(),
            product_id: item.product_id.clone(),
            slug: item.slug.clone(),
            name: item.name.clone(),
            image: item.image.clone(),
            unit_price_cents: item.unit_price_cents,
            quantity: item.quantity,
            line_total_cents: item.line_total_cents,
            options: item.options.clone().into_iter().collect(),
            options_label: item.options_label.clone(),
            plugin_id: azahar_store::normalize_plugin_id(&item.plugin_id),
        })
        .collect()
}

/// One row of the admin customer list: `customerDTO` in `handlers_admin.go`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CustomerDto {
    pub id: String,
    pub name: String,
    pub email: String,
    pub orders_count: i32,
    pub total_spent_cents: i64,
    pub last_order_at: Option<String>,
    pub created_at: String,
}

impl From<&CustomerRow> for CustomerDto {
    fn from(customer: &CustomerRow) -> Self {
        Self {
            id: customer.id.clone(),
            name: customer.name.clone(),
            email: customer.email.clone(),
            orders_count: customer.orders_count,
            total_spent_cents: customer.total_spent_cents,
            last_order_at: customer.last_order_at.as_ref().map(format_time),
            created_at: format_time_or_zero(customer.created_at.as_ref()),
        }
    }
}

/// One member of the admin team list: `teamMemberDTO` in `handlers_admin.go`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TeamMemberDto {
    pub id: String,
    pub name: String,
    pub email: String,
    pub role: String,
    pub created_at: String,
}

impl From<&User> for TeamMemberDto {
    fn from(user: &User) -> Self {
        Self {
            id: user.id.clone(),
            name: user.name.clone(),
            email: user.email.clone(),
            role: user.role.clone(),
            created_at: format_time_or_zero(user.created_at.as_ref()),
        }
    }
}

/// The admin plugin shape: `adminPluginDTO` in `handlers_framework.go`.
///
/// Go embeds the manifest, so its fields are inlined ahead of the state fields.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AdminPluginDto {
    #[serde(flatten)]
    pub manifest: Manifest,
    pub configured: bool,
    pub settings_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub enabled: bool,
    pub effective_enabled: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dependents: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<String>,
}

fn is_zero_i32(value: &i32) -> bool {
    *value == 0
}
