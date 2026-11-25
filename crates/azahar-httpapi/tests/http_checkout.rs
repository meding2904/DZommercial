//! Checkout, captcha/challenge and admin shipment routes.
//!
//! `src/lib.rs` is owned by the orchestrator while the admin surface lands, so
//! this test compiles the new modules from source against the crate's public
//! helpers. When `lib.rs` exports them, the `mod` block below can be replaced
//! with plain `azahar_httpapi::{...}` imports without changing the tests.

mod dto {
    pub use azahar_httpapi::dto::*;
}
mod extract {
    pub use azahar_httpapi::extract::*;
}
mod middleware {
    pub use azahar_httpapi::middleware::*;
}
mod ratelimit {
    pub use azahar_httpapi::ratelimit::*;
}
mod respond {
    pub use azahar_httpapi::respond::*;
}
mod session_layer {
    pub use azahar_httpapi::session_layer::*;
}
mod state {
    pub use azahar_httpapi::state::*;
}
mod validate {
    pub use azahar_httpapi::validate::*;
}

#[allow(dead_code)]
#[path = "../src/captcha.rs"]
mod captcha;
#[allow(dead_code)]
#[path = "../src/handlers_checkout.rs"]
mod handlers_checkout;
#[allow(dead_code)]
#[path = "../src/handlers_shipments.rs"]
mod handlers_shipments;
#[allow(dead_code)]
#[path = "../src/turnstile.rs"]
mod turnstile;

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header};
use axum::response::Response;
use azahar_auth::{hash_password, hash_token};
use azahar_config::Config;
use azahar_courier::{
    APIError, Commune, Courier, Desk, Error as CourierError, OrderRequest, Shipment, Tracking,
    TrackingEvent, Wilaya,
};
use azahar_httpapi::AppState;
use azahar_httpapi::session_layer::session_layer;
use azahar_httpapi::session_store::LibsqlSessionStore;
use azahar_httpapi::state::configured_ttl;
use azahar_plugins::Registry;
use azahar_store::{Order, Store, User, parse_time};
use chrono::{DateTime, Utc};
use libsql::Builder;
use serde_json::{Value, json};
use tower::ServiceExt;
use tower_sessions::SessionStore;
use tower_sessions::session::{Id as SessionId, Record as SessionRecord};

use captcha::{CAPTCHA_MAX_ENTRIES, CAPTCHA_MIN_AGE, CAPTCHA_TTL, CaptchaCheck, CaptchaStore};
use handlers_checkout::{CHECKOUT_CHALLENGE_PATH, checkout_routes_with_captcha};
use handlers_shipments::shipment_routes;

const ORIGIN: &str = "http://localhost:3000";
const PASSWORD: &str = "correct horse battery staple";
const PHONE_CONFIG: &str = "12 GB RAM + 256 GB";
const PHONE_COLOR: &str = "Midnight";

// ---------------------------------------------------------------------------
// Courier stub
// ---------------------------------------------------------------------------

#[derive(Default)]
struct StubState {
    map_error: bool,
    create_error: Option<CourierError>,
    track_error: Option<CourierError>,
    created: Vec<OrderRequest>,
    tracked: Vec<(String, String)>,
    shipment: Option<Shipment>,
    tracking: Option<Tracking>,
}

#[derive(Clone, Default)]
struct StubCourier(Arc<Mutex<StubState>>);

impl StubCourier {
    fn state(&self) -> std::sync::MutexGuard<'_, StubState> {
        self.0.lock().expect("courier state")
    }

    fn fail_map(&self) {
        self.state().map_error = true;
    }

    fn fail_create(&self, error: CourierError) {
        self.state().create_error = Some(error);
    }

    fn fail_track(&self, error: CourierError) {
        self.state().track_error = Some(error);
    }

    fn shipment(&self, shipment: Shipment) {
        self.state().shipment = Some(shipment);
    }

    fn tracking(&self, tracking: Tracking) {
        self.state().tracking = Some(tracking);
    }
}

#[async_trait::async_trait]
impl Courier for StubCourier {
    async fn wilayas(&self) -> Result<Vec<Wilaya>, CourierError> {
        Ok(vec![alger()])
    }

    async fn wilaya(&self, code: i32) -> Result<Wilaya, CourierError> {
        if self.state().map_error {
            return Err(CourierError::transport("map down"));
        }
        if code == 16 {
            Ok(alger())
        } else {
            Err(CourierError::UnknownWilaya)
        }
    }

    async fn communes(&self, _wilaya_code: i32) -> Result<Vec<Commune>, CourierError> {
        Err(CourierError::transport("not used"))
    }

    async fn commune(&self, wilaya_code: i32, name: &str) -> Result<String, CourierError> {
        if self.state().map_error {
            return Err(CourierError::transport("map down"));
        }
        if wilaya_code != 16 {
            return Err(CourierError::UnknownCommune);
        }
        match name.trim() {
            "Alger Centre" => Ok("Alger Centre".to_string()),
            "Bab Ezzouar" => Ok("Bab Ezzouar".to_string()),
            _ => Err(CourierError::UnknownCommune),
        }
    }

    async fn desks(&self, _wilaya_code: i32) -> Result<Vec<Desk>, CourierError> {
        Ok(vec![desk_one()])
    }

    async fn desk(&self, wilaya_code: i32, id: &str) -> Result<Desk, CourierError> {
        if self.state().map_error {
            return Err(CourierError::transport("map down"));
        }
        if wilaya_code == 16 && id == "desk_1" {
            Ok(desk_one())
        } else {
            Err(CourierError::UnknownDesk)
        }
    }

    async fn create_order(&self, request: OrderRequest) -> Result<Shipment, CourierError> {
        let mut state = self.state();
        if let Some(error) = state.create_error.clone() {
            return Err(error);
        }
        state.created.push(request);
        Ok(state.shipment.clone().unwrap_or_else(|| Shipment {
            tracking_number: "TRK-1".to_string(),
            courier_reference: "REF-1".to_string(),
            status: "created".to_string(),
            reference: "AZ".to_string(),
        }))
    }

    async fn track(
        &self,
        courier_key: &str,
        tracking_number: &str,
    ) -> Result<Tracking, CourierError> {
        let mut state = self.state();
        if let Some(error) = state.track_error.clone() {
            return Err(error);
        }
        state
            .tracked
            .push((courier_key.to_string(), tracking_number.to_string()));
        Ok(state.tracking.clone().unwrap_or_else(|| Tracking {
            tracking_number: tracking_number.to_string(),
            status: "in_transit".to_string(),
            events: vec![TrackingEvent {
                status: "in_transit".to_string(),
                raw_status: "TRANSIT".to_string(),
                timestamp: "2026-11-24T09:00:00Z".to_string(),
            }],
        }))
    }
}

fn alger() -> Wilaya {
    Wilaya {
        code: 16,
        name_ar: "الجزائر".to_string(),
        name_fr: "Alger".to_string(),
        is_deep_south: false,
        commune_count: 57,
        courier_supported: true,
    }
}

fn desk_one() -> Desk {
    Desk {
        id: "desk_1".to_string(),
        name: "Desk One".to_string(),
        wilaya_code: 16,
        commune_name: "Bab Ezzouar".to_string(),
        address: "1 Desk Road".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

struct Fixture {
    app: Router,
    store: Arc<Store>,
    courier: StubCourier,
    captcha: Arc<CaptchaStore>,
    clock: Arc<Mutex<DateTime<Utc>>>,
}

impl Fixture {
    fn advance(&self, seconds: i64) {
        let mut now = self.clock.lock().expect("clock");
        *now += chrono::Duration::seconds(seconds);
    }

    fn captcha_pair(&self) -> (String, String) {
        let issued = self.captcha.issue("").expect("issue captcha");
        self.advance(3);
        (issued.token, issued.code)
    }
}

fn dev_config() -> Config {
    Config {
        app_env: "development".to_string(),
        app_origin: ORIGIN.to_string(),
        cookie_secure: false,
        trust_proxy: false,
        session_ttl: Duration::from_secs(48 * 60 * 60),
        plugin_secret_key: "azahar-test-plugin-key-2026".to_string(),
        courier_key: "sandbox".to_string(),
        ..Config::default()
    }
}

async fn fixture() -> Fixture {
    fixture_with(|_, _| {}).await
}

async fn fixture_with(configure: impl FnOnce(&mut Config, &StubCourier)) -> Fixture {
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

    let mut cfg = dev_config();
    let courier = StubCourier::default();
    configure(&mut cfg, &courier);

    let clock = Arc::new(Mutex::new(
        DateTime::parse_from_rfc3339("2026-11-24T10:00:00Z")
            .expect("timestamp")
            .with_timezone(&Utc),
    ));
    let clock_for_store = clock.clone();
    let store_clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync> =
        Arc::new(move || *clock_for_store.lock().expect("clock"));
    let captcha = Arc::new(CaptchaStore::for_tests(
        CAPTCHA_TTL,
        CAPTCHA_MIN_AGE,
        CAPTCHA_MAX_ENTRIES,
        store_clock,
    ));

    let state = AppState::new(
        cfg.clone(),
        store.clone(),
        registry,
        Arc::new(courier.clone()),
    );
    let sessions = session_layer(
        LibsqlSessionStore::new(store.conn().clone()),
        cfg.cookie_secure,
        configured_ttl(&cfg),
    );
    let app = checkout_routes_with_captcha(captcha.clone())
        .merge(shipment_routes())
        .with_state(state)
        .layer(sessions);
    Fixture {
        app,
        store,
        courier,
        captcha,
        clock,
    }
}

// ---------------------------------------------------------------------------
// Seed data
// ---------------------------------------------------------------------------

async fn insert_category(store: &Store, id: &str, slug: &str, name: &str, plugin_id: &str) {
    store
        .conn()
        .execute(
            "INSERT INTO categories (id, slug, name, description, sort_order, plugin_id)
             VALUES (?, ?, ?, '', 1, ?)",
            libsql::params![id, slug, name, plugin_id],
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
                "2026-01-01T00:00:00Z",
                "2026-01-01T00:00:00Z",
            ],
        )
        .await
        .expect("insert product");
}

async fn insert_user(store: &Store, id: &str, name: &str, email: &str, role: &str) {
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
                created_at: parse_time("2026-01-01T09:00:00Z"),
                updated_at: parse_time("2026-01-01T09:00:00Z"),
            },
        )
        .await
        .expect("insert user");
}

async fn seed(store: &Store) {
    insert_category(store, "cat_core", "skincare", "Skincare", "core").await;
    insert_category(store, "cat_face", "face", "Face", "oils").await;
    insert_category(store, "cat_phone", "phones", "Phones", "phone").await;
    insert_category(store, "cat_cpu", "cpus", "CPU", "pcparts").await;

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
        },
    )
    .await;

    insert_user(
        store,
        "u_admin",
        "Amina Admin",
        "admin@example.com",
        "admin",
    )
    .await;
    insert_user(
        store,
        "u_staff",
        "Sam Staff",
        "staff@example.com",
        "employee",
    )
    .await;
    insert_user(
        store,
        "u_customer",
        "Amina Customer",
        "amina@example.com",
        "customer",
    )
    .await;

    store
        .conn()
        .execute(
            "CREATE TABLE phone_devices (
                id TEXT PRIMARY KEY,
                product_id TEXT NOT NULL REFERENCES products(id),
                variant_key TEXT NOT NULL DEFAULT '',
                storage TEXT NOT NULL DEFAULT '',
                ram TEXT NOT NULL DEFAULT '',
                color TEXT NOT NULL DEFAULT '',
                imei TEXT NOT NULL UNIQUE,
                status TEXT NOT NULL DEFAULT 'in_stock',
                order_id TEXT NOT NULL DEFAULT '',
                order_item_id TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                config TEXT NOT NULL DEFAULT ''
            )",
            (),
        )
        .await
        .expect("phone_devices table");
    store
        .conn()
        .execute(
            "INSERT INTO phone_devices (id, product_id, variant_key, config, color, imei,
             status, order_id, order_item_id, created_at, updated_at)
             VALUES ('dev_1', 'p_phone', '', ?, ?, '356789012345678', 'in_stock', '', '',
             '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            libsql::params![PHONE_CONFIG, PHONE_COLOR],
        )
        .await
        .expect("insert device");
}

fn sample_order(id: &str, number: &str, status: &str) -> Order {
    Order {
        id: id.to_string(),
        number: number.to_string(),
        name: "Amina Customer".to_string(),
        email: "amina@example.com".to_string(),
        phone: "0550123456".to_string(),
        address: "12 Rue de la Paix".to_string(),
        city: "Alger Centre".to_string(),
        notes: "Leave at the door".to_string(),
        subtotal_cents: 150_000,
        shipping_cents: 60_000,
        total_cents: 210_000,
        currency: "DZD".to_string(),
        status: status.to_string(),
        token_hash: hash_token(&format!("token-{id}")),
        created_at: parse_time("2026-11-20T10:00:00Z"),
        updated_at: parse_time("2026-11-20T10:00:00Z"),
        wilaya_code: 16,
        wilaya_name: "Alger".to_string(),
        commune_name: "Alger Centre".to_string(),
        delivery_type: "home".to_string(),
        ..Order::default()
    }
}

async fn seed_order(store: &Store, order: &Order) {
    store
        .create_order(store.conn(), order, &[])
        .await
        .expect("insert order");
}

async fn seed_session(store: &Store, user_id: &str) -> String {
    let id = SessionId::default();
    let mut record = SessionRecord {
        id,
        data: HashMap::from([("user_id".to_string(), json!(user_id))]),
        expiry_date: time::OffsetDateTime::now_utc() + time::Duration::hours(1),
    };
    LibsqlSessionStore::new(store.conn().clone())
        .create(&mut record)
        .await
        .expect("create session");
    id.to_string()
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

fn json_request(method: Method, uri: &str, body: &Value) -> Request<Body> {
    let mut request = request(method, uri);
    request.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    *request.body_mut() = Body::from(body.to_string());
    request
}

fn with_session(mut request: Request<Body>, session: &str) -> Request<Body> {
    request.headers_mut().insert(
        header::COOKIE,
        HeaderValue::from_str(&format!("azahar_session={session}")).expect("cookie"),
    );
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

fn problem_field<'a>(body: &'a Value, key: &str) -> &'a str {
    body["error"]["fields"][key]
        .as_str()
        .unwrap_or_else(|| panic!("missing field {key} in {body}"))
}

fn checkout_body() -> Value {
    json!({
        "name": "Amina Buyer",
        "email": "amina@example.com",
        "phone": "0550123456",
        "address": "12 Rue de la Paix",
        "wilaya_code": 16,
        "commune_name": "Alger Centre",
        "delivery_type": "home",
        "stop_desk_id": "",
        "notes": "",
        "items": [{
            "product_id": "p_core",
            "quantity": 1,
            "options": {},
            "expected_price_cents": null
        }],
        "website": "",
        "captcha_token": "",
        "captcha_answer": "",
        "turnstile_token": "",
        "terms_accepted": true,
        "terms_version": "2026-11-14"
    })
}

fn with_captcha(mut body: Value, token: &str, answer: &str) -> Value {
    body["captcha_token"] = json!(token);
    body["captcha_answer"] = json!(answer);
    body
}

async fn checkout(app: &Router, _fixture: &Fixture, body: Value) -> (StatusCode, HeaderMap, Value) {
    body_json(send(app, json_request(Method::POST, "/api/checkout", &body)).await).await
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

async fn order_token_hash(store: &Store, id: &str) -> String {
    let mut rows = store
        .conn()
        .query(
            "SELECT token_hash FROM orders WHERE id = ?",
            libsql::params![id],
        )
        .await
        .expect("order query");
    rows.next()
        .await
        .expect("order row")
        .expect("order")
        .get(0)
        .expect("token hash")
}

// ---------------------------------------------------------------------------
// Checkout validation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn honeypot_is_rejected_before_any_validation() {
    let fixture = fixture().await;
    let mut body = checkout_body();
    body["website"] = json!("https://spam.example");
    let (status, _headers, body) = checkout(&fixture.app, &fixture, body).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "bot_detected");
    assert_eq!(body["error"]["message"], "The order could not be verified.");
    assert_eq!(stock(&fixture.store, "p_core").await, 4);
}

#[tokio::test]
async fn checkout_validation_matches_the_go_field_matrix() {
    type Case = (
        &'static str,
        Box<dyn Fn(&mut Value)>,
        &'static str,
        &'static str,
    );

    let fixture = fixture().await;

    let cases: Vec<Case> = vec![
        (
            "missing terms",
            Box::new(|body| body["terms_accepted"] = json!(false)),
            "terms",
            "Accept the current terms of service before placing the order.",
        ),
        (
            "stale terms",
            Box::new(|body| body["terms_version"] = json!("2025-01-01")),
            "terms",
            "Accept the current terms of service before placing the order.",
        ),
        (
            "missing captcha",
            Box::new(|body| {
                body["captcha_token"] = json!("");
                body["captcha_answer"] = json!("");
            }),
            "captcha",
            "Enter the code from the security check.",
        ),
        (
            "short name",
            Box::new(|body| body["name"] = json!("A")),
            "name",
            "Use between 2 and 120 characters.",
        ),
        (
            "bad email",
            Box::new(|body| body["email"] = json!("not-an-email")),
            "email",
            "Enter a valid email.",
        ),
        (
            "bad phone",
            Box::new(|body| body["phone"] = json!("0412345678")),
            "phone",
            "Use an Algerian mobile number, for example 0550 12 34 56.",
        ),
        (
            "bad wilaya",
            Box::new(|body| body["wilaya_code"] = json!(59)),
            "wilaya_code",
            "Choose a wilaya.",
        ),
        (
            "short commune",
            Box::new(|body| body["commune_name"] = json!("A")),
            "commune_name",
            "Choose a commune.",
        ),
        (
            "bad delivery type",
            Box::new(|body| body["delivery_type"] = json!("pickup")),
            "delivery_type",
            "Choose home delivery or a stop desk.",
        ),
        (
            "short address",
            Box::new(|body| body["address"] = json!("1 Rd")),
            "address",
            "Use between 5 and 200 characters.",
        ),
        (
            "stop desk without desk",
            Box::new(|body| {
                body["delivery_type"] = json!("stopdesk");
                body["stop_desk_id"] = json!("");
            }),
            "stop_desk_id",
            "Choose a stop desk.",
        ),
        (
            "long notes",
            Box::new(|body| body["notes"] = json!("n".repeat(501))),
            "notes",
            "Keep the notes under 500 characters.",
        ),
        (
            "no items",
            Box::new(|body| body["items"] = json!([])),
            "items",
            "Add between 1 and 20 products.",
        ),
        (
            "too many items",
            Box::new(|body| {
                body["items"] =
                    Value::Array(vec![json!({"product_id": "p_core", "quantity": 1}); 21]);
            }),
            "items",
            "Add between 1 and 20 products.",
        ),
    ];

    for (index, (label, mutate, field, message)) in cases.iter().enumerate() {
        let (token, answer) = fixture.captcha_pair();
        let mut body = with_captcha(checkout_body(), &token, &answer);
        mutate(&mut body);
        let (status, _headers, response) = checkout(&fixture.app, &fixture, body).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{index} {label}");
        assert_eq!(
            response["error"]["message"], "Check the marked fields.",
            "{label}"
        );
        assert_eq!(problem_field(&response, field), *message, "{label}");
    }
}

#[tokio::test]
async fn build_lines_errors_use_the_items_field() {
    let fixture = fixture().await;

    let (token, answer) = fixture.captcha_pair();
    let mut body = with_captcha(checkout_body(), &token, &answer);
    body["items"] = json!([{"product_id": "p_core", "quantity": 0, "options": {}}]);
    let (status, _headers, response) = checkout(&fixture.app, &fixture, body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&response, "items"),
        "Use between 1 and 10 units per product."
    );

    let (token, answer) = fixture.captcha_pair();
    let mut body = with_captcha(checkout_body(), &token, &answer);
    body["items"] = json!([{"product_id": "  ", "quantity": 1}]);
    let (status, _headers, response) = checkout(&fixture.app, &fixture, body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&response, "items"),
        "Choose a product for every line."
    );
}

#[tokio::test]
async fn destination_validation_matches_the_shipping_helpers() {
    let fixture = fixture().await;

    let (token, answer) = fixture.captcha_pair();
    let mut body = with_captcha(checkout_body(), &token, &answer);
    body["wilaya_code"] = json!(58);
    let (status, _headers, response) = checkout(&fixture.app, &fixture, body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&response, "wilaya_code"),
        "Choose a wilaya from the list."
    );

    let (token, answer) = fixture.captcha_pair();
    let mut body = with_captcha(checkout_body(), &token, &answer);
    body["commune_name"] = json!("Nowhere");
    let (status, _headers, response) = checkout(&fixture.app, &fixture, body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&response, "commune_name"),
        "Choose a commune from the list."
    );

    let (token, answer) = fixture.captcha_pair();
    let mut body = with_captcha(checkout_body(), &token, &answer);
    body["delivery_type"] = json!("stopdesk");
    body["stop_desk_id"] = json!("nope");
    let (status, _headers, response) = checkout(&fixture.app, &fixture, body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&response, "stop_desk_id"),
        "Choose a stop desk from the list."
    );

    let failing = fixture_with(|_, courier| courier.fail_map()).await;
    let (token, answer) = failing.captcha_pair();
    let body = with_captcha(checkout_body(), &token, &answer);
    let (status, _headers, response) = checkout(&failing.app, &failing, body).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response["error"]["code"], "courier_unavailable");
    assert_eq!(
        response["error"]["message"],
        "The delivery map is unavailable. Try again in a moment."
    );
}

// ---------------------------------------------------------------------------
// Checkout success paths
// ---------------------------------------------------------------------------

#[tokio::test]
async fn guest_checkout_creates_the_order_and_records_the_terms() {
    let fixture = fixture().await;
    let (token, answer) = fixture.captcha_pair();
    let body = with_captcha(checkout_body(), &token, &answer);

    let (status, _headers, response) = checkout(&fixture.app, &fixture, body).await;
    assert_eq!(status, StatusCode::CREATED, "{response}");
    let order = &response["order"];
    let order_token = order["token"].as_str().expect("token");
    assert_eq!(order_token.len(), 43, "32 random bytes base64url");
    assert_eq!(order["status"], "pending");
    assert_eq!(order["address"], "12 Rue de la Paix");
    assert_eq!(order["delivery_type"], "home");
    assert_eq!(order["wilaya_name"], "Alger");
    assert_eq!(order["commune_name"], "Alger Centre");
    assert_eq!(order["subtotal_cents"], 150_000);
    assert_eq!(order["shipping_cents"], 60_000);
    assert_eq!(order["total_cents"], 210_000);
    assert!(order["customer"]["id"].is_null() || order["customer"]["id"] == "");

    let order_id = order["id"].as_str().expect("order id");
    assert_eq!(
        order_token_hash(&fixture.store, order_id).await,
        hash_token(order_token)
    );
    assert_eq!(stock(&fixture.store, "p_core").await, 3);

    let (entity_id, detail) = latest_audit(&fixture.store, "order.terms_accepted")
        .await
        .expect("terms audit");
    assert_eq!(entity_id, order_id);
    let detail: Value = serde_json::from_str(&detail).expect("audit detail");
    assert_eq!(detail["version"], "2026-11-14");
    assert!(detail["accepted_at"].as_str().is_some());

    let mut rows = fixture
        .store
        .conn()
        .query(
            "SELECT actor_user_id FROM audit_outbox WHERE action = 'order.terms_accepted'",
            (),
        )
        .await
        .expect("actor query");
    let actor: String = rows
        .next()
        .await
        .expect("row")
        .expect("actor")
        .get(0)
        .expect("actor value");
    assert_eq!(actor, "");
}

#[tokio::test]
async fn signed_in_checkout_attaches_the_customer() {
    let fixture = fixture().await;
    let session = seed_session(&fixture.store, "u_customer").await;
    let (token, answer) = fixture.captcha_pair();
    let body = with_captcha(checkout_body(), &token, &answer);

    let response = send(
        &fixture.app,
        with_session(json_request(Method::POST, "/api/checkout", &body), &session),
    )
    .await;
    let (status, _headers, response) = body_json(response).await;
    assert_eq!(status, StatusCode::CREATED, "{response}");
    assert_eq!(response["order"]["customer"]["id"], "u_customer");

    let order_id = response["order"]["id"].as_str().expect("order id");
    let mut rows = fixture
        .store
        .conn()
        .query(
            "SELECT user_id FROM orders WHERE id = ?",
            libsql::params![order_id],
        )
        .await
        .expect("user query");
    let user_id: String = rows
        .next()
        .await
        .expect("row")
        .expect("order")
        .get(0)
        .expect("user id");
    assert_eq!(user_id, "u_customer");

    let mut rows = fixture
        .store
        .conn()
        .query(
            "SELECT actor_user_id FROM audit_outbox WHERE action = 'order.terms_accepted'",
            (),
        )
        .await
        .expect("actor query");
    let actor: String = rows
        .next()
        .await
        .expect("row")
        .expect("actor")
        .get(0)
        .expect("actor value");
    assert_eq!(actor, "u_customer");
}

#[tokio::test]
async fn a_stop_desk_order_uses_the_desk_commune_and_address() {
    let fixture = fixture().await;
    let (token, answer) = fixture.captcha_pair();
    let mut body = with_captcha(checkout_body(), &token, &answer);
    body["delivery_type"] = json!("stopdesk");
    body["stop_desk_id"] = json!("desk_1");
    body["commune_name"] = json!("Alger Centre");
    body["address"] = json!("");

    let (status, _headers, response) = checkout(&fixture.app, &fixture, body).await;
    assert_eq!(status, StatusCode::CREATED, "{response}");
    let order = &response["order"];
    assert_eq!(order["delivery_type"], "stopdesk");
    assert_eq!(order["stop_desk_id"], "desk_1");
    assert_eq!(order["stop_desk_name"], "Desk One");
    assert_eq!(order["commune_name"], "Bab Ezzouar");
    assert_eq!(order["address"], "1 Desk Road");
}

#[tokio::test]
async fn price_changes_and_stock_shortages_are_refused() {
    let fixture = fixture().await;

    let (token, answer) = fixture.captcha_pair();
    let mut body = with_captcha(checkout_body(), &token, &answer);
    body["items"] = json!([{
        "product_id": "p_core",
        "quantity": 1,
        "options": {},
        "expected_price_cents": 149_999
    }]);
    let (status, _headers, response) = checkout(&fixture.app, &fixture, body).await;
    assert_eq!(status, StatusCode::CONFLICT, "{response}");
    assert_eq!(response["error"]["code"], "price_changed");
    assert_eq!(
        problem_field(&response, "items"),
        "Gentle soap now costs a different amount."
    );
    assert_eq!(stock(&fixture.store, "p_core").await, 4);

    let (token, answer) = fixture.captcha_pair();
    let mut body = with_captcha(checkout_body(), &token, &answer);
    body["items"] = json!([{"product_id": "p_core", "quantity": 5, "options": {}}]);
    let (status, _headers, response) = checkout(&fixture.app, &fixture, body).await;
    assert_eq!(status, StatusCode::CONFLICT, "{response}");
    assert_eq!(response["error"]["code"], "conflict");
    assert_eq!(
        problem_field(&response, "items"),
        "Reduce the quantity for Gentle soap."
    );
    assert_eq!(stock(&fixture.store, "p_core").await, 4);
}

#[tokio::test]
async fn phone_options_and_imei_shortages_are_refused() {
    let fixture = fixture().await;

    let (token, answer) = fixture.captcha_pair();
    let mut body = with_captcha(checkout_body(), &token, &answer);
    body["items"] = json!([{
        "product_id": "p_phone",
        "quantity": 1,
        "options": {"config": "8 GB RAM + 128 GB", "color": PHONE_COLOR}
    }]);
    let (status, _headers, response) = checkout(&fixture.app, &fixture, body).await;
    assert_eq!(status, StatusCode::CONFLICT, "{response}");
    assert_eq!(response["error"]["code"], "conflict");
    assert_eq!(
        response["error"]["message"],
        "Some product options have changed."
    );
    assert_eq!(
        problem_field(&response, "items"),
        "Choose an available memory and storage."
    );

    let (token, answer) = fixture.captcha_pair();
    let mut body = with_captcha(checkout_body(), &token, &answer);
    body["items"] = json!([{
        "product_id": "p_phone",
        "quantity": 2,
        "options": {"config": PHONE_CONFIG, "color": PHONE_COLOR}
    }]);
    let (status, _headers, response) = checkout(&fixture.app, &fixture, body).await;
    assert_eq!(status, StatusCode::CONFLICT, "{response}");
    assert_eq!(
        problem_field(&response, "items"),
        "Not enough IMEI-registered handsets for Pixel phone in that configuration."
    );
}

#[tokio::test]
async fn phone_checkout_reserves_the_oldest_imei() {
    let fixture = fixture().await;
    let (token, answer) = fixture.captcha_pair();
    let mut body = with_captcha(checkout_body(), &token, &answer);
    body["items"] = json!([{
        "product_id": "p_phone",
        "quantity": 1,
        "options": {"config": PHONE_CONFIG, "color": PHONE_COLOR}
    }]);

    let (status, _headers, response) = checkout(&fixture.app, &fixture, body).await;
    assert_eq!(status, StatusCode::CREATED, "{response}");
    let order_id = response["order"]["id"].as_str().expect("order id");

    let mut rows = fixture
        .store
        .conn()
        .query(
            "SELECT status, order_id FROM phone_devices WHERE id = 'dev_1'",
            (),
        )
        .await
        .expect("device query");
    let row = rows.next().await.expect("row").expect("device");
    let status: String = row.get(0).expect("status");
    let device_order: String = row.get(1).expect("order");
    assert_eq!(status, "reserved");
    assert_eq!(device_order, order_id);
}

#[tokio::test]
async fn checkout_limiter_stops_the_twenty_first_attempt() {
    let fixture = fixture().await;
    let mut body = checkout_body();
    body["website"] = json!("bot");
    for attempt in 0..20 {
        let (status, _headers, response) = checkout(&fixture.app, &fixture, body.clone()).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "attempt {attempt}: {response}"
        );
    }

    let (status, headers, response) = checkout(&fixture.app, &fixture, body).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response["error"]["code"], "rate_limited");
    assert_eq!(
        response["error"]["message"],
        "Too many requests. Try again later."
    );
    assert!(
        headers
            .get(header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .is_some()
    );
}

// ---------------------------------------------------------------------------
// Challenge and captcha verification
// ---------------------------------------------------------------------------

#[tokio::test]
async fn challenge_requires_the_web_header_and_does_not_spend_the_bucket() {
    let fixture = fixture().await;
    for _ in 0..31 {
        let (status, _headers, body) =
            body_json(send(&fixture.app, request(Method::GET, CHECKOUT_CHALLENGE_PATH)).await)
                .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"]["code"], "forbidden");
        assert_eq!(
            body["error"]["message"],
            "The request could not be verified."
        );
    }

    let mut request = request(Method::GET, CHECKOUT_CHALLENGE_PATH);
    request
        .headers_mut()
        .insert("x-requested-with", HeaderValue::from_static("azahar-web"));
    let (status, headers, body) = body_json(send(&fixture.app, request).await).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        headers
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok()),
        Some("no-store")
    );
    assert!(
        body["token"]
            .as_str()
            .is_some_and(|token| !token.is_empty())
    );
    assert!(
        body["image"]
            .as_str()
            .is_some_and(|image| image.starts_with("data:image/png;base64,"))
    );
    assert_eq!(body["expires_in"], 600);
}

#[tokio::test]
async fn challenge_tokens_enforce_min_age_one_time_use_and_wrong_answers() {
    let fixture = fixture().await;

    let mut request = request(Method::GET, CHECKOUT_CHALLENGE_PATH);
    request
        .headers_mut()
        .insert("x-requested-with", HeaderValue::from_static("azahar-web"));
    let (status, _headers, body) = body_json(send(&fixture.app, request).await).await;
    assert_eq!(status, StatusCode::OK);
    let token = body["token"].as_str().expect("token").to_string();
    let code = fixture.captcha.code_for(&token).expect("issued code");

    // A form filled instantly is refused, and the token stays usable.
    let body = with_captcha(checkout_body(), &token, &code);
    let (status, _headers, response) = checkout(&fixture.app, &fixture, body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&response, "captcha"),
        "Wait a moment, then enter the code."
    );

    fixture.advance(3);
    let wrong = if code.starts_with('A') {
        "BBBBB"
    } else {
        "AAAAA"
    };
    let body = with_captcha(checkout_body(), &token, wrong);
    let (status, _headers, response) = checkout(&fixture.app, &fixture, body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&response, "captcha"),
        "The code is wrong. Check the characters and try again."
    );

    // A wrong answer burns the token.
    let body = with_captcha(checkout_body(), &token, &code);
    let (status, _headers, response) = checkout(&fixture.app, &fixture, body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&response, "captcha"),
        "The code expired or was already used. Request a new one."
    );

    // A fresh challenge at the right age succeeds.
    let (token, code) = fixture.captcha_pair();
    let body = with_captcha(checkout_body(), &token, &code);
    let (status, _headers, response) = checkout(&fixture.app, &fixture, body).await;
    assert_eq!(status, StatusCode::CREATED, "{response}");
}

#[tokio::test]
async fn expired_challenges_are_refused() {
    let fixture = fixture().await;
    let token = fixture.captcha.issue("").expect("issue").token;
    let code = fixture.captcha.code_for(&token).expect("code");
    let body = with_captcha(checkout_body(), &token, &code);
    let (status, _headers, response) = checkout(&fixture.app, &fixture, body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&response, "captcha"),
        "Wait a moment, then enter the code."
    );

    fixture.advance(601);
    let body = with_captcha(checkout_body(), &token, &code);
    let (status, _headers, response) = checkout(&fixture.app, &fixture, body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&response, "captcha"),
        "The code expired or was already used. Request a new one."
    );
    assert_eq!(
        fixture.captcha.verify(&token, &code, ""),
        CaptchaCheck::Expired
    );
}

#[tokio::test]
async fn a_configured_turnstile_secret_requires_a_token_without_the_network() {
    let fixture = fixture_with(|cfg, _| {
        cfg.turnstile_secret = "test-secret".to_string();
    })
    .await;
    // No captcha answer is needed: the Turnstile branch owns the `captcha`
    // field and the empty token fails validation before siteverify is called.
    let body = checkout_body();
    let (status, _headers, response) = checkout(&fixture.app, &fixture, body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        problem_field(&response, "captcha"),
        "Complete the security check and try again."
    );
}

// ---------------------------------------------------------------------------
// Admin shipments
// ---------------------------------------------------------------------------

async fn staff_fixture() -> (Fixture, String) {
    let fixture = fixture().await;
    let session = seed_session(&fixture.store, "u_staff").await;
    (fixture, session)
}

#[tokio::test]
async fn creating_a_shipment_stores_it_and_writes_the_audit() {
    let (fixture, session) = staff_fixture().await;
    seed_order(
        &fixture.store,
        &sample_order("ord_ship", "AZ-2026-1001", "pending"),
    )
    .await;
    fixture.courier.shipment(Shipment {
        tracking_number: "TRK-9".to_string(),
        courier_reference: "REF-9".to_string(),
        status: "created".to_string(),
        reference: "AZ-2026-1001".to_string(),
    });

    let response = send(
        &fixture.app,
        with_session(
            request(Method::POST, "/api/admin/orders/ord_ship/shipment"),
            &session,
        ),
    )
    .await;
    let (status, _headers, body) = body_json(response).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["order"]["tracking_number"], "TRK-9");
    assert_eq!(body["order"]["courier_key"], "sandbox");
    assert_eq!(body["order"]["courier_status"], "created");

    let created = fixture.courier.state().created.clone();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].reference, "AZ-2026-1001");
    assert_eq!(created[0].recipient.phone, "0550123456");
    assert_eq!(created[0].cod_amount, 2_100);
    assert!(created[0].free_shipping);
    assert_eq!(created[0].notes, "Leave at the door");

    let (entity_id, detail) = latest_audit(&fixture.store, "order.shipment")
        .await
        .expect("shipment audit");
    assert_eq!(entity_id, "ord_ship");
    let detail: Value = serde_json::from_str(&detail).expect("detail");
    assert_eq!(detail["courier"], "sandbox");
    assert_eq!(detail["tracking_number"], "TRK-9");

    let mut rows = fixture
        .store
        .conn()
        .query(
            "SELECT tracking_number, courier_status FROM orders WHERE id = 'ord_ship'",
            (),
        )
        .await
        .expect("order query");
    let row = rows.next().await.expect("row").expect("order");
    assert_eq!(row.get::<String>(0).expect("tracking"), "TRK-9");
    assert_eq!(row.get::<String>(1).expect("status"), "created");
}

#[tokio::test]
async fn shipment_creation_refuses_closed_or_incomplete_orders() {
    let (fixture, session) = staff_fixture().await;

    let mut shipped = sample_order("ord_has_tracking", "AZ-2026-1002", "pending");
    shipped.tracking_number = "TRK-OLD".to_string();
    seed_order(&fixture.store, &shipped).await;
    let (status, _headers, body) = body_json(
        send(
            &fixture.app,
            with_session(
                request(Method::POST, "/api/admin/orders/ord_has_tracking/shipment"),
                &session,
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        body["error"]["message"],
        "This order already has a shipment."
    );

    seed_order(
        &fixture.store,
        &sample_order("ord_cancelled", "AZ-2026-1003", "cancelled"),
    )
    .await;
    let (status, _headers, body) = body_json(
        send(
            &fixture.app,
            with_session(
                request(Method::POST, "/api/admin/orders/ord_cancelled/shipment"),
                &session,
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        body["error"]["message"],
        "This order is closed and cannot be shipped."
    );

    let mut no_address = sample_order("ord_no_address", "AZ-2026-1004", "pending");
    no_address.address = "1 Rd".to_string();
    seed_order(&fixture.store, &no_address).await;
    let (status, _headers, body) = body_json(
        send(
            &fixture.app,
            with_session(
                request(Method::POST, "/api/admin/orders/ord_no_address/shipment"),
                &session,
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["error"]["message"],
        "This order needs delivery details first."
    );
    assert_eq!(
        problem_field(&body, "address"),
        "This order has no deliverable address."
    );

    let mut bad_phone = sample_order("ord_bad_phone", "AZ-2026-1005", "pending");
    bad_phone.phone = "0412345678".to_string();
    seed_order(&fixture.store, &bad_phone).await;
    let (status, _headers, body) = body_json(
        send(
            &fixture.app,
            with_session(
                request(Method::POST, "/api/admin/orders/ord_bad_phone/shipment"),
                &session,
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["error"]["message"],
        "This order needs a valid phone first."
    );
    assert_eq!(
        problem_field(&body, "phone"),
        "Use an Algerian mobile number."
    );
}

#[tokio::test]
async fn courier_failures_keep_the_go_status_and_messages() {
    let (fixture, session) = staff_fixture().await;
    seed_order(
        &fixture.store,
        &sample_order("ord_err", "AZ-2026-1006", "pending"),
    )
    .await;

    fixture.courier.fail_create(CourierError::Api(APIError {
        status: 422,
        code: "invalid".to_string(),
        message: "Invalid phone.".to_string(),
        fields: BTreeMap::from([("phone".to_string(), "Rejected.".to_string())]),
        retry_after: Duration::ZERO,
    }));
    let (status, _headers, body) = body_json(
        send(
            &fixture.app,
            with_session(
                request(Method::POST, "/api/admin/orders/ord_err/shipment"),
                &session,
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "courier_error");
    assert_eq!(body["error"]["message"], "Invalid phone.");
    assert_eq!(problem_field(&body, "phone"), "Rejected.");

    fixture.courier.fail_create(CourierError::Api(APIError {
        status: 429,
        message: "slow down".to_string(),
        retry_after: Duration::from_secs(2),
        ..APIError::default()
    }));
    let (status, headers, body) = body_json(
        send(
            &fixture.app,
            with_session(
                request(Method::POST, "/api/admin/orders/ord_err/shipment"),
                &session,
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        headers
            .get(header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok()),
        Some("2")
    );
    assert_eq!(body["error"]["code"], "courier_rate_limited");

    fixture.courier.fail_create(CourierError::transport("down"));
    let (status, _headers, body) = body_json(
        send(
            &fixture.app,
            with_session(
                request(Method::POST, "/api/admin/orders/ord_err/shipment"),
                &session,
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body["error"]["code"], "courier_error");
    assert_eq!(
        body["error"]["message"],
        "The courier service is unavailable. Try again in a moment."
    );
}

#[tokio::test]
async fn tracking_refresh_updates_the_events_and_audit() {
    let (fixture, session) = staff_fixture().await;
    let mut order = sample_order("ord_track", "AZ-2026-1007", "shipped");
    order.tracking_number = "TRK-OLD".to_string();
    order.courier_key = "sandbox".to_string();
    order.courier_status = "shipped".to_string();
    seed_order(&fixture.store, &order).await;
    fixture.courier.tracking(Tracking {
        tracking_number: "TRK-OLD".to_string(),
        status: "in_transit".to_string(),
        events: vec![
            TrackingEvent {
                status: "picked_up".to_string(),
                raw_status: "PICKUP".to_string(),
                timestamp: "2026-11-24T08:00:00Z".to_string(),
            },
            TrackingEvent {
                status: "in_transit".to_string(),
                raw_status: "TRANSIT".to_string(),
                timestamp: "2026-11-24T09:00:00Z".to_string(),
            },
        ],
    });

    let (status, _headers, body) = body_json(
        send(
            &fixture.app,
            with_session(
                request(Method::POST, "/api/admin/orders/ord_track/tracking"),
                &session,
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["order"]["courier_status"], "in_transit");
    assert_eq!(
        body["order"]["courier_events"]
            .as_array()
            .expect("events")
            .len(),
        2
    );
    assert_eq!(
        fixture.courier.state().tracked,
        vec![("sandbox".to_string(), "TRK-OLD".to_string())]
    );

    let (entity_id, detail) = latest_audit(&fixture.store, "order.tracking")
        .await
        .expect("tracking audit");
    assert_eq!(entity_id, "ord_track");
    let detail: Value = serde_json::from_str(&detail).expect("detail");
    assert_eq!(detail["status"], "in_transit");
}

#[tokio::test]
async fn tracking_requires_a_shipment_and_maps_courier_errors() {
    let (fixture, session) = staff_fixture().await;
    seed_order(
        &fixture.store,
        &sample_order("ord_untracked", "AZ-2026-1008", "pending"),
    )
    .await;
    let (status, _headers, body) = body_json(
        send(
            &fixture.app,
            with_session(
                request(Method::POST, "/api/admin/orders/ord_untracked/tracking"),
                &session,
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["message"], "This order has no shipment yet.");

    let mut shipped = sample_order("ord_track_err", "AZ-2026-1009", "shipped");
    shipped.tracking_number = "TRK-ERR".to_string();
    seed_order(&fixture.store, &shipped).await;
    fixture.courier.fail_track(CourierError::Api(APIError {
        status: 500,
        message: "Courier exploded".to_string(),
        ..APIError::default()
    }));
    let (status, _headers, body) = body_json(
        send(
            &fixture.app,
            with_session(
                request(Method::POST, "/api/admin/orders/ord_track_err/tracking"),
                &session,
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body["error"]["code"], "courier_error");
    assert_eq!(body["error"]["message"], "Courier exploded");
}

#[tokio::test]
async fn shipment_routes_need_a_staff_account() {
    let fixture = fixture().await;
    seed_order(
        &fixture.store,
        &sample_order("ord_guard", "AZ-2026-1010", "pending"),
    )
    .await;

    let (status, _headers, body) = body_json(
        send(
            &fixture.app,
            request(Method::POST, "/api/admin/orders/ord_guard/shipment"),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["message"], "Sign in to continue.");

    let customer = seed_session(&fixture.store, "u_customer").await;
    let (status, _headers, body) = body_json(
        send(
            &fixture.app,
            with_session(
                request(Method::POST, "/api/admin/orders/ord_guard/shipment"),
                &customer,
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body["error"]["message"],
        "This account does not have access."
    );
}
