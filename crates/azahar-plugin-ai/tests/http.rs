//! Local libSQL integration tests for the AI plugin routes.
//!
//! Each fixture opens an in-memory database, applies the core and AI
//! migrations, seeds a store, registers the plugin, encrypts settings with a
//! known key and starts a local OpenAI-compatible mock. The router under test
//! is [`azahar_plugin_ai::ai_routes`] with the real session layer, so the admin
//! guards and the mutation limiter run exactly as the orchestrator will mount
//! them.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::State;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use azahar_config::Config;
use azahar_courier::{
    APIError, Commune, Courier, Desk, Error as CourierError, Shipment, Tracking, Wilaya,
};
use azahar_httpapi::state::configured_ttl;
use azahar_httpapi::{AppState, LibsqlSessionStore, session_layer};
use azahar_plugin_ai::ai_routes;
use azahar_plugins::{Env, Registry};
use azahar_store::{Store, format_time};
use chrono::{Duration as ChronoDuration, Utc};
use libsql::Builder;
use serde_json::{Value, json};
use tower::ServiceExt;
use tower_sessions::Session;

const KEY: &str = "azahar-test-plugin-key-2026";
const OTHER_KEY: &str = "another-plugin-key-2026";
const ADMIN_ID: &str = "usr_admin";
const CUSTOMER_ID: &str = "usr_customer";
const PRODUCT_ID: &str = "prd_serum";

// ---------------------------------------------------------------------------
// Courier stub (AppState needs one)
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

// ---------------------------------------------------------------------------
// OpenAI mock
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MockMode {
    Success,
    Error,
    Timeout,
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<CapturedRequest>>>);

#[derive(Clone)]
struct MockState {
    mode: MockMode,
    capture: Capture,
}

#[derive(Debug)]
struct CapturedRequest {
    authorization: String,
    body: Value,
}

async fn completions(State(state): State<MockState>, headers: HeaderMap, body: String) -> Response {
    state
        .capture
        .0
        .lock()
        .expect("capture lock")
        .push(CapturedRequest {
            authorization: headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string(),
            body: serde_json::from_str(&body).expect("request json"),
        });
    match state.mode {
        MockMode::Success => (
            StatusCode::OK,
            r#"{"choices":[{"message":{"content":"  A clear answer.  "}}]}"#,
        )
            .into_response(),
        MockMode::Error => (
            StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"error":{"message":"provider secret detail"}}"#,
        )
            .into_response(),
        MockMode::Timeout => {
            tokio::time::sleep(Duration::from_millis(300)).await;
            (
                StatusCode::OK,
                r#"{"choices":[{"message":{"content":"too late"}}]}"#,
            )
                .into_response()
        }
    }
}

async fn start_mock(mode: MockMode, capture: Capture) -> String {
    let app = Router::new()
        .route("/v1/chat/completions", post(completions))
        .with_state(MockState { mode, capture });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock server");
    let address = listener.local_addr().expect("mock address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{address}/v1")
}

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

async fn login_admin(session: Session) -> StatusCode {
    session
        .insert(azahar_httpapi::USER_ID_KEY, ADMIN_ID)
        .await
        .expect("insert admin session");
    StatusCode::NO_CONTENT
}

async fn login_customer(session: Session) -> StatusCode {
    session
        .insert(azahar_httpapi::USER_ID_KEY, CUSTOMER_ID)
        .await
        .expect("insert customer session");
    StatusCode::NO_CONTENT
}

struct Options {
    settings: Option<HashMap<String, String>>,
    settings_key: String,
    mode: MockMode,
    register: bool,
    disable_plugin: bool,
    ai_timeout: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            settings: Some(settings()),
            settings_key: KEY.to_string(),
            mode: MockMode::Success,
            register: true,
            disable_plugin: false,
            ai_timeout: Duration::from_secs(5),
        }
    }
}

fn settings() -> HashMap<String, String> {
    HashMap::from([
        ("enabled".to_string(), "true".to_string()),
        ("api_key".to_string(), "sk-test-secret".to_string()),
        ("model".to_string(), "gpt-4o-mini".to_string()),
        ("language".to_string(), "en".to_string()),
    ])
}

struct Fixture {
    app: Router,
    store: Arc<Store>,
    capture: Capture,
}

async fn fixture() -> Fixture {
    fixture_with(Options::default()).await
}

async fn fixture_with(options: Options) -> Fixture {
    let database = Builder::new_local(":memory:")
        .build()
        .await
        .expect("open in-memory database");
    let store = Arc::new(Store::new(database).expect("store"));
    azahar_db::migrate(store.conn()).await.expect("migrate");

    let migrations = azahar_plugins::Plugin::migrations(&azahar_plugin_ai::new())
        .into_iter()
        .map(|migration| azahar_db::PluginMigration {
            plugin_id: "ai".to_string(),
            version: migration.version,
            sql: migration.sql,
        })
        .collect();
    azahar_db::migrate_plugins(store.conn(), migrations)
        .await
        .expect("ai migrations");

    seed(&store).await;

    if let Some(settings) = &options.settings {
        let payload = Env::new(store.clone(), options.settings_key.clone())
            .encode_settings(settings)
            .expect("encrypt settings");
        store
            .conn()
            .execute(
                "INSERT INTO plugin_settings (plugin_id, settings, updated_at) VALUES ('ai', ?, ?)",
                libsql::params![payload, format_time(&Utc::now())],
            )
            .await
            .expect("insert settings");
    }

    let capture = Capture::default();
    let base_url = start_mock(options.mode, capture.clone()).await;

    let mut registry = Registry::new();
    if options.register {
        registry
            .register(Arc::new(azahar_plugin_ai::new()))
            .expect("register ai");
        if options.disable_plugin {
            registry.set_enabled("ai", false);
        }
    }

    let cfg = Config {
        app_env: "development".to_string(),
        app_origin: "http://localhost:3000".to_string(),
        cookie_secure: false,
        session_ttl: Duration::from_secs(48 * 60 * 60),
        plugin_secret_key: KEY.to_string(),
        ai_base_url: base_url,
        ai_timeout: options.ai_timeout,
        ..Config::default()
    };
    let state = AppState::new(cfg.clone(), store.clone(), registry, Arc::new(StubCourier));
    let sessions = session_layer(
        LibsqlSessionStore::new(store.conn().clone()),
        false,
        configured_ttl(&cfg),
    );
    let app = Router::new()
        .route("/test/login-admin", get(login_admin))
        .route("/test/login-customer", get(login_customer))
        .merge(ai_routes())
        .with_state(state)
        .layer(sessions);
    Fixture {
        app,
        store,
        capture,
    }
}

// ---------------------------------------------------------------------------
// Seed data
// ---------------------------------------------------------------------------

async fn insert_user(store: &Store, id: &str, email: &str, role: &str) {
    store
        .conn()
        .execute(
            "INSERT INTO users (id, name, email, password_hash, role, theme_preset, created_at, updated_at)
             VALUES (?, ?, ?, 'hash', ?, 'cellar', ?, ?)",
            libsql::params![id, id, email, role, format_time(&Utc::now()), format_time(&Utc::now())],
        )
        .await
        .expect("insert user");
}

async fn insert_category(store: &Store, id: &str, slug: &str, name: &str) {
    store
        .conn()
        .execute(
            "INSERT INTO categories (id, slug, name, description, sort_order, plugin_id)
             VALUES (?, ?, ?, '', 0, 'oils')",
            libsql::params![id, slug, name],
        )
        .await
        .expect("insert category");
}

struct SeedProduct<'a> {
    id: &'a str,
    slug: &'a str,
    name: &'a str,
    subtitle: &'a str,
    description: &'a str,
    price_cents: i64,
    size: &'a str,
    stock: i64,
    category_id: &'a str,
    concerns: &'a str,
    ingredients: &'a str,
    attributes: &'a str,
    active: bool,
}

async fn insert_product(store: &Store, product: SeedProduct<'_>) {
    store
        .conn()
        .execute(
            "INSERT INTO products (id, slug, name, subtitle, description, price_cents, currency,
                size, category_id, concerns, ingredients, image, gallery, stock, featured, active,
                plugin_id, attributes, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, 'DZD', ?, ?, ?, ?, '', '[]', ?, 0, ?, 'oils', ?, ?, ?)",
            libsql::params![
                product.id,
                product.slug,
                product.name,
                product.subtitle,
                product.description,
                product.price_cents,
                product.size,
                product.category_id,
                product.concerns,
                product.ingredients,
                product.stock,
                product.active,
                product.attributes,
                format_time(&Utc::now()),
                format_time(&Utc::now())
            ],
        )
        .await
        .expect("insert product");
}

async fn insert_order(
    store: &Store,
    id: &str,
    number: &str,
    status: &str,
    total_cents: i64,
    created_at: chrono::DateTime<Utc>,
) {
    store
        .conn()
        .execute(
            "INSERT INTO orders (id, number, name, email, phone, address, city,
                subtotal_cents, shipping_cents, total_cents, currency, status, token_hash,
                created_at, updated_at, wilaya_code)
             VALUES (?, ?, 'Buyer', 'buyer@example.com', '0550000000', 'Street', 'Alger',
                ?, 0, ?, 'DZD', ?, ?, ?, ?, 16)",
            libsql::params![
                id,
                number,
                total_cents,
                total_cents,
                status,
                format!("token_{id}"),
                format_time(&created_at),
                format_time(&created_at)
            ],
        )
        .await
        .expect("insert order");
}

async fn insert_item(
    store: &Store,
    id: &str,
    order_id: &str,
    product_id: &str,
    quantity: i64,
    line_total_cents: i64,
) {
    store
        .conn()
        .execute(
            "INSERT INTO order_items (id, order_id, product_id, slug, name, image,
                unit_price_cents, quantity, line_total_cents, plugin_id)
             VALUES (?, ?, ?, 'slug', 'Hydrating serum', '', ?, ?, ?, 'oils')",
            libsql::params![
                id,
                order_id,
                product_id,
                line_total_cents / quantity.max(1),
                quantity,
                line_total_cents
            ],
        )
        .await
        .expect("insert item");
}

async fn seed(store: &Store) {
    insert_user(store, ADMIN_ID, "admin@example.com", "admin").await;
    insert_user(store, CUSTOMER_ID, "customer@example.com", "customer").await;
    insert_category(store, "cat_skincare", "skincare", "Skincare").await;
    insert_category(store, "cat_creams", "creams", "Creams").await;

    insert_product(
        store,
        SeedProduct {
            id: PRODUCT_ID,
            slug: "hydrating-serum",
            name: "Hydrating serum",
            subtitle: "  Light daily serum  ",
            description: "A fragrance-free serum with hyaluronic acid.",
            price_cents: 450_000,
            size: "30 ml",
            stock: 12,
            category_id: "cat_skincare",
            concerns: r#"["dryness"]"#,
            ingredients: r#"["hyaluronic acid"]"#,
            attributes: r#"{"scent":["rose","unscented"],"empty":[]}"#,
            active: true,
        },
    )
    .await;
    insert_product(
        store,
        SeedProduct {
            id: "prd_cream",
            slug: "night-cream",
            name: "Cream",
            subtitle: "",
            description: "",
            price_cents: 200_000,
            size: "",
            stock: 2,
            category_id: "cat_creams",
            concerns: "[]",
            ingredients: "[]",
            attributes: "{}",
            active: true,
        },
    )
    .await;

    let now = Utc::now();
    insert_order(
        store,
        "ord_recent",
        "A-0001",
        "paid",
        405_000,
        now - ChronoDuration::days(2),
    )
    .await;
    insert_order(
        store,
        "ord_pending",
        "A-0002",
        "pending",
        1_000,
        now - ChronoDuration::days(3),
    )
    .await;
    insert_order(
        store,
        "ord_cancelled",
        "A-0003",
        "cancelled",
        4_500_000,
        now - ChronoDuration::days(1),
    )
    .await;
    insert_order(
        store,
        "ord_old",
        "A-0004",
        "paid",
        405_000,
        now - ChronoDuration::days(40),
    )
    .await;

    insert_item(store, "itm_recent", "ord_recent", PRODUCT_ID, 4, 180_000).await;
    insert_item(
        store,
        "itm_cancelled",
        "ord_cancelled",
        PRODUCT_ID,
        100,
        4_500_000,
    )
    .await;
    insert_item(store, "itm_old", "ord_old", PRODUCT_ID, 9, 405_000).await;
}

// ---------------------------------------------------------------------------
// Request helpers
// ---------------------------------------------------------------------------

async fn login(app: &Router, path: &str) -> String {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .body(Body::empty())
                .expect("login request"),
        )
        .await
        .expect("login response");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .expect("session cookie")
        .to_str()
        .expect("cookie text");
    cookie.split(';').next().expect("cookie pair").to_string()
}

async fn call(
    app: &Router,
    method: Method,
    uri: &str,
    cookie: Option<&str>,
    body: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    if let Some(body) = body {
        builder = builder
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_LENGTH, body.len().to_string());
    }
    let request = builder
        .body(Body::from(body.unwrap_or_default().to_string()))
        .expect("request");
    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    };
    (status, value)
}

async fn analyze(app: &Router, cookie: &str, body: Value) -> (StatusCode, Value) {
    call(
        app,
        Method::POST,
        "/api/plugins/ai/admin/analyze",
        Some(cookie),
        Some(&body.to_string()),
    )
    .await
}

async fn runs(app: &Router, cookie: &str, query: &str) -> (StatusCode, Value) {
    call(
        app,
        Method::GET,
        &format!("/api/plugins/ai/admin/runs{query}"),
        Some(cookie),
        None,
    )
    .await
}

fn captured(fixture: &Fixture) -> Vec<CapturedRequest> {
    fixture
        .capture
        .0
        .lock()
        .expect("capture lock")
        .iter()
        .map(|request| CapturedRequest {
            authorization: request.authorization.clone(),
            body: request.body.clone(),
        })
        .collect()
}

async fn run_count(store: &Store) -> i64 {
    let mut rows = store
        .conn()
        .query("SELECT COUNT(*) FROM plugin_ai_runs", ())
        .await
        .expect("count runs");
    let row = rows.next().await.expect("count row").expect("count value");
    row.get(0).expect("count")
}

// ---------------------------------------------------------------------------
// Store analysis
// ---------------------------------------------------------------------------

#[tokio::test]
async fn analyze_store_writes_a_run_and_lists_it() {
    let fixture = fixture().await;
    let cookie = login(&fixture.app, "/test/login-admin").await;

    let (status, body) = analyze(&fixture.app, &cookie, json!({"kind": "store", "days": 30})).await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let run = &body["run"];
    assert_eq!(run["kind"], "store");
    assert_eq!(run["subject"], "store:30d");
    assert_eq!(run["model"], "gpt-4o-mini");
    assert_eq!(run["analysis"], "A clear answer.");
    assert!(
        run["id"].as_str().expect("run id").starts_with("air_"),
        "id prefix: {}",
        run["id"]
    );
    assert_eq!(
        run["created_at"].as_str().expect("created_at").len(),
        "2026-11-24T16:00:00Z".len()
    );

    let requests = captured(&fixture);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].authorization, "Bearer sk-test-secret");
    let payload = &requests[0].body;
    assert_eq!(payload["model"], "gpt-4o-mini");
    assert_eq!(payload["temperature"], 0.3);
    assert_eq!(payload["max_tokens"], 500);
    assert_eq!(payload["messages"][0]["role"], "system");
    assert_eq!(payload["messages"][1]["role"], "user");
    let system = payload["messages"][0]["content"].as_str().expect("system");
    assert!(system.ends_with("Write the answer in English."));
    let user = payload["messages"][1]["content"].as_str().expect("user");
    for line in [
        "Period: last 30 days (",
        "Revenue: DZD 4060.00.",
        "Previous period revenue: DZD 4050.00.",
        "Orders: 2.",
        "Units sold: 4.",
        "New customers: 1.",
        "Top products by units:",
        "- Hydrating serum: 4 units, DZD 1800.00 revenue.",
        "Low stock:",
        "- Cream: 2 in stock.",
        "Order status split:",
        "- cancelled: 1 orders.",
        "- paid: 1 orders.",
        "- pending: 1 orders.",
    ] {
        assert!(user.contains(line), "missing {line:?} in prompt:\n{user}");
    }

    let mut rows = fixture
        .store
        .conn()
        .query(
            "SELECT actor_user_id, kind, subject, model, analysis, created_at FROM plugin_ai_runs WHERE id = ?",
            libsql::params![run["id"].as_str().expect("id")],
        )
        .await
        .expect("select run");
    let row = rows.next().await.expect("row").expect("stored run");
    let actor: String = row.get(0).expect("actor");
    let kind: String = row.get(1).expect("kind");
    assert_eq!(actor, ADMIN_ID);
    assert_eq!(kind, "store");

    let (status, body) = runs(&fixture.app, &cookie, "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["runs"].as_array().expect("runs").len(), 1);
    assert_eq!(body["runs"][0]["id"], run["id"]);
    assert_eq!(body["runs"][0]["analysis"], "A clear answer.");
}

#[tokio::test]
async fn analyze_product_uses_sales_facts_and_defaults_days() {
    let fixture = fixture().await;
    let cookie = login(&fixture.app, "/test/login-admin").await;

    let (status, body) = analyze(
        &fixture.app,
        &cookie,
        json!({"kind": " product ", "product_id": format!(" {PRODUCT_ID} "), "days": 0}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["run"]["kind"], "product");
    assert_eq!(body["run"]["subject"], "Hydrating serum");

    let requests = captured(&fixture);
    let user = requests[0].body["messages"][1]["content"]
        .as_str()
        .expect("user prompt");
    assert_eq!(
        user,
        "Product name: Hydrating serum\n\
         Subtitle: Light daily serum\n\
         Description: A fragrance-free serum with hyaluronic acid.\n\
         Category: Skincare\n\
         Price: DZD 4500.00\n\
         Size: 30 ml\n\
         Stock: 12\n\
         Sales, last 30 days: 4 units, DZD 1800.00 revenue, 1 orders.\n\
         Concerns: dryness\n\
         Ingredients: hyaluronic acid\n\
         Options:\n\
         - scent: rose, unscented\n"
    );
    assert_eq!(run_count(&fixture.store).await, 1);
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

#[tokio::test]
async fn analyze_validates_the_request_body() {
    let fixture = fixture().await;
    let cookie = login(&fixture.app, "/test/login-admin").await;

    let (status, body) = analyze(&fixture.app, &cookie, json!({"kind": "other"})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "validation_error");
    assert_eq!(body["error"]["message"], "Check the marked fields.");
    assert_eq!(body["error"]["fields"]["kind"], "Choose store or product.");

    let (status, body) = analyze(&fixture.app, &cookie, json!({"kind": "store", "days": 5})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["fields"]["days"], "Choose 7, 30 or 90.");

    let (status, body) = analyze(&fixture.app, &cookie, json!({"kind": "product"})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["fields"]["product_id"], "Choose a product.");

    let (status, body) = analyze(
        &fixture.app,
        &cookie,
        json!({"kind": "product", "product_id": "prd_missing"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
    assert_eq!(body["error"]["message"], "That product no longer exists.");

    let (status, body) = analyze(&fixture.app, &cookie, json!({"kind": "store", "extra": 1})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "invalid_json");
    assert_eq!(
        body["error"]["message"],
        "The request body does not match the expected shape."
    );

    let (status, body) = call(
        &fixture.app,
        Method::POST,
        "/api/plugins/ai/admin/analyze",
        Some(&cookie),
        Some("null"),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["fields"]["kind"], "Choose store or product.");

    let response = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/plugins/ai/admin/analyze")
                .header(header::COOKIE, &cookie)
                .header(header::CONTENT_TYPE, "text/plain")
                .body(Body::from("{}"))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    assert_eq!(captured(&fixture).len(), 0);
}

#[tokio::test]
async fn analyze_refuses_disabled_unreadable_and_unconfigured_settings() {
    let disabled = fixture_with(Options {
        settings: Some(HashMap::from([
            ("enabled".to_string(), "false".to_string()),
            ("api_key".to_string(), "sk-test-secret".to_string()),
            ("model".to_string(), "gpt-4o-mini".to_string()),
        ])),
        ..Options::default()
    })
    .await;
    let cookie = login(&disabled.app, "/test/login-admin").await;
    let (status, body) = analyze(&disabled.app, &cookie, json!({"kind": "store"})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "plugin_disabled");
    assert_eq!(
        body["error"]["message"],
        "Enable the plugin in its settings before asking for an analysis."
    );

    let unreadable = fixture_with(Options {
        settings_key: OTHER_KEY.to_string(),
        ..Options::default()
    })
    .await;
    let cookie = login(&unreadable.app, "/test/login-admin").await;
    let (status, body) = analyze(&unreadable.app, &cookie, json!({"kind": "store"})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "plugin_settings_unreadable");
    assert_eq!(
        body["error"]["message"],
        "These settings cannot be read with the current PLUGIN_SECRET_KEY. Enter them again to replace the old values."
    );

    let unconfigured = fixture_with(Options {
        settings: Some(HashMap::from([
            ("enabled".to_string(), "true".to_string()),
            ("model".to_string(), "gpt-4o-mini".to_string()),
        ])),
        ..Options::default()
    })
    .await;
    let cookie = login(&unconfigured.app, "/test/login-admin").await;
    let (status, body) = analyze(&unconfigured.app, &cookie, json!({"kind": "store"})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "plugin_not_configured");

    let no_settings = fixture_with(Options {
        settings: None,
        ..Options::default()
    })
    .await;
    let cookie = login(&no_settings.app, "/test/login-admin").await;
    let (status, body) = analyze(&no_settings.app, &cookie, json!({"kind": "store"})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "plugin_not_configured");

    for fixture in [&disabled, &unreadable, &unconfigured, &no_settings] {
        assert_eq!(run_count(&fixture.store).await, 0);
        assert_eq!(captured(fixture).len(), 0);
    }
}

#[tokio::test]
async fn analyze_maps_upstream_failures_and_timeouts() {
    let failing = fixture_with(Options {
        mode: MockMode::Error,
        ..Options::default()
    })
    .await;
    let cookie = login(&failing.app, "/test/login-admin").await;
    let (status, body) = analyze(&failing.app, &cookie, json!({"kind": "store"})).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body["error"]["code"], "ai_upstream_error");
    assert_eq!(
        body["error"]["message"],
        "The AI provider did not return a usable answer. Try again shortly."
    );
    assert!(!body.to_string().contains("provider secret detail"));
    assert_eq!(run_count(&failing.store).await, 0);
    assert_eq!(captured(&failing).len(), 1);

    let slow = fixture_with(Options {
        mode: MockMode::Timeout,
        ai_timeout: Duration::from_millis(50),
        ..Options::default()
    })
    .await;
    let cookie = login(&slow.app, "/test/login-admin").await;
    let (status, body) = analyze(&slow.app, &cookie, json!({"kind": "store"})).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body["error"]["code"], "ai_upstream_error");
    assert_eq!(run_count(&slow.store).await, 0);
}

// ---------------------------------------------------------------------------
// Runs listing and guards
// ---------------------------------------------------------------------------

async fn insert_run(store: &Store, id: &str, created_at: &str) {
    store
        .conn()
        .execute(
            "INSERT INTO plugin_ai_runs (id, actor_user_id, kind, subject, model, analysis, created_at)
             VALUES (?, ?, 'store', 'store:7d', 'gpt-4o-mini', ?, ?)",
            libsql::params![id, ADMIN_ID, format!("analysis {id}"), created_at],
        )
        .await
        .expect("insert run");
}

#[tokio::test]
async fn runs_list_orders_and_limits() {
    let fixture = fixture().await;
    let cookie = login(&fixture.app, "/test/login-admin").await;
    insert_run(&fixture.store, "air_a", "2026-11-20T10:00:00Z").await;
    insert_run(&fixture.store, "air_b", "2026-11-22T10:00:00Z").await;
    insert_run(&fixture.store, "air_c", "2026-11-21T10:00:00Z").await;
    insert_run(&fixture.store, "air_d", "2026-11-22T10:00:00Z").await;

    let (status, body) = runs(&fixture.app, &cookie, "").await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = body["runs"]
        .as_array()
        .expect("runs")
        .iter()
        .map(|run| run["id"].as_str().expect("id"))
        .collect();
    assert_eq!(ids, ["air_d", "air_b", "air_c", "air_a"]);

    let (status, body) = runs(&fixture.app, &cookie, "?limit=2").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["runs"].as_array().expect("runs").len(), 2);
    assert_eq!(body["runs"][0]["id"], "air_d");

    for rejected in ["?limit=0", "?limit=51", "?limit=x", "?limit=-2"] {
        let (status, body) = runs(&fixture.app, &cookie, rejected).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{rejected}");
        assert_eq!(body["error"]["code"], "validation_error");
        assert_eq!(
            body["error"]["fields"]["limit"],
            "Choose a limit from 1 to 50."
        );
    }

    let (status, _) = runs(&fixture.app, &cookie, "?limit=+20").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn admin_guards_reject_missing_and_customer_sessions() {
    let fixture = fixture().await;

    let (status, body) = analyze(&fixture.app, "", json!({"kind": "store"})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthorized");

    let (status, _) = runs(&fixture.app, "", "").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let customer = login(&fixture.app, "/test/login-customer").await;
    let (status, body) = analyze(&fixture.app, &customer, json!({"kind": "store"})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "forbidden");

    let (status, _) = runs(&fixture.app, &customer, "").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn unregistered_or_disabled_plugin_answers_404() {
    let unregistered = fixture_with(Options {
        register: false,
        ..Options::default()
    })
    .await;
    let cookie = login(&unregistered.app, "/test/login-admin").await;
    let (status, body) = analyze(&unregistered.app, &cookie, json!({"kind": "store"})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["message"], "Not found.");
    let (status, body) = runs(&unregistered.app, &cookie, "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");

    let disabled = fixture_with(Options {
        disable_plugin: true,
        ..Options::default()
    })
    .await;
    let cookie = login(&disabled.app, "/test/login-admin").await;
    let (status, body) = analyze(&disabled.app, &cookie, json!({"kind": "store"})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["message"], "Plugin not found.");
    assert_eq!(captured(&disabled).len(), 0);
}
