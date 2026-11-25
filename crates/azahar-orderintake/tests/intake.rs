//! Order intake behaviour against an in-memory libSQL database, with fake
//! hook providers for the checkout and status hook contracts.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use async_trait::async_trait;
use azahar_db::migrate;
use azahar_orderintake::{
    Change, CheckoutHook, CheckoutLine, Customer, Destination, HookError, HookProvider, Intake,
    IntakeError, Line, NoopHooks, OrderRef, OrderStatusHook, Problem, Request, build_lines,
    checkout_channel, counter_channel,
};
use azahar_plugins::{
    Manifest, OptionField, Plugin, Problem as PluginProblem, ProductType, Registry,
};
use azahar_store::{Category, Order, Product, Store, format_time};
use chrono::{Datelike, Utc};
use libsql::Builder;
use serde_json::Value;

async fn memory_store() -> Arc<Store> {
    let database = Builder::new_local(":memory:")
        .build()
        .await
        .expect("open in-memory database");
    let store = Store::new(database).expect("store");
    migrate(store.conn()).await.expect("migrate");
    Arc::new(store)
}

fn fixture_manifest() -> Manifest {
    Manifest {
        id: "itoopt".to_string(),
        name: "Intake fixture".to_string(),
        version: "1.0.0".to_string(),
        product_type: Some(ProductType {
            label: "Fixture product".to_string(),
            fields: vec![
                OptionField {
                    key: "scent".to_string(),
                    label: "Scent".to_string(),
                    field_type: "select".to_string(),
                    required: true,
                    ..OptionField::default()
                },
                OptionField {
                    key: "finish".to_string(),
                    label: "Finish".to_string(),
                    field_type: "select".to_string(),
                    ..OptionField::default()
                },
                OptionField {
                    key: "engraving".to_string(),
                    label: "Engraving".to_string(),
                    field_type: "text".to_string(),
                    ..OptionField::default()
                },
            ],
            ..ProductType::default()
        }),
        ..Manifest::default()
    }
}

struct FixturePlugin {
    manifest: Manifest,
}

impl Plugin for FixturePlugin {
    fn manifest(&self) -> Manifest {
        self.manifest.clone()
    }
}

fn fixture_registry() -> Arc<RwLock<Registry>> {
    let mut registry = Registry::new();
    registry
        .register(Arc::new(FixturePlugin {
            manifest: fixture_manifest(),
        }))
        .expect("register fixture plugin");
    Arc::new(RwLock::new(registry))
}

async fn fixture_category(store: &Store, slug: &str) -> Category {
    let category = Category {
        slug: slug.to_string(),
        name: format!("Category {slug}"),
        ..Category::default()
    };
    store
        .upsert_category("core", &category)
        .await
        .expect("create category");
    store.category_by_slug(slug).await.expect("load category")
}

fn options(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

fn attributes() -> HashMap<String, Vec<String>> {
    HashMap::from([
        (
            "scent".to_string(),
            vec!["Rose".to_string(), "Oud".to_string()],
        ),
        (
            "finish".to_string(),
            vec!["Matte".to_string(), "Gloss".to_string()],
        ),
    ])
}

#[allow(clippy::too_many_arguments)]
async fn fixture_product(
    store: &Store,
    category: &Category,
    label: &str,
    plugin_id: &str,
    price_cents: i64,
    stock: i32,
    active: bool,
    attributes: HashMap<String, Vec<String>>,
) -> Product {
    let now = Utc::now();
    let product = Product {
        id: format!("ito_prod_{label}"),
        slug: format!("ito-product-{label}"),
        name: format!("Fixture {label}"),
        description: "Temporary order intake fixture.".to_string(),
        price_cents,
        currency: "DZD".to_string(),
        size: "10 ml".to_string(),
        category: category.clone(),
        image: format!("/images/{label}.webp"),
        stock,
        active,
        plugin_id: plugin_id.to_string(),
        attributes,
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

fn line(product: &Product, quantity: i64) -> Line {
    Line {
        product_id: product.id.clone(),
        quantity,
        options: HashMap::new(),
        expected_price_cents: None,
    }
}

fn with_options(product: &Product, quantity: i64, options: HashMap<String, String>) -> Line {
    Line {
        product_id: product.id.clone(),
        quantity,
        options,
        expected_price_cents: None,
    }
}

fn checkout_request(lines: Vec<Line>, token_hash: &str) -> Request {
    Request {
        lines,
        channel: checkout_channel("2026-11-14"),
        customer: Customer {
            name: "Test Buyer".to_string(),
            email: "buyer@example.test".to_string(),
            phone: "0550123456".to_string(),
            address: "1 Rue des Tests".to_string(),
            ..Customer::default()
        },
        destination: Destination {
            wilaya_code: 16,
            wilaya_name: "Alger".to_string(),
            commune_name: "Alger Centre".to_string(),
            delivery_type: "home".to_string(),
            ..Destination::default()
        },
        status: "pending".to_string(),
        token_hash: token_hash.to_string(),
        actor_user_id: "usr_buyer".to_string(),
    }
}

fn counter_request(lines: Vec<Line>, token_hash: &str) -> Request {
    Request {
        lines,
        channel: counter_channel(),
        customer: Customer {
            name: "Counter sale".to_string(),
            ..Customer::default()
        },
        destination: Destination {
            delivery_type: "pickup".to_string(),
            ..Destination::default()
        },
        status: "pending".to_string(),
        token_hash: token_hash.to_string(),
        actor_user_id: "usr_test".to_string(),
    }
}

fn expect_problem(error: IntakeError, status: u16, code: &str) -> Problem {
    match error {
        IntakeError::Problem(problem) => {
            assert_eq!(problem.status, status, "problem status");
            assert_eq!(problem.code, code, "problem code");
            problem
        }
        other => panic!("expected an intake problem, got {other:?}"),
    }
}

fn items_field(problem: &Problem) -> String {
    problem
        .fields
        .as_ref()
        .and_then(|fields| fields.get("items"))
        .cloned()
        .unwrap_or_default()
}

async fn stock(store: &Store, product: &Product) -> i32 {
    store
        .product_by_id(&product.id, false)
        .await
        .expect("reload product")
        .stock
}

async fn order_count(store: &Store, token_hash: &str) -> i64 {
    let mut rows = store
        .conn()
        .query(
            "SELECT COUNT(*) FROM orders WHERE token_hash = ?",
            libsql::params![token_hash],
        )
        .await
        .expect("count orders");
    rows.next()
        .await
        .expect("count row")
        .expect("count value")
        .get(0)
        .expect("count")
}

async fn order_item_count(store: &Store, order_id: &str) -> i64 {
    let mut rows = store
        .conn()
        .query(
            "SELECT COUNT(*) FROM order_items WHERE order_id = ?",
            libsql::params![order_id],
        )
        .await
        .expect("count items");
    rows.next()
        .await
        .expect("count row")
        .expect("count value")
        .get(0)
        .expect("count")
}

async fn audit_detail(store: &Store, order_id: &str, action: &str) -> Value {
    let mut rows = store
        .conn()
        .query(
            "SELECT detail FROM audit_outbox
             WHERE entity_type = 'order' AND entity_id = ? AND action = ?",
            libsql::params![order_id, action],
        )
        .await
        .expect("load audit row");
    let row = rows
        .next()
        .await
        .expect("audit step")
        .expect("audit row exists");
    let detail: String = row.get(0).expect("audit detail");
    serde_json::from_str(&detail).expect("audit detail json")
}

async fn audit_count(store: &Store, order_id: &str) -> i64 {
    let mut rows = store
        .conn()
        .query(
            "SELECT COUNT(*) FROM audit_outbox WHERE entity_type = 'order' AND entity_id = ?",
            libsql::params![order_id],
        )
        .await
        .expect("count audits");
    rows.next()
        .await
        .expect("count row")
        .expect("count value")
        .get(0)
        .expect("count")
}

async fn stored_order(store: &Store, id: &str) -> Order {
    store.order_by_id(id).await.expect("stored order")
}

fn sequence(number: &str, year: i32) -> i64 {
    number
        .strip_prefix(&format!("AZ-{year}-"))
        .unwrap_or_else(|| panic!("unexpected order number {number:?}"))
        .parse()
        .expect("numeric sequence")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedLine {
    product_id: String,
    quantity: i64,
    options: HashMap<String, String>,
    order_id: String,
    item_id: String,
}

fn record(line: &CheckoutLine) -> RecordedLine {
    RecordedLine {
        product_id: line.product.id.clone(),
        quantity: line.quantity,
        options: line.options.clone(),
        order_id: line.order_id.clone(),
        item_id: line.item_id.clone(),
    }
}

#[derive(Debug, Default)]
struct CheckoutCalls {
    validated: Vec<RecordedLine>,
    created: Vec<(OrderRef, Vec<RecordedLine>)>,
}

#[derive(Debug)]
struct FakeCheckout {
    problem: Option<PluginProblem>,
    fail_created: bool,
    calls: Mutex<CheckoutCalls>,
}

impl FakeCheckout {
    fn accepting() -> Self {
        Self {
            problem: None,
            fail_created: false,
            calls: Mutex::new(CheckoutCalls::default()),
        }
    }

    fn refusing(problem: PluginProblem) -> Self {
        Self {
            problem: Some(problem),
            ..Self::accepting()
        }
    }

    fn failing_created() -> Self {
        Self {
            fail_created: true,
            ..Self::accepting()
        }
    }

    fn calls(&self) -> std::sync::MutexGuard<'_, CheckoutCalls> {
        self.calls.lock().expect("checkout calls")
    }
}

#[async_trait]
impl CheckoutHook for FakeCheckout {
    async fn validate_checkout_line(
        &self,
        _conn: &libsql::Connection,
        line: &CheckoutLine,
    ) -> Option<PluginProblem> {
        self.calls().validated.push(record(line));
        self.problem.clone()
    }

    async fn on_order_created(
        &self,
        _conn: &libsql::Connection,
        order: &OrderRef,
        lines: &[CheckoutLine],
    ) -> Result<(), HookError> {
        self.calls()
            .created
            .push((order.clone(), lines.iter().map(record).collect()));
        if self.fail_created {
            return Err(HookError::message("checkout hook failed"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StatusCall {
    order_id: String,
    from: String,
    to: String,
}

#[derive(Debug)]
struct FakeStatus {
    fail: bool,
    calls: Mutex<Vec<StatusCall>>,
}

impl FakeStatus {
    fn accepting() -> Self {
        Self {
            fail: false,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn failing() -> Self {
        Self {
            fail: true,
            ..Self::accepting()
        }
    }

    fn calls(&self) -> Vec<StatusCall> {
        self.calls.lock().expect("status calls").clone()
    }
}

#[async_trait]
impl OrderStatusHook for FakeStatus {
    async fn on_order_status_changed(
        &self,
        _conn: &libsql::Connection,
        order_id: &str,
        from: &str,
        to: &str,
    ) -> Result<(), HookError> {
        self.calls.lock().expect("status calls").push(StatusCall {
            order_id: order_id.to_string(),
            from: from.to_string(),
            to: to.to_string(),
        });
        if self.fail {
            return Err(HookError::message("status hook failed"));
        }
        Ok(())
    }
}

#[derive(Default)]
struct FakeHooks {
    checkout: Vec<(String, Arc<FakeCheckout>)>,
    status: Vec<Arc<FakeStatus>>,
}

impl FakeHooks {
    fn with_checkout(plugin_id: &str, hook: Arc<FakeCheckout>) -> Self {
        Self {
            checkout: vec![(plugin_id.to_string(), hook)],
            status: Vec::new(),
        }
    }

    fn with_status(hook: Arc<FakeStatus>) -> Self {
        Self {
            checkout: Vec::new(),
            status: vec![hook],
        }
    }

    fn with_both(checkout: Arc<FakeCheckout>, status: Arc<FakeStatus>) -> Self {
        Self {
            checkout: vec![("itoopt".to_string(), checkout)],
            status: vec![status],
        }
    }
}

impl HookProvider for FakeHooks {
    fn checkout_hook(&self, plugin_id: &str) -> Option<Arc<dyn CheckoutHook>> {
        self.checkout
            .iter()
            .find(|(id, _)| id == plugin_id)
            .map(|(_, hook)| Arc::clone(hook) as Arc<dyn CheckoutHook>)
    }

    fn status_hooks(&self) -> Vec<Arc<dyn OrderStatusHook>> {
        self.status
            .iter()
            .map(|hook| Arc::clone(hook) as Arc<dyn OrderStatusHook>)
            .collect()
    }
}

fn build_intake(
    store: &Arc<Store>,
    registry: &Arc<RwLock<Registry>>,
    hooks: Arc<dyn HookProvider>,
) -> Intake {
    Intake::new(Arc::clone(store), Arc::clone(registry), hooks)
}

fn noop_intake(store: &Arc<Store>, registry: &Arc<RwLock<Registry>>) -> Intake {
    build_intake(store, registry, Arc::new(NoopHooks))
}

#[tokio::test]
async fn checkout_places_order_with_prices_shipping_stock_number_and_audit() {
    let store = memory_store().await;
    let registry = fixture_registry();
    let intake = noop_intake(&store, &registry);
    let category = fixture_category(&store, "ito-checkout").await;
    let product = fixture_product(
        &store,
        &category,
        "checkout",
        "core",
        400_000,
        10,
        true,
        HashMap::new(),
    )
    .await;
    let year = Utc::now().year();

    let placed = intake
        .place(checkout_request(
            vec![line(&product, 2)],
            "ito_token_checkout_a",
        ))
        .await
        .expect("place order");

    assert_eq!(placed.order.currency, "DZD");
    assert_eq!(placed.order.status, "pending");
    assert_eq!(placed.order.city, "Alger Centre");
    assert_eq!(placed.order.delivery_type, "home");
    assert_eq!(placed.order.subtotal_cents, 800_000);
    assert_eq!(placed.order.shipping_cents, 60_000);
    assert_eq!(placed.order.total_cents, 860_000);
    assert_eq!(placed.order.token_hash, "ito_token_checkout_a");
    assert_eq!(placed.order.user_id, None);
    assert!(placed.order.created_at.is_some());
    assert!(placed.order.number.starts_with(&format!("AZ-{year}-")));
    let first_sequence = sequence(&placed.order.number, year);

    assert_eq!(placed.items.len(), 1);
    let item = &placed.items[0];
    assert_eq!(item.product_id, product.id);
    assert_eq!(item.slug, product.slug);
    assert_eq!(item.name, product.name);
    assert_eq!(item.image, product.image);
    assert_eq!(item.unit_price_cents, 400_000);
    assert_eq!(item.quantity, 2);
    assert_eq!(item.line_total_cents, 800_000);
    assert_eq!(item.plugin_id, "core");
    assert!(item.options.is_empty());
    assert_eq!(item.options_label, "");

    assert_eq!(stock(&store, &product).await, 8);
    let detail = audit_detail(&store, &placed.order.id, "order.terms_accepted").await;
    assert_eq!(detail["version"], "2026-11-14");
    assert_eq!(
        detail["accepted_at"],
        format_time(placed.order.created_at.as_ref().expect("created"))
    );

    let second = intake
        .place(checkout_request(
            vec![line(&product, 3)],
            "ito_token_checkout_b",
        ))
        .await
        .expect("place second order");
    assert_eq!(second.order.subtotal_cents, 1_200_000);
    assert_eq!(second.order.shipping_cents, 0);
    assert_eq!(second.order.total_cents, 1_200_000);
    assert!(sequence(&second.order.number, year) > first_sequence);
    assert_eq!(stock(&store, &product).await, 5);
}

#[tokio::test]
async fn options_are_validated_and_labelled() {
    let store = memory_store().await;
    let registry = fixture_registry();
    let intake = noop_intake(&store, &registry);
    let category = fixture_category(&store, "ito-options").await;
    let core = fixture_product(
        &store,
        &category,
        "core-options",
        "core",
        100_000,
        10,
        true,
        HashMap::new(),
    )
    .await;
    let plugin = fixture_product(
        &store,
        &category,
        "plugin-options",
        "itoopt",
        123_400,
        50,
        true,
        attributes(),
    )
    .await;

    let refusal = intake
        .place(checkout_request(
            vec![with_options(&core, 1, options(&[("scent", "Rose")]))],
            "ito_token_core_options",
        ))
        .await
        .expect_err("core products take no options");
    let refusal = expect_problem(refusal, 409, "conflict");
    assert_eq!(refusal.message, "Some product options have changed.");
    assert_eq!(items_field(&refusal), "This product does not take options.");

    let refusal = intake
        .place(checkout_request(
            vec![line(&plugin, 1)],
            "ito_token_missing_option",
        ))
        .await
        .expect_err("required option is missing");
    let refusal = expect_problem(refusal, 409, "conflict");
    assert_eq!(items_field(&refusal), "Choose a scent for every product.");

    let refusal = intake
        .place(checkout_request(
            vec![with_options(&plugin, 1, options(&[("scent", "Lavender")]))],
            "ito_token_bad_option",
        ))
        .await
        .expect_err("option value is not offered");
    let refusal = expect_problem(refusal, 409, "conflict");
    assert_eq!(items_field(&refusal), "Choose an available scent.");

    let refusal = intake
        .place(checkout_request(
            vec![with_options(
                &plugin,
                1,
                options(&[("scent", "Rose"), ("size", "10 ml")]),
            )],
            "ito_token_unknown_option",
        ))
        .await
        .expect_err("unknown option key");
    let refusal = expect_problem(refusal, 409, "conflict");
    assert_eq!(
        items_field(&refusal),
        "This product does not take that option."
    );

    assert_eq!(order_count(&store, "ito_token_bad_option").await, 0);
    assert_eq!(stock(&store, &plugin).await, 50);

    let placed = intake
        .place(checkout_request(
            vec![with_options(
                &plugin,
                2,
                options(&[("scent", " Rose "), ("finish", " Matte ")]),
            )],
            "ito_token_good_options",
        ))
        .await
        .expect("valid options");
    assert_eq!(placed.items.len(), 1);
    assert_eq!(
        placed.items[0].options,
        options(&[("scent", " Rose "), ("finish", " Matte ")])
    );
    assert_eq!(placed.items[0].options_label, "Scent: Rose · Finish: Matte");
    assert_eq!(placed.items[0].plugin_id, "itoopt");
    assert_eq!(stock(&store, &plugin).await, 48);

    let placed = intake
        .place(checkout_request(
            vec![with_options(&plugin, 1, options(&[("scent", "Oud")]))],
            "ito_token_single_option",
        ))
        .await
        .expect("optional field may stay empty");
    assert_eq!(placed.items[0].options_label, "Scent: Oud");
}

#[tokio::test]
async fn missing_inactive_and_disabled_products_refuse() {
    let store = memory_store().await;
    let registry = fixture_registry();
    let intake = noop_intake(&store, &registry);
    let category = fixture_category(&store, "ito-unavailable").await;
    let inactive = fixture_product(
        &store,
        &category,
        "inactive",
        "core",
        100_000,
        5,
        false,
        HashMap::new(),
    )
    .await;
    let plugin = fixture_product(
        &store,
        &category,
        "disabled",
        "itoopt",
        100_000,
        5,
        true,
        attributes(),
    )
    .await;

    let missing = Line {
        product_id: "ito_prod_missing".to_string(),
        quantity: 1,
        ..Line::default()
    };
    let refusal = intake
        .place(checkout_request(vec![missing], "ito_token_missing_product"))
        .await
        .expect_err("missing product");
    let refusal = expect_problem(refusal, 409, "conflict");
    assert_eq!(refusal.message, "Some products are no longer available.");
    assert_eq!(
        items_field(&refusal),
        "A product in the bag is no longer available."
    );

    let refusal = intake
        .place(checkout_request(
            vec![line(&inactive, 1)],
            "ito_token_inactive_product",
        ))
        .await
        .expect_err("inactive product");
    let refusal = expect_problem(refusal, 409, "conflict");
    assert_eq!(
        items_field(&refusal),
        "A product in the bag is no longer available."
    );

    assert!(
        registry
            .write()
            .expect("registry")
            .set_enabled("itoopt", false)
    );
    let refusal = intake
        .place(checkout_request(
            vec![with_options(&plugin, 1, options(&[("scent", "Rose")]))],
            "ito_token_disabled_plugin",
        ))
        .await
        .expect_err("disabled plugin");
    let refusal = expect_problem(refusal, 409, "conflict");
    assert_eq!(
        items_field(&refusal),
        format!("{} is no longer available.", plugin.name)
    );
    for token in [
        "ito_token_missing_product",
        "ito_token_inactive_product",
        "ito_token_disabled_plugin",
    ] {
        assert_eq!(order_count(&store, token).await, 0, "{token}");
    }
    assert_eq!(stock(&store, &plugin).await, 5);
}

#[tokio::test]
async fn stock_shortage_and_price_change_write_nothing() {
    let store = memory_store().await;
    let registry = fixture_registry();
    let intake = noop_intake(&store, &registry);
    let category = fixture_category(&store, "ito-refusals").await;
    let product = fixture_product(
        &store,
        &category,
        "refusal",
        "core",
        123_400,
        2,
        true,
        HashMap::new(),
    )
    .await;

    let refusal = intake
        .place(checkout_request(
            vec![line(&product, 3)],
            "ito_token_short_stock",
        ))
        .await
        .expect_err("stock shortage");
    let refusal = expect_problem(refusal, 409, "conflict");
    assert_eq!(refusal.message, "Some products do not have enough stock.");
    assert_eq!(
        items_field(&refusal),
        format!("Reduce the quantity for {}.", product.name)
    );

    let wrong_price = Line {
        product_id: product.id.clone(),
        quantity: 1,
        expected_price_cents: Some(100_000),
        ..Line::default()
    };
    let refusal = intake
        .place(checkout_request(
            vec![wrong_price],
            "ito_token_price_change",
        ))
        .await
        .expect_err("price changed");
    let refusal = expect_problem(refusal, 409, "price_changed");
    assert_eq!(
        refusal.message,
        "A price changed while the bag was open. Review the bag and try again."
    );
    assert_eq!(
        items_field(&refusal),
        format!("{} now costs a different amount.", product.name)
    );

    assert_eq!(stock(&store, &product).await, 2);
    assert_eq!(order_count(&store, "ito_token_short_stock").await, 0);
    assert_eq!(order_count(&store, "ito_token_price_change").await, 0);
}

#[tokio::test]
async fn duplicate_lines_decrement_once_per_request_line() {
    let store = memory_store().await;
    let registry = fixture_registry();
    let intake = noop_intake(&store, &registry);
    let category = fixture_category(&store, "ito-duplicates").await;
    let product = fixture_product(
        &store,
        &category,
        "duplicate",
        "core",
        10_000,
        5,
        true,
        HashMap::new(),
    )
    .await;

    let placed = intake
        .place(checkout_request(
            vec![line(&product, 2), line(&product, 2)],
            "ito_token_duplicate_lines",
        ))
        .await
        .expect("two request lines");
    assert_eq!(placed.items.len(), 2);
    assert_eq!(placed.items[0].quantity, 2);
    assert_eq!(placed.items[1].quantity, 2);
    assert_eq!(placed.order.subtotal_cents, 40_000);
    assert_eq!(stock(&store, &product).await, 1);

    let merged_product = fixture_product(
        &store,
        &category,
        "merged",
        "core",
        10_000,
        5,
        true,
        HashMap::new(),
    )
    .await;
    let (merged_lines, fields) =
        build_lines(vec![line(&merged_product, 2), line(&merged_product, 2)]);
    assert!(fields.is_empty());
    assert_eq!(merged_lines.len(), 1);
    assert_eq!(merged_lines[0].quantity, 4);
    let merged = intake
        .place(checkout_request(merged_lines, "ito_token_merged_lines"))
        .await
        .expect("merged line");
    assert_eq!(merged.items.len(), 1);
    assert_eq!(merged.items[0].quantity, 4);
    assert_eq!(stock(&store, &merged_product).await, 1);

    let short_product = fixture_product(
        &store,
        &category,
        "short-duplicate",
        "core",
        10_000,
        3,
        true,
        HashMap::new(),
    )
    .await;
    let refusal = intake
        .place(checkout_request(
            vec![line(&short_product, 2), line(&short_product, 2)],
            "ito_token_duplicate_rollback",
        ))
        .await
        .expect_err("second decrement is short");
    let refusal = expect_problem(refusal, 409, "conflict");
    assert_eq!(refusal.message, "Some products do not have enough stock.");
    assert_eq!(items_field(&refusal), "Reduce the quantity in the bag.");
    assert_eq!(stock(&store, &short_product).await, 3);
    assert_eq!(order_count(&store, "ito_token_duplicate_rollback").await, 0);
}

#[tokio::test]
async fn counter_sale_is_delivered_without_shipping() {
    let store = memory_store().await;
    let registry = fixture_registry();
    let intake = noop_intake(&store, &registry);
    let category = fixture_category(&store, "ito-counter").await;
    let product = fixture_product(
        &store,
        &category,
        "counter",
        "core",
        123_400,
        5,
        true,
        HashMap::new(),
    )
    .await;

    let placed = intake
        .place(counter_request(
            vec![line(&product, 2)],
            "ito_token_counter",
        ))
        .await
        .expect("counter sale");
    assert_eq!(placed.order.status, "delivered");
    assert_eq!(placed.order.delivery_type, "pickup");
    assert_eq!(placed.order.subtotal_cents, 246_800);
    assert_eq!(placed.order.shipping_cents, 0);
    assert_eq!(placed.order.total_cents, 246_800);
    assert_eq!(stock(&store, &product).await, 3);
    assert_eq!(
        stored_order(&store, &placed.order.id).await.status,
        "delivered"
    );

    let detail = audit_detail(&store, &placed.order.id, "order.counter_sale").await;
    assert_eq!(detail["channel"], "counter");
    assert_eq!(detail["total_cents"], 246_800);
    assert_eq!(detail["items"], 1);
    let mut rows = store
        .conn()
        .query(
            "SELECT actor_user_id FROM audit_outbox WHERE entity_id = ? AND action = 'order.counter_sale'",
            libsql::params![placed.order.id.clone()],
        )
        .await
        .expect("counter audit");
    let row = rows
        .next()
        .await
        .expect("audit step")
        .expect("counter audit row");
    let actor: String = row.get(0).expect("actor");
    assert_eq!(actor, "usr_test");
}

#[tokio::test]
async fn change_status_allows_refuses_and_restocks() {
    let store = memory_store().await;
    let registry = fixture_registry();
    let intake = noop_intake(&store, &registry);
    let category = fixture_category(&store, "ito-status").await;
    let product = fixture_product(
        &store,
        &category,
        "status",
        "core",
        400_000,
        5,
        true,
        HashMap::new(),
    )
    .await;

    let first = intake
        .place(checkout_request(
            vec![line(&product, 2)],
            "ito_token_status_a",
        ))
        .await
        .expect("first order");
    let confirmed = intake
        .change_status(Change {
            order_id: first.order.id.clone(),
            to: "confirmed".to_string(),
            actor_user_id: "usr_employee".to_string(),
            actor_role: "employee".to_string(),
        })
        .await
        .expect("confirm order");
    assert_eq!(confirmed.order.status, "confirmed");
    assert_eq!(confirmed.items.len(), 1);
    assert_eq!(
        stored_order(&store, &first.order.id).await.status,
        "confirmed"
    );
    let detail = audit_detail(&store, &first.order.id, "order.status").await;
    assert_eq!(detail["from"], "pending");
    assert_eq!(detail["to"], "confirmed");

    let refusal = intake
        .change_status(Change {
            order_id: first.order.id.clone(),
            to: "shipped".to_string(),
            actor_user_id: "usr_employee".to_string(),
            actor_role: "employee".to_string(),
        })
        .await
        .expect_err("confirmed cannot ship directly");
    let refusal = expect_problem(refusal, 409, "invalid_transition");
    assert_eq!(refusal.message, "That status change is not allowed.");
    assert_eq!(
        refusal
            .fields
            .as_ref()
            .and_then(|fields| fields.get("status")),
        Some(&"This order cannot move from confirmed to shipped.".to_string())
    );

    let refusal = intake
        .change_status(Change {
            order_id: first.order.id.clone(),
            to: "paid".to_string(),
            actor_user_id: "usr_admin".to_string(),
            actor_role: "admin".to_string(),
        })
        .await
        .expect_err("unknown status");
    let refusal = expect_problem(refusal, 422, "validation_error");
    assert_eq!(
        refusal
            .fields
            .as_ref()
            .and_then(|fields| fields.get("status")),
        Some(&"Unknown order status.".to_string())
    );

    let refusal = intake
        .change_status(Change {
            order_id: "ito_order_missing".to_string(),
            to: "confirmed".to_string(),
            actor_user_id: "usr_admin".to_string(),
            actor_role: "admin".to_string(),
        })
        .await
        .expect_err("missing order");
    let refusal = expect_problem(refusal, 404, "not_found");
    assert_eq!(refusal.message, "Order not found.");

    let second = intake
        .place(checkout_request(
            vec![line(&product, 2)],
            "ito_token_status_b",
        ))
        .await
        .expect("second order");
    assert_eq!(stock(&store, &product).await, 1);

    let refusal = intake
        .change_status(Change {
            order_id: second.order.id.clone(),
            to: "cancelled".to_string(),
            actor_user_id: "usr_employee".to_string(),
            actor_role: "employee".to_string(),
        })
        .await
        .expect_err("employees cannot cancel");
    let refusal = expect_problem(refusal, 403, "forbidden");
    assert_eq!(
        refusal.message,
        "Only an administrator can cancel an order."
    );
    assert_eq!(
        stored_order(&store, &second.order.id).await.status,
        "pending"
    );
    assert_eq!(stock(&store, &product).await, 1);

    let cancelled = intake
        .change_status(Change {
            order_id: second.order.id.clone(),
            to: "cancelled".to_string(),
            actor_user_id: "usr_admin".to_string(),
            actor_role: "admin".to_string(),
        })
        .await
        .expect("admin cancels");
    assert_eq!(cancelled.order.status, "cancelled");
    assert_eq!(stock(&store, &product).await, 3);
    let detail = audit_detail(&store, &second.order.id, "order.status").await;
    assert_eq!(detail["from"], "pending");
    assert_eq!(detail["to"], "cancelled");
}

#[tokio::test]
async fn hook_failures_roll_back_the_whole_write() {
    let store = memory_store().await;
    let registry = fixture_registry();
    let category = fixture_category(&store, "ito-hooks").await;
    let product = fixture_product(
        &store,
        &category,
        "hook",
        "itoopt",
        100_000,
        5,
        true,
        attributes(),
    )
    .await;

    let checkout = Arc::new(FakeCheckout::failing_created());
    let hooks = Arc::new(FakeHooks::with_checkout("itoopt", Arc::clone(&checkout)));
    let intake = build_intake(&store, &registry, hooks);
    let error = intake
        .place(checkout_request(
            vec![with_options(&product, 1, options(&[("scent", "Rose")]))],
            "ito_token_hook_created",
        ))
        .await
        .expect_err("created hook fails");
    assert!(matches!(error, IntakeError::Hook(_)), "{error:?}");
    assert_eq!(checkout.calls().validated.len(), 1);
    assert_eq!(checkout.calls().created.len(), 1);
    assert_eq!(order_count(&store, "ito_token_hook_created").await, 0);
    assert_eq!(stock(&store, &product).await, 5);
    let order_id = checkout.calls().created[0].0.id.clone();
    assert_eq!(audit_count(&store, &order_id).await, 0);
    assert_eq!(order_item_count(&store, &order_id).await, 0);

    let refusal_problem = PluginProblem {
        status: 409,
        code: "conflict".to_string(),
        message: "The phone does not match.".to_string(),
        fields: Some(HashMap::from([(
            "items".to_string(),
            "Choose another phone.".to_string(),
        )])),
    };
    let checkout = Arc::new(FakeCheckout::refusing(refusal_problem));
    let hooks = Arc::new(FakeHooks::with_checkout("itoopt", Arc::clone(&checkout)));
    let intake = build_intake(&store, &registry, hooks);
    let error = intake
        .place(checkout_request(
            vec![with_options(&product, 1, options(&[("scent", "Rose")]))],
            "ito_token_hook_refused",
        ))
        .await
        .expect_err("validate hook refuses");
    let refusal = expect_problem(error, 409, "conflict");
    assert_eq!(refusal.message, "The phone does not match.");
    assert_eq!(items_field(&refusal), "Choose another phone.");
    assert_eq!(order_count(&store, "ito_token_hook_refused").await, 0);
    assert_eq!(stock(&store, &product).await, 5);

    let status = Arc::new(FakeStatus::failing());
    let hooks = Arc::new(FakeHooks::with_status(Arc::clone(&status)));
    let intake = build_intake(&store, &registry, hooks);
    let error = intake
        .place(counter_request(
            vec![with_options(&product, 1, options(&[("scent", "Rose")]))],
            "ito_token_hook_status",
        ))
        .await
        .expect_err("status hook fails during the counter sale");
    assert!(matches!(error, IntakeError::Hook(_)), "{error:?}");
    let calls = status.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].from, "pending");
    assert_eq!(calls[0].to, "delivered");
    assert_eq!(order_count(&store, "ito_token_hook_status").await, 0);
    assert_eq!(stock(&store, &product).await, 5);
    assert_eq!(audit_count(&store, &calls[0].order_id).await, 0);

    let placed = noop_intake(&store, &registry)
        .place(checkout_request(
            vec![with_options(&product, 2, options(&[("scent", "Oud")]))],
            "ito_token_hook_change",
        ))
        .await
        .expect("place before the failing status change");
    assert_eq!(stock(&store, &product).await, 3);

    let status = Arc::new(FakeStatus::failing());
    let hooks = Arc::new(FakeHooks::with_status(Arc::clone(&status)));
    let intake = build_intake(&store, &registry, hooks);
    let error = intake
        .change_status(Change {
            order_id: placed.order.id.clone(),
            to: "cancelled".to_string(),
            actor_user_id: "usr_admin".to_string(),
            actor_role: "admin".to_string(),
        })
        .await
        .expect_err("status hook fails during the cancellation");
    assert!(matches!(error, IntakeError::Hook(_)), "{error:?}");
    assert_eq!(
        stored_order(&store, &placed.order.id).await.status,
        "pending"
    );
    assert_eq!(stock(&store, &product).await, 3);
    assert_eq!(audit_count(&store, &placed.order.id).await, 1);
}

#[tokio::test]
async fn hooks_receive_lines_and_order_references() {
    let store = memory_store().await;
    let registry = fixture_registry();
    let category = fixture_category(&store, "ito-hook-calls").await;
    let product = fixture_product(
        &store,
        &category,
        "hook-calls",
        "itoopt",
        100_000,
        5,
        true,
        attributes(),
    )
    .await;

    let checkout = Arc::new(FakeCheckout::accepting());
    let status = Arc::new(FakeStatus::accepting());
    let hooks = Arc::new(FakeHooks::with_both(
        Arc::clone(&checkout),
        Arc::clone(&status),
    ));
    let intake = build_intake(&store, &registry, hooks);
    let placed = intake
        .place(checkout_request(
            vec![with_options(&product, 2, options(&[("scent", "Rose")]))],
            "ito_token_hook_calls",
        ))
        .await
        .expect("place with hooks");

    {
        let calls = checkout.calls();
        assert_eq!(calls.validated.len(), 1);
        assert_eq!(calls.validated[0].product_id, product.id);
        assert_eq!(calls.validated[0].quantity, 2);
        assert_eq!(calls.validated[0].order_id, placed.order.id);
        assert_eq!(calls.validated[0].item_id, placed.items[0].id);
        assert_eq!(calls.created.len(), 1);
        assert_eq!(calls.created[0].0.id, placed.order.id);
        assert_eq!(calls.created[0].0.number, placed.order.number);
        assert_eq!(calls.created[0].0.status, "pending");
        assert_eq!(calls.created[0].1, calls.validated);
    }
    assert!(status.calls().is_empty());

    intake
        .change_status(Change {
            order_id: placed.order.id.clone(),
            to: "confirmed".to_string(),
            actor_user_id: "usr_admin".to_string(),
            actor_role: "admin".to_string(),
        })
        .await
        .expect("confirm with hooks");
    assert_eq!(
        status.calls(),
        vec![StatusCall {
            order_id: placed.order.id.clone(),
            from: "pending".to_string(),
            to: "confirmed".to_string(),
        }]
    );

    let counter = intake
        .place(counter_request(
            vec![with_options(&product, 1, options(&[("scent", "Oud")]))],
            "ito_token_hook_counter",
        ))
        .await
        .expect("counter sale with hooks");
    assert_eq!(counter.order.status, "delivered");
    assert_eq!(status.calls().len(), 2);
    assert_eq!(status.calls()[1].from, "pending");
    assert_eq!(status.calls()[1].to, "delivered");
}

#[tokio::test]
async fn order_items_and_totals_use_effective_sale_prices() {
    let store = memory_store().await;
    let registry = fixture_registry();
    let intake = noop_intake(&store, &registry);
    let category = fixture_category(&store, "ito-prices").await;
    let now = Utc::now();
    let product = Product {
        id: "ito_prod_sale".to_string(),
        slug: "ito-product-sale".to_string(),
        name: "Fixture sale".to_string(),
        price_cents: 400_000,
        sale_price_cents: Some(300_000),
        sale_starts_at: Some(now - chrono::Duration::hours(1)),
        sale_ends_at: Some(now + chrono::Duration::hours(1)),
        currency: "DZD".to_string(),
        category: category.clone(),
        stock: 5,
        active: true,
        plugin_id: "core".to_string(),
        created_at: Some(now),
        updated_at: Some(now),
        ..Product::default()
    };
    store
        .create_product(store.conn(), &product)
        .await
        .expect("create sale product");

    let placed = intake
        .place(checkout_request(
            vec![Line {
                expected_price_cents: Some(300_000),
                ..line(&product, 2)
            }],
            "ito_token_sale_price",
        ))
        .await
        .expect("sale price is charged");
    assert_eq!(placed.items[0].unit_price_cents, 300_000);
    assert_eq!(placed.items[0].line_total_cents, 600_000);
    assert_eq!(placed.order.subtotal_cents, 600_000);
    assert_eq!(placed.order.shipping_cents, 60_000);
    assert_eq!(placed.order.total_cents, 660_000);
}
