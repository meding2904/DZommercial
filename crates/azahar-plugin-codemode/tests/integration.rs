//! Local libSQL integration tests for the code-mode state machine.
//!
//! Every test runs against an in-memory database with the real migrations and
//! the real store. Preview talks to a loopback mock of an OpenAI-compatible
//! endpoint; no test reaches the network.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use azahar_auth::hash_password;
use azahar_config::Config;
use azahar_db::{PluginMigration, migrate_plugins};
use azahar_httpapi::AppState;
use azahar_httpapi::session_layer::session_layer;
use azahar_httpapi::session_store::LibsqlSessionStore;
use azahar_httpapi::state::configured_ttl;
use azahar_plugin_phone::PhoneHooks;
use azahar_plugins::{Manifest, OptionField, Plugin as PluginTrait, ProductType, Registry};
use azahar_store::{
    Category, Order, OrderItem, Product, ProductFilter, Store, format_time, parse_time,
};
use chrono::{DateTime, Duration, TimeZone, Utc};
use serde_json::{Value, json};
use tower::ServiceExt;

use azahar_plugin_codemode::{
    ApplyInput, Codemode, Plan, Plugin, PreviewInput, build_prompt, encode_plan, parse_plan,
    plan_baseline, plan_dto, product_fingerprint, prompt_plugins,
};

const ACTOR: &str = "usr_itcm_admin";
const OTHER_ACTOR: &str = "usr_itcm_other";
const SECRET_KEY: &str = "azahar-itcm-plugin-key";

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

struct TestOils;

impl PluginTrait for TestOils {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: "oils".to_string(),
            name: "Oil shelf".to_string(),
            version: "1.0.0".to_string(),
            description: "Test shelf".to_string(),
            product_type: Some(ProductType {
                label: "Oil".to_string(),
                fields: vec![
                    OptionField {
                        key: "size".to_string(),
                        label: "Size".to_string(),
                        field_type: "text".to_string(),
                        required: true,
                        ..OptionField::default()
                    },
                    OptionField {
                        key: "concerns".to_string(),
                        label: "Concerns".to_string(),
                        field_type: "list".to_string(),
                        ..OptionField::default()
                    },
                ],
                ..ProductType::default()
            }),
            ..Manifest::default()
        }
    }
}

struct Fixture {
    store: Arc<Store>,
    registry: Arc<RwLock<Registry>>,
    hooks: PhoneHooks,
}

async fn fixture() -> Fixture {
    let database = libsql::Builder::new_local(":memory:")
        .build()
        .await
        .expect("open in-memory database");
    let store = Arc::new(Store::new(database).expect("store"));
    azahar_db::migrate(store.conn()).await.expect("migrate");
    let migrations: Vec<PluginMigration> = Plugin
        .migrations()
        .into_iter()
        .map(|migration| PluginMigration {
            plugin_id: "codemode".to_string(),
            version: migration.version,
            sql: migration.sql,
        })
        .collect();
    migrate_plugins(store.conn(), migrations)
        .await
        .expect("plugin migrations");
    let phone_migrations: Vec<PluginMigration> = azahar_plugin_phone::new()
        .migrations()
        .into_iter()
        .map(|migration| PluginMigration {
            plugin_id: "phone".to_string(),
            version: migration.version,
            sql: migration.sql,
        })
        .collect();
    migrate_plugins(store.conn(), phone_migrations)
        .await
        .expect("phone migrations");

    let mut registry = Registry::new();
    registry
        .register(Arc::new(TestOils))
        .expect("register oils");
    registry
        .register(Arc::new(Plugin))
        .expect("register codemode");

    Fixture {
        store,
        registry: Arc::new(RwLock::new(registry)),
        hooks: PhoneHooks::new(),
    }
}

impl Fixture {
    fn service(&self) -> Codemode<'_> {
        Codemode::new(&self.store, Some(self.registry.as_ref()), &self.hooks)
    }

    async fn save_settings(&self, values: &[(&str, &str)]) {
        let map: HashMap<String, String> = values
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect();
        let env = azahar_plugins::Env::new(self.store.clone(), SECRET_KEY);
        let payload = env.encode_settings(&map).expect("encrypt");
        self.store
            .save_plugin_settings(self.store.conn(), "codemode", &payload, Utc::now())
            .await
            .expect("save settings");
    }

    async fn settings(&self) -> HashMap<String, String> {
        azahar_plugins::Env::new(self.store.clone(), SECRET_KEY)
            .settings("codemode")
            .await
            .expect("settings")
    }
}

fn at(year: i32, month: u32, day: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, 10, 0, 0).unwrap()
}

async fn seed_category(store: &Store) -> Category {
    store
        .upsert_category(
            "oils",
            &Category {
                slug: "body".to_string(),
                name: "Body".to_string(),
                description: "Body oils".to_string(),
                sort_order: 1,
                ..Category::default()
            },
        )
        .await
        .expect("category");
    store.category_by_slug("body").await.expect("load category")
}

async fn seed_product(store: &Store, category: &Category, stock: i32) -> Product {
    let now = at(2026, 11, 20);
    let product = Product {
        id: "prd_itcm_oil".to_string(),
        slug: "itcm-rose-oil".to_string(),
        name: "Rose oil".to_string(),
        subtitle: "A rose oil".to_string(),
        description: "Pressed roses.".to_string(),
        price_cents: 4200,
        currency: "DZD".to_string(),
        category: category.clone(),
        image: "/images/rose.webp".to_string(),
        stock,
        active: true,
        plugin_id: "oils".to_string(),
        attributes: HashMap::from([("size".to_string(), vec!["50 ml".to_string()])]),
        created_at: Some(now),
        updated_at: Some(now),
        ..Product::default()
    };
    store
        .create_product(store.conn(), &product)
        .await
        .expect("create product");
    product
}

async fn seed_order(store: &Store, product: &Product, quantity: i32) -> Order {
    let now = at(2026, 11, 21);
    let order = Order {
        id: "ord_itcm_1".to_string(),
        number: "AZ-ITCM-1".to_string(),
        name: "Buyer".to_string(),
        email: "buyer@example.test".to_string(),
        phone: "0550123456".to_string(),
        address: "1 Rue de Test".to_string(),
        city: "Alger".to_string(),
        subtotal_cents: product.price_cents * i64::from(quantity),
        total_cents: product.price_cents * i64::from(quantity),
        currency: "DZD".to_string(),
        status: "pending".to_string(),
        token_hash: "itcm-token".to_string(),
        created_at: Some(now),
        updated_at: Some(now),
        wilaya_code: 16,
        wilaya_name: "Alger".to_string(),
        commune_name: "Alger Centre".to_string(),
        delivery_type: "home".to_string(),
        ..Order::default()
    };
    let item = OrderItem {
        id: "oit_itcm_1".to_string(),
        order_id: order.id.clone(),
        product_id: product.id.clone(),
        slug: product.slug.clone(),
        name: product.name.clone(),
        image: product.image.clone(),
        unit_price_cents: product.price_cents,
        quantity,
        line_total_cents: product.price_cents * i64::from(quantity),
        ..OrderItem::default()
    };
    store
        .create_order(store.conn(), &order, &[item])
        .await
        .expect("create order");
    order
}

async fn insert_plan(
    store: &Store,
    id: &str,
    actor: &str,
    plan: &Plan,
    status: &str,
    baseline: &str,
    created_at: &str,
) {
    store
        .conn()
        .execute(
            "INSERT INTO codemode_plans (id, actor_user_id, instruction, plan, status, result, baseline, created_at)
             VALUES (?, ?, 'test', ?, ?, '', ?, ?)",
            libsql::params![id, actor, encode_plan(plan), status, baseline, created_at],
        )
        .await
        .expect("insert plan");
}

async fn baseline_for(fixture: &Fixture, plan: &Plan) -> String {
    let service = fixture.service();
    let theme = service.load_theme().await.expect("theme");
    let products = fixture
        .store
        .list_products(&ProductFilter {
            include_inactive: true,
            limit: 500,
            ..ProductFilter::default()
        })
        .await
        .expect("products");
    let orders = service.recent_orders().await.expect("orders");
    plan_baseline(plan, &products, &orders, &theme).expect("baseline")
}

async fn plan_count(store: &Store, status: &str) -> i64 {
    let mut rows = store
        .conn()
        .query(
            "SELECT COUNT(*) FROM codemode_plans WHERE status = ?",
            libsql::params![status],
        )
        .await
        .expect("count");
    let row = rows.next().await.expect("row").expect("some");
    row.get(0).expect("count")
}

async fn plan_row(store: &Store, id: &str) -> (String, String) {
    let mut rows = store
        .conn()
        .query(
            "SELECT status, result FROM codemode_plans WHERE id = ?",
            libsql::params![id],
        )
        .await
        .expect("plan row");
    let row = rows.next().await.expect("row").expect("some");
    (row.get(0).expect("status"), row.get(1).expect("result"))
}

async fn audit_rows(store: &Store) -> Vec<(String, String, String, String)> {
    let mut rows = store
        .conn()
        .query(
            "SELECT action, entity_type, entity_id, detail FROM audit_outbox
             WHERE id LIKE 'aud_%' ORDER BY rowid",
            (),
        )
        .await
        .expect("audits");
    let mut out = Vec::new();
    while let Some(row) = rows.next().await.expect("row") {
        out.push((
            row.get(0).expect("action"),
            row.get(1).expect("entity_type"),
            row.get(2).expect("entity_id"),
            row.get(3).expect("detail"),
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// Mock OpenAI
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct MockState {
    response: Arc<Mutex<String>>,
    requests: Arc<Mutex<Vec<(String, String)>>>,
}

async fn mock_chat(State(state): State<MockState>, headers: HeaderMap, body: String) -> Response {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    state
        .requests
        .lock()
        .expect("requests")
        .push((authorization, body));
    let content = state.response.lock().expect("response").clone();
    axum::Json(json!({"choices": [{"message": {"content": content}}]})).into_response()
}

async fn mock_openai(plan: &str) -> (String, MockState) {
    let state = MockState {
        response: Arc::new(Mutex::new(plan.to_string())),
        requests: Arc::new(Mutex::new(Vec::new())),
    };
    let app = Router::new()
        .route("/chat/completions", post(mock_chat))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock");
    let address = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{address}"), state)
}

fn planner_settings() -> HashMap<String, String> {
    HashMap::from([
        ("enabled".to_string(), "true".to_string()),
        ("api_key".to_string(), "sk-test".to_string()),
        ("model".to_string(), "gpt-4o-mini".to_string()),
    ])
}

// ---------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------

#[tokio::test]
async fn preview_uses_the_prompt_and_stores_a_baseline() {
    let fixture = fixture().await;
    let category = seed_category(&fixture.store).await;
    let product = seed_product(&fixture.store, &category, 4).await;

    let plan_text = r#"{"summary":"Raise the price and update the hero.","actions":[
        {"type":"theme.update","patch":{"hero_heading":"A shorter shelf."}},
        {"type":"product.update","product_id":"prd_itcm_oil","patch":{"price_cents":4500}}
    ]}"#;
    let (base_url, mock) = mock_openai(plan_text).await;
    let settings = planner_settings();
    let now = at(2026, 11, 24);

    let body = fixture
        .service()
        .preview(&PreviewInput {
            instruction: "  Raise the price and update the hero.  ",
            actor_user_id: ACTOR,
            settings: Some(&settings),
            ai_base_url: &base_url,
            ai_timeout: std::time::Duration::from_secs(5),
            now,
        })
        .await
        .expect("preview");

    let value: Value = serde_json::from_str(&body).expect("json");
    let plan = value.get("plan").expect("plan");
    let id = plan.get("id").and_then(Value::as_str).expect("id");
    assert!(id.starts_with("cmp_"));
    assert_eq!(plan.get("status").and_then(Value::as_str), Some("preview"));
    assert_eq!(
        plan.get("created_at").and_then(Value::as_str),
        Some("2026-11-24T10:00:00Z")
    );
    assert_eq!(
        plan.get("summary").and_then(Value::as_str),
        Some("Raise the price and update the hero.")
    );
    assert_eq!(
        plan.pointer("/actions/0/patch/hero_heading")
            .and_then(Value::as_str),
        Some("A shorter shelf.")
    );

    let (authorization, request_body) = {
        let requests = mock.requests.lock().expect("requests");
        assert_eq!(requests.len(), 1);
        (requests[0].0.clone(), requests[0].1.clone())
    };
    assert_eq!(authorization, "Bearer sk-test");
    let payload: Value = serde_json::from_str(&request_body).expect("payload");
    assert_eq!(
        payload.get("model").and_then(Value::as_str),
        Some("gpt-4o-mini")
    );
    assert_eq!(
        payload.get("max_tokens").and_then(Value::as_i64),
        Some(2000)
    );
    let messages = payload
        .get("messages")
        .and_then(Value::as_array)
        .expect("messages");
    assert_eq!(
        messages[0].get("role").and_then(Value::as_str),
        Some("system")
    );
    let system = messages[0]
        .get("content")
        .and_then(Value::as_str)
        .expect("content");
    assert!(system.contains("Current theme JSON"));
    assert!(system.contains("itcm-rose-oil"));
    assert!(system.contains(r#""id":"oils","product_type":"Oil""#));
    assert_eq!(
        messages[1].get("content").and_then(Value::as_str),
        Some("Raise the price and update the hero.")
    );

    let (status, stored_plan) = {
        let mut rows = fixture
            .store
            .conn()
            .query(
                "SELECT status, plan, baseline, instruction FROM codemode_plans WHERE id = ?",
                libsql::params![id],
            )
            .await
            .expect("row");
        let row = rows.next().await.expect("row").expect("some");
        (
            row.get::<String>(0).expect("status"),
            row.get::<String>(1).expect("plan"),
        )
    };
    assert_eq!(status, "preview");
    assert_eq!(
        stored_plan,
        encode_plan(&parse_plan(plan_text).expect("plan"))
    );
    let baseline = baseline_for(&fixture, &parse_plan(plan_text).expect("plan")).await;
    let baseline_value: Value = serde_json::from_str(&baseline).expect("baseline");
    assert!(baseline_value.get("theme").is_some());
    assert_eq!(
        baseline_value
            .get("product:prd_itcm_oil")
            .and_then(Value::as_str),
        Some(product_fingerprint(&product).as_str())
    );
}

#[tokio::test]
async fn preview_rejects_bad_instructions_and_settings() {
    let fixture = fixture().await;
    let settings = planner_settings();
    let now = at(2026, 11, 24);

    let problem = fixture
        .service()
        .preview(&PreviewInput {
            instruction: "   ",
            actor_user_id: ACTOR,
            settings: Some(&settings),
            ai_base_url: "http://127.0.0.1:1",
            ai_timeout: std::time::Duration::from_secs(1),
            now,
        })
        .await
        .expect_err("empty instruction");
    assert_eq!(problem.status.as_u16(), 422);
    assert_eq!(
        problem
            .fields
            .expect("fields")
            .get("instruction")
            .map(String::as_str),
        Some("Describe the change in 1 to 2000 plain characters.")
    );

    let unreadable = fixture
        .service()
        .preview(&PreviewInput {
            instruction: "Do something.",
            actor_user_id: ACTOR,
            settings: None,
            ai_base_url: "http://127.0.0.1:1",
            ai_timeout: std::time::Duration::from_secs(1),
            now,
        })
        .await
        .expect_err("unreadable");
    assert_eq!(unreadable.code, "plugin_settings_unreadable");

    let disabled_settings = HashMap::from([("enabled".to_string(), "false".to_string())]);
    let disabled = fixture
        .service()
        .preview(&PreviewInput {
            instruction: "Do something.",
            actor_user_id: ACTOR,
            settings: Some(&disabled_settings),
            ai_base_url: "http://127.0.0.1:1",
            ai_timeout: std::time::Duration::from_secs(1),
            now,
        })
        .await
        .expect_err("disabled");
    assert_eq!(disabled.code, "plugin_disabled");

    let unconfigured = fixture
        .service()
        .preview(&PreviewInput {
            instruction: "Do something.",
            actor_user_id: ACTOR,
            settings: Some(&HashMap::new()),
            ai_base_url: "http://127.0.0.1:1",
            ai_timeout: std::time::Duration::from_secs(1),
            now,
        })
        .await
        .expect_err("unconfigured");
    assert_eq!(unconfigured.code, "plugin_not_configured");
    assert_eq!(plan_count(&fixture.store, "preview").await, 0);
}

#[tokio::test]
async fn preview_rejects_unusable_provider_answers() {
    let fixture = fixture().await;
    let settings = planner_settings();
    let now = at(2026, 11, 24);
    let (base_url, _mock) = mock_openai("not json").await;

    let problem = fixture
        .service()
        .preview(&PreviewInput {
            instruction: "Do something.",
            actor_user_id: ACTOR,
            settings: Some(&settings),
            ai_base_url: &base_url,
            ai_timeout: std::time::Duration::from_secs(5),
            now,
        })
        .await
        .expect_err("invalid plan");
    assert_eq!(problem.status.as_u16(), 422);
    assert_eq!(problem.code, "invalid_plan");
    assert_eq!(
        problem.message,
        "The AI provider returned a plan this server cannot apply. Try rephrasing the instruction."
    );
    assert_eq!(plan_count(&fixture.store, "preview").await, 0);
}

#[tokio::test]
async fn preview_reports_an_unreachable_provider_as_502() {
    let fixture = fixture().await;
    let settings = planner_settings();
    let problem = fixture
        .service()
        .preview(&PreviewInput {
            instruction: "Do something.",
            actor_user_id: ACTOR,
            settings: Some(&settings),
            ai_base_url: "http://127.0.0.1:1",
            ai_timeout: std::time::Duration::from_millis(200),
            now: at(2026, 11, 24),
        })
        .await
        .expect_err("upstream");
    assert_eq!(problem.status.as_u16(), 502);
    assert_eq!(problem.code, "ai_upstream_error");
}

// ---------------------------------------------------------------------------
// Apply
// ---------------------------------------------------------------------------

#[tokio::test]
async fn apply_theme_and_product_update_then_list_plans() {
    let fixture = fixture().await;
    let category = seed_category(&fixture.store).await;
    seed_product(&fixture.store, &category, 4).await;

    let plan = parse_plan(
        r#"{"summary":"Change the shelf.","actions":[
            {"type":"theme.update","patch":{"hero_heading":"A smaller shelf."}},
            {"type":"product.update","product_id":"prd_itcm_oil","patch":{"price_cents":4500,"stock":9}}
        ]}"#,
    )
    .expect("plan");
    let baseline = baseline_for(&fixture, &plan).await;
    insert_plan(
        &fixture.store,
        "cmp_itcm_1",
        ACTOR,
        &plan,
        "preview",
        &baseline,
        "2026-11-24T10:00:00Z",
    )
    .await;
    let settings = planner_settings();

    let body = fixture
        .service()
        .apply(&ApplyInput {
            plan_id: "cmp_itcm_1",
            actor_user_id: ACTOR,
            settings: Some(&settings),
            now: at(2026, 11, 24) + Duration::hours(1),
        })
        .await
        .expect("apply");
    let value: Value = serde_json::from_str(&body).expect("json");
    assert_eq!(
        value.pointer("/plan/status").and_then(Value::as_str),
        Some("applied")
    );
    let results = value
        .get("results")
        .and_then(Value::as_array)
        .expect("results");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].get("ok").and_then(Value::as_bool), Some(true));
    assert_eq!(
        results[0].get("message").and_then(Value::as_str),
        Some("Updated the storefront theme.")
    );
    assert_eq!(
        results[1].get("message").and_then(Value::as_str),
        Some("Updated \"Rose oil\".")
    );

    let theme = fixture.service().load_theme().await.expect("theme");
    assert_eq!(theme.hero_heading, "A smaller shelf.");
    let product = fixture
        .store
        .product_by_id("prd_itcm_oil", false)
        .await
        .expect("product");
    assert_eq!(product.price_cents, 4500);
    assert_eq!(product.stock, 9);
    assert_eq!(
        product.updated_at.map(|value| format_time(&value)),
        Some("2026-11-24T11:00:00Z".to_string())
    );

    let audits = audit_rows(&fixture.store).await;
    assert_eq!(audits.len(), 2);
    assert_eq!(audits[0].0, "codemode.theme_update");
    assert_eq!(audits[0].1, "storefront");
    assert_eq!(audits[0].2, "default");
    assert_eq!(audits[0].3, r#"{"plan":"cmp_itcm_1","via":"codemode"}"#);
    assert_eq!(audits[1].0, "codemode.product_update");
    assert_eq!(audits[1].2, "prd_itcm_oil");

    let (status, result) = plan_row(&fixture.store, "cmp_itcm_1").await;
    assert_eq!(status, "applied");
    let results: Value = serde_json::from_str(&result).expect("stored results");
    assert_eq!(
        results
            .get("results")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2)
    );

    // A second apply is refused before any work.
    let again = fixture
        .service()
        .apply(&ApplyInput {
            plan_id: "cmp_itcm_1",
            actor_user_id: ACTOR,
            settings: Some(&settings),
            now: at(2026, 11, 24) + Duration::hours(2),
        })
        .await
        .expect_err("already applied");
    assert_eq!(again.status.as_u16(), 409);
    assert_eq!(again.code, "plan_already_applied");

    // The history only lists this actor's plans, newest first.
    let listing = fixture.service().plans(ACTOR, 20).await.expect("plans");
    let listing: Value = serde_json::from_str(&listing).expect("json");
    assert_eq!(
        listing.pointer("/plans/0/summary").and_then(Value::as_str),
        Some("Change the shelf.")
    );
    let other = fixture
        .service()
        .plans(OTHER_ACTOR, 20)
        .await
        .expect("plans");
    let other: Value = serde_json::from_str(&other).expect("json");
    assert_eq!(
        other.get("plans").and_then(Value::as_array).map(Vec::len),
        Some(0)
    );
}

#[tokio::test]
async fn apply_archives_and_creates_products() {
    let fixture = fixture().await;
    let category = seed_category(&fixture.store).await;
    seed_product(&fixture.store, &category, 4).await;

    let plan = parse_plan(
        r#"{"summary":"Replace the shelf.","actions":[
            {"type":"product.archive","product_id":"prd_itcm_oil"},
            {"type":"product.create","product":{"name":"Saffron balm","slug":"itcm-saffron-balm","price_cents":3200,"category_slug":"body","plugin_id":"oils","stock":6,"image":"/images/saffron.webp","description":"A balm.","attributes":{"size":["30 ml"],"concerns":["dry"]}}}
        ]}"#,
    )
    .expect("plan");
    let baseline = baseline_for(&fixture, &plan).await;
    insert_plan(
        &fixture.store,
        "cmp_itcm_create",
        ACTOR,
        &plan,
        "preview",
        &baseline,
        "2026-11-24T10:00:00Z",
    )
    .await;
    let settings = planner_settings();

    let body = fixture
        .service()
        .apply(&ApplyInput {
            plan_id: "cmp_itcm_create",
            actor_user_id: ACTOR,
            settings: Some(&settings),
            now: at(2026, 11, 24) + Duration::hours(1),
        })
        .await
        .expect("apply");
    let value: Value = serde_json::from_str(&body).expect("json");
    let results = value
        .get("results")
        .and_then(Value::as_array)
        .expect("results");
    assert_eq!(
        results[0].get("message").and_then(Value::as_str),
        Some("Archived \"Rose oil\".")
    );
    assert_eq!(
        results[1].get("message").and_then(Value::as_str),
        Some("Created \"Saffron balm\".")
    );

    let archived = fixture
        .store
        .product_by_id("prd_itcm_oil", false)
        .await
        .expect("product");
    assert!(!archived.active);
    let created = fixture
        .store
        .product_by_slug("itcm-saffron-balm", false)
        .await
        .expect("created");
    assert_eq!(created.name, "Saffron balm");
    assert_eq!(created.stock, 6);
    assert_eq!(
        created.attributes.get("size"),
        Some(&vec!["30 ml".to_string()])
    );
    assert!(created.active);
    assert_eq!(
        created.created_at.map(|value| format_time(&value)),
        Some("2026-11-24T11:00:00Z".to_string())
    );
    let audits = audit_rows(&fixture.store).await;
    assert_eq!(audits.len(), 2);
    assert_eq!(audits[1].0, "codemode.product_create");
    assert_eq!(audits[1].2, created.id);
}

#[tokio::test]
async fn apply_order_status_restores_stock_on_cancel() {
    let fixture = fixture().await;
    let category = seed_category(&fixture.store).await;
    let product = seed_product(&fixture.store, &category, 5).await;
    seed_order(&fixture.store, &product, 2).await;

    let plan = parse_plan(
        r#"{"summary":"Cancel the order.","actions":[
            {"type":"order.status","order_id":"ord_itcm_1","status":"cancelled"}
        ]}"#,
    )
    .expect("plan");
    let baseline = baseline_for(&fixture, &plan).await;
    insert_plan(
        &fixture.store,
        "cmp_itcm_cancel",
        ACTOR,
        &plan,
        "preview",
        &baseline,
        "2026-11-24T10:00:00Z",
    )
    .await;
    let settings = planner_settings();

    let body = fixture
        .service()
        .apply(&ApplyInput {
            plan_id: "cmp_itcm_cancel",
            actor_user_id: ACTOR,
            settings: Some(&settings),
            now: at(2026, 11, 24) + Duration::hours(1),
        })
        .await
        .expect("apply");
    let value: Value = serde_json::from_str(&body).expect("json");
    assert_eq!(
        value.pointer("/results/0/message").and_then(Value::as_str),
        Some("Moved order AZ-ITCM-1 from pending to cancelled.")
    );

    let order = fixture
        .store
        .order_by_id("ord_itcm_1")
        .await
        .expect("order");
    assert_eq!(order.status, "cancelled");
    assert_eq!(
        order.updated_at.map(|value| format_time(&value)),
        Some("2026-11-24T11:00:00Z".to_string())
    );
    let restocked = fixture
        .store
        .product_by_id("prd_itcm_oil", false)
        .await
        .expect("product");
    assert_eq!(restocked.stock, 7);
    let audits = audit_rows(&fixture.store).await;
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0].0, "codemode.order_status");
    assert_eq!(audits[0].1, "order");
}

#[tokio::test]
async fn apply_rejects_a_stale_baseline() {
    let fixture = fixture().await;
    let category = seed_category(&fixture.store).await;
    let product = seed_product(&fixture.store, &category, 4).await;
    let stale = product_fingerprint(&product).replace("\"stock\":4", "\"stock\":3");

    let plan = parse_plan(
        r#"{"summary":"Change the price.","actions":[
            {"type":"product.update","product_id":"prd_itcm_oil","patch":{"price_cents":4500}}
        ]}"#,
    )
    .expect("plan");
    let mut baseline =
        serde_json::from_str::<Value>(&baseline_for(&fixture, &plan).await).expect("baseline");
    baseline["product:prd_itcm_oil"] = Value::String(stale);
    insert_plan(
        &fixture.store,
        "cmp_itcm_stale",
        ACTOR,
        &plan,
        "preview",
        &baseline.to_string(),
        "2026-11-24T10:00:00Z",
    )
    .await;
    let settings = planner_settings();

    let problem = fixture
        .service()
        .apply(&ApplyInput {
            plan_id: "cmp_itcm_stale",
            actor_user_id: ACTOR,
            settings: Some(&settings),
            now: at(2026, 11, 24) + Duration::hours(1),
        })
        .await
        .expect_err("stale");
    assert_eq!(problem.status.as_u16(), 409);
    assert_eq!(problem.code, "plan_stale");
    assert_eq!(
        problem.message,
        "The store changed after this preview. Preview the change again."
    );
    let (status, result) = plan_row(&fixture.store, "cmp_itcm_stale").await;
    assert_eq!(status, "failed");
    assert_eq!(
        result,
        r#"{"results":[{"type":"product.update","ok":false,"message":"codemode plan is stale"}]}"#
    );
    let product = fixture
        .store
        .product_by_id("prd_itcm_oil", false)
        .await
        .expect("product");
    assert_eq!(product.price_cents, 4200);
}

#[tokio::test]
async fn apply_rolls_back_every_action_on_failure() {
    let fixture = fixture().await;
    let category = seed_category(&fixture.store).await;
    let mut product = seed_product(&fixture.store, &category, 4).await;
    // A valid sale window blocks a later price change below the sale price.
    product.sale_price_cents = Some(4000);
    product.sale_starts_at = Some(at(2026, 11, 1));
    product.sale_ends_at = Some(at(2026, 12, 1));
    fixture
        .store
        .update_product(fixture.store.conn(), &product)
        .await
        .expect("sale");

    let plan = parse_plan(
        r#"{"summary":"Archive then discount.","actions":[
            {"type":"product.archive","product_id":"prd_itcm_oil"},
            {"type":"product.update","product_id":"prd_itcm_oil","patch":{"price_cents":3000}}
        ]}"#,
    )
    .expect("plan");
    let baseline = baseline_for(&fixture, &plan).await;
    insert_plan(
        &fixture.store,
        "cmp_itcm_rollback",
        ACTOR,
        &plan,
        "preview",
        &baseline,
        "2026-11-24T10:00:00Z",
    )
    .await;
    let settings = planner_settings();

    let body = fixture
        .service()
        .apply(&ApplyInput {
            plan_id: "cmp_itcm_rollback",
            actor_user_id: ACTOR,
            settings: Some(&settings),
            now: at(2026, 11, 24) + Duration::hours(1),
        })
        .await
        .expect("apply returns the failed results");
    let value: Value = serde_json::from_str(&body).expect("json");
    assert_eq!(
        value.pointer("/plan/status").and_then(Value::as_str),
        Some("failed")
    );
    let results = value
        .get("results")
        .and_then(Value::as_array)
        .expect("results");
    assert_eq!(results[0].get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        results[0].get("message").and_then(Value::as_str),
        Some("Rolled back because a later action failed. Archived \"Rose oil\".")
    );
    assert_eq!(
        results[1].get("message").and_then(Value::as_str),
        Some("the sale price must stay below the regular price")
    );

    let product = fixture
        .store
        .product_by_id("prd_itcm_oil", false)
        .await
        .expect("product");
    assert!(product.active, "the archive was rolled back");
    assert_eq!(product.price_cents, 4200);
    assert!(audit_rows(&fixture.store).await.is_empty());
    let (status, result) = plan_row(&fixture.store, "cmp_itcm_rollback").await;
    assert_eq!(status, "failed");
    assert!(result.contains("The plan has been rolled back") || result.contains("Rolled back"));
}

#[tokio::test]
async fn apply_checks_settings_and_the_plan_id() {
    let fixture = fixture().await;
    let plan =
        parse_plan(r#"{"summary":"A.","actions":[{"type":"theme.update","patch":{"radius":4}}]}"#)
            .expect("plan");

    let bad_id = fixture
        .service()
        .apply(&ApplyInput {
            plan_id: "   ",
            actor_user_id: ACTOR,
            settings: Some(&planner_settings()),
            now: at(2026, 11, 24),
        })
        .await
        .expect_err("empty id");
    assert_eq!(bad_id.status.as_u16(), 422);
    assert_eq!(
        bad_id
            .fields
            .expect("fields")
            .get("plan_id")
            .map(String::as_str),
        Some("Choose a saved preview plan.")
    );

    let missing = fixture
        .service()
        .apply(&ApplyInput {
            plan_id: "cmp_missing",
            actor_user_id: ACTOR,
            settings: Some(&planner_settings()),
            now: at(2026, 11, 24),
        })
        .await
        .expect_err("missing");
    assert_eq!(missing.status.as_u16(), 404);
    assert_eq!(missing.message, "That plan no longer exists.");

    insert_plan(
        &fixture.store,
        "cmp_itcm_settings",
        ACTOR,
        &plan,
        "preview",
        &baseline_for(&fixture, &plan).await,
        "2026-11-24T10:00:00Z",
    )
    .await;

    let unreadable = fixture
        .service()
        .apply(&ApplyInput {
            plan_id: "cmp_itcm_settings",
            actor_user_id: ACTOR,
            settings: None,
            now: at(2026, 11, 24),
        })
        .await
        .expect_err("unreadable");
    assert_eq!(unreadable.code, "plugin_settings_unreadable");

    let disabled_settings = HashMap::from([("enabled".to_string(), "false".to_string())]);
    let disabled = fixture
        .service()
        .apply(&ApplyInput {
            plan_id: "cmp_itcm_settings",
            actor_user_id: ACTOR,
            settings: Some(&disabled_settings),
            now: at(2026, 11, 24),
        })
        .await
        .expect_err("disabled");
    assert_eq!(disabled.code, "plugin_disabled");
    assert_eq!(
        disabled.message,
        "Enable the plugin in its settings before applying a plan."
    );

    // The plan belongs to its actor: another admin cannot apply it.
    let foreign = fixture
        .service()
        .apply(&ApplyInput {
            plan_id: "cmp_itcm_settings",
            actor_user_id: OTHER_ACTOR,
            settings: Some(&planner_settings()),
            now: at(2026, 11, 24),
        })
        .await
        .expect_err("foreign");
    assert_eq!(foreign.status.as_u16(), 404);
}

#[tokio::test]
async fn apply_marks_a_plan_without_a_readable_baseline_stale() {
    let fixture = fixture().await;
    let plan =
        parse_plan(r#"{"summary":"A.","actions":[{"type":"theme.update","patch":{"radius":4}}]}"#)
            .expect("plan");
    insert_plan(
        &fixture.store,
        "cmp_itcm_nobase",
        ACTOR,
        &plan,
        "preview",
        "",
        "2026-11-24T10:00:00Z",
    )
    .await;

    let problem = fixture
        .service()
        .apply(&ApplyInput {
            plan_id: "cmp_itcm_nobase",
            actor_user_id: ACTOR,
            settings: Some(&planner_settings()),
            now: at(2026, 11, 24),
        })
        .await
        .expect_err("baseline");
    assert_eq!(problem.code, "plan_stale");
    assert_eq!(
        problem.message,
        "This preview has no readable baseline. Preview the change again."
    );
    let (status, _) = plan_row(&fixture.store, "cmp_itcm_nobase").await;
    assert_eq!(status, "failed");
}

// ---------------------------------------------------------------------------
// Route-level guards
// ---------------------------------------------------------------------------

async fn insert_user(store: &Store, id: &str, email: &str, role: &str) {
    let hash = hash_password("correct horse battery staple").expect("hash");
    store
        .create_user(
            store.conn(),
            &azahar_store::User {
                id: id.to_string(),
                name: id.to_string(),
                email: email.to_string(),
                password_hash: hash,
                role: role.to_string(),
                theme_preset: "cellar".to_string(),
                created_at: parse_time("2026-01-01T00:00:00Z"),
                updated_at: parse_time("2026-01-01T00:00:00Z"),
            },
        )
        .await
        .expect("user");
}

async fn router_fixture() -> (Router, Arc<Store>, AppState) {
    let fixture = fixture().await;
    insert_user(&fixture.store, ACTOR, "admin@example.com", "admin").await;
    insert_user(
        &fixture.store,
        "usr_itcm_staff",
        "staff@example.com",
        "employee",
    )
    .await;
    insert_user(
        &fixture.store,
        "usr_itcm_client",
        "client@example.com",
        "customer",
    )
    .await;

    let mut registry = Registry::new();
    registry
        .register(Arc::new(TestOils))
        .expect("register oils");
    registry
        .register(Arc::new(Plugin))
        .expect("register codemode");

    let cfg = Config {
        app_env: "development".to_string(),
        app_origin: "http://localhost:3000".to_string(),
        plugin_secret_key: SECRET_KEY.to_string(),
        session_ttl: std::time::Duration::from_secs(48 * 60 * 60),
        ..Config::default()
    };
    let state = AppState::new(
        cfg.clone(),
        fixture.store.clone(),
        registry,
        Arc::new(NoCourier),
    );
    let sessions = session_layer(
        LibsqlSessionStore::new(fixture.store.conn().clone()),
        cfg.cookie_secure,
        configured_ttl(&cfg),
    );
    let test_state = state.clone();
    let app = Router::new()
        .route(
            "/api/auth/login",
            post(azahar_httpapi::handlers_auth::login),
        )
        .merge(azahar_plugin_codemode::codemode_routes())
        .with_state(state)
        .layer(sessions);
    (app, fixture.store, test_state)
}

/// The router fixture needs a courier value; every method is unused by these
/// routes, so each one is a typed "unavailable".
struct NoCourier;

#[async_trait::async_trait]
impl azahar_courier::Courier for NoCourier {
    async fn wilayas(&self) -> Result<Vec<azahar_courier::Wilaya>, azahar_courier::Error> {
        Err(azahar_courier::Error::transport("unused"))
    }

    async fn wilaya(&self, _code: i32) -> Result<azahar_courier::Wilaya, azahar_courier::Error> {
        Err(azahar_courier::Error::UnknownWilaya)
    }

    async fn communes(
        &self,
        _wilaya_code: i32,
    ) -> Result<Vec<azahar_courier::Commune>, azahar_courier::Error> {
        Err(azahar_courier::Error::transport("unused"))
    }

    async fn commune(
        &self,
        _wilaya_code: i32,
        _name: &str,
    ) -> Result<String, azahar_courier::Error> {
        Err(azahar_courier::Error::UnknownCommune)
    }

    async fn desks(
        &self,
        _wilaya_code: i32,
    ) -> Result<Vec<azahar_courier::Desk>, azahar_courier::Error> {
        Err(azahar_courier::Error::transport("unused"))
    }

    async fn desk(
        &self,
        _wilaya_code: i32,
        _id: &str,
    ) -> Result<azahar_courier::Desk, azahar_courier::Error> {
        Err(azahar_courier::Error::UnknownDesk)
    }

    async fn create_order(
        &self,
        _request: azahar_courier::OrderRequest,
    ) -> Result<azahar_courier::Shipment, azahar_courier::Error> {
        Err(azahar_courier::Error::transport("unused"))
    }

    async fn track(
        &self,
        _courier_key: &str,
        _tracking_number: &str,
    ) -> Result<azahar_courier::Tracking, azahar_courier::Error> {
        Err(azahar_courier::Error::transport("unused"))
    }
}

async fn login(app: &Router, email: &str) -> HeaderValue {
    let request = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"email": email, "password": "correct horse battery staple"}).to_string(),
        ))
        .expect("request");
    let response = app.clone().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .expect("set-cookie")
        .to_str()
        .expect("cookie")
        .split(';')
        .next()
        .expect("pair")
        .to_string();
    HeaderValue::from_str(&cookie).expect("header")
}

async fn call(app: &Router, method: &str, uri: &str, cookie: Option<&HeaderValue>) -> Response {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    app.clone()
        .oneshot(builder.body(Body::empty()).expect("request"))
        .await
        .expect("response")
}

async fn body_value(response: Response) -> Value {
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    serde_json::from_slice(&bytes).expect("json")
}

#[tokio::test]
async fn routes_enforce_the_admin_guard_and_plugin_state() {
    let (app, store, state) = router_fixture().await;
    insert_plan(
        &store,
        "cmp_itcm_route",
        ACTOR,
        &parse_plan(r#"{"summary":"A.","actions":[{"type":"theme.update","patch":{"radius":4}}]}"#)
            .expect("plan"),
        "preview",
        "{}",
        "2026-11-24T10:00:00Z",
    )
    .await;

    let anonymous = call(&app, "GET", "/api/plugins/codemode/admin/plans", None).await;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    let customer = login(&app, "client@example.com").await;
    let forbidden = call(
        &app,
        "GET",
        "/api/plugins/codemode/admin/plans",
        Some(&customer),
    )
    .await;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    let staff = login(&app, "staff@example.com").await;
    let staff_forbidden = call(
        &app,
        "GET",
        "/api/plugins/codemode/admin/plans",
        Some(&staff),
    )
    .await;
    assert_eq!(staff_forbidden.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_value(staff_forbidden)
            .await
            .pointer("/error/code")
            .and_then(Value::as_str),
        Some("forbidden")
    );

    let admin = login(&app, "admin@example.com").await;
    let ok = call(
        &app,
        "GET",
        "/api/plugins/codemode/admin/plans",
        Some(&admin),
    )
    .await;
    assert_eq!(ok.status(), StatusCode::OK);
    let value = body_value(ok).await;
    assert_eq!(
        value.pointer("/plans/0/id").and_then(Value::as_str),
        Some("cmp_itcm_route")
    );

    let bad_limit = call(
        &app,
        "GET",
        "/api/plugins/codemode/admin/plans?limit=0",
        Some(&admin),
    )
    .await;
    assert_eq!(bad_limit.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let preview_without_body = call(
        &app,
        "POST",
        "/api/plugins/codemode/admin/preview",
        Some(&admin),
    )
    .await;
    assert_eq!(
        preview_without_body.status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );

    state
        .plugins
        .write()
        .expect("registry")
        .set_enabled("codemode", false);
    let disabled = call(
        &app,
        "GET",
        "/api/plugins/codemode/admin/plans",
        Some(&admin),
    )
    .await;
    assert_eq!(disabled.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        body_value(disabled)
            .await
            .pointer("/error/message")
            .and_then(Value::as_str),
        Some("Plugin not found.")
    );
}

// ---------------------------------------------------------------------------
// Small helpers that need the public API surface
// ---------------------------------------------------------------------------

#[tokio::test]
async fn settings_round_trip_through_the_plugin_env() {
    let fixture = fixture().await;
    fixture
        .save_settings(&[
            ("enabled", "true"),
            ("api_key", "sk-test"),
            ("model", "gpt-4o-mini"),
        ])
        .await;
    let settings = fixture.settings().await;
    assert_eq!(settings.get("api_key").map(String::as_str), Some("sk-test"));
    assert_eq!(
        settings.get("model").map(String::as_str),
        Some("gpt-4o-mini")
    );
}

#[test]
fn public_plan_helpers_keep_the_go_shapes() {
    let plan =
        parse_plan(r#"{"summary":"A.","actions":[{"type":"theme.update","patch":{"radius":4}}]}"#)
            .expect("plan");
    let dto = plan_dto("cmp_1", &plan, "preview", "2026-11-24T10:00:00Z");
    assert!(dto.starts_with("{\"actions\":"));
    assert!(dto.ends_with("}"));
    let value: Value = serde_json::from_str(&dto).expect("json");
    assert_eq!(value.get("id").and_then(Value::as_str), Some("cmp_1"));

    let plugins = prompt_plugins(None);
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].id, "core");

    let theme = azahar_storefront::Theme::default();
    let prompt = build_prompt(&theme, &[], &[], &[], &plugins);
    assert!(prompt.contains("Allowed plugins with option fields JSON"));
}
