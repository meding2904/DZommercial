//! Admin surface: orders, customers, team, preferences, counter sale, products,
//! plugins, metrics, storefront theme and product images.
//!
//! Every test drives the real router with `tower::ServiceExt::oneshot` against
//! an in-memory libSQL database seeded with core, oils, phone and pcparts rows.
//! Sessions are created by the real login route, so the guards and the CSRF
//! middleware run exactly as in production.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header};
use axum::response::Response;
use azahar_auth::{hash_password, hash_token};
use azahar_config::Config;
use azahar_courier::{
    APIError, Commune, Courier, Desk, Error as CourierError, Shipment, Tracking, Wilaya,
};
use azahar_httpapi::session_layer::session_layer;
use azahar_httpapi::session_store::LibsqlSessionStore;
use azahar_httpapi::state::configured_ttl;
use azahar_httpapi::{AppState, build_router};
use azahar_plugins::{Manifest, Plugin, Registry, SettingField, SettingOption};
use azahar_store::{Order, OrderItem, Store, User, format_time, parse_time};
use chrono::Utc;
use libsql::Builder;
use serde_json::{Value, json};
use tower::ServiceExt;

const ORIGIN: &str = "http://localhost:3000";
const CSRF_TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const PASSWORD: &str = "correct horse battery staple";

const ADMIN_EMAIL: &str = "admin@example.com";
const STAFF_EMAIL: &str = "staff@example.com";
const CUSTOMER_EMAIL: &str = "amina@example.com";
const CUSTOMER2_EMAIL: &str = "yacine@example.com";

const TINY_PNG: &[u8] = &[
    0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D', b'R',
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, b'I', b'D', b'A', b'T', b'x', 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, b'I', b'E', b'N', b'D', 0xAE,
    0x42, 0x60, 0x82,
];

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct StubCourier;

#[async_trait::async_trait]
impl Courier for StubCourier {
    async fn wilayas(&self) -> Result<Vec<Wilaya>, CourierError> {
        Err(CourierError::transport("not used"))
    }

    async fn wilaya(&self, _code: i32) -> Result<Wilaya, CourierError> {
        Err(CourierError::UnknownWilaya)
    }

    async fn communes(&self, _wilaya_code: i32) -> Result<Vec<Commune>, CourierError> {
        Err(CourierError::transport("not used"))
    }

    async fn commune(&self, _wilaya_code: i32, _name: &str) -> Result<String, CourierError> {
        Err(CourierError::UnknownCommune)
    }

    async fn desks(&self, _wilaya_code: i32) -> Result<Vec<Desk>, CourierError> {
        Err(CourierError::Api(APIError::default()))
    }

    async fn desk(&self, _wilaya_code: i32, _id: &str) -> Result<Desk, CourierError> {
        Err(CourierError::UnknownDesk)
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
        plugin_secret_key: "azahar-test-plugin-key-2026".to_string(),
        ..Config::default()
    }
}

async fn fixture() -> Fixture {
    fixture_with(|_| {}).await
}

async fn fixture_with(configure: impl FnOnce(&mut Registry)) -> Fixture {
    let database = Builder::new_local(":memory:")
        .build()
        .await
        .expect("open in-memory database");
    let store = Arc::new(Store::new(database).expect("store"));
    azahar_db::migrate(store.conn()).await.expect("migrate");
    let phone_migrations: Vec<_> = azahar_plugins::Plugin::migrations(&azahar_plugin_phone::new())
        .into_iter()
        .map(|migration| azahar_db::PluginMigration {
            plugin_id: "phone".to_string(),
            version: migration.version,
            sql: migration.sql,
        })
        .collect();
    azahar_db::migrate_plugins(store.conn(), phone_migrations)
        .await
        .expect("phone migrations");
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
    let state = AppState::new(cfg.clone(), store.clone(), registry, Arc::new(StubCourier));
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

// ---------------------------------------------------------------------------
// Seed data
// ---------------------------------------------------------------------------

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
    price_cents: i64,
    category_id: &'a str,
    plugin_id: &'a str,
    attributes: &'a str,
    stock: i64,
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
             VALUES (?, ?, ?, '', '', ?, NULL, NULL, '', '', 'DZD', '', ?, '[]', '[]',
             '', '/images/products/seed.webp', '[]', ?, 0, ?, ?, ?, ?, ?)",
            libsql::params![
                product.id,
                product.slug,
                product.name,
                product.price_cents,
                product.category_id,
                product.stock,
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

async fn insert_user(store: &Store, id: &str, name: &str, email: &str, role: &str, created: &str) {
    let hash = hash_password(PASSWORD).expect("hash password");
    store
        .create_user(
            store.conn(),
            &User {
                id: id.to_string(),
                name: name.to_string(),
                email: email.to_string(),
                password_hash: hash,
                role: role.to_string(),
                theme_preset: "cellar".to_string(),
                created_at: parse_time(created),
                updated_at: parse_time(created),
            },
        )
        .await
        .expect("insert user");
}

fn order(
    id: &str,
    number: &str,
    user_id: Option<&str>,
    name: &str,
    status: &str,
    total_cents: i64,
    created_at: &str,
) -> Order {
    Order {
        id: id.to_string(),
        number: number.to_string(),
        user_id: user_id.map(str::to_string),
        name: name.to_string(),
        email: "amina@example.com".to_string(),
        phone: "0550123456".to_string(),
        address: "12 Rue de la Paix".to_string(),
        city: "Alger".to_string(),
        notes: "Leave at the door".to_string(),
        subtotal_cents: total_cents,
        shipping_cents: 0,
        total_cents,
        currency: "DZD".to_string(),
        status: status.to_string(),
        token_hash: hash_token(&format!("token-{id}")),
        created_at: parse_time(created_at),
        updated_at: parse_time(created_at),
        wilaya_code: 16,
        wilaya_name: "Alger".to_string(),
        commune_name: "Alger Centre".to_string(),
        delivery_type: "home".to_string(),
        ..Order::default()
    }
}

fn item(
    order_id: &str,
    id: &str,
    product_id: &str,
    name: &str,
    quantity: i32,
    price: i64,
) -> OrderItem {
    OrderItem {
        id: id.to_string(),
        order_id: order_id.to_string(),
        product_id: product_id.to_string(),
        slug: name.to_lowercase().replace(' ', "-"),
        name: name.to_string(),
        image: "/images/products/seed.webp".to_string(),
        unit_price_cents: price,
        quantity,
        line_total_cents: price * i64::from(quantity),
        options: HashMap::new(),
        options_label: String::new(),
        plugin_id: "core".to_string(),
    }
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
            price_cents: 150_000,
            category_id: "cat_core",
            plugin_id: "",
            attributes: "{}",
            stock: 4,
            active: true,
            created_at: "2026-01-01T00:00:00Z",
        },
    )
    .await;
    insert_product(
        store,
        &SeedProduct {
            id: "p_oil",
            slug: "neroli-oil",
            name: "Neroli oil",
            price_cents: 300_000,
            category_id: "cat_face",
            plugin_id: "oils",
            attributes: r#"{"size":["100 ml"]}"#,
            stock: 10,
            active: true,
            created_at: "2026-01-02T00:00:00Z",
        },
    )
    .await;
    insert_product(
        store,
        &SeedProduct {
            id: "p_phone",
            slug: "pixel-phone",
            name: "Pixel phone",
            price_cents: 9_000_000,
            category_id: "cat_phone",
            plugin_id: "phone",
            attributes: r#"{"config":["12 GB RAM + 256 GB"],"color":["Midnight"]}"#,
            stock: 2,
            active: true,
            created_at: "2026-01-03T00:00:00Z",
        },
    )
    .await;
    insert_product(
        store,
        &SeedProduct {
            id: "p_pc",
            slug: "ryzen-cpu",
            name: "Ryzen CPU",
            price_cents: 4_000_000,
            category_id: "cat_cpu",
            plugin_id: "pcparts",
            attributes: r#"{"socket":["AM5"]}"#,
            stock: 8,
            active: true,
            created_at: "2026-01-04T00:00:00Z",
        },
    )
    .await;
    insert_product(
        store,
        &SeedProduct {
            id: "p_retired",
            slug: "retired-oil",
            name: "Retired oil",
            price_cents: 100_000,
            category_id: "cat_face",
            plugin_id: "oils",
            attributes: r#"{"size":["50 ml"]}"#,
            stock: 1,
            active: false,
            created_at: "2026-01-05T00:00:00Z",
        },
    )
    .await;

    insert_user(
        store,
        "u_admin",
        "Amina Admin",
        ADMIN_EMAIL,
        "admin",
        "2026-01-01T09:00:00Z",
    )
    .await;
    insert_user(
        store,
        "u_staff",
        "Sam Staff",
        STAFF_EMAIL,
        "employee",
        "2026-01-02T09:00:00Z",
    )
    .await;
    insert_user(
        store,
        "u_customer",
        "Amina Customer",
        CUSTOMER_EMAIL,
        "customer",
        "2026-01-03T09:00:00Z",
    )
    .await;
    insert_user(
        store,
        "u_customer2",
        "Yacine Customer",
        CUSTOMER2_EMAIL,
        "customer",
        "2026-01-04T09:00:00Z",
    )
    .await;

    store
        .create_order(
            store.conn(),
            &order(
                "ord_pending",
                "AZ-2026-0001",
                Some("u_customer"),
                "Amina Customer",
                "pending",
                150_000,
                "2026-11-20T10:00:00Z",
            ),
            &[item(
                "ord_pending",
                "oi_1",
                "p_core",
                "Gentle soap",
                1,
                150_000,
            )],
        )
        .await
        .expect("insert pending order");
    store
        .create_order(
            store.conn(),
            &order(
                "ord_shipped",
                "AZ-2026-0002",
                Some("u_customer"),
                "Amina Customer",
                "shipped",
                200_000,
                "2026-11-22T10:00:00Z",
            ),
            &[item(
                "ord_shipped",
                "oi_2",
                "p_oil",
                "Neroli oil",
                2,
                100_000,
            )],
        )
        .await
        .expect("insert shipped order");
    store
        .create_order(
            store.conn(),
            &order(
                "ord_cancelled",
                "AZ-2026-0003",
                None,
                "Guest Buyer",
                "cancelled",
                50_000,
                "2026-11-23T10:00:00Z",
            ),
            &[],
        )
        .await
        .expect("insert cancelled order");

    store
        .conn()
        .execute(
            "INSERT INTO order_sequences (year, last_value) VALUES (2026, 900)",
            (),
        )
        .await
        .expect("seed order sequence");

    store
        .conn()
        .execute(
            "INSERT INTO storefront_settings (id, data, updated_at) VALUES ('default', ?, ?)",
            libsql::params![
                r#"{"brand_name":"Test House","font_scale":1.5}"#,
                "2026-02-03T04:05:06Z"
            ],
        )
        .await
        .expect("insert theme");
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

fn authed(method: Method, uri: &str, session: &str) -> Request<Body> {
    with_cookies(
        with_csrf_headers(request(method, uri)),
        &[("azahar_session", session), ("azahar_csrf", CSRF_TOKEN)],
    )
}

fn authed_json(method: Method, uri: &str, session: &str, body: &Value) -> Request<Body> {
    let mut request = authed(method, uri, session);
    request.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    *request.body_mut() = Body::from(body.to_string());
    request
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

async fn body_bytes(response: Response) -> (StatusCode, HeaderMap, Vec<u8>) {
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    (status, headers, bytes.to_vec())
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

async fn login(app: &Router, email: &str) -> String {
    let mut request = with_cookies(
        with_csrf_headers(request(Method::POST, "/api/auth/login")),
        &[("azahar_csrf", CSRF_TOKEN)],
    );
    request.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    *request.body_mut() = Body::from(json!({"email": email, "password": PASSWORD}).to_string());
    let (status, headers, body) = body_json(send(app, request).await).await;
    assert_eq!(status, StatusCode::OK, "login {email}: {body}");
    cookie_value(&headers, "azahar_session").expect("session cookie")
}

async fn get(uri: &str) -> Request<Body> {
    request(Method::GET, uri)
}

async fn get_json(app: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, Value) {
    body_json(send(app, request).await).await
}

fn problem_field<'a>(body: &'a Value, key: &str) -> &'a str {
    body["error"]["fields"][key]
        .as_str()
        .unwrap_or_else(|| panic!("missing field {key} in {body}"))
}

async fn latest_audit(store: &Store, action: &str) -> Option<(String, String)> {
    let mut rows = store
        .conn()
        .query(
            "SELECT entity_id, detail FROM audit_outbox WHERE action = ? ORDER BY rowid DESC LIMIT 1",
            libsql::params![action],
        )
        .await
        .expect("audit query");
    rows.next()
        .await
        .expect("audit row")
        .map(|row| (row.get(0).expect("entity"), row.get(1).expect("detail")))
}

async fn audit_count(store: &Store, action: &str) -> i64 {
    let mut rows = store
        .conn()
        .query(
            "SELECT COUNT(*) FROM audit_outbox WHERE action = ?",
            libsql::params![action],
        )
        .await
        .expect("audit count");
    rows.next()
        .await
        .expect("count row")
        .expect("count")
        .get(0)
        .expect("count value")
}

async fn stock(store: &Store, id: &str) -> i64 {
    let mut rows = store
        .conn()
        .query(
            "SELECT stock FROM products WHERE id = ?",
            libsql::params![id],
        )
        .await
        .expect("stock query");
    rows.next()
        .await
        .expect("stock row")
        .expect("stock")
        .get(0)
        .expect("stock value")
}

async fn product_active(store: &Store, id: &str) -> bool {
    let mut rows = store
        .conn()
        .query(
            "SELECT active FROM products WHERE id = ?",
            libsql::params![id],
        )
        .await
        .expect("active query");
    let active: i64 = rows
        .next()
        .await
        .expect("active row")
        .expect("active")
        .get(0)
        .expect("active value");
    active != 0
}

fn multipart_body(boundary: &str, filename: &str, bytes: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

fn upload_request(session: &str, boundary: &str, body: Vec<u8>) -> Request<Body> {
    let mut request = authed(Method::POST, "/api/admin/product-images", session);
    request.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&format!("multipart/form-data; boundary={boundary}"))
            .expect("content type"),
    );
    *request.body_mut() = Body::from(body);
    request
}

// ---------------------------------------------------------------------------
// Guards
// ---------------------------------------------------------------------------

#[tokio::test]
async fn admin_routes_enforce_user_staff_and_admin_guards() {
    let fixture = fixture().await;
    let staff = login(&fixture.app, STAFF_EMAIL).await;
    let customer = login(&fixture.app, CUSTOMER_EMAIL).await;

    // No session: 401 everywhere under /api/admin.
    for uri in [
        "/api/admin/orders",
        "/api/admin/customers",
        "/api/admin/metrics",
        "/api/admin/products",
        "/api/admin/team",
        "/api/admin/plugins",
        "/api/admin/preferences",
        "/api/admin/storefront/theme",
    ] {
        let (status, _, body) = get_json(&fixture.app, get(uri).await).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}");
        assert_eq!(body["error"]["code"], "unauthorized", "{uri}");
        assert_eq!(body["error"]["message"], "Sign in to continue.", "{uri}");
    }

    // A customer account has no staff access.
    let (status, _, body) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/orders", &customer),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "forbidden");
    assert_eq!(
        body["error"]["message"],
        "This account does not have access."
    );

    // Staff may read orders, customers, products, metrics and preferences.
    for uri in [
        "/api/admin/orders",
        "/api/admin/customers",
        "/api/admin/metrics",
        "/api/admin/products",
        "/api/admin/preferences",
    ] {
        let (status, _, body) = get_json(&fixture.app, authed(Method::GET, uri, &staff)).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {body}");
    }

    // The admin-only group refuses staff.
    let admin_only = [
        (Method::POST, "/api/admin/products"),
        (Method::GET, "/api/admin/team"),
        (Method::GET, "/api/admin/plugins"),
        (Method::GET, "/api/admin/storefront/theme"),
        (Method::POST, "/api/admin/product-images"),
        (Method::GET, "/api/admin/plugins/oils/settings"),
    ];
    for (method, uri) in admin_only {
        let (status, _, body) = get_json(&fixture.app, authed(method, uri, &staff)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri}: {body}");
    }
}

// ---------------------------------------------------------------------------
// Orders
// ---------------------------------------------------------------------------

#[tokio::test]
async fn orders_list_filters_and_status_changes() {
    let fixture = fixture().await;
    let staff = login(&fixture.app, STAFF_EMAIL).await;

    let (status, _, body) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/orders", &staff),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let orders = body["orders"].as_array().expect("orders array");
    assert_eq!(orders.len(), 3);
    assert_eq!(orders[0]["id"], "ord_cancelled", "newest first");
    assert_eq!(orders[1]["id"], "ord_shipped");
    assert_eq!(orders[2]["id"], "ord_pending");
    assert_eq!(orders[1]["address"], "12 Rue de la Paix");
    assert!(
        orders[1].get("token").is_none(),
        "the list never exposes the order token"
    );
    assert_eq!(orders[1]["items"][0]["plugin_id"], "core");
    assert_eq!(orders[1]["customer"]["id"], "u_customer");

    let (_, _, shipped) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/orders?status=shipped", &staff),
    )
    .await;
    assert_eq!(shipped["orders"].as_array().map(Vec::len), Some(1));
    assert_eq!(shipped["orders"][0]["id"], "ord_shipped");

    let (_, _, searched) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/orders?q=AZ-2026-0001", &staff),
    )
    .await;
    assert_eq!(searched["orders"].as_array().map(Vec::len), Some(1));
    assert_eq!(searched["orders"][0]["id"], "ord_pending");

    let (status, _, bad) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/orders?status=lost", &staff),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(bad["error"]["code"], "validation_error");
    assert_eq!(problem_field(&bad, "status"), "Unknown order status.");

    // pending -> confirmed is allowed for staff.
    let (status, _, changed) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                "/api/admin/orders/ord_pending/status",
                &staff,
                &json!({"status": "confirmed"}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(changed["order"]["status"], "confirmed");
    assert_eq!(changed["order"]["id"], "ord_pending");
    assert_eq!(audit_count(&fixture.store, "order.status").await, 1);

    // confirmed -> delivered skips a step.
    let (status, _, invalid) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                "/api/admin/orders/ord_pending/status",
                &staff,
                &json!({"status": "delivered"}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(invalid["error"]["code"], "invalid_transition");
    assert_eq!(
        problem_field(&invalid, "status"),
        "This order cannot move from confirmed to delivered."
    );

    // Unknown status and body errors.
    let (status, _, unknown) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                "/api/admin/orders/ord_pending/status",
                &staff,
                &json!({"status": "lost"}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem_field(&unknown, "status"), "Unknown order status.");

    let (status, _, unknown_field) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                "/api/admin/orders/ord_pending/status",
                &staff,
                &json!({"status": "confirmed", "extra": 1}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        unknown_field["error"]["message"],
        "The request body contains an unknown field."
    );

    // Missing order.
    let (status, _, missing) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                "/api/admin/orders/nope/status",
                &staff,
                &json!({"status": "confirmed"}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing["error"]["message"], "Order not found.");

    // Only an administrator can cancel, and a cancellation restocks.
    let (status, _, refused) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                "/api/admin/orders/ord_pending/status",
                &staff,
                &json!({"status": "cancelled"}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        refused["error"]["message"],
        "Only an administrator can cancel an order."
    );

    let admin = login(&fixture.app, ADMIN_EMAIL).await;
    let before = stock(&fixture.store, "p_core").await;
    let (status, _, cancelled) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                "/api/admin/orders/ord_pending/status",
                &admin,
                &json!({"status": "cancelled"}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cancelled}");
    assert_eq!(cancelled["order"]["status"], "cancelled");
    assert_eq!(stock(&fixture.store, "p_core").await, before + 1);
}

// ---------------------------------------------------------------------------
// Customers and team
// ---------------------------------------------------------------------------

#[tokio::test]
async fn customers_and_team_match_the_go_dtos() {
    let fixture = fixture().await;
    let admin = login(&fixture.app, ADMIN_EMAIL).await;

    let (status, _, body) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/customers", &admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let customers = body["customers"].as_array().expect("customers array");
    assert_eq!(customers.len(), 2);
    assert_eq!(customers[0]["id"], "u_customer2", "newest first");
    assert_eq!(customers[0]["orders_count"], 0);
    assert_eq!(customers[0]["total_spent_cents"], 0);
    assert!(customers[0]["last_order_at"].is_null());
    assert_eq!(customers[0]["created_at"], "2026-01-04T09:00:00Z");

    assert_eq!(customers[1]["id"], "u_customer");
    assert_eq!(customers[1]["orders_count"], 2);
    assert_eq!(customers[1]["total_spent_cents"], 350_000);
    assert_eq!(customers[1]["last_order_at"], "2026-11-22T10:00:00Z");
    assert_eq!(customers[1]["email"], CUSTOMER_EMAIL);

    let (status, _, team) =
        get_json(&fixture.app, authed(Method::GET, "/api/admin/team", &admin)).await;
    assert_eq!(status, StatusCode::OK);
    let users = team["users"].as_array().expect("users array");
    let roles: Vec<&str> = users
        .iter()
        .map(|user| user["role"].as_str().expect("role"))
        .collect();
    assert_eq!(roles, ["admin", "employee", "customer", "customer"]);
    assert_eq!(users[0]["email"], ADMIN_EMAIL);
    assert_eq!(users[2]["email"], CUSTOMER2_EMAIL, "customers newest first");

    // Promote the first customer to employee.
    let (status, _, updated) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                "/api/admin/team/u_customer",
                &admin,
                &json!({"role": "employee"}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["user"]["role"], "employee");
    assert_eq!(updated["user"]["created_at"], "2026-01-03T09:00:00Z");
    let (entity, detail) = latest_audit(&fixture.store, "user.role")
        .await
        .expect("user.role audit");
    assert_eq!(entity, "u_customer");
    assert_eq!(detail, r#"{"from":"customer","to":"employee"}"#);

    // Invalid role and missing user.
    let (status, _, invalid) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                "/api/admin/team/u_customer",
                &admin,
                &json!({"role": "owner"}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&invalid, "role"),
        "Choose admin, employee or customer."
    );

    let (status, _, missing) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                "/api/admin/team/nope",
                &admin,
                &json!({"role": "employee"}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing["error"]["message"], "User not found.");

    // The signed-in administrator cannot demote themselves.
    let (status, _, self_demote) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                "/api/admin/team/u_admin",
                &admin,
                &json!({"role": "employee"}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&self_demote, "role"),
        "You cannot remove your own administrator role."
    );
}

// ---------------------------------------------------------------------------
// Preferences
// ---------------------------------------------------------------------------

#[tokio::test]
async fn preferences_validate_and_audit() {
    let fixture = fixture().await;
    let staff = login(&fixture.app, STAFF_EMAIL).await;

    let (status, _, body) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/preferences", &staff),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"theme_preset": "cellar"}));

    let (status, _, updated) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PUT,
                "/api/admin/preferences",
                &staff,
                &json!({"theme_preset": " plaster "}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated, json!({"theme_preset": "plaster"}));
    let user = fixture
        .store
        .user_by_id("u_staff")
        .await
        .expect("staff row");
    assert_eq!(user.theme_preset, "plaster");
    let (entity, detail) = latest_audit(&fixture.store, "preferences.update")
        .await
        .expect("preferences audit");
    assert_eq!(entity, "u_staff");
    assert_eq!(detail, r#"{"theme_preset":"plaster"}"#);

    for value in ["", "PLASTER", "rose"] {
        let (status, _, invalid) = body_json(
            send(
                &fixture.app,
                authed_json(
                    Method::PUT,
                    "/api/admin/preferences",
                    &staff,
                    &json!({"theme_preset": value}),
                ),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{value}");
        assert_eq!(
            problem_field(&invalid, "theme_preset"),
            "Choose plaster, cellar, terracotta or lagoon."
        );
    }

    let (status, _, null_preset) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PUT,
                "/api/admin/preferences",
                &staff,
                &json!({"theme_preset": null}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&null_preset, "theme_preset"),
        "Choose plaster, cellar, terracotta or lagoon."
    );
}

// ---------------------------------------------------------------------------
// Counter sale
// ---------------------------------------------------------------------------

#[tokio::test]
async fn counter_sale_records_a_delivered_pickup_and_moves_stock() {
    let fixture = fixture().await;
    let staff = login(&fixture.app, STAFF_EMAIL).await;
    fixture
        .store
        .conn()
        .execute(
            "INSERT INTO phone_devices (id, product_id, variant_key, config, color, imei, status, order_id, order_item_id, created_at, updated_at)
             VALUES ('phn_counter', 'p_phone', '', '12 GB RAM + 256 GB', 'Midnight', '356938035640003', 'in_stock', '', '', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            (),
        )
        .await
        .expect("seed phone device");

    let (status, _, body) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::POST,
                "/api/admin/sales",
                &staff,
                &json!({
                    "items": [
                        {"product_id": "p_core", "quantity": 2},
                        {"product_id": "p_phone", "quantity": 1, "options": {"config": "12 GB RAM + 256 GB", "color": "Midnight"}}
                    ],
                    "customer_name": " Walk-in ",
                    "phone": "0550 12 34 56",
                    "note": " Paid cash "
                }),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let order = &body["order"];
    assert_eq!(order["status"], "delivered");
    assert_eq!(order["delivery_type"], "pickup");
    assert_eq!(order["name"], "Walk-in");
    assert_eq!(order["phone"], "0550 12 34 56");
    assert_eq!(order["notes"], "Paid cash");
    assert_eq!(order["total_cents"], 9_300_000);
    assert_eq!(order["currency"], "DZD");
    assert_eq!(order["items"].as_array().map(Vec::len), Some(2));
    assert!(
        order["token"]
            .as_str()
            .is_some_and(|token| !token.is_empty()),
        "the counter response carries the plaintext token"
    );
    assert_eq!(audit_count(&fixture.store, "order.counter_sale").await, 1);
    let (entity, detail) = latest_audit(&fixture.store, "order.counter_sale")
        .await
        .expect("counter audit");
    assert_eq!(entity, order["id"].as_str().expect("order id"));
    assert!(detail.contains("\"channel\":\"counter\""), "{detail}");
    assert!(detail.contains("\"items\":2"), "{detail}");
    assert_eq!(stock(&fixture.store, "p_core").await, 2);
    assert_eq!(stock(&fixture.store, "p_phone").await, 1);

    // Validation matrix.
    let cases: Vec<(Value, &str, &str)> = vec![
        (
            json!({"items": []}),
            "items",
            "Add between 1 and 20 products.",
        ),
        (
            json!({"items": [{"product_id": "p_core", "quantity": 2}], "customer_name": "x"}),
            "customer_name",
            "Use between 2 and 120 characters, or leave it empty.",
        ),
        (
            json!({"items": [{"product_id": "p_core", "quantity": 2}], "phone": "1234"}),
            "phone",
            "Use an Algerian mobile number, for example 0550 12 34 56.",
        ),
        (
            json!({"items": [{"product_id": "p_core", "quantity": 2}], "note": "x".repeat(501)}),
            "note",
            "Keep the note under 500 characters.",
        ),
        (
            json!({"items": [{"product_id": "p_core", "quantity": 0}]}),
            "items",
            "Use between 1 and 10 units per product.",
        ),
    ];
    for (payload, key, message) in cases {
        let (status, _, invalid) = body_json(
            send(
                &fixture.app,
                authed_json(Method::POST, "/api/admin/sales", &staff, &payload),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{payload}");
        assert_eq!(problem_field(&invalid, key), message, "{payload}");
    }

    let (status, _, unavailable) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::POST,
                "/api/admin/sales",
                &staff,
                &json!({"items": [{"product_id": "p_missing", "quantity": 1}]}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        unavailable["error"]["message"],
        "Some products are no longer available."
    );

    let (status, _, options) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::POST,
                "/api/admin/sales",
                &staff,
                &json!({"items": [{"product_id": "p_phone", "quantity": 1}]}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        options["error"]["message"],
        "Some product options have changed."
    );
    assert_eq!(
        problem_field(&options, "items"),
        "Choose a color for every product."
    );
}

// ---------------------------------------------------------------------------
// Products
// ---------------------------------------------------------------------------

fn core_product(slug: &str) -> Value {
    json!({
        "name": "New soap",
        "slug": slug,
        "subtitle": "A mild bar",
        "description": "Made in small batches.",
        "price_cents": 120_000,
        "size": "100 g",
        "category_slug": "skincare",
        "concerns": ["dry"],
        "ingredients": ["olive oil"],
        "how_to": "Use daily.",
        "image": "/images/products/new-soap.webp",
        "gallery": [],
        "stock": 10,
        "featured": false, "active": true,
        "plugin_id": "",
        "attributes": {}
    })
}

fn oil_product(slug: &str) -> Value {
    json!({
        "name": "New oil",
        "slug": slug,
        "price_cents": 250_000,
        "category_slug": "face",
        "image": "/images/products/new-oil.webp",
        "stock": 5,
        "plugin_id": "oils",
        "attributes": {"size": ["100 ml"]}
    })
}

#[tokio::test]
async fn admin_product_list_includes_inactive_rows() {
    let fixture = fixture().await;
    let staff = login(&fixture.app, STAFF_EMAIL).await;

    let (status, _, body) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/products", &staff),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let products = body["products"].as_array().expect("products array");
    assert_eq!(products.len(), 5);
    let slugs: Vec<&str> = products
        .iter()
        .map(|product| product["slug"].as_str().expect("slug"))
        .collect();
    assert!(slugs.contains(&"retired-oil"), "{slugs:?}");
    let retired = products
        .iter()
        .find(|product| product["slug"] == "retired-oil")
        .expect("retired product");
    assert_eq!(retired["active"], false);
    assert_eq!(retired["created_at"], "2026-01-05T00:00:00Z");
    assert_eq!(retired["updated_at"], "2026-01-05T00:00:00Z");
}

#[tokio::test]
async fn admin_product_create_validates_and_writes() {
    let fixture = fixture().await;
    let admin = login(&fixture.app, ADMIN_EMAIL).await;

    let (status, _, created) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::POST,
                "/api/admin/products",
                &admin,
                &core_product("new-soap"),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let product = &created["product"];
    assert_eq!(product["slug"], "new-soap");
    assert_eq!(product["plugin_id"], "core");
    assert_eq!(product["plugin_name"], "Core");
    assert_eq!(product["option_schema"], json!([]));
    assert_eq!(product["category"]["slug"], "skincare");
    assert_eq!(product["active"], true);
    assert_eq!(product["currency"], "DZD");
    assert_eq!(product["concerns"], json!(["dry"]));
    assert_eq!(product["image"], "/images/products/new-soap.webp");
    assert!(
        product["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("prd_"))
    );
    assert_eq!(audit_count(&fixture.store, "product.create").await, 1);
    let (entity, detail) = latest_audit(&fixture.store, "product.create")
        .await
        .expect("product audit");
    assert_eq!(entity, product["id"].as_str().expect("id"));
    assert_eq!(detail, r#"{"slug":"new-soap"}"#);
    assert!(
        fixture
            .store
            .product_by_id(product["id"].as_str().expect("id"), false)
            .await
            .is_ok()
    );

    let (status, _, oil) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::POST,
                "/api/admin/products",
                &admin,
                &oil_product("new-oil"),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{oil}");
    assert_eq!(oil["product"]["plugin_id"], "oils");
    assert_eq!(oil["product"]["plugin_name"], "Oil shelf");
    assert_eq!(oil["product"]["attributes"]["size"], json!(["100 ml"]));
    assert!(
        oil["product"]["option_schema"]
            .as_array()
            .is_some_and(|schema| !schema.is_empty())
    );

    // Missing category.
    let mut missing_category = core_product("no-category");
    missing_category["category_slug"] = json!("missing");
    let (status, _, invalid) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::POST,
                "/api/admin/products",
                &admin,
                &missing_category,
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&invalid, "category_slug"),
        "Choose a category."
    );

    // Category owned by another plugin.
    let mut wrong_category = core_product("wrong-category");
    wrong_category["category_slug"] = json!("face");
    let (status, _, invalid) = body_json(
        send(
            &fixture.app,
            authed_json(Method::POST, "/api/admin/products", &admin, &wrong_category),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&invalid, "category_slug"),
        "Choose a category that belongs to this product type."
    );

    // Core products do not take plugin options.
    let mut core_options = core_product("core-options");
    core_options["attributes"] = json!({"size": ["100 ml"]});
    let (status, _, invalid) = body_json(
        send(
            &fixture.app,
            authed_json(Method::POST, "/api/admin/products", &admin, &core_options),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&invalid, "attributes"),
        "A core product does not take plugin options."
    );

    // Unknown plugin.
    let mut unknown_plugin = core_product("unknown-plugin");
    unknown_plugin["plugin_id"] = json!("ghost");
    let (status, _, invalid) = body_json(
        send(
            &fixture.app,
            authed_json(Method::POST, "/api/admin/products", &admin, &unknown_plugin),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&invalid, "plugin_id"),
        "Choose an installed plugin."
    );

    // Oil options must satisfy the manifest.
    let mut oil_missing_size = oil_product("oil-missing-size");
    oil_missing_size["attributes"] = json!({});
    let (status, _, invalid) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::POST,
                "/api/admin/products",
                &admin,
                &oil_missing_size,
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&invalid, "attributes.size"),
        "Add at least one size value."
    );

    let mut oil_unknown = oil_product("oil-unknown-option");
    oil_unknown["attributes"] = json!({"size": ["100 ml"], "colour": ["rose"]});
    let (status, _, invalid) = body_json(
        send(
            &fixture.app,
            authed_json(Method::POST, "/api/admin/products", &admin, &oil_unknown),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&invalid, "attributes"),
        "Remove options this product type does not define."
    );

    let mut oil_duplicate = oil_product("oil-duplicate");
    oil_duplicate["attributes"] = json!({"size": ["100 ml"], "concerns": ["dry", "dry"]});
    let (status, _, invalid) = body_json(
        send(
            &fixture.app,
            authed_json(Method::POST, "/api/admin/products", &admin, &oil_duplicate),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&invalid, "attributes.concerns"),
        "Each value can appear once."
    );

    // Duplicate slug.
    let (status, _, conflict) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::POST,
                "/api/admin/products",
                &admin,
                &core_product("gentle-soap"),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["error"]["code"], "conflict");
    assert_eq!(conflict["error"]["message"], "That slug is already in use.");
    assert_eq!(
        problem_field(&conflict, "slug"),
        "This slug is already in use."
    );
}

#[tokio::test]
async fn admin_product_create_validation_matrix() {
    let fixture = fixture().await;
    let admin = login(&fixture.app, ADMIN_EMAIL).await;

    struct Case {
        name: &'static str,
        patch: Value,
        field: &'static str,
        message: &'static str,
    }
    let cases = vec![
        Case {
            name: "name",
            patch: json!({"name": "x"}),
            field: "name",
            message: "Use between 2 and 120 characters.",
        },
        Case {
            name: "null name",
            patch: json!({"name": null}),
            field: "name",
            message: "Use between 2 and 120 characters.",
        },
        Case {
            name: "null price",
            patch: json!({"price_cents": null}),
            field: "price_cents",
            message: "Enter a price above 0 and below 100000000.",
        },
        Case {
            name: "slug",
            patch: json!({"slug": "Bad Slug"}),
            field: "slug",
            message: "Use lowercase letters, numbers and hyphens.",
        },
        Case {
            name: "subtitle",
            patch: json!({"subtitle": "x".repeat(161)}),
            field: "subtitle",
            message: "Keep the subtitle under 160 characters.",
        },
        Case {
            name: "description",
            patch: json!({"description": "x".repeat(4001)}),
            field: "description",
            message: "Keep the description under 4000 characters.",
        },
        Case {
            name: "price_cents",
            patch: json!({"price_cents": 0}),
            field: "price_cents",
            message: "Enter a price above 0 and below 100000000.",
        },
        Case {
            name: "compare_at_cents",
            patch: json!({"compare_at_cents": 0}),
            field: "compare_at_cents",
            message: "Enter a compare-at price above 0, or leave it empty.",
        },
        Case {
            name: "sale window without price",
            patch: json!({"sale_starts_at": "2026-12-01T00:00:00Z"}),
            field: "sale_price_cents",
            message: "Add a sale price to use a sale window.",
        },
        Case {
            name: "sale price without window",
            patch: json!({"sale_price_cents": 100_000}),
            field: "sale_starts_at",
            message: "Choose when the sale starts.",
        },
        Case {
            name: "sale price too high",
            patch: json!({
                "sale_price_cents": 200_000,
                "sale_starts_at": "2026-12-01T00:00:00Z",
                "sale_ends_at": "2026-12-10T00:00:00Z"
            }),
            field: "sale_price_cents",
            message: "Enter a sale price above 0 and below the regular price.",
        },
        Case {
            name: "sale window reversed",
            patch: json!({
                "sale_price_cents": 100_000,
                "sale_starts_at": "2026-12-10T00:00:00Z",
                "sale_ends_at": "2026-12-01T00:00:00Z"
            }),
            field: "sale_ends_at",
            message: "The sale must end after it starts.",
        },
        Case {
            name: "size",
            patch: json!({"size": "x".repeat(61)}),
            field: "size",
            message: "Keep the size under 60 characters.",
        },
        Case {
            name: "concerns count",
            patch: json!({"concerns": (0..13).map(|i| format!("c{i}")).collect::<Vec<_>>()}),
            field: "concerns",
            message: "Use at most 12 concerns.",
        },
        Case {
            name: "concern length",
            patch: json!({"concerns": ["x".repeat(81)]}),
            field: "concerns",
            message: "Keep each concern under 80 characters.",
        },
        Case {
            name: "ingredients count",
            patch: json!({"ingredients": (0..13).map(|i| format!("i{i}")).collect::<Vec<_>>()}),
            field: "ingredients",
            message: "Use at most 12 ingredients.",
        },
        Case {
            name: "ingredient length",
            patch: json!({"ingredients": ["x".repeat(121)]}),
            field: "ingredients",
            message: "Keep each ingredient under 120 characters.",
        },
        Case {
            name: "how_to",
            patch: json!({"how_to": "x".repeat(1001)}),
            field: "how_to",
            message: "Keep the instructions under 1000 characters.",
        },
        Case {
            name: "image",
            patch: json!({"image": "https://example.test/a.png"}),
            field: "image",
            message: "Use an uploaded image.",
        },
        Case {
            name: "gallery count",
            patch: json!({"gallery": (0..13).map(|i| format!("/images/products/{i}.webp")).collect::<Vec<_>>()}),
            field: "gallery",
            message: "Use at most 12 images.",
        },
        Case {
            name: "gallery path",
            patch: json!({"gallery": ["/secret/a.png"]}),
            field: "gallery",
            message: "Each image must be an uploaded image.",
        },
        Case {
            name: "stock",
            patch: json!({"stock": 100_001}),
            field: "stock",
            message: "Use a stock count between 0 and 100000.",
        },
    ];

    let mut index = 0;
    for case in cases {
        index += 1;
        let mut payload = core_product(&format!("case-{index}"));
        if let Value::Object(patch) = case.patch {
            for (key, value) in patch {
                payload[key] = value;
            }
        }
        let (status, _, body) = body_json(
            send(
                &fixture.app,
                authed_json(Method::POST, "/api/admin/products", &admin, &payload),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{}", case.name);
        assert_eq!(
            problem_field(&body, case.field),
            case.message,
            "{}",
            case.name
        );
    }

    // Unknown body field.
    let mut unknown = core_product("unknown-field");
    unknown["mystery"] = json!(true);
    let (status, _, body) = body_json(
        send(
            &fixture.app,
            authed_json(Method::POST, "/api/admin/products", &admin, &unknown),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["error"]["message"],
        "The request body contains an unknown field."
    );

    // Zero sale price clears the window silently.
    let mut zero_sale = core_product("zero-sale");
    zero_sale["sale_price_cents"] = json!(0);
    zero_sale["sale_starts_at"] = json!("2026-12-01T00:00:00Z");
    zero_sale["sale_ends_at"] = json!("2026-12-10T00:00:00Z");
    let (status, _, body) = body_json(
        send(
            &fixture.app,
            authed_json(Method::POST, "/api/admin/products", &admin, &zero_sale),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(body["product"]["sale_price_cents"].is_null());
    assert_eq!(body["product"]["sale_starts_at"], "");
}

#[tokio::test]
async fn admin_product_update_is_partial_and_archives() {
    let fixture = fixture().await;
    let admin = login(&fixture.app, ADMIN_EMAIL).await;

    // A name-only patch leaves every other column untouched.
    let (status, _, updated) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                "/api/admin/products/p_core",
                &admin,
                &json!({"name": "  Better soap  "}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["product"]["name"], "Better soap");
    assert_eq!(updated["product"]["slug"], "gentle-soap");
    assert_eq!(updated["product"]["price_cents"], 150_000);
    assert_eq!(updated["product"]["stock"], 4);
    let (_, detail) = latest_audit(&fixture.store, "product.update")
        .await
        .expect("product.update audit");
    assert_eq!(detail, r#"{"fields":["name"],"slug":"gentle-soap"}"#);

    // Null for a required field is a field error.
    let (status, _, invalid) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                "/api/admin/products/p_core",
                &admin,
                &json!({"name": null}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&invalid, "name"),
        "This field cannot be empty."
    );

    // Sale fields apply together; clearing the price clears the window.
    let (status, _, sale) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                "/api/admin/products/p_core",
                &admin,
                &json!({
                    "sale_price_cents": 100_000,
                    "sale_starts_at": "2026-12-01T00:00:00Z",
                    "sale_ends_at": "2026-12-10T00:00:00Z"
                }),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sale}");
    assert_eq!(sale["product"]["sale_price_cents"], 100_000);
    assert_eq!(sale["product"]["sale_starts_at"], "2026-12-01T00:00:00Z");

    let (status, _, cleared) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                "/api/admin/products/p_core",
                &admin,
                &json!({"sale_price_cents": 0}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(cleared["product"]["sale_price_cents"].is_null());
    assert_eq!(cleared["product"]["sale_starts_at"], "");
    assert_eq!(cleared["product"]["sale_ends_at"], "");

    // Bad dates.
    let (status, _, invalid) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                "/api/admin/products/p_core",
                &admin,
                &json!({"sale_starts_at": "tomorrow"}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&invalid, "sale_starts_at"),
        "Use a valid date and time."
    );

    // A category must belong to the product type.
    let (status, _, invalid) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                "/api/admin/products/p_oil",
                &admin,
                &json!({"category_slug": "skincare"}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&invalid, "category_slug"),
        "Choose a category that belongs to this product type."
    );

    // Changing the plugin without its options and category fails.
    let (status, _, invalid) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                "/api/admin/products/p_core",
                &admin,
                &json!({"plugin_id": "oils"}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&invalid, "category_slug"),
        "Choose a category that belongs to this product type."
    );
    assert_eq!(
        problem_field(&invalid, "attributes.size"),
        "Add at least one size value."
    );

    // A stock-only patch ignores the legacy attributes schema.
    fixture
        .store
        .conn()
        .execute(
            "UPDATE products SET attributes = '{}' WHERE id = 'p_oil'",
            (),
        )
        .await
        .expect("blank attributes");
    let (status, _, stock_update) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                "/api/admin/products/p_oil",
                &admin,
                &json!({"stock": 7}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{stock_update}");
    assert_eq!(stock_update["product"]["stock"], 7);

    // Duplicate slug.
    let (status, _, conflict) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                "/api/admin/products/p_core",
                &admin,
                &json!({"slug": "neroli-oil"}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        problem_field(&conflict, "slug"),
        "This slug is already in use."
    );

    // A missing id is reported before the body is decoded.
    let (status, _, missing) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                "/api/admin/products/nope",
                &admin,
                &json!({"name": 42}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing["error"]["message"], "Product not found.");

    // Archive removes it from the active catalogue and writes an audit row.
    let (status, _, bytes) = body_bytes(
        send(
            &fixture.app,
            authed(Method::DELETE, "/api/admin/products/p_core", &admin),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(bytes.is_empty());
    assert!(!product_active(&fixture.store, "p_core").await);
    let (entity, detail) = latest_audit(&fixture.store, "product.archive")
        .await
        .expect("archive audit");
    assert_eq!(entity, "p_core");
    assert_eq!(detail, r#"{"slug":"gentle-soap"}"#);

    let (status, _, _) = get_json(
        &fixture.app,
        authed(Method::DELETE, "/api/admin/products/nope", &admin),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn admin_product_price_patch_checks_the_sale_window() {
    let fixture = fixture().await;
    let admin = login(&fixture.app, ADMIN_EMAIL).await;

    let mut payload = core_product("sale-price-floor");
    payload["price_cents"] = json!(1_000);
    payload["sale_price_cents"] = json!(900);
    payload["sale_starts_at"] = json!("2026-12-01T00:00:00Z");
    payload["sale_ends_at"] = json!("2026-12-10T00:00:00Z");
    let (status, _, created) = body_json(
        send(
            &fixture.app,
            authed_json(Method::POST, "/api/admin/products", &admin, &payload),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["product"]["id"]
        .as_str()
        .expect("product id")
        .to_string();

    // Dropping the regular price below the stored sale price is a field error.
    let (status, _, invalid) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PATCH,
                &format!("/api/admin/products/{id}"),
                &admin,
                &json!({"price_cents": 500}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{invalid}");
    assert_eq!(
        problem_field(&invalid, "sale_price_cents"),
        "Enter a sale price above 0 and below the regular price."
    );
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

#[tokio::test]
async fn metrics_shape_days_and_previous_window() {
    let fixture = fixture().await;
    let admin = login(&fixture.app, ADMIN_EMAIL).await;
    fixture
        .store
        .conn()
        .execute("DELETE FROM order_items", ())
        .await
        .expect("clear items");
    fixture
        .store
        .conn()
        .execute("DELETE FROM orders", ())
        .await
        .expect("clear orders");

    let now = Utc::now();
    let today = now.format("%Y-%m-%d").to_string();
    let today_at = format!("{today}T10:00:00Z");
    let previous_day = (now - chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();
    let previous_at = format!("{previous_day}T10:00:00Z");

    fixture
        .store
        .create_order(
            fixture.store.conn(),
            &order(
                "m_today",
                "AZ-2026-1001",
                None,
                "Today",
                "pending",
                100_000,
                &today_at,
            ),
            &[item("m_today", "mi_1", "p_oil", "Neroli oil", 2, 50_000)],
        )
        .await
        .expect("today order");
    fixture
        .store
        .create_order(
            fixture.store.conn(),
            &order(
                "m_cancelled",
                "AZ-2026-1002",
                None,
                "Cancelled",
                "cancelled",
                500_000,
                &today_at,
            ),
            &[],
        )
        .await
        .expect("cancelled order");
    fixture
        .store
        .create_order(
            fixture.store.conn(),
            &order(
                "m_previous",
                "AZ-2026-1003",
                None,
                "Previous",
                "delivered",
                50_000,
                &previous_at,
            ),
            &[],
        )
        .await
        .expect("previous order");

    let (status, _, body) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/metrics", &admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["demo"], false);
    assert_eq!(body["range"]["days"], 30);
    assert_eq!(body["range"]["to"], today);
    assert_eq!(body["revenue_cents"], 100_000);
    assert_eq!(body["previous_revenue_cents"], 50_000);
    assert_eq!(body["delta_pct"], 100.0);
    assert_eq!(body["orders_count"], 1);
    assert_eq!(body["previous_orders_count"], 1);
    assert_eq!(body["aov_cents"], 100_000);
    assert_eq!(body["units_sold"], 2);
    let series = body["revenue_series"].as_array().expect("series");
    assert_eq!(series.len(), 30);
    assert_eq!(series[29]["date"], today);
    assert_eq!(series[29]["revenue_cents"], 100_000);
    assert_eq!(series[29]["orders"], 1);
    let statuses: Vec<(&str, i64)> = body["status_split"]
        .as_array()
        .expect("status split")
        .iter()
        .map(|row| {
            (
                row["status"].as_str().expect("status"),
                row["count"].as_i64().expect("count"),
            )
        })
        .collect();
    assert_eq!(
        statuses,
        vec![
            ("pending", 1),
            ("confirmed", 0),
            ("packed", 0),
            ("shipped", 0),
            ("delivered", 0),
            ("cancelled", 1),
        ]
    );
    assert_eq!(body["top_products"][0]["product_id"], "p_oil");
    assert_eq!(body["top_products"][0]["units"], 2);
    assert_eq!(body["top_products"][0]["revenue_cents"], 100_000);
    let recent = body["recent_orders"].as_array().expect("recent");
    assert_eq!(recent.len(), 2, "both orders created today");
    assert!(recent.iter().all(|row| {
        row["created_at"]
            .as_str()
            .is_some_and(|value| value.ends_with('Z'))
    }));
    let low_stock = body["low_stock"].as_array().expect("low stock");
    assert_eq!(low_stock[0]["id"], "p_phone");
    assert_eq!(low_stock[0]["stock"], 2);

    for (days, expected) in [("7", 7), ("90", 90)] {
        let (status, _, body) = get_json(
            &fixture.app,
            authed(
                Method::GET,
                &format!("/api/admin/metrics?days={days}"),
                &admin,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["range"]["days"], expected);
        assert_eq!(
            body["revenue_series"].as_array().map(Vec::len),
            Some(expected)
        );
    }

    for bad in ["1", "abc", "30.0", "-7", ""] {
        let uri = format!("/api/admin/metrics?days={bad}");
        let (status, _, body) = get_json(&fixture.app, authed(Method::GET, &uri, &admin)).await;
        if bad.is_empty() {
            assert_eq!(status, StatusCode::OK, "an empty days value is absent");
            continue;
        }
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
        assert_eq!(problem_field(&body, "days"), "Choose 7, 30 or 90.");
    }
}

// ---------------------------------------------------------------------------
// Plugins
// ---------------------------------------------------------------------------

fn setting(
    key: &str,
    label: &str,
    field_type: &str,
    required: bool,
    default: &str,
) -> SettingField {
    SettingField {
        key: key.to_string(),
        label: label.to_string(),
        field_type: field_type.to_string(),
        required,
        default: default.to_string(),
        ..SettingField::default()
    }
}

struct TestPlugin {
    manifest: Manifest,
}

impl Plugin for TestPlugin {
    fn manifest(&self) -> Manifest {
        self.manifest.clone()
    }
}

fn vault_plugin(id: &str, name: &str, dependencies: &[&str]) -> TestPlugin {
    TestPlugin {
        manifest: Manifest {
            id: id.to_string(),
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: "Stores an API credential.".to_string(),
            dependencies: dependencies.iter().map(|id| (*id).to_string()).collect(),
            settings: vec![
                setting("api_url", "API URL", "text", true, ""),
                SettingField {
                    max_length: 100,
                    ..setting("api_key", "API key", "secret", true, "")
                },
                setting("retries", "Retries", "number", false, "3"),
                setting("debug", "Debug", "boolean", false, "false"),
                SettingField {
                    options: vec![
                        SettingOption {
                            value: "fast".to_string(),
                            label: "Fast".to_string(),
                        },
                        SettingOption {
                            value: "safe".to_string(),
                            label: "Safe".to_string(),
                        },
                    ],
                    ..setting("mode", "Mode", "select", false, "fast")
                },
            ],
            ..Manifest::default()
        },
    }
}

fn register_vault(registry: &mut Registry) {
    registry
        .register(Arc::new(vault_plugin("vault", "Vault", &[])))
        .expect("register vault");
    registry
        .register(Arc::new(vault_plugin(
            "vault_child",
            "Child vault",
            &["vault"],
        )))
        .expect("register vault child");
}

#[tokio::test]
async fn admin_plugins_expose_state_and_guard_dependencies() {
    let fixture = fixture_with(register_vault).await;
    let admin = login(&fixture.app, ADMIN_EMAIL).await;

    let (status, _, body) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/plugins", &admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let plugins = body["plugins"].as_array().expect("plugins array");
    let ids: Vec<&str> = plugins
        .iter()
        .map(|plugin| plugin["id"].as_str().expect("id"))
        .collect();
    assert_eq!(ids, ["oils", "phone", "pcparts", "vault", "vault_child"]);
    let vault = &plugins[3];
    assert_eq!(vault["name"], "Vault");
    assert_eq!(vault["configured"], false);
    assert_eq!(vault["settings_state"], "ready");
    assert_eq!(vault["enabled"], true);
    assert_eq!(vault["effective_enabled"], true);
    assert_eq!(vault["dependents"], json!(["vault_child"]));
    assert!(vault.get("blocked_by").is_none(), "empty lists are omitted");
    assert!(vault["settings"].as_array().is_some_and(|s| s.len() == 5));
    let child = &plugins[4];
    assert_eq!(child["dependencies"], json!(["vault"]));
    assert_eq!(child["effective_enabled"], true);

    // Missing enabled.
    let (status, _, invalid) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PUT,
                "/api/admin/plugins/vault/state",
                &admin,
                &json!({}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&invalid, "enabled"),
        "Choose whether the plugin is enabled."
    );

    // Unknown plugin wins over a malformed body.
    let (status, _, missing) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PUT,
                "/api/admin/plugins/ghost/state",
                &admin,
                &json!({"enabled": "yes"}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing["error"]["message"], "Plugin not found.");

    // Disabling a dependency with an enabled dependent needs confirmation.
    let (status, _, warning) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PUT,
                "/api/admin/plugins/vault/state",
                &admin,
                &json!({"enabled": false}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(warning["error"]["code"], "plugin_dependency_warning");
    assert_eq!(
        warning["error"]["message"],
        "These enabled plugins depend on Vault: Child vault. They will stop working until it is enabled again."
    );
    assert_eq!(problem_field(&warning, "dependents"), "Child vault");

    let (status, _, disabled) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PUT,
                "/api/admin/plugins/vault/state",
                &admin,
                &json!({"enabled": false, "confirm_dependents": true}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{disabled}");
    assert_eq!(disabled["plugin"]["enabled"], false);
    assert_eq!(disabled["plugin"]["effective_enabled"], false);
    assert!(
        latest_audit(&fixture.store, "plugin.disabled")
            .await
            .is_some()
    );

    let (status, _, child) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PUT,
                "/api/admin/plugins/vault_child/state",
                &admin,
                &json!({"enabled": true}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(child["error"]["code"], "plugin_dependency");
    assert_eq!(
        child["error"]["message"],
        "Enable Vault before enabling Child vault."
    );
    assert_eq!(problem_field(&child, "dependencies"), "Vault");

    let (status, _, enabled) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PUT,
                "/api/admin/plugins/vault/state",
                &admin,
                &json!({"enabled": true}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(enabled["plugin"]["enabled"], true);
    assert_eq!(audit_count(&fixture.store, "plugin.enabled").await, 1);

    // Restarting the registry from the stored flags keeps the same state.
    let states = fixture.store.plugin_states().await.expect("states");
    assert_eq!(states.get("vault"), Some(&true));
}

#[tokio::test]
async fn plugin_settings_mask_merge_and_validate() {
    let fixture = fixture_with(register_vault).await;
    let admin = login(&fixture.app, ADMIN_EMAIL).await;

    let (status, _, body) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/plugins/vault/settings", &admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["plugin_id"], "vault");
    assert_eq!(body["configured"], false);
    assert_eq!(body["secrets_set"], json!([]));
    assert_eq!(body["settings"]["api_url"], "");
    assert_eq!(body["settings"]["api_key"], "");
    assert_eq!(body["settings"]["retries"], "3");
    assert_eq!(body["settings"]["mode"], "fast");
    assert!(body.get("updated_at").is_none());

    // Required secret missing.
    let (status, _, invalid) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PUT,
                "/api/admin/plugins/vault/settings",
                &admin,
                &json!({"settings": {"api_url": "https://api.example"}}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&invalid, "api_key"),
        "This setting is required."
    );

    // Type and schema checks.
    let cases: Vec<(Value, &str, &str)> = vec![
        (
            json!({"api_url": "https://api.example", "api_key": "k", "retries": "many"}),
            "retries",
            "Enter a number.",
        ),
        (
            json!({"api_url": "https://api.example", "api_key": "k", "debug": "yes"}),
            "debug",
            "Choose true or false.",
        ),
        (
            json!({"api_url": "https://api.example", "api_key": "k", "mode": "slow"}),
            "mode",
            "Choose one of the listed options.",
        ),
        (
            json!({"api_url": "https://api.example", "api_key": "k", "extra": "x"}),
            "extra",
            "This plugin has no such setting.",
        ),
    ];
    for (settings, key, message) in cases {
        let (status, _, invalid) = body_json(
            send(
                &fixture.app,
                authed_json(
                    Method::PUT,
                    "/api/admin/plugins/vault/settings",
                    &admin,
                    &json!({"settings": settings}),
                ),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{settings}");
        assert_eq!(problem_field(&invalid, key), message, "{settings}");
    }

    // A complete form saves; the secret never comes back.
    let (status, _, saved) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PUT,
                "/api/admin/plugins/vault/settings",
                &admin,
                &json!({"settings": {
                    "api_url": "https://api.example",
                    "api_key": "super-secret",
                    "retries": "5",
                    "debug": "true",
                    "mode": "safe"
                }}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["configured"], true);
    assert_eq!(saved["secrets_set"], json!(["api_key"]));
    assert_eq!(saved["settings"]["api_key"], "");
    assert_eq!(saved["settings"]["api_url"], "https://api.example");
    assert!(saved["updated_at"].as_str().is_some());
    let (_, detail) = latest_audit(&fixture.store, "plugin.settings_update")
        .await
        .expect("settings audit");
    assert!(detail.contains("\"api_key\""), "{detail}");
    assert!(detail.contains("\"api_url\""), "{detail}");

    let (sealed, _) = fixture
        .store
        .plugin_settings("vault")
        .await
        .expect("stored settings");
    assert!(
        sealed.starts_with("enc:v1:"),
        "settings are encrypted at rest"
    );
    assert!(!sealed.contains("super-secret"));

    let (status, _, body) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/plugins/vault/settings", &admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["configured"], true);
    assert_eq!(body["secrets_set"], json!(["api_key"]));
    assert_eq!(body["settings"]["retries"], "5");

    // A blank secret keeps the stored one.
    let (status, _, _) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PUT,
                "/api/admin/plugins/vault/settings",
                &admin,
                &json!({"settings": {"api_url": "https://api.example/v2", "api_key": ""}}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, body) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/plugins/vault/settings", &admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["settings"]["api_url"], "https://api.example/v2");
    assert_eq!(body["secrets_set"], json!(["api_key"]));
    assert_eq!(body["configured"], true, "the old secret is still stored");

    // A value sealed with another key is unreadable, not an internal error.
    fixture
        .store
        .conn()
        .execute(
            "UPDATE plugin_settings SET settings = 'garbage' WHERE plugin_id = 'vault'",
            (),
        )
        .await
        .expect("corrupt settings");
    let (status, _, unreadable) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/plugins/vault/settings", &admin),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(unreadable["error"]["code"], "plugin_settings_unreadable");
    assert_eq!(
        unreadable["error"]["message"],
        "These settings cannot be read with the current PLUGIN_SECRET_KEY. Enter them again to replace the old values."
    );

    // Writing over an unreadable row is refused before the merge, so the
    // stored settings are not silently replaced by the submitted subset.
    let (status, _, refused) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PUT,
                "/api/admin/plugins/vault/settings",
                &admin,
                &json!({"settings": {"api_url": "https://api.example/v3"}}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refused["error"]["code"], "plugin_settings_unreadable");
    let (sealed, _) = fixture
        .store
        .plugin_settings("vault")
        .await
        .expect("stored settings");
    assert_eq!(sealed, "garbage", "the stored row is unchanged");

    // The admin list reports the same unreadable state.
    let (_, _, list) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/plugins", &admin),
    )
    .await;
    let vault = list["plugins"]
        .as_array()
        .expect("plugins")
        .iter()
        .find(|plugin| plugin["id"] == "vault")
        .expect("vault");
    assert_eq!(vault["settings_state"], "unreadable");
    assert_eq!(vault["configured"], false);
}

// ---------------------------------------------------------------------------
// Storefront theme
// ---------------------------------------------------------------------------

#[tokio::test]
async fn storefront_theme_get_put_and_validation() {
    let fixture = fixture().await;
    let admin = login(&fixture.app, ADMIN_EMAIL).await;

    let (status, _, body) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/storefront/theme", &admin),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["theme"]["brand_name"], "Test House");
    assert_eq!(body["theme"]["font_scale"], 1.5);
    assert_eq!(body["theme"]["updated_at"], "2026-02-03T04:05:06Z");
    assert_eq!(body["defaults"]["brand_name"], "Azahar");

    let (_, _, public) =
        get_json(&fixture.app, request(Method::GET, "/api/storefront/theme")).await;
    let mut theme = public["theme"].clone();
    theme["brand_name"] = json!("Updated House");
    theme["font_scale"] = json!(1.0);
    theme["light"]["accent"] = json!("#123456");

    let (status, _, saved) = body_json(
        send(
            &fixture.app,
            authed_json(Method::PUT, "/api/admin/storefront/theme", &admin, &theme),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["theme"]["brand_name"], "Updated House");
    assert_eq!(saved["theme"]["light"]["accent"], "#123456");
    assert!(saved["theme"]["updated_at"].as_str().is_some());
    let (_, detail) = latest_audit(&fixture.store, "storefront.theme_update")
        .await
        .expect("theme audit");
    let changed: Value = serde_json::from_str(&detail).expect("detail json");
    assert_eq!(
        changed["changed"],
        json!(["brand_name", "font_scale", "light.accent"]),
        "{detail}"
    );

    // Empty payload: every required field is marked.
    let (status, _, invalid) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PUT,
                "/api/admin/storefront/theme",
                &admin,
                &json!({}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        invalid["error"]["message"],
        "Check the marked storefront settings."
    );
    assert_eq!(
        problem_field(&invalid, "brand_name"),
        "Use at least 1 characters."
    );

    // Invalid palette colour.
    let mut bad_colour = theme.clone();
    bad_colour["light"]["accent"] = json!("red");
    let (status, _, invalid) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PUT,
                "/api/admin/storefront/theme",
                &admin,
                &bad_colour,
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&invalid, "light.accent"),
        "Use a six-digit hex colour, for example #2c2325."
    );

    // Unknown top-level field.
    let mut unknown = theme.clone();
    unknown["mystery"] = json!("x");
    let (status, _, invalid) = body_json(
        send(
            &fixture.app,
            authed_json(Method::PUT, "/api/admin/storefront/theme", &admin, &unknown),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        invalid["error"]["message"],
        "The request body contains an unknown field."
    );

    // A bad font name.
    let mut bad_font = theme.clone();
    bad_font["font_display"] = json!("comic");
    let (status, _, invalid) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PUT,
                "/api/admin/storefront/theme",
                &admin,
                &bad_font,
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&invalid, "font_display"),
        "Choose one of the available display fonts."
    );
}

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

#[tokio::test]
async fn image_upload_serves_binary_with_cache_headers() {
    let fixture = fixture().await;
    let admin = login(&fixture.app, ADMIN_EMAIL).await;
    let staff = login(&fixture.app, STAFF_EMAIL).await;

    let (status, _, error) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::POST,
                "/api/admin/product-images",
                &staff,
                &json!({}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error["error"]["code"], "forbidden");

    // Non-multipart body.
    let (status, _, invalid) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::POST,
                "/api/admin/product-images",
                &admin,
                &json!({"image": "x"}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(invalid["error"]["code"], "invalid_upload");
    assert_eq!(
        invalid["error"]["message"],
        "Send the image as multipart form data."
    );

    // No file part.
    let boundary = "azahar-test-boundary";
    let body = b"--azahar-test-boundary--\r\n".to_vec();
    let (status, _, invalid) =
        body_json(send(&fixture.app, upload_request(&admin, boundary, body)).await).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(invalid["error"]["message"], "Choose an image to upload.");

    // A text file is not an accepted image.
    let body = multipart_body(boundary, "notes.txt", b"hello there");
    let (status, _, invalid) =
        body_json(send(&fixture.app, upload_request(&admin, boundary, body)).await).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(invalid["error"]["code"], "invalid_image");
    assert_eq!(
        invalid["error"]["message"],
        "Use a JPEG, PNG or WebP image."
    );

    // A real PNG uploads and returns its immutable URL.
    let body = multipart_body(boundary, "pixel.png", TINY_PNG);
    let (status, _, uploaded) =
        body_json(send(&fixture.app, upload_request(&admin, boundary, body)).await).await;
    assert_eq!(status, StatusCode::CREATED, "{uploaded}");
    let image = &uploaded["image"];
    let id = image["id"].as_str().expect("image id");
    assert!(id.starts_with("img_"));
    assert_eq!(image["url"], format!("/api/images/{id}"));
    assert_eq!(image["mime"], "image/png");
    assert_eq!(image["bytes"], TINY_PNG.len());
    let (_, detail) = latest_audit(&fixture.store, "product_image.create")
        .await
        .expect("image audit");
    assert_eq!(detail, r#"{"mime":"image/png"}"#);

    let (status, headers, bytes) = body_bytes(
        send(
            &fixture.app,
            request(Method::GET, &format!("/api/images/{id}")),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    assert_eq!(headers[header::CONTENT_LENGTH], TINY_PNG.len().to_string());
    assert_eq!(headers[header::CONTENT_DISPOSITION], "inline");
    assert_eq!(
        headers[header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );
    assert_eq!(bytes, TINY_PNG);

    // Unknown and malformed ids are both 404.
    for image_id in [
        "img_000000000000000000000000",
        "not-an-image",
        &"a".repeat(40),
    ] {
        let (status, _, missing) = body_json(
            send(
                &fixture.app,
                request(Method::GET, &format!("/api/images/{image_id}")),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{image_id}");
        assert_eq!(missing["error"]["message"], "Image not found.");
    }

    // Over 5 MiB but inside the multipart ceiling.
    let mut oversized = vec![0u8; (5 << 20) + 1];
    oversized[..8].copy_from_slice(&TINY_PNG[..8]);
    let body = multipart_body(boundary, "huge.png", &oversized);
    let (status, _, too_large) =
        body_json(send(&fixture.app, upload_request(&admin, boundary, body)).await).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        too_large["error"]["message"],
        "The image is too large. Use 5 MB or less."
    );

    // Past the multipart ceiling the route sets its own 6 MiB body limit.
    let body = multipart_body(boundary, "huge.png", &vec![0u8; 7 << 20]);
    let (status, _, too_large) =
        body_json(send(&fixture.app, upload_request(&admin, boundary, body)).await).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        too_large["error"]["message"],
        "The image is too large. Use 5 MB or less."
    );
}

// ---------------------------------------------------------------------------
// Read cache
// ---------------------------------------------------------------------------

#[tokio::test]
async fn admin_reads_are_cached_until_a_write_clears_them() {
    let fixture = fixture().await;
    let admin = login(&fixture.app, ADMIN_EMAIL).await;

    let (_, _, first) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/customers", &admin),
    )
    .await;
    assert_eq!(first["customers"].as_array().map(Vec::len), Some(2));

    let hash = hash_password(PASSWORD).expect("hash");
    fixture
        .store
        .create_user(
            fixture.store.conn(),
            &User {
                id: "u_late".to_string(),
                name: "Late Customer".to_string(),
                email: "late@example.com".to_string(),
                password_hash: hash,
                role: "customer".to_string(),
                theme_preset: "cellar".to_string(),
                created_at: Some(Utc::now()),
                updated_at: Some(Utc::now()),
            },
        )
        .await
        .expect("insert late customer");

    let (_, _, stale) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/customers", &admin),
    )
    .await;
    assert_eq!(
        stale["customers"].as_array().map(Vec::len),
        Some(2),
        "the 5 second TTL serves the first body"
    );

    let (status, _, _) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PUT,
                "/api/admin/preferences",
                &admin,
                &json!({"theme_preset": "lagoon"}),
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, _, fresh) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/customers", &admin),
    )
    .await;
    assert_eq!(
        fresh["customers"].as_array().map(Vec::len),
        Some(3),
        "a write clears the whole read cache"
    );

    // Products, orders and metrics use their own, shorter TTLs but the same
    // write invalidation.
    let (_, _, products) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/products", &admin),
    )
    .await;
    let product_count = products["products"]
        .as_array()
        .map(Vec::len)
        .expect("products");
    insert_product(
        &fixture.store,
        &SeedProduct {
            id: "p_cache",
            slug: "cache-product",
            name: "Cache product",
            price_cents: 100_000,
            category_id: "cat_core",
            plugin_id: "",
            attributes: "{}",
            stock: 1,
            active: true,
            created_at: "2026-09-01T00:00:00Z",
        },
    )
    .await;
    let (_, _, stale_products) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/products", &admin),
    )
    .await;
    assert_eq!(
        stale_products["products"].as_array().map(Vec::len),
        Some(product_count)
    );
    let (_, _, _) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PUT,
                "/api/admin/preferences",
                &admin,
                &json!({"theme_preset": "terracotta"}),
            ),
        )
        .await,
    )
    .await;
    let (_, _, fresh_products) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/products", &admin),
    )
    .await;
    assert_eq!(
        fresh_products["products"].as_array().map(Vec::len),
        Some(product_count + 1)
    );

    let (_, _, metrics) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/metrics", &admin),
    )
    .await;
    let revenue = metrics["revenue_cents"].as_i64().expect("revenue");
    fixture
        .store
        .create_order(
            fixture.store.conn(),
            &order(
                "ord_cache",
                "AZ-2026-9001",
                None,
                "Cache",
                "pending",
                77_000,
                &format_time(&Utc::now()),
            ),
            &[],
        )
        .await
        .expect("insert cached order");
    let (_, _, stale_metrics) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/metrics", &admin),
    )
    .await;
    assert_eq!(
        stale_metrics["revenue_cents"].as_i64(),
        Some(revenue),
        "the 3 second metrics TTL serves the first body"
    );
    let (_, _, _) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PUT,
                "/api/admin/preferences",
                &admin,
                &json!({"theme_preset": "plaster"}),
            ),
        )
        .await,
    )
    .await;
    let (_, _, fresh_metrics) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/metrics", &admin),
    )
    .await;
    assert_eq!(
        fresh_metrics["revenue_cents"].as_i64(),
        Some(revenue + 77_000)
    );

    let (_, _, orders) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/orders", &admin),
    )
    .await;
    let order_count = orders["orders"].as_array().map(Vec::len).expect("orders");
    fixture
        .store
        .create_order(
            fixture.store.conn(),
            &order(
                "ord_late",
                "AZ-2026-9002",
                None,
                "Late",
                "pending",
                1_000,
                &format_time(&Utc::now()),
            ),
            &[],
        )
        .await
        .expect("insert late order");
    let (_, _, stale_orders) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/orders", &admin),
    )
    .await;
    assert_eq!(
        stale_orders["orders"].as_array().map(Vec::len),
        Some(order_count),
        "the 1 second orders TTL serves the first body"
    );
    let (_, _, _) = body_json(
        send(
            &fixture.app,
            authed_json(
                Method::PUT,
                "/api/admin/preferences",
                &admin,
                &json!({"theme_preset": "lagoon"}),
            ),
        )
        .await,
    )
    .await;
    let (_, _, fresh_orders) = get_json(
        &fixture.app,
        authed(Method::GET, "/api/admin/orders", &admin),
    )
    .await;
    assert_eq!(
        fresh_orders["orders"].as_array().map(Vec::len),
        Some(order_count + 1)
    );
}
