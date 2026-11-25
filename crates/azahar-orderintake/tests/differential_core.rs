//! Differential test for the order intake core against
//! `backend/cmd/oracle-orderintake`.
//!
//! Both sides point at `AZAHAR_TEST_DATABASE_URL` (default
//! `http://127.0.0.1:8880`). The Go fixture registry installs a product type
//! but no checkout or status hooks; the Rust side uses [`NoopHooks`]. That is
//! the differential scope: the core path with an empty hook provider.
//!
//! Every fixture row and every order this test writes carries an `ito_*`
//! prefix and is deleted before the test returns.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, RwLock};

use azahar_db::{migrate, open};
use azahar_orderintake::{
    Change, Customer, Destination, Intake, IntakeError, Line, NoopHooks, Request, checkout_channel,
    counter_channel, option_label, option_schema, selected_option_fields,
};
use azahar_plugins::{Manifest, OptionField, Plugin, ProductType, Registry};
use azahar_store::{Category, Order, OrderItem, Product, Store, User, format_optional_time};
use chrono::Utc;
use libsql::Connection;
use serde_json::{Value, json};

struct Oracle {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Oracle {
    fn start(dsn: &str) -> Self {
        let backend = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../backend");
        let mut child = Command::new("go")
            .args(["run", "-tags", "oracle", "./cmd/oracle-orderintake"])
            .current_dir(&backend)
            .env("AZAHAR_TEST_DATABASE_URL", dsn)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap_or_else(|error| panic!("start Go oracle in {}: {error}", backend.display()));
        let stdin = child.stdin.take().expect("oracle stdin");
        let stdout = child.stdout.take().expect("oracle stdout");
        Self {
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
        }
    }

    fn call(&mut self, request: &Value) -> Value {
        {
            let stdin = self.stdin.as_mut().expect("oracle stdin open");
            serde_json::to_writer(&mut *stdin, request).expect("encode oracle request");
            stdin.write_all(b"\n").expect("write oracle request");
            stdin.flush().expect("flush oracle request");
        }
        let mut line = String::new();
        let read = self
            .stdout
            .read_line(&mut line)
            .expect("read oracle response");
        assert!(
            read > 0,
            "Go oracle exited before answering; is AZAHAR_TEST_DATABASE_URL reachable? run `make dev-db`"
        );
        serde_json::from_str(&line).expect("decode oracle response")
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Go,
    Rust,
}

impl Side {
    fn suffix(self) -> &'static str {
        match self {
            Self::Go => "go",
            Self::Rust => "rs",
        }
    }
}

#[derive(Debug, Clone)]
struct LineSpec {
    label: &'static str,
    product_id_override: Option<String>,
    quantity: i64,
    options: HashMap<String, String>,
    expected_price_cents: Option<i64>,
}

#[derive(Debug, Clone)]
struct RequestSpec {
    lines: Vec<LineSpec>,
    counter: bool,
    status: String,
    actor_user_id: String,
    user_id: String,
}

impl RequestSpec {
    fn checkout(lines: Vec<LineSpec>) -> Self {
        Self {
            lines,
            counter: false,
            status: "pending".to_string(),
            actor_user_id: "usr_buyer".to_string(),
            user_id: String::new(),
        }
    }

    fn counter(lines: Vec<LineSpec>) -> Self {
        Self {
            lines,
            counter: true,
            status: "pending".to_string(),
            actor_user_id: "usr_counter".to_string(),
            user_id: String::new(),
        }
    }
}

fn line(label: &'static str, quantity: i64) -> LineSpec {
    LineSpec {
        label,
        product_id_override: None,
        quantity,
        options: HashMap::new(),
        expected_price_cents: None,
    }
}

fn line_with_options(label: &'static str, quantity: i64, pairs: &[(&str, &str)]) -> LineSpec {
    LineSpec {
        options: pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect(),
        ..line(label, quantity)
    }
}

fn line_with_override(product_id: &str, quantity: i64) -> LineSpec {
    LineSpec {
        product_id_override: Some(product_id.to_string()),
        ..line("missing", quantity)
    }
}

struct World {
    prefix: String,
    go: HashMap<&'static str, Product>,
    rust: HashMap<&'static str, Product>,
    go_names: HashMap<String, String>,
    rust_names: HashMap<String, String>,
}

impl World {
    fn products(&self, side: Side) -> &HashMap<&'static str, Product> {
        match side {
            Side::Go => &self.go,
            Side::Rust => &self.rust,
        }
    }

    fn names(&self, side: Side) -> &HashMap<String, String> {
        match side {
            Side::Go => &self.go_names,
            Side::Rust => &self.rust_names,
        }
    }

    fn token(&self, side: Side, label: &str) -> String {
        format!("{}_token_{label}_{}", self.prefix, side.suffix())
    }

    fn product_id(&self, side: Side, spec: &LineSpec) -> String {
        match &spec.product_id_override {
            Some(id) => id.clone(),
            None => self.products(side)[spec.label].id.clone(),
        }
    }

    fn request(&self, side: Side, spec: &RequestSpec, token: String) -> Request {
        let products = self.products(side);
        let lines = spec
            .lines
            .iter()
            .map(|line| Line {
                product_id: self.product_id(side, line),
                quantity: line.quantity,
                options: line.options.clone(),
                expected_price_cents: line.expected_price_cents,
            })
            .collect();
        debug_assert!(spec
            .lines
            .iter()
            .all(|line| line.product_id_override.is_some()
                || products.contains_key(line.label)));
        if spec.counter {
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
                status: spec.status.clone(),
                token_hash: token,
                actor_user_id: spec.actor_user_id.clone(),
            }
        } else {
            Request {
                lines,
                channel: checkout_channel("2026-11-14"),
                customer: Customer {
                    user_id: spec.user_id.clone(),
                    name: "Test Buyer".to_string(),
                    email: "buyer@example.test".to_string(),
                    phone: "0550123456".to_string(),
                    address: "1 Rue des Tests".to_string(),
                    notes: "Differential fixture.".to_string(),
                },
                destination: Destination {
                    wilaya_code: 16,
                    wilaya_name: "Alger".to_string(),
                    commune_name: "Alger Centre".to_string(),
                    delivery_type: "home".to_string(),
                    stop_desk_id: "desk-ito".to_string(),
                    stop_desk_name: "Agence ITO".to_string(),
                },
                status: spec.status.clone(),
                token_hash: token,
                actor_user_id: spec.actor_user_id.clone(),
            }
        }
    }

    fn payload(&self, side: Side, spec: &RequestSpec, token: String) -> Value {
        let lines: Vec<Value> = spec
            .lines
            .iter()
            .map(|line| {
                json!({
                    "product_id": self.product_id(side, line),
                    "quantity": line.quantity,
                    "options": line.options,
                    "expected_price_cents": line.expected_price_cents,
                })
            })
            .collect();
        let (customer, destination) = if spec.counter {
            (
                json!({"name": "Counter sale"}),
                json!({"delivery_type": "pickup"}),
            )
        } else {
            (
                json!({
                    "user_id": spec.user_id,
                    "name": "Test Buyer",
                    "email": "buyer@example.test",
                    "phone": "0550123456",
                    "address": "1 Rue des Tests",
                    "notes": "Differential fixture.",
                }),
                json!({
                    "wilaya_code": 16,
                    "wilaya_name": "Alger",
                    "commune_name": "Alger Centre",
                    "delivery_type": "home",
                    "stop_desk_id": "desk-ito",
                    "stop_desk_name": "Agence ITO",
                }),
            )
        };
        json!({
            "op": "place",
            "channel": if spec.counter { "counter" } else { "checkout" },
            "terms_version": "2026-11-14",
            "lines": lines,
            "customer": customer,
            "destination": destination,
            "status": spec.status,
            "token_hash": token,
            "actor_user_id": spec.actor_user_id,
        })
    }
}

fn order_json(order: &Order) -> Value {
    json!({
        "id": order.id,
        "number": order.number,
        "user_id": order.user_id,
        "name": order.name,
        "email": order.email,
        "phone": order.phone,
        "address": order.address,
        "city": order.city,
        "notes": order.notes,
        "subtotal_cents": order.subtotal_cents,
        "shipping_cents": order.shipping_cents,
        "total_cents": order.total_cents,
        "currency": order.currency,
        "status": order.status,
        "token_hash": order.token_hash,
        "created_at": format_optional_time(order.created_at.as_ref()),
        "updated_at": format_optional_time(order.updated_at.as_ref()),
        "wilaya_code": order.wilaya_code,
        "wilaya_name": order.wilaya_name,
        "commune_name": order.commune_name,
        "delivery_type": order.delivery_type,
        "stop_desk_id": order.stop_desk_id,
        "stop_desk_name": order.stop_desk_name,
    })
}

fn item_json(item: &OrderItem) -> Value {
    json!({
        "id": item.id,
        "order_id": item.order_id,
        "product_id": item.product_id,
        "slug": item.slug,
        "name": item.name,
        "image": item.image,
        "unit_price_cents": item.unit_price_cents,
        "quantity": item.quantity,
        "line_total_cents": item.line_total_cents,
        "options": item.options,
        "options_label": item.options_label,
        "plugin_id": item.plugin_id,
    })
}

fn place_response(result: &Result<azahar_orderintake::PlaceResult, IntakeError>) -> Value {
    match result {
        Ok(placed) => json!({
            "ok": true,
            "order": order_json(&placed.order),
            "items": placed.items.iter().map(item_json).collect::<Vec<_>>(),
        }),
        Err(IntakeError::Problem(problem)) => json!({
            "ok": false,
            "problem": {
                "status": problem.status,
                "code": problem.code,
                "message": problem.message,
                "fields": problem.fields,
            },
        }),
        Err(error) => json!({"ok": false, "error": error.to_string()}),
    }
}

fn change_response(result: &Result<azahar_orderintake::Changed, IntakeError>) -> Value {
    match result {
        Ok(changed) => json!({
            "ok": true,
            "order": order_json(&changed.order),
            "items": changed.items.iter().map(item_json).collect::<Vec<_>>(),
        }),
        Err(IntakeError::Problem(problem)) => json!({
            "ok": false,
            "problem": {
                "status": problem.status,
                "code": problem.code,
                "message": problem.message,
                "fields": problem.fields,
            },
        }),
        Err(error) => json!({"ok": false, "error": error.to_string()}),
    }
}

fn set(object: &mut Value, key: &str, value: impl Into<Value>) {
    if let Some(map) = object.as_object_mut() {
        map.insert(key.to_string(), value.into());
    }
}

fn named(names: &HashMap<String, String>, value: &Value) -> Option<Value> {
    let text = value.as_str()?;
    names.get(text).map(|name| Value::String(name.clone()))
}

fn canonical_order(order: &mut Value) {
    set(order, "id", "<order>");
    set(order, "number", "<number>");
    set(order, "token_hash", "<token>");
    set(order, "created_at", "<timestamp>");
    set(order, "updated_at", "<timestamp>");
}

fn canonical_item(item: &mut Value, names: &HashMap<String, String>, index: usize) {
    set(item, "id", format!("<item:{index}>"));
    set(item, "order_id", "<order>");
    for key in ["product_id", "slug"] {
        if let Some(mapped) = item.get(key).and_then(|value| named(names, value)) {
            set(item, key, mapped);
        }
    }
}

fn canonical_response(value: &Value, names: &HashMap<String, String>) -> Value {
    let mut out = value.clone();
    if out.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        if let Some(order) = out.get_mut("order") {
            canonical_order(order);
        }
        if let Some(items) = out.get_mut("items").and_then(Value::as_array_mut) {
            for (index, item) in items.iter_mut().enumerate() {
                canonical_item(item, names, index);
            }
        }
    }
    out
}

async fn load_order_row(conn: &Connection, id: &str) -> Value {
    let mut rows = conn
        .query(
            "SELECT id, number, user_id, name, email, phone, address, city, notes,
                    subtotal_cents, shipping_cents, total_cents, currency, status,
                    token_hash, created_at, updated_at, wilaya_code, wilaya_name,
                    commune_name, delivery_type, stop_desk_id, stop_desk_name,
                    courier_key, tracking_number, courier_status, courier_events,
                    courier_synced_at
             FROM orders WHERE id = ?",
            libsql::params![id],
        )
        .await
        .expect("load order row");
    let row = rows
        .next()
        .await
        .expect("order row step")
        .expect("order row exists");
    let user_id: Option<String> = row.get(2).expect("user id");
    let wilaya_code: Option<i64> = row.get(17).expect("wilaya code");
    json!({
        "id": row.get::<String>(0).expect("id"),
        "number": row.get::<String>(1).expect("number"),
        "user_id": user_id,
        "name": row.get::<String>(3).expect("name"),
        "email": row.get::<String>(4).expect("email"),
        "phone": row.get::<String>(5).expect("phone"),
        "address": row.get::<String>(6).expect("address"),
        "city": row.get::<String>(7).expect("city"),
        "notes": row.get::<String>(8).expect("notes"),
        "subtotal_cents": row.get::<i64>(9).expect("subtotal"),
        "shipping_cents": row.get::<i64>(10).expect("shipping"),
        "total_cents": row.get::<i64>(11).expect("total"),
        "currency": row.get::<String>(12).expect("currency"),
        "status": row.get::<String>(13).expect("status"),
        "token_hash": row.get::<String>(14).expect("token"),
        "created_at": row.get::<String>(15).expect("created"),
        "updated_at": row.get::<String>(16).expect("updated"),
        "wilaya_code": wilaya_code,
        "wilaya_name": row.get::<String>(18).expect("wilaya name"),
        "commune_name": row.get::<String>(19).expect("commune"),
        "delivery_type": row.get::<String>(20).expect("delivery"),
        "stop_desk_id": row.get::<String>(21).expect("desk id"),
        "stop_desk_name": row.get::<String>(22).expect("desk name"),
        "courier_key": row.get::<String>(23).expect("courier key"),
        "tracking_number": row.get::<String>(24).expect("tracking"),
        "courier_status": row.get::<String>(25).expect("courier status"),
        "courier_events": row.get::<String>(26).expect("courier events"),
        "courier_synced_at": row.get::<String>(27).expect("courier sync"),
    })
}

async fn load_item_rows(conn: &Connection, order_id: &str) -> Vec<Value> {
    let mut rows = conn
        .query(
            "SELECT id, order_id, product_id, slug, name, image, unit_price_cents,
                    quantity, line_total_cents, options, options_label, plugin_id
             FROM order_items WHERE order_id = ? ORDER BY rowid",
            libsql::params![order_id],
        )
        .await
        .expect("load item rows");
    let mut items = Vec::new();
    while let Some(row) = rows.next().await.expect("item row") {
        let options: String = row.get(9).expect("options");
        items.push(json!({
            "id": row.get::<String>(0).expect("id"),
            "order_id": row.get::<String>(1).expect("order id"),
            "product_id": row.get::<String>(2).expect("product id"),
            "slug": row.get::<String>(3).expect("slug"),
            "name": row.get::<String>(4).expect("name"),
            "image": row.get::<String>(5).expect("image"),
            "unit_price_cents": row.get::<i64>(6).expect("unit price"),
            "quantity": row.get::<i64>(7).expect("quantity"),
            "line_total_cents": row.get::<i64>(8).expect("line total"),
            "options": serde_json::from_str::<Value>(&options).expect("options json"),
            "options_label": row.get::<String>(10).expect("options label"),
            "plugin_id": row.get::<String>(11).expect("plugin id"),
        }));
    }
    items
}

async fn load_audit_rows(conn: &Connection, order_id: &str) -> Vec<Value> {
    // A running API flushes audit_outbox into audit_log. Read both in one
    // statement (one snapshot) so the comparison sees a row wherever it lives.
    let mut rows = conn
        .query(
            "SELECT id, actor_user_id, action, entity_type, entity_id, detail, created_at
             FROM (
                 SELECT id, actor_user_id, action, entity_type, entity_id, detail, created_at
                 FROM audit_outbox WHERE entity_type = 'order' AND entity_id = ?
                 UNION
                 SELECT id, actor_user_id, action, entity_type, entity_id, detail, created_at
                 FROM audit_log WHERE entity_type = 'order' AND entity_id = ?
             )
             ORDER BY created_at, action, detail",
            libsql::params![order_id, order_id],
        )
        .await
        .expect("load audit rows");
    let mut audits = Vec::new();
    while let Some(row) = rows.next().await.expect("audit row") {
        let detail: String = row.get(5).expect("detail");
        let mut detail: Value = serde_json::from_str(&detail).expect("audit detail json");
        if let Some(object) = detail.as_object_mut()
            && object.contains_key("accepted_at")
        {
            object.insert("accepted_at".to_string(), json!("<timestamp>"));
        }
        audits.push(json!({
            "id": "<audit>",
            "actor_user_id": row.get::<Option<String>>(1).expect("actor"),
            "action": row.get::<String>(2).expect("action"),
            "entity_type": row.get::<String>(3).expect("entity type"),
            "entity_id": "<order>",
            "detail": detail,
            "created_at": "<timestamp>",
        }));
    }
    audits
}

fn canonical_row(mut row: Value) -> Value {
    canonical_order(&mut row);
    row
}

fn canonical_item_row(mut row: Value, names: &HashMap<String, String>, index: usize) -> Value {
    canonical_item(&mut row, names, index);
    row
}

async fn compare_rows(
    store: &Store,
    go_order: &str,
    rust_order: &str,
    go_names: &HashMap<String, String>,
    rust_names: &HashMap<String, String>,
) {
    let go = canonical_row(load_order_row(store.conn(), go_order).await);
    let rust = canonical_row(load_order_row(store.conn(), rust_order).await);
    assert_eq!(go, rust, "order row differs");

    let go_items: Vec<Value> = load_item_rows(store.conn(), go_order)
        .await
        .iter()
        .enumerate()
        .map(|(index, row)| canonical_item_row(row.clone(), go_names, index))
        .collect();
    let rust_items: Vec<Value> = load_item_rows(store.conn(), rust_order)
        .await
        .iter()
        .enumerate()
        .map(|(index, row)| canonical_item_row(row.clone(), rust_names, index))
        .collect();
    assert_eq!(go_items, rust_items, "order item rows differ");

    // Timestamps are second precision, so the relative order of two rows in
    // the same second is not comparable across two runs. Compare the audit
    // rows as a multiset of their canonical content.
    let mut go_audits = load_audit_rows(store.conn(), go_order).await;
    let mut rust_audits = load_audit_rows(store.conn(), rust_order).await;
    go_audits.sort_by_key(|row| row.to_string());
    rust_audits.sort_by_key(|row| row.to_string());
    assert_eq!(go_audits, rust_audits, "audit rows differ");
}

struct StatusSpec {
    to: &'static str,
    actor_user_id: &'static str,
    actor_role: &'static str,
}

struct Differential<'a> {
    store: &'a Store,
    oracle: &'a mut Oracle,
    intake: &'a Intake,
    registry: &'a Arc<RwLock<Registry>>,
    world: &'a World,
    orders: Vec<(String, String)>,
}

impl Differential<'_> {
    async fn place(&mut self, spec: &RequestSpec, label: &str) -> Option<(String, String)> {
        let go_token = self.world.token(Side::Go, label);
        let rust_token = self.world.token(Side::Rust, label);
        let go_response = self
            .oracle
            .call(&self.world.payload(Side::Go, spec, go_token));
        let rust_response = place_response(
            &self
                .intake
                .place(self.world.request(Side::Rust, spec, rust_token))
                .await,
        );
        assert!(
            go_response.get("error").is_none(),
            "Go oracle place {label} failed: {go_response}"
        );
        assert!(
            rust_response.get("error").is_none(),
            "Rust place {label} failed: {rust_response}"
        );
        assert_eq!(
            canonical_response(&go_response, self.world.names(Side::Go)),
            canonical_response(&rust_response, self.world.names(Side::Rust)),
            "place {label} responses differ"
        );

        let ids = if go_response["ok"].as_bool().unwrap_or(false) {
            let go_id = go_response["order"]["id"]
                .as_str()
                .expect("Go order id")
                .to_string();
            let rust_id = rust_response["order"]["id"]
                .as_str()
                .expect("Rust order id")
                .to_string();
            compare_rows(
                self.store,
                &go_id,
                &rust_id,
                self.world.names(Side::Go),
                self.world.names(Side::Rust),
            )
            .await;
            self.orders.push((go_id.clone(), rust_id.clone()));
            Some((go_id, rust_id))
        } else {
            None
        };
        self.compare_stocks(label).await;
        ids
    }

    async fn status(&mut self, orders: &(String, String), spec: &StatusSpec, label: &str) {
        let go_response = self.oracle.call(&json!({
            "op": "status",
            "order_id": orders.0,
            "to": spec.to,
            "actor_user_id": spec.actor_user_id,
            "actor_role": spec.actor_role,
        }));
        let rust_response = change_response(
            &self
                .intake
                .change_status(Change {
                    order_id: orders.1.clone(),
                    to: spec.to.to_string(),
                    actor_user_id: spec.actor_user_id.to_string(),
                    actor_role: spec.actor_role.to_string(),
                })
                .await,
        );
        assert!(
            go_response.get("error").is_none(),
            "Go oracle status {label} failed: {go_response}"
        );
        assert!(
            rust_response.get("error").is_none(),
            "Rust status {label} failed: {rust_response}"
        );
        assert_eq!(
            canonical_response(&go_response, self.world.names(Side::Go)),
            canonical_response(&rust_response, self.world.names(Side::Rust)),
            "status {label} responses differ"
        );
        let go_exists = self.store.order_by_id(&orders.0).await.is_ok();
        let rust_exists = self.store.order_by_id(&orders.1).await.is_ok();
        assert_eq!(
            go_exists, rust_exists,
            "status {label} order existence differs"
        );
        if go_exists {
            compare_rows(
                self.store,
                &orders.0,
                &orders.1,
                self.world.names(Side::Go),
                self.world.names(Side::Rust),
            )
            .await;
        }
        self.compare_stocks(label).await;
    }

    async fn set_plugin_enabled(&mut self, enabled: bool) -> bool {
        let response = self.oracle.call(&json!({
            "op": "plugin_state",
            "plugin_id": "itoopt",
            "plugin_enabled": enabled,
        }));
        assert_eq!(
            response["ok"],
            json!(true),
            "oracle plugin state: {response}"
        );
        let changed = self
            .intake_registry()
            .write()
            .expect("registry")
            .set_enabled("itoopt", enabled);
        assert_eq!(changed, response["changed"], "plugin state differs");
        changed
    }

    fn intake_registry(&self) -> &Arc<RwLock<Registry>> {
        self.registry
    }

    async fn compare_stocks(&self, label: &str) {
        let mut labels: HashSet<&'static str> = HashSet::new();
        for product in self.world.go.keys() {
            labels.insert(product);
        }
        for product in labels {
            let go_stock = self
                .store
                .product_by_id(&self.world.go[product].id, false)
                .await
                .unwrap_or_else(|error| panic!("reload Go product {product}: {error}"))
                .stock;
            let rust_stock = self
                .store
                .product_by_id(&self.world.rust[product].id, false)
                .await
                .unwrap_or_else(|error| panic!("reload Rust product {product}: {error}"))
                .stock;
            assert_eq!(
                go_stock, rust_stock,
                "stock differs for {product} after {label}"
            );
        }
    }
}

async fn compare_options(
    oracle: &mut Oracle,
    registry: &Arc<RwLock<Registry>>,
    plugin_id: &str,
    attributes: &HashMap<String, Vec<String>>,
    options: &HashMap<String, String>,
    label: &str,
) {
    let expected = {
        let registry = registry.read().expect("registry");
        let product = Product {
            plugin_id: plugin_id.to_string(),
            ..Product::default()
        };
        let schema = option_schema(&registry, &product);
        json!({
            "ok": true,
            "schema": schema,
            "fields": selected_option_fields(&schema, attributes, options),
            "label": option_label(&schema, options),
        })
    };
    let response = oracle.call(&json!({
        "op": "options",
        "product_plugin_id": plugin_id,
        "attributes": attributes,
        "options": options,
    }));
    assert!(
        response.get("error").is_none(),
        "Go oracle options {label} failed: {response}"
    );
    assert_eq!(response, expected, "options {label} differ");
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
    let manifest = Manifest {
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
    };
    let mut registry = Registry::new();
    registry
        .register(Arc::new(FixturePlugin { manifest }))
        .expect("register fixture plugin");
    Arc::new(RwLock::new(registry))
}

async fn fixture_category(store: &Store, slug: &str) -> Category {
    let category = Category {
        slug: slug.to_string(),
        name: "Intake differential".to_string(),
        ..Category::default()
    };
    store
        .upsert_category("core", &category)
        .await
        .expect("create category");
    store.category_by_slug(slug).await.expect("load category")
}

#[allow(clippy::too_many_arguments)]
async fn fixture_product(
    store: &Store,
    category: &Category,
    prefix: &str,
    side: Side,
    label: &str,
    plugin_id: &str,
    price_cents: i64,
    stock: i32,
    active: bool,
    attributes: HashMap<String, Vec<String>>,
) -> Product {
    let now = Utc::now();
    let product = Product {
        id: format!("{prefix}_prod_{label}_{}", side.suffix()),
        slug: format!("{prefix}-{label}-{}", side.suffix()),
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
        .unwrap_or_else(|error| panic!("create product {label}: {error}"));
    product
}

fn normalisation_map(products: &HashMap<&'static str, Product>) -> HashMap<String, String> {
    let mut names = HashMap::new();
    for (label, product) in products {
        names.insert(product.id.clone(), format!("<product:{label}>"));
        names.insert(product.slug.clone(), format!("<slug:{label}>"));
    }
    names
}

async fn cleanup(
    store: &Store,
    world: &World,
    orders: &[(String, String)],
    category_slug: &str,
    user_id: &str,
) {
    for (go_id, rust_id) in orders {
        for id in [go_id, rust_id] {
            for table in ["audit_outbox", "audit_log"] {
                store
                    .conn()
                    .execute(
                        &format!(
                            "DELETE FROM {table} WHERE entity_type = 'order' AND entity_id = ?"
                        ),
                        libsql::params![id.clone()],
                    )
                    .await
                    .expect("cleanup audit");
            }
            store
                .conn()
                .execute(
                    "DELETE FROM order_items WHERE order_id = ?",
                    libsql::params![id.clone()],
                )
                .await
                .expect("cleanup items");
            store
                .conn()
                .execute(
                    "DELETE FROM orders WHERE id = ?",
                    libsql::params![id.clone()],
                )
                .await
                .expect("cleanup order");
        }
    }
    for product in world.go.values().chain(world.rust.values()) {
        store
            .conn()
            .execute(
                "DELETE FROM products WHERE id = ?",
                libsql::params![product.id.clone()],
            )
            .await
            .expect("cleanup product");
    }
    store
        .conn()
        .execute(
            "DELETE FROM categories WHERE slug = ?",
            libsql::params![category_slug],
        )
        .await
        .expect("cleanup category");
    store
        .conn()
        .execute("DELETE FROM users WHERE id = ?", libsql::params![user_id])
        .await
        .expect("cleanup user");
}

#[tokio::test]
async fn differential_intake_core_matches_the_go_oracle() {
    let dsn = std::env::var("AZAHAR_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8880".to_string());
    let database = open(&dsn).await.unwrap_or_else(|error| {
        panic!(
            "order intake differential needs libSQL at {dsn}: {error}\n\
             run `make dev-db` or set AZAHAR_TEST_DATABASE_URL"
        )
    });
    let store = Arc::new(Store::new(database).expect("store"));
    migrate(store.conn()).await.expect("migrate");

    let mut oracle = Oracle::start(&dsn);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let prefix = format!("ito_{nanos}");
    let category_slug = format!("{prefix}_cat");
    let category = fixture_category(&store, &category_slug).await;
    let user_id = format!("{prefix}_user");
    store
        .create_user(
            store.conn(),
            &User {
                id: user_id.clone(),
                name: "Differential Buyer".to_string(),
                email: format!("{prefix}@example.test"),
                password_hash: "hash".to_string(),
                role: "customer".to_string(),
                ..User::default()
            },
        )
        .await
        .expect("create fixture user");

    let attributes = HashMap::from([
        (
            "scent".to_string(),
            vec!["Rose".to_string(), "Oud".to_string()],
        ),
        (
            "finish".to_string(),
            vec!["Matte".to_string(), "Gloss".to_string()],
        ),
    ]);
    let mut go = HashMap::new();
    let mut rust = HashMap::new();
    for (label, plugin_id, price, stock, active, with_attributes) in [
        ("core", "core", 400_000_i64, 30_i32, true, false),
        ("plugin", "itoopt", 123_400, 30, true, true),
        ("ghost", "ghost", 50_000, 30, true, false),
        ("short", "core", 100_000, 2, true, false),
        ("dup", "core", 10_000, 5, true, false),
        ("dup_short", "core", 10_000, 3, true, false),
        ("inactive", "core", 100_000, 5, false, false),
    ] {
        let attributes = if with_attributes {
            attributes.clone()
        } else {
            HashMap::new()
        };
        go.insert(
            label,
            fixture_product(
                &store,
                &category,
                &prefix,
                Side::Go,
                label,
                plugin_id,
                price,
                stock,
                active,
                attributes.clone(),
            )
            .await,
        );
        rust.insert(
            label,
            fixture_product(
                &store,
                &category,
                &prefix,
                Side::Rust,
                label,
                plugin_id,
                price,
                stock,
                active,
                attributes,
            )
            .await,
        );
    }
    let world = World {
        prefix: prefix.clone(),
        go_names: normalisation_map(&go),
        rust_names: normalisation_map(&rust),
        go,
        rust,
    };
    let registry = fixture_registry();
    compare_options(
        &mut oracle,
        &registry,
        "core",
        &HashMap::new(),
        &HashMap::new(),
        "core-empty",
    )
    .await;
    compare_options(
        &mut oracle,
        &registry,
        "core",
        &HashMap::new(),
        &HashMap::from([("scent".to_string(), "Rose".to_string())]),
        "core-options",
    )
    .await;
    compare_options(
        &mut oracle,
        &registry,
        "itoopt",
        &HashMap::new(),
        &HashMap::new(),
        "plugin-empty",
    )
    .await;
    compare_options(
        &mut oracle,
        &registry,
        "itoopt",
        &attributes,
        &HashMap::new(),
        "plugin-missing-required",
    )
    .await;
    compare_options(
        &mut oracle,
        &registry,
        "itoopt",
        &attributes,
        &HashMap::from([("scent".to_string(), " Rose ".to_string())]),
        "plugin-trimmed-select",
    )
    .await;
    compare_options(
        &mut oracle,
        &registry,
        "itoopt",
        &attributes,
        &HashMap::from([
            ("scent".to_string(), "Oud".to_string()),
            ("finish".to_string(), "Gloss".to_string()),
        ]),
        "plugin-both-selects",
    )
    .await;
    compare_options(
        &mut oracle,
        &registry,
        "itoopt",
        &attributes,
        &HashMap::from([("finish".to_string(), "Matte".to_string())]),
        "plugin-optional-only",
    )
    .await;
    compare_options(
        &mut oracle,
        &registry,
        "itoopt",
        &attributes,
        &HashMap::from([("scent".to_string(), "Lavender".to_string())]),
        "plugin-unavailable",
    )
    .await;
    compare_options(
        &mut oracle,
        &registry,
        "itoopt",
        &attributes,
        &HashMap::from([
            ("scent".to_string(), "Rose".to_string()),
            ("size".to_string(), "10 ml".to_string()),
        ]),
        "plugin-unknown-key",
    )
    .await;
    compare_options(
        &mut oracle,
        &registry,
        "itoopt",
        &attributes,
        &HashMap::from([("engraving".to_string(), "A name".to_string())]),
        "plugin-text-field",
    )
    .await;
    compare_options(
        &mut oracle,
        &registry,
        "itoopt",
        &attributes,
        &HashMap::from([
            ("scent".to_string(), String::new()),
            ("finish".to_string(), "Matte".to_string()),
        ]),
        "plugin-blank-required",
    )
    .await;
    compare_options(
        &mut oracle,
        &registry,
        "itoopt",
        &HashMap::new(),
        &HashMap::from([("scent".to_string(), "Rose".to_string())]),
        "plugin-no-attributes",
    )
    .await;
    compare_options(
        &mut oracle,
        &registry,
        "ghost",
        &attributes,
        &HashMap::from([("scent".to_string(), "Rose".to_string())]),
        "unknown-plugin",
    )
    .await;
    compare_options(
        &mut oracle,
        &registry,
        "",
        &attributes,
        &HashMap::new(),
        "empty-plugin",
    )
    .await;

    let intake = Intake::new(
        Arc::clone(&store),
        Arc::clone(&registry),
        Arc::new(NoopHooks),
    );

    let mut diff = Differential {
        store: &store,
        oracle: &mut oracle,
        intake: &intake,
        registry: &registry,
        world: &world,
        orders: Vec::new(),
    };

    let mut user_spec = RequestSpec::checkout(vec![line("core", 1)]);
    user_spec.user_id = user_id.clone();
    let with_user = diff
        .place(&user_spec, "user")
        .await
        .expect("checkout with a user id");
    diff.status(
        &with_user,
        &StatusSpec {
            to: "confirmed",
            actor_user_id: "usr_admin",
            actor_role: "admin",
        },
        "user-confirm",
    )
    .await;

    let checkout = diff
        .place(
            &RequestSpec::checkout(vec![line("core", 2)]),
            "checkout-threshold",
        )
        .await
        .expect("checkout at the shipping threshold");
    diff.status(
        &checkout,
        &StatusSpec {
            to: "confirmed",
            actor_user_id: "usr_employee",
            actor_role: "employee",
        },
        "confirm",
    )
    .await;
    diff.status(
        &checkout,
        &StatusSpec {
            to: "packed",
            actor_user_id: "usr_employee",
            actor_role: "employee",
        },
        "pack",
    )
    .await;
    diff.status(
        &checkout,
        &StatusSpec {
            to: "shipped",
            actor_user_id: "usr_employee",
            actor_role: "employee",
        },
        "ship",
    )
    .await;
    diff.status(
        &checkout,
        &StatusSpec {
            to: "delivered",
            actor_user_id: "usr_employee",
            actor_role: "employee",
        },
        "deliver",
    )
    .await;
    diff.status(
        &checkout,
        &StatusSpec {
            to: "cancelled",
            actor_user_id: "usr_admin",
            actor_role: "admin",
        },
        "delivered-cancel",
    )
    .await;

    diff.place(
        &RequestSpec::checkout(vec![line("core", 3)]),
        "checkout-free-shipping",
    )
    .await
    .expect("checkout over the free-shipping line");

    let plugin_order = diff
        .place(
            &RequestSpec::checkout(vec![line_with_options(
                "plugin",
                1,
                &[("scent", "Rose"), ("finish", "Matte")],
            )]),
            "plugin-options",
        )
        .await
        .expect("plugin product with options");
    diff.status(
        &plugin_order,
        &StatusSpec {
            to: "cancelled",
            actor_user_id: "usr_employee",
            actor_role: "employee",
        },
        "employee-cancel",
    )
    .await;
    diff.status(
        &plugin_order,
        &StatusSpec {
            to: "cancelled",
            actor_user_id: "usr_admin",
            actor_role: "admin",
        },
        "admin-cancel",
    )
    .await;
    diff.status(
        &plugin_order,
        &StatusSpec {
            to: "cancelled",
            actor_user_id: "usr_admin",
            actor_role: "admin",
        },
        "cancel-again",
    )
    .await;

    let pending = diff
        .place(&RequestSpec::checkout(vec![line("ghost", 1)]), "ghost")
        .await
        .expect("unknown product type sells without options");
    diff.status(
        &pending,
        &StatusSpec {
            to: "paid",
            actor_user_id: "usr_admin",
            actor_role: "admin",
        },
        "unknown-status",
    )
    .await;
    diff.status(
        &pending,
        &StatusSpec {
            to: "shipped",
            actor_user_id: "usr_admin",
            actor_role: "admin",
        },
        "invalid-transition",
    )
    .await;

    let _ = diff
        .place(
            &RequestSpec::checkout(vec![line_with_options("ghost", 1, &[("scent", "Rose")])]),
            "ghost-options",
        )
        .await;

    let _ = diff
        .place(
            &RequestSpec::checkout(vec![line_with_options("core", 1, &[("scent", "Rose")])]),
            "core-options",
        )
        .await;

    let _ = diff
        .place(
            &RequestSpec::checkout(vec![line("plugin", 1)]),
            "plugin-required-option",
        )
        .await;

    let _ = diff
        .place(
            &RequestSpec::checkout(vec![line_with_options(
                "plugin",
                1,
                &[("scent", "Lavender")],
            )]),
            "plugin-unavailable-option",
        )
        .await;

    let _ = diff
        .place(
            &RequestSpec::checkout(vec![line_with_options(
                "plugin",
                1,
                &[("scent", "Rose"), ("size", "10 ml")],
            )]),
            "plugin-unknown-option",
        )
        .await;

    let _ = diff
        .place(
            &RequestSpec::checkout(vec![line("inactive", 1)]),
            "inactive-product",
        )
        .await;

    let _ = diff
        .place(
            &RequestSpec::checkout(vec![line("short", 3)]),
            "stock-shortage",
        )
        .await;

    let mut price_changed = RequestSpec::checkout(vec![line("core", 1)]);
    price_changed.lines[0].expected_price_cents = Some(1);
    let _ = diff.place(&price_changed, "price-changed").await;

    let mut missing = RequestSpec::checkout(vec![line_with_override(
        &format!("{prefix}_missing_product"),
        1,
    )]);
    missing.lines[0].label = "missing";
    let _ = diff.place(&missing, "missing-product").await;

    diff.place(
        &RequestSpec::checkout(vec![line("dup", 2), line("dup", 2)]),
        "duplicate-lines",
    )
    .await
    .expect("duplicate request lines");

    let merged = diff
        .place(
            &RequestSpec::checkout(vec![line("dup_short", 2), line("dup_short", 2)]),
            "duplicate-rollback",
        )
        .await;
    assert!(merged.is_none(), "the second decrement must refuse");

    diff.place(
        &RequestSpec::checkout(vec![line_with_options(
            "plugin",
            2,
            &[("scent", " Oud "), ("finish", " Gloss ")],
        )]),
        "trimmed-options",
    )
    .await
    .expect("options are trimmed for validation and labels");

    diff.place(
        &RequestSpec::counter(vec![line_with_options("plugin", 2, &[("scent", "Rose")])]),
        "counter-plugin",
    )
    .await
    .expect("counter sale with options");

    diff.place(&RequestSpec::counter(vec![line("core", 1)]), "counter-core")
        .await
        .expect("counter sale without options");

    let missing_status = (
        format!("{prefix}_missing_order"),
        format!("{prefix}_missing_order"),
    );
    diff.status(
        &missing_status,
        &StatusSpec {
            to: "confirmed",
            actor_user_id: "usr_admin",
            actor_role: "admin",
        },
        "missing-order",
    )
    .await;

    assert!(
        diff.set_plugin_enabled(false).await,
        "both registries know the fixture plugin"
    );
    let _ = diff
        .place(
            &RequestSpec::checkout(vec![line_with_options("plugin", 1, &[("scent", "Rose")])]),
            "disabled-plugin",
        )
        .await;
    assert!(
        diff.set_plugin_enabled(true).await,
        "both registries can re-enable the fixture plugin"
    );
    diff.place(
        &RequestSpec::checkout(vec![line_with_options("plugin", 1, &[("scent", "Rose")])]),
        "re-enabled-plugin",
    )
    .await
    .expect("re-enabled plugin sells again");

    let orders = std::mem::take(&mut diff.orders);
    drop(diff);
    cleanup(&store, &world, &orders, &category_slug, &user_id).await;
    drop(oracle);
}
