//! Public catalogue, theme, plugin, shipping and customer-order routes.
//!
//! Every test drives the real router with `tower::ServiceExt::oneshot` against
//! an in-memory libSQL database seeded with core, oils, phone and pcparts rows,
//! so the store queries, plugin states and cache middleware are the production
//! code paths.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header};
use axum::response::Response;
use azahar_auth::hash_token;
use azahar_config::Config;
use azahar_courier::{
    APIError, Commune, Courier, Desk, Error as CourierError, Shipment, Tracking, Wilaya,
};
use azahar_httpapi::session_layer::session_layer;
use azahar_httpapi::session_store::LibsqlSessionStore;
use azahar_httpapi::state::configured_ttl;
use azahar_httpapi::{AppState, build_router};
use azahar_plugins::Registry;
use azahar_store::{Order, OrderItem, Store, parse_time};
use libsql::Builder;
use serde_json::{Value, json};
use tower::ServiceExt;

const ORIGIN: &str = "http://localhost:3000";
const CSRF_TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const PASSWORD: &str = "correct horse battery staple";

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShippingMode {
    Ready,
    NoDesks,
    Unavailable,
}

#[derive(Clone, Copy)]
struct StubCourier {
    mode: ShippingMode,
}

fn sample_wilaya() -> Wilaya {
    Wilaya {
        code: 16,
        name_ar: "الجزائر".to_string(),
        name_fr: "Alger".to_string(),
        is_deep_south: false,
        commune_count: 57,
        courier_supported: true,
    }
}

#[async_trait::async_trait]
impl Courier for StubCourier {
    async fn wilayas(&self) -> Result<Vec<Wilaya>, CourierError> {
        match self.mode {
            ShippingMode::Unavailable => Err(CourierError::transport("offline")),
            _ => Ok(vec![sample_wilaya()]),
        }
    }

    async fn wilaya(&self, code: i32) -> Result<Wilaya, CourierError> {
        if code == 16 {
            Ok(sample_wilaya())
        } else {
            Err(CourierError::UnknownWilaya)
        }
    }

    async fn communes(&self, wilaya_code: i32) -> Result<Vec<Commune>, CourierError> {
        match self.mode {
            ShippingMode::Unavailable => Err(CourierError::transport("offline")),
            _ if wilaya_code != 16 => Err(CourierError::UnknownWilaya),
            _ => Ok(vec![Commune {
                wilaya_code: 16,
                name_fr: "Alger Centre".to_string(),
                name_ar: "الجزائر الوسطى".to_string(),
            }]),
        }
    }

    async fn commune(&self, wilaya_code: i32, name: &str) -> Result<String, CourierError> {
        let communes = self.communes(wilaya_code).await?;
        communes
            .into_iter()
            .find(|commune| commune.name_fr == name.trim())
            .map(|commune| commune.name_fr)
            .ok_or(CourierError::UnknownCommune)
    }

    async fn desks(&self, wilaya_code: i32) -> Result<Vec<Desk>, CourierError> {
        match self.mode {
            ShippingMode::Unavailable => Err(CourierError::transport("offline")),
            ShippingMode::NoDesks => Err(CourierError::Api(APIError {
                status: 422,
                message: "No desks for this wilaya.".to_string(),
                ..APIError::default()
            })),
            ShippingMode::Ready if wilaya_code == 16 => Ok(vec![Desk {
                id: "D-16".to_string(),
                name: "Alger desk".to_string(),
                wilaya_code: 16,
                commune_name: "Alger Centre".to_string(),
                address: "1 Rue de la Paix".to_string(),
            }]),
            ShippingMode::Ready => Err(CourierError::UnknownWilaya),
        }
    }

    async fn desk(&self, wilaya_code: i32, id: &str) -> Result<Desk, CourierError> {
        self.desks(wilaya_code)
            .await?
            .into_iter()
            .find(|desk| desk.id == id)
            .ok_or(CourierError::UnknownDesk)
    }

    async fn create_order(
        &self,
        _request: azahar_courier::OrderRequest,
    ) -> Result<Shipment, CourierError> {
        Err(CourierError::transport("not used"))
    }

    async fn track(
        &self,
        _courier_key: &str,
        _tracking_number: &str,
    ) -> Result<Tracking, CourierError> {
        Err(CourierError::transport("not used"))
    }
}

struct Fixture {
    app: Router,
    store: Arc<Store>,
}

fn dev_config() -> Config {
    Config {
        app_env: "development".to_string(),
        app_origin: ORIGIN.to_string(),
        cookie_secure: false,
        trust_proxy: false,
        session_ttl: std::time::Duration::from_secs(48 * 60 * 60),
        ..Config::default()
    }
}

async fn fixture() -> Fixture {
    fixture_with(ShippingMode::Ready, |_| {}).await
}

async fn fixture_with(mode: ShippingMode, configure: impl FnOnce(&mut Registry)) -> Fixture {
    let database = Builder::new_local(":memory:")
        .build()
        .await
        .expect("open in-memory database");
    let store = Arc::new(Store::new(database).expect("store"));
    azahar_db::migrate(store.conn()).await.expect("migrate");
    seed(&store).await;

    let mut registry = Registry::new();
    registry
        .register(Arc::new(azahar_plugin_oils::new()))
        .expect("register oils");
    registry
        .register(Arc::new(azahar_plugin_phone::new()))
        .expect("register phone");
    registry
        .register(Arc::new(azahar_plugin_pcparts::new()))
        .expect("register pcparts");
    configure(&mut registry);

    let cfg = dev_config();
    let state = AppState::new(
        cfg.clone(),
        store.clone(),
        registry,
        Arc::new(StubCourier { mode }),
    );
    let sessions = session_layer(
        LibsqlSessionStore::new(store.conn().clone()),
        cfg.cookie_secure,
        configured_ttl(&cfg),
    );
    Fixture {
        app: build_router(state, sessions, axum::Router::new()),
        store,
    }
}

async fn insert_category(
    store: &Store,
    id: &str,
    slug: &str,
    name: &str,
    description: &str,
    sort_order: i64,
    plugin_id: &str,
) {
    store
        .conn()
        .execute(
            "INSERT INTO categories (id, slug, name, description, sort_order, plugin_id)
             VALUES (?, ?, ?, ?, ?, ?)",
            libsql::params![id, slug, name, description, sort_order, plugin_id],
        )
        .await
        .expect("insert category");
}

struct SeedProduct<'a> {
    id: &'a str,
    slug: &'a str,
    name: &'a str,
    subtitle: &'a str,
    price_cents: i64,
    category_id: &'a str,
    plugin_id: &'a str,
    concerns: &'a str,
    attributes: &'a str,
    featured: bool,
    active: bool,
    created_at: &'a str,
}

async fn insert_product(store: &Store, product: &SeedProduct<'_>) {
    store
        .conn()
        .execute(
            "INSERT INTO products (id, slug, name, subtitle, description, price_cents,
             compare_at_cents, sale_price_cents, sale_starts_at, sale_ends_at, currency,
             size, category_id, concerns, ingredients, how_to, image, gallery, stock,
             featured, active, plugin_id, attributes, created_at, updated_at)
             VALUES (?, ?, ?, ?, '', ?, NULL, NULL, '', '', 'DZD', '100 ml', ?, ?, '[]',
             '', '', '[]', 10, ?, ?, ?, ?, ?, ?)",
            libsql::params![
                product.id,
                product.slug,
                product.name,
                product.subtitle,
                product.price_cents,
                product.category_id,
                product.concerns,
                i64::from(product.featured),
                i64::from(product.active),
                product.plugin_id,
                product.attributes,
                product.created_at,
                product.created_at,
            ],
        )
        .await
        .expect("insert product");
}

async fn seed(store: &Store) {
    insert_category(
        store,
        "cat_core",
        "skincare",
        "Skincare",
        "Core shelf",
        1,
        "core",
    )
    .await;
    insert_category(store, "cat_face", "face", "Face", "Face oils", 1, "oils").await;
    insert_category(store, "cat_body", "body", "Body", "Body oils", 2, "oils").await;
    insert_category(
        store,
        "cat_phone",
        "phones",
        "Phones",
        "Handsets",
        1,
        "phone",
    )
    .await;
    insert_category(store, "cat_cpu", "cpus", "CPU", "Processors", 1, "pcparts").await;

    insert_product(
        store,
        &SeedProduct {
            id: "p_core",
            slug: "gentle-soap",
            name: "Gentle soap",
            subtitle: "A mild bar",
            price_cents: 150_000,
            category_id: "cat_core",
            plugin_id: "",
            concerns: r#"["dry"]"#,
            attributes: "{}",
            featured: true,
            active: true,
            created_at: "2026-01-01T00:00:00Z",
        },
    )
    .await;
    insert_product(
        store,
        &SeedProduct {
            id: "p_oil_1",
            slug: "neroli-oil",
            name: "Neroli oil",
            subtitle: "Bright and green",
            price_cents: 300_000,
            category_id: "cat_face",
            plugin_id: "oils",
            concerns: "[]",
            attributes: r#"{"size":["100 ml"],"concerns":["dry","dull"]}"#,
            featured: false,
            active: true,
            created_at: "2026-01-02T00:00:00Z",
        },
    )
    .await;
    insert_product(
        store,
        &SeedProduct {
            id: "p_oil_2",
            slug: "rose-oil",
            name: "Rose oil",
            subtitle: "Soft floral",
            price_cents: 200_000,
            category_id: "cat_face",
            plugin_id: "oils",
            concerns: "[]",
            attributes: "{}",
            featured: false,
            active: true,
            created_at: "2026-01-03T00:00:00Z",
        },
    )
    .await;
    insert_product(
        store,
        &SeedProduct {
            id: "p_phone",
            slug: "pixel-phone",
            name: "Pixel phone",
            subtitle: "A handset",
            price_cents: 9_000_000,
            category_id: "cat_phone",
            plugin_id: "phone",
            concerns: "[]",
            attributes: r#"{"config":["12 GB RAM + 256 GB"]}"#,
            featured: false,
            active: true,
            created_at: "2026-01-04T00:00:00Z",
        },
    )
    .await;
    insert_product(
        store,
        &SeedProduct {
            id: "p_pc",
            slug: "ryzen-cpu",
            name: "Ryzen CPU",
            subtitle: "Eight cores",
            price_cents: 4_000_000,
            category_id: "cat_cpu",
            plugin_id: "pcparts",
            concerns: "[]",
            attributes: r#"{"socket":["AM5"]}"#,
            featured: false,
            active: true,
            created_at: "2026-01-05T00:00:00Z",
        },
    )
    .await;
    insert_product(
        store,
        &SeedProduct {
            id: "p_inactive",
            slug: "retired-oil",
            name: "Retired oil",
            subtitle: "No longer sold",
            price_cents: 100_000,
            category_id: "cat_face",
            plugin_id: "oils",
            concerns: "[]",
            attributes: "{}",
            featured: false,
            active: false,
            created_at: "2026-01-06T00:00:00Z",
        },
    )
    .await;

    store
        .conn()
        .execute(
            "INSERT INTO storefront_settings (id, data, updated_at) VALUES ('default', ?, ?)",
            libsql::params![
                r#"{"brand_name":"Test House","font_scale":1.5}"#,
                "2026-02-03T04:05:06Z",
            ],
        )
        .await
        .expect("insert theme");
}

fn order(
    id: &str,
    number: &str,
    token: &str,
    user_id: Option<&str>,
    created_at: &str,
    address: &str,
) -> Order {
    Order {
        id: id.to_string(),
        number: number.to_string(),
        user_id: user_id.map(str::to_string),
        name: "Amina".to_string(),
        email: "amina@example.com".to_string(),
        phone: "0550123456".to_string(),
        address: address.to_string(),
        city: "Alger".to_string(),
        notes: "Leave at the door".to_string(),
        subtotal_cents: 100_000,
        shipping_cents: 50_000,
        total_cents: 150_000,
        currency: "DZD".to_string(),
        status: "pending".to_string(),
        token_hash: hash_token(token),
        created_at: parse_time(created_at),
        updated_at: parse_time(created_at),
        wilaya_code: 16,
        wilaya_name: "Alger".to_string(),
        commune_name: "Alger Centre".to_string(),
        delivery_type: "home".to_string(),
        ..Order::default()
    }
}

fn item(order_id: &str, id: &str, plugin_id: &str) -> OrderItem {
    OrderItem {
        id: id.to_string(),
        order_id: order_id.to_string(),
        product_id: "p_oil_1".to_string(),
        slug: "neroli-oil".to_string(),
        name: "Neroli oil".to_string(),
        image: "https://example.test/oil.jpg".to_string(),
        unit_price_cents: 100_000,
        quantity: 1,
        line_total_cents: 100_000,
        options: HashMap::from([("size".to_string(), "100 ml".to_string())]),
        options_label: "100 ml".to_string(),
        plugin_id: plugin_id.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Request helpers
// ---------------------------------------------------------------------------

fn request(method: Method, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .expect("build request")
}

fn json_request(uri: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("build request")
}

fn with_cookies(mut request: Request<Body>, cookies: &[(&str, &str)]) -> Request<Body> {
    let value = cookies
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ");
    request.headers_mut().insert(
        header::COOKIE,
        HeaderValue::from_str(&value).expect("cookie"),
    );
    request
}

fn with_csrf_headers(mut request: Request<Body>) -> Request<Body> {
    request
        .headers_mut()
        .insert("x-requested-with", HeaderValue::from_static("azahar-web"));
    request
        .headers_mut()
        .insert(header::ORIGIN, HeaderValue::from_static(ORIGIN));
    request
        .headers_mut()
        .insert("x-csrf-token", HeaderValue::from_static(CSRF_TOKEN));
    request
}

fn signed_json(uri: &str, body: &Value) -> Request<Body> {
    with_cookies(
        with_csrf_headers(json_request(uri, body)),
        &[("azahar_csrf", CSRF_TOKEN)],
    )
}

async fn send(app: &Router, request: Request<Body>) -> Response {
    app.clone().oneshot(request).await.expect("router response")
}

async fn body_json(response: Response) -> (StatusCode, HeaderMap, Value) {
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("json body")
    };
    (status, headers, value)
}

async fn get_json(app: &Router, uri: &str) -> (StatusCode, HeaderMap, Value) {
    body_json(send(app, request(Method::GET, uri)).await).await
}

async fn body_text(response: Response) -> (StatusCode, HeaderMap, String) {
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    (
        status,
        headers,
        String::from_utf8(bytes.to_vec()).expect("utf8"),
    )
}

fn raw_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find(|raw| raw.starts_with(&format!("{name}=")))
        .map(str::to_owned)
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    raw_cookie(headers, name).map(|raw| {
        raw.strip_prefix(&format!("{name}="))
            .unwrap_or_default()
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_string()
    })
}

async fn register(app: &Router, email: &str) -> (StatusCode, HeaderMap, Value) {
    let body = json!({"name": "Amina", "email": email, "password": PASSWORD});
    body_json(send(app, signed_json("/api/auth/register", &body)).await).await
}

fn product_names(body: &Value) -> Vec<&str> {
    body["products"]
        .as_array()
        .expect("products array")
        .iter()
        .map(|product| product["name"].as_str().expect("product name"))
        .collect()
}

fn category_slugs(body: &Value) -> Vec<&str> {
    body["categories"]
        .as_array()
        .expect("categories array")
        .iter()
        .map(|category| category["slug"].as_str().expect("category slug"))
        .collect()
}

// ---------------------------------------------------------------------------
// Categories
// ---------------------------------------------------------------------------

#[tokio::test]
async fn categories_filter_empty_shelves_and_disabled_plugins() {
    let fixture = fixture().await;

    let (status, headers, body) = get_json(&fixture.app, "/api/categories").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "application/json; charset=utf-8"
    );
    assert_eq!(
        category_slugs(&body),
        ["skincare", "face", "cpus", "phones"],
        "public categories omit the empty oils shelf, ordered by plugin"
    );
    let core = &body["categories"][0];
    assert_eq!(core["slug"], "skincare");
    assert_eq!(core["name"], "Skincare");
    assert_eq!(core["description"], "Core shelf");
    assert_eq!(core["plugin_id"], "core");
    assert!(
        core.get("id").is_none() && core.get("sort_order").is_none(),
        "only the four public fields are served: {core}"
    );

    let (_, _, all) = get_json(&fixture.app, "/api/categories?all=1").await;
    assert_eq!(
        category_slugs(&all),
        ["skincare", "face", "body", "cpus", "phones"],
        "all=1 keeps empty shelves"
    );

    let disabled = fixture_with(ShippingMode::Ready, |registry| {
        registry.set_enabled("phone", false);
    })
    .await;
    let (_, _, visible) = get_json(&disabled.app, "/api/categories").await;
    assert!(!category_slugs(&visible).contains(&"phones"));
    let (_, _, everything) = get_json(&disabled.app, "/api/categories?all=1").await;
    assert!(
        category_slugs(&everything).contains(&"phones"),
        "all=1 does not filter by plugin state, exactly like Go"
    );
}

// ---------------------------------------------------------------------------
// Products
// ---------------------------------------------------------------------------

#[tokio::test]
async fn product_list_filters_sorts_and_omits_timestamps() {
    let fixture = fixture().await;

    let (status, _, body) = get_json(&fixture.app, "/api/products").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        product_names(&body),
        [
            "Gentle soap",
            "Ryzen CPU",
            "Pixel phone",
            "Rose oil",
            "Neroli oil"
        ],
        "featured first, then newest"
    );
    let first = &body["products"][0];
    assert!(first.get("created_at").is_none());
    assert!(first.get("updated_at").is_none());

    let (_, _, face) = get_json(&fixture.app, "/api/products?category=face").await;
    assert_eq!(product_names(&face), ["Rose oil", "Neroli oil"]);

    let (_, _, dry) = get_json(&fixture.app, "/api/products?concern=dry").await;
    assert_eq!(product_names(&dry), ["Gentle soap", "Neroli oil"]);

    let (_, _, search) = get_json(&fixture.app, "/api/products?q=%20%20rose%20%20").await;
    assert_eq!(product_names(&search), ["Rose oil"], "q is trimmed");

    let (_, _, ascending) = get_json(&fixture.app, "/api/products?sort=price_asc").await;
    assert_eq!(
        product_names(&ascending),
        [
            "Gentle soap",
            "Rose oil",
            "Neroli oil",
            "Ryzen CPU",
            "Pixel phone"
        ]
    );

    let (_, _, descending) = get_json(&fixture.app, "/api/products?sort=price_desc").await;
    assert_eq!(
        product_names(&descending),
        [
            "Pixel phone",
            "Ryzen CPU",
            "Neroli oil",
            "Rose oil",
            "Gentle soap"
        ]
    );

    let (_, _, newest) = get_json(&fixture.app, "/api/products?sort=new").await;
    assert_eq!(
        product_names(&newest),
        [
            "Ryzen CPU",
            "Pixel phone",
            "Rose oil",
            "Neroli oil",
            "Gentle soap"
        ]
    );

    let (_, _, none) = get_json(&fixture.app, "/api/products?category=missing").await;
    assert!(none["products"].as_array().expect("array").is_empty());
}

#[tokio::test]
async fn disabled_plugin_products_disappear() {
    let fixture = fixture_with(ShippingMode::Ready, |registry| {
        registry.set_enabled("phone", false);
    })
    .await;

    let (_, _, body) = get_json(&fixture.app, "/api/products").await;
    assert!(!product_names(&body).contains(&"Pixel phone"));
    assert!(product_names(&body).contains(&"Neroli oil"));

    let (status, _, error) = get_json(&fixture.app, "/api/products/pixel-phone").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error["error"]["code"], "not_found");
    assert_eq!(error["error"]["message"], "Product not found.");
}

#[tokio::test]
async fn product_detail_matches_the_go_dto() {
    let fixture = fixture().await;

    let (status, _, body) = get_json(&fixture.app, "/api/products/gentle-soap").await;
    assert_eq!(status, StatusCode::OK);
    let product = &body["product"];
    assert_eq!(product["id"], "p_core");
    assert_eq!(product["slug"], "gentle-soap");
    assert_eq!(product["name"], "Gentle soap");
    assert_eq!(product["subtitle"], "A mild bar");
    assert_eq!(product["description"], "");
    assert_eq!(product["price_cents"], 150_000);
    assert!(product["compare_at_cents"].is_null());
    assert!(product["sale_price_cents"].is_null());
    assert_eq!(product["sale_starts_at"], "");
    assert_eq!(product["sale_ends_at"], "");
    assert_eq!(product["effective_price_cents"], 150_000);
    assert_eq!(product["on_sale"], false);
    assert_eq!(product["currency"], "DZD");
    assert_eq!(product["size"], "100 ml");
    assert_eq!(product["category"]["slug"], "skincare");
    assert_eq!(product["category"]["name"], "Skincare");
    assert_eq!(product["category"]["plugin_id"], "core");
    assert_eq!(product["concerns"], json!(["dry"]));
    assert_eq!(product["ingredients"], json!([]));
    assert_eq!(product["gallery"], json!([]));
    assert_eq!(product["stock"], 10);
    assert_eq!(product["featured"], true);
    assert_eq!(product["active"], true);
    assert_eq!(
        product["plugin_id"], "core",
        "an empty stored plugin id is normalised"
    );
    assert_eq!(product["plugin_name"], "Core");
    assert_eq!(product["plugin_summary"], "");
    assert_eq!(product["option_schema"], json!([]));
    assert_eq!(product["attributes"], json!({}));
    assert!(
        product.get("plugin_data").is_none(),
        "core products never carry plugin data"
    );
    assert!(product.get("created_at").is_none());
    assert!(product.get("updated_at").is_none());

    let (_, _, oil_body) = get_json(&fixture.app, "/api/products/neroli-oil").await;
    let oil = &oil_body["product"];
    assert_eq!(oil["plugin_id"], "oils");
    assert_eq!(oil["plugin_name"], "Oil shelf");
    assert_eq!(
        oil["plugin_summary"],
        "A formula with its size, concerns, materials and instructions."
    );
    let schema = oil["option_schema"].as_array().expect("option schema");
    assert!(!schema.is_empty());
    assert_eq!(schema[0]["key"], "size");
    assert_eq!(oil["attributes"]["size"], json!(["100 ml"]));
    assert_eq!(oil["attributes"]["concerns"], json!(["dry", "dull"]));
    assert!(
        oil.get("plugin_data").is_none(),
        "the oils plugin declares no enricher"
    );

    let related = oil_body["related"].as_array().expect("related array");
    assert_eq!(related.len(), 1);
    assert_eq!(related[0]["slug"], "rose-oil");
}

#[tokio::test]
async fn product_missing_and_inactive_are_404() {
    let fixture = fixture().await;

    for slug in ["does-not-exist", "retired-oil"] {
        let (status, _, body) = get_json(&fixture.app, &format!("/api/products/{slug}")).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{slug}");
        assert_eq!(body["error"]["code"], "not_found");
        assert_eq!(body["error"]["message"], "Product not found.");
    }
}

// ---------------------------------------------------------------------------
// Plugin manifests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn plugin_manifests_expose_the_public_fields_only() {
    let fixture = fixture().await;
    let (status, _, body) = get_json(&fixture.app, "/api/plugins").await;
    assert_eq!(status, StatusCode::OK);

    let plugins = body["plugins"].as_array().expect("plugins array");
    let ids: Vec<&str> = plugins
        .iter()
        .map(|plugin| plugin["id"].as_str().expect("plugin id"))
        .collect();
    assert_eq!(ids, ["oils", "phone", "pcparts"], "registration order");

    let oils = &plugins[0];
    assert_eq!(oils["name"], "Oil shelf");
    assert_eq!(oils["version"], "1.0.0");
    assert!(
        oils["description"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert_eq!(oils["author"], "Azahar");
    assert_eq!(oils["admin_path"], "/admin/plugins/oils");
    assert_eq!(oils["enabled"], true);
    assert_eq!(oils["product_type"]["label"], "Oil");
    assert!(
        oils["product_type"]["fields"]
            .as_array()
            .is_some_and(|fields| !fields.is_empty())
    );
    assert_eq!(oils["categories"].as_array().map(Vec::len), Some(3));
    assert!(oils.get("dependencies").is_none());
    assert!(oils.get("storefront_pages").is_none());
    assert!(
        oils.get("settings").is_none(),
        "the public DTO does not include settings"
    );

    let phone = &plugins[1];
    assert_eq!(phone["product_type"]["label"], "Phone");
    assert_eq!(phone["enabled"], true);

    let disabled = fixture_with(ShippingMode::Ready, |registry| {
        registry.set_enabled("phone", false);
    })
    .await;
    let (_, _, disabled_body) = get_json(&disabled.app, "/api/plugins").await;
    let phone = &disabled_body["plugins"][1];
    assert_eq!(phone["id"], "phone");
    assert_eq!(phone["enabled"], false);
    assert!(phone["product_type"].is_object());
}

// ---------------------------------------------------------------------------
// Storefront theme
// ---------------------------------------------------------------------------

#[tokio::test]
async fn theme_row_is_parsed_and_serialised_with_go_float_formatting() {
    let fixture = fixture().await;
    let (status, headers, body) =
        body_text(send(&fixture.app, request(Method::GET, "/api/storefront/theme")).await).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "application/json; charset=utf-8"
    );
    assert!(body.starts_with("{\"theme\":{\"brand_name\":\"Test House\","));
    assert!(
        body.contains("\"font_scale\":1.5,"),
        "Go renders 1.5, not 1.500000: {body}"
    );
    assert!(
        !body.contains("1.500000"),
        "float formatting must match encoding/json"
    );
    assert!(
        body.ends_with("\"updated_at\":\"2026-02-03T04:05:06Z\"}}\n"),
        "the store timestamp is RFC3339 seconds and the body has Go's newline: {body}"
    );

    let value: Value = serde_json::from_str(body.trim()).expect("json");
    assert_eq!(value["theme"]["brand_name"], "Test House");
    assert_eq!(value["theme"]["font_scale"], 1.5);
    assert_eq!(value["theme"]["radius"], 4);
    assert_eq!(
        value["theme"]["light"]["ground"].as_str().map(str::len),
        Some(7)
    );
}

#[tokio::test]
async fn missing_theme_row_serves_the_default_without_updated_at() {
    let fixture = fixture().await;
    fixture
        .store
        .conn()
        .execute("DELETE FROM storefront_settings", ())
        .await
        .expect("delete theme");

    let (status, _, body) =
        body_text(send(&fixture.app, request(Method::GET, "/api/storefront/theme")).await).await;
    assert_eq!(status, StatusCode::OK);
    let value: Value = serde_json::from_str(body.trim()).expect("json");
    assert_eq!(value["theme"]["brand_name"], "Azahar");
    assert!(
        value["theme"].get("updated_at").is_none(),
        "the default theme leaves updated_at empty, and omitempty drops it"
    );
    assert!(body.contains("\"font_scale\":1,"));
}

#[tokio::test]
async fn malformed_theme_row_falls_back_leniently() {
    let fixture = fixture().await;
    fixture
        .store
        .conn()
        .execute(
            "UPDATE storefront_settings SET data = ? WHERE id = 'default'",
            libsql::params![r#"{"brand_name":42,"font_scale":"wide"}"#],
        )
        .await
        .expect("corrupt theme");

    let (status, _, body) =
        body_text(send(&fixture.app, request(Method::GET, "/api/storefront/theme")).await).await;
    assert_eq!(status, StatusCode::OK);
    let value: Value = serde_json::from_str(body.trim()).expect("json");
    assert_eq!(
        value["theme"]["brand_name"], "Azahar",
        "parse_lenient falls back to the default theme"
    );
    assert_eq!(
        value["theme"]["updated_at"], "2026-02-03T04:05:06Z",
        "the stored timestamp still wins"
    );
}

// ---------------------------------------------------------------------------
// Shipping
// ---------------------------------------------------------------------------

#[tokio::test]
async fn shipping_map_routes_match_the_go_contract() {
    let fixture = fixture().await;

    let (status, _, body) = get_json(&fixture.app, "/api/shipping/wilayas").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["wilayas"][0]["code"], 16);
    assert_eq!(body["wilayas"][0]["nameFr"], "Alger");
    assert_eq!(body["wilayas"][0]["courierSupported"], true);

    let (status, _, communes) = get_json(&fixture.app, "/api/shipping/communes?wilaya=16").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(communes["communes"][0]["nameFr"], "Alger Centre");
    assert_eq!(communes["communes"][0]["wilayaCode"], 16);

    for bad in ["0", "59", "abc", "", "-3", "16.0"] {
        let (status, _, problem) = get_json(
            &fixture.app,
            &format!("/api/shipping/communes?wilaya={bad}"),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
        assert_eq!(problem["error"]["code"], "validation_error");
        assert_eq!(problem["error"]["message"], "Check the marked fields.");
        assert_eq!(
            problem["error"]["fields"]["wilaya"],
            "Choose a wilaya between 1 and 58."
        );
    }

    let (status, _, desks) = get_json(&fixture.app, "/api/shipping/desks?wilaya=16").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(desks["desks"][0]["id"], "D-16");
}

#[tokio::test]
async fn shipping_desks_422_becomes_an_empty_list() {
    let fixture = fixture_with(ShippingMode::NoDesks, |_| {}).await;
    let (status, _, body) = get_json(&fixture.app, "/api/shipping/desks?wilaya=16").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["desks"], json!([]));
}

#[tokio::test]
async fn shipping_unavailable_map_is_503() {
    let fixture = fixture_with(ShippingMode::Unavailable, |_| {}).await;
    for uri in ["/api/shipping/wilayas", "/api/shipping/communes?wilaya=16"] {
        let (status, _, body) = get_json(&fixture.app, uri).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{uri}");
        assert_eq!(body["error"]["code"], "courier_unavailable");
        assert_eq!(
            body["error"]["message"],
            "The delivery map is unavailable. Try again in a moment."
        );
    }
}

// ---------------------------------------------------------------------------
// Orders
// ---------------------------------------------------------------------------

#[tokio::test]
async fn order_by_token_returns_the_address_and_hides_the_token() {
    let fixture = fixture().await;
    fixture
        .store
        .create_order(
            fixture.store.conn(),
            &order(
                "ord_public",
                "AZ-2026-0001",
                "public-token",
                None,
                "2026-03-01T10:00:00Z",
                "12 Rue de la Paix",
            ),
            &[item("ord_public", "oi_1", "")],
        )
        .await
        .expect("create order");

    let (status, _, body) = get_json(&fixture.app, "/api/orders/public-token").await;
    assert_eq!(status, StatusCode::OK);
    let order = &body["order"];
    assert_eq!(order["id"], "ord_public");
    assert_eq!(order["number"], "AZ-2026-0001");
    assert_eq!(order["address"], "12 Rue de la Paix");
    assert!(
        order.get("token").is_none(),
        "the by-token route passes an empty token, which omitempty drops"
    );
    assert_eq!(order["items"][0]["plugin_id"], "core");
    assert_eq!(order["items"][0]["options"], json!({"size": "100 ml"}));
    assert!(order["customer"].get("id").is_none());
    assert_eq!(order["created_at"], "2026-03-01T10:00:00Z");
    assert_eq!(order["updated_at"], "2026-03-01T10:00:00Z");

    for token in ["wrong-token", "public-token-wrong"] {
        let (status, _, error) = get_json(&fixture.app, &format!("/api/orders/{token}")).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{token}");
        assert_eq!(error["error"]["code"], "not_found");
        assert_eq!(error["error"]["message"], "Order not found.");
    }

    let long = "a".repeat(201);
    let (status, _, error) = get_json(&fixture.app, &format!("/api/orders/{long}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error["error"]["message"], "Order not found.");
}

#[tokio::test]
async fn account_orders_require_a_session_and_omit_the_address() {
    let fixture = fixture().await;

    let (status, _, error) = get_json(&fixture.app, "/api/account/orders").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error["error"]["code"], "unauthorized");
    assert_eq!(error["error"]["message"], "Sign in to continue.");

    let (status, headers, registered) = register(&fixture.app, "amina@example.com").await;
    assert_eq!(status, StatusCode::CREATED);
    let user_id = registered["user"]["id"]
        .as_str()
        .expect("user id")
        .to_string();
    let session = cookie_value(&headers, "azahar_session").expect("session cookie");

    fixture
        .store
        .create_order(
            fixture.store.conn(),
            &order(
                "ord_one",
                "AZ-2026-0002",
                "token-one",
                Some(&user_id),
                "2026-03-02T10:00:00Z",
                "1 Hidden Street",
            ),
            &[item("ord_one", "oi_2", "oils")],
        )
        .await
        .expect("create first order");
    fixture
        .store
        .create_order(
            fixture.store.conn(),
            &order(
                "ord_two",
                "AZ-2026-0003",
                "token-two",
                Some(&user_id),
                "2026-03-03T10:00:00Z",
                "2 Hidden Street",
            ),
            &[],
        )
        .await
        .expect("create second order");
    fixture
        .store
        .create_order(
            fixture.store.conn(),
            &order(
                "ord_other",
                "AZ-2026-0004",
                "token-other",
                None,
                "2026-03-04T10:00:00Z",
                "Other Street",
            ),
            &[],
        )
        .await
        .expect("create unrelated order");

    let request = with_cookies(
        Request::builder()
            .uri("/api/account/orders")
            .body(Body::empty())
            .expect("request"),
        &[("azahar_session", &session)],
    );
    let (status, _, body) = body_json(send(&fixture.app, request).await).await;
    assert_eq!(status, StatusCode::OK);
    let orders = body["orders"].as_array().expect("orders array");
    let ids: Vec<&str> = orders
        .iter()
        .map(|order| order["id"].as_str().expect("order id"))
        .collect();
    assert_eq!(ids, ["ord_two", "ord_one"], "newest first");
    assert!(orders[0].get("address").is_none());
    assert!(orders[1].get("address").is_none());
    assert!(orders[0].get("token").is_none());
    assert_eq!(orders[1]["customer"]["id"], user_id);
    assert_eq!(orders[0]["items"], json!([]));
    assert_eq!(orders[1]["items"][0]["plugin_id"], "oils");
}

// ---------------------------------------------------------------------------
// Read cache
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cached_reads_stay_stale_until_a_write_clears_them() {
    let fixture = fixture().await;

    let (status, _, first) = get_json(&fixture.app, "/api/categories").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!category_slugs(&first).contains(&"fresh"));

    insert_category(
        &fixture.store,
        "cat_new",
        "fresh",
        "Fresh",
        "New shelf",
        1,
        "core",
    )
    .await;
    insert_product(
        &fixture.store,
        &SeedProduct {
            id: "p_new",
            slug: "new-soap",
            name: "New soap",
            subtitle: "",
            price_cents: 100_000,
            category_id: "cat_new",
            plugin_id: "core",
            concerns: "[]",
            attributes: "{}",
            featured: false,
            active: true,
            created_at: "2026-04-01T00:00:00Z",
        },
    )
    .await;

    let (_, _, stale) = get_json(&fixture.app, "/api/categories").await;
    assert!(
        !category_slugs(&stale).contains(&"fresh"),
        "the 5 second TTL still serves the first body"
    );

    let login = signed_json(
        "/api/auth/login",
        &json!({"email": "nobody@example.com", "password": "1234567890"}),
    );
    let (status, _, _) = body_json(send(&fixture.app, login).await).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (_, _, fresh) = get_json(&fixture.app, "/api/categories").await;
    assert!(
        category_slugs(&fresh).contains(&"fresh"),
        "a POST clears the cache before the handler runs"
    );
}

#[tokio::test]
async fn conditional_get_headers_do_not_produce_304() {
    let fixture = fixture().await;
    let request = Request::builder()
        .uri("/api/products")
        .header(header::IF_NONE_MATCH, "\"anything\"")
        .header(header::IF_MODIFIED_SINCE, "Wed, 21 Oct 2099 07:28:00 GMT")
        .body(Body::empty())
        .expect("request");
    let (status, _, body) = body_json(send(&fixture.app, request).await).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!product_names(&body).is_empty());
}

#[tokio::test]
async fn cache_hit_keeps_the_handler_headers_and_current_request_id() {
    let fixture = fixture().await;
    let first = Request::builder()
        .uri("/api/plugins")
        .header("x-request-id", "first-request")
        .body(Body::empty())
        .expect("request");
    let (status, headers, _) = body_json(send(&fixture.app, first).await).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["x-request-id"], "first-request");

    let second = Request::builder()
        .uri("/api/plugins")
        .header("x-request-id", "second-request")
        .body(Body::empty())
        .expect("request");
    let (status, headers, _) = body_json(send(&fixture.app, second).await).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers["x-request-id"], "second-request",
        "the cached response never leaks the first request id"
    );
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "application/json; charset=utf-8"
    );
}

#[tokio::test]
async fn long_search_queries_are_truncated_to_100_runes() {
    let fixture = fixture().await;
    let name = "a".repeat(100);
    insert_product(
        &fixture.store,
        &SeedProduct {
            id: "p_long",
            slug: "long-name",
            name: &name,
            subtitle: "",
            price_cents: 100_000,
            category_id: "cat_core",
            plugin_id: "core",
            concerns: "[]",
            attributes: "{}",
            featured: false,
            active: true,
            created_at: "2026-05-01T00:00:00Z",
        },
    )
    .await;

    let query = "a".repeat(101);
    let (status, _, body) = get_json(&fixture.app, &format!("/api/products?q={query}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        product_names(&body),
        [name],
        "the 101st rune is dropped before the LIKE search"
    );
}

#[tokio::test]
async fn error_responses_are_not_cached() {
    let fixture = fixture().await;
    let (status, _, _) = get_json(&fixture.app, "/api/products/late-product").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    insert_product(
        &fixture.store,
        &SeedProduct {
            id: "p_late",
            slug: "late-product",
            name: "Late product",
            subtitle: "",
            price_cents: 100_000,
            category_id: "cat_core",
            plugin_id: "core",
            concerns: "[]",
            attributes: "{}",
            featured: false,
            active: true,
            created_at: "2026-06-01T00:00:00Z",
        },
    )
    .await;

    let (status, _, body) = get_json(&fixture.app, "/api/products/late-product").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["product"]["name"], "Late product");
}

struct RichPlugin;

impl azahar_plugins::Plugin for RichPlugin {
    fn manifest(&self) -> azahar_plugins::Manifest {
        azahar_plugins::Manifest {
            id: "rich".to_string(),
            name: "Rich data".to_string(),
            version: "1.0.0".to_string(),
            description: "Adds computed fields.".to_string(),
            ..azahar_plugins::Manifest::default()
        }
    }

    fn as_product_enricher(&self) -> Option<&dyn azahar_plugins::ProductEnricher> {
        Some(self)
    }
}

impl azahar_plugins::ProductEnricher for RichPlugin {
    fn enrich_product(
        &self,
        product: &azahar_store::Product,
    ) -> Result<HashMap<String, Value>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(HashMap::from([(
            "computed_slug".to_string(),
            json!(product.slug),
        )]))
    }
}

struct QuietPlugin;

impl azahar_plugins::Plugin for QuietPlugin {
    fn manifest(&self) -> azahar_plugins::Manifest {
        azahar_plugins::Manifest {
            id: "quiet".to_string(),
            name: "Quiet data".to_string(),
            version: "1.0.0".to_string(),
            description: "Enriches with nothing.".to_string(),
            ..azahar_plugins::Manifest::default()
        }
    }

    fn as_product_enricher(&self) -> Option<&dyn azahar_plugins::ProductEnricher> {
        Some(self)
    }
}

impl azahar_plugins::ProductEnricher for QuietPlugin {
    fn enrich_product(
        &self,
        _product: &azahar_store::Product,
    ) -> Result<HashMap<String, Value>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(HashMap::new())
    }
}

#[tokio::test]
async fn plugin_data_is_only_served_for_non_core_products_with_a_value() {
    let fixture = fixture_with(ShippingMode::Ready, |registry| {
        registry
            .register(Arc::new(RichPlugin))
            .expect("register rich plugin");
        registry
            .register(Arc::new(QuietPlugin))
            .expect("register quiet plugin");
    })
    .await;

    for (id, slug, plugin_id) in [
        ("p_rich", "rich-product", "rich"),
        ("p_quiet", "quiet-product", "quiet"),
    ] {
        insert_product(
            &fixture.store,
            &SeedProduct {
                id,
                slug,
                name: slug,
                subtitle: "",
                price_cents: 100_000,
                category_id: "cat_core",
                plugin_id,
                concerns: "[]",
                attributes: "{}",
                featured: false,
                active: true,
                created_at: "2026-07-01T00:00:00Z",
            },
        )
        .await;
    }

    let (status, _, body) = get_json(&fixture.app, "/api/products/rich-product").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["product"]["plugin_data"],
        json!({"computed_slug": "rich-product"})
    );
    assert_eq!(body["product"]["plugin_name"], "Rich data");
    assert_eq!(body["product"]["option_schema"], json!([]));

    let (status, _, body) = get_json(&fixture.app, "/api/products/quiet-product").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["product"].get("plugin_data").is_none(),
        "an empty enrichment map is omitted, like Go's omitempty"
    );
}
