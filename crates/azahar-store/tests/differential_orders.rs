//! Differential tests against the Go store oracle in
//! `backend/cmd/oracle-store-orders`.
//!
//! Both sides point at `AZAHAR_TEST_DATABASE_URL` (default
//! `http://127.0.0.1:8880`). Fixtures use a unique prefix, and every row the
//! test creates is deleted at the end.

use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use azahar_db::{migrate, open};
use azahar_store::{
    AuditEntry, CourierEvent, Order, OrderItem, ProductImage, Store, User, format_optional_time,
    format_time, metrics::Metrics, orders::AdminOrderFilter,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

struct Oracle {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Oracle {
    fn spawn() -> Self {
        let backend = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../backend");
        let mut child = Command::new("go")
            .args(["run", "./cmd/oracle-store-orders"])
            .current_dir(&backend)
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

    fn batch(&mut self, requests: &[Value]) -> Vec<Value> {
        {
            let stdin = self.stdin.as_mut().expect("oracle stdin open");
            for request in requests {
                serde_json::to_writer(&mut *stdin, request).expect("encode oracle request");
                stdin.write_all(b"\n").expect("write oracle request");
            }
            stdin.flush().expect("flush oracle requests");
        }

        requests
            .iter()
            .map(|_| {
                let mut line = String::new();
                let read = self
                    .stdout
                    .read_line(&mut line)
                    .expect("read oracle response");
                assert!(
                    read > 0,
                    "Go oracle exited before answering; is AZAHAR_TEST_DATABASE_URL reachable?"
                );
                serde_json::from_str(&line).expect("decode oracle response")
            })
            .collect()
    }

    fn one(&mut self, request: Value) -> Value {
        let responses = self.batch(&[request]);
        responses.into_iter().next().expect("oracle response")
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

fn ts(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .expect("RFC3339 timestamp")
        .with_timezone(&Utc)
}

fn fmt(value: Option<DateTime<Utc>>) -> String {
    format_optional_time(value.as_ref())
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
        "created_at": fmt(order.created_at),
        "updated_at": fmt(order.updated_at),
        "wilaya_code": order.wilaya_code,
        "wilaya_name": order.wilaya_name,
        "commune_name": order.commune_name,
        "delivery_type": order.delivery_type,
        "stop_desk_id": order.stop_desk_id,
        "stop_desk_name": order.stop_desk_name,
        "courier_key": order.courier_key,
        "tracking_number": order.tracking_number,
        "courier_status": order.courier_status,
        "courier_events": order.courier_events,
        "courier_synced_at": fmt(order.courier_synced_at),
    })
}

fn orders_json(orders: &[Order]) -> Value {
    Value::Array(orders.iter().map(order_json).collect())
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

fn items_json(items: &[OrderItem]) -> Value {
    Value::Array(items.iter().map(item_json).collect())
}

fn order_input(order: &Order, courier_synced_at: &str) -> Value {
    let mut value = order_json(order);
    value["courier_synced_at"] = json!(courier_synced_at);
    value
}

fn item_input(item: &OrderItem) -> Value {
    item_json(item)
}

fn image_json(image: &ProductImage) -> Value {
    json!({
        "id": image.id,
        "mime": image.content_type,
        "bytes_base64": STANDARD.encode(&image.data),
        "created_at": fmt(image.created_at),
    })
}

fn metrics_json(metrics: &Metrics) -> Value {
    json!({
        "demo": metrics.demo,
        "from": format_time(&metrics.from),
        "to": format_time(&metrics.to),
        "revenue_cents": metrics.revenue_cents,
        "previous_revenue_cents": metrics.previous_revenue_cents,
        "orders_count": metrics.orders_count,
        "previous_orders_count": metrics.previous_orders_count,
        "units_sold": metrics.units_sold,
        "new_customers": metrics.new_customers,
        "low_stock": metrics.low_stock.iter().map(|item| json!({
            "id": item.id,
            "name": item.name,
            "stock": item.stock,
            "image": item.image,
        })).collect::<Vec<_>>(),
        "revenue_series": metrics.revenue_series.iter().map(|point| json!({
            "date": point.date,
            "revenue_cents": point.revenue_cents,
            "orders": point.orders,
        })).collect::<Vec<_>>(),
        "status_split": metrics.status_split,
        "top_products": metrics.top_products.iter().map(|product| json!({
            "product_id": product.product_id,
            "name": product.name,
            "units": product.units,
            "revenue_cents": product.revenue_cents,
        })).collect::<Vec<_>>(),
        "recent_orders": metrics.recent_orders.iter().map(|order| json!({
            "id": order.id,
            "number": order.number,
            "name": order.name,
            "total_cents": order.total_cents,
            "status": order.status,
            "created_at": fmt(order.created_at),
        })).collect::<Vec<_>>(),
    })
}

fn sample_order(id: &str, number: &str, token_hash: &str, created_at: &str) -> Order {
    Order {
        id: id.to_string(),
        number: number.to_string(),
        name: format!("Customer {id}"),
        email: format!("{id}@example.test"),
        phone: "0550123456".to_string(),
        address: "1 Test Road".to_string(),
        city: "Alger".to_string(),
        notes: "Differential fixture.".to_string(),
        subtotal_cents: 250_000,
        shipping_cents: 50_000,
        total_cents: 300_000,
        currency: "DZD".to_string(),
        status: "pending".to_string(),
        token_hash: token_hash.to_string(),
        created_at: Some(ts(created_at)),
        updated_at: Some(ts(created_at)),
        wilaya_code: 16,
        wilaya_name: "Alger".to_string(),
        commune_name: "Alger Centre".to_string(),
        delivery_type: "home".to_string(),
        stop_desk_id: "desk-1".to_string(),
        stop_desk_name: "Agence Alger".to_string(),
        courier_key: "dzship".to_string(),
        tracking_number: "TRK-INITIAL".to_string(),
        courier_status: "created".to_string(),
        courier_events: vec![CourierEvent {
            status: "created".to_string(),
            raw_status: "CREATED".to_string(),
            timestamp: "2026-06-01T09:00:00Z".to_string(),
        }],
        courier_synced_at: Some(ts("2026-06-01T09:00:00Z")),
        ..Order::default()
    }
}

fn sample_item(id: &str, order_id: &str, product_id: &str, plugin_id: &str) -> OrderItem {
    OrderItem {
        id: id.to_string(),
        order_id: order_id.to_string(),
        product_id: product_id.to_string(),
        slug: format!("{product_id}-slug"),
        name: format!("Product {product_id}"),
        image: format!("/images/{product_id}.webp"),
        unit_price_cents: 50_000,
        quantity: 2,
        line_total_cents: 100_000,
        options: std::collections::HashMap::from([
            ("size".to_string(), "50 ml".to_string()),
            ("finish".to_string(), "matt".to_string()),
        ]),
        options_label: "Size: 50 ml".to_string(),
        plugin_id: plugin_id.to_string(),
    }
}

async fn shared_store() -> Store {
    let dsn = std::env::var("AZAHAR_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8880".to_string());
    let database = open(&dsn)
        .await
        .unwrap_or_else(|err| panic!("cannot open AZAHAR_TEST_DATABASE_URL={dsn}: {err}"));
    {
        let conn = database.connect().expect("database connection");
        migrate(&conn)
            .await
            .unwrap_or_else(|err| panic!("migrate {dsn}: {err}"));
    }
    Store::new(database).expect("store")
}

#[tokio::test]
async fn differential_orders_images_metrics_and_audit_flush() {
    let store = shared_store().await;
    let mut oracle = Oracle::spawn();

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let prefix = format!("it_orders_{nanos}");

    let year = 900_000 + (nanos % 90_000) as i32;
    let year2 = year + 1;

    // Every order this test creates, so cleanup only touches our rows.
    let mut order_ids: Vec<String> = Vec::new();
    let mut image_ids: Vec<String> = Vec::new();
    let mut audit_ids: Vec<String> = Vec::new();
    let user_id = format!("{prefix}_u1");

    store
        .create_user(
            store.conn(),
            &User {
                id: user_id.clone(),
                name: "Differential Customer".to_string(),
                email: format!("{prefix}@example.test"),
                password_hash: "hash".to_string(),
                role: "customer".to_string(),
                theme_preset: String::new(),
                created_at: Some(ts("2026-05-01T00:00:00Z")),
                updated_at: Some(ts("2026-05-01T00:00:00Z")),
            },
        )
        .await
        .expect("fixture user");

    // --- order number sequence -------------------------------------------------
    let response = oracle.one(json!({"op": "next_order_number", "year": year}));
    assert_eq!(response["ok"], json!(true), "oracle number: {response}");
    let go_number = response["number"].as_str().expect("Go number").to_string();
    let go_last = sequence_value(&go_number, year);

    let rust_number = store
        .next_order_number(store.conn(), year)
        .await
        .expect("rust number");
    assert_eq!(
        rust_number,
        format!("AZ-{year}-{:04}", go_last + 1),
        "Rust must take the next slot after Go"
    );

    let rust_first = store
        .next_order_number(store.conn(), year2)
        .await
        .expect("rust first number");
    assert_eq!(rust_first, format!("AZ-{year2}-0001"));
    let response = oracle.one(json!({"op": "next_order_number", "year": year2}));
    assert_eq!(
        response["number"],
        json!(format!("AZ-{year2}-0002")),
        "Go must take the next slot after Rust: {response}"
    );

    // --- Go writes an order, Rust reads it -------------------------------------
    let id1 = format!("{prefix}_o1");
    let id2 = format!("{prefix}_o2");
    let id3 = format!("{prefix}_o3");
    let token1 = format!("{prefix}_tok1");
    let token2 = format!("{prefix}_tok2");
    let token3 = format!("{prefix}_tok3");

    // Empty currency and delivery type exercise the stored defaults.
    let mut order1 = sample_order(
        &id1,
        &format!("{prefix}-1"),
        &token1,
        "2026-06-01T10:00:00Z",
    );
    order1.user_id = Some(user_id.clone());
    order1.currency.clear();
    order1.delivery_type.clear();
    order1.courier_synced_at = None;
    let order1_item1 = sample_item(&format!("{prefix}_i1"), &id1, "prd_alpha", "");
    let order1_item2 = sample_item(&format!("{prefix}_i2"), &id1, "prd_beta", "phone");
    order_ids.push(id1.clone());

    let response = oracle.one(json!({
        "op": "create_order",
        "order": order_input(&order1, ""),
        "items": [item_input(&order1_item1), item_input(&order1_item2)],
    }));
    assert_eq!(response["ok"], json!(true), "oracle create: {response}");

    let response = oracle.one(json!({"op": "order_by_id", "id": id1}));
    let rust_order1 = store.order_by_id(&id1).await.expect("rust order1");
    assert_eq!(response["order"], order_json(&rust_order1), "order by id");
    assert_eq!(rust_order1.currency, "DZD");
    assert_eq!(rust_order1.delivery_type, "home");

    let response = oracle.one(json!({"op": "order_by_token", "token_hash": token1}));
    assert_eq!(
        response["order"],
        order_json(&rust_order1),
        "order by token hash"
    );

    let response = oracle.one(json!({"op": "order_items", "order_id": id1}));
    let rust_items1 = store.order_items(&id1).await.expect("rust items1");
    assert_eq!(response["items"], items_json(&rust_items1), "order items");
    assert_eq!(rust_items1.len(), 2);
    assert_eq!(rust_items1[0].plugin_id, "core");
    assert_eq!(rust_items1[1].plugin_id, "phone");

    // --- Rust writes an order, Go reads it -------------------------------------
    let mut order2 = sample_order(
        &id2,
        &format!("{prefix}-2"),
        &token2,
        "2026-06-02T10:00:00Z",
    );
    order2.user_id = Some(user_id.clone());
    order2.status = format!("it_status_{nanos}");
    order2.phone = format!("055{}", &nanos.to_string()[..7]);
    order2.currency.clear();
    order2.delivery_type.clear();
    let order2_item = sample_item(&format!("{prefix}_i3"), &id2, "prd_alpha", "");
    order_ids.push(id2.clone());
    store
        .create_order(store.conn(), &order2, &[order2_item])
        .await
        .expect("rust create order2");

    let rust_order2 = store.order_by_id(&id2).await.expect("rust order2");
    let response = oracle.one(json!({"op": "order_by_id", "id": id2}));
    assert_eq!(
        response["order"],
        order_json(&rust_order2),
        "go reads rust order"
    );
    assert_eq!(rust_order2.currency, "DZD");
    assert_eq!(rust_order2.delivery_type, "home");

    let response = oracle.one(json!({"op": "order_items", "order_id": id2}));
    assert_eq!(
        response["items"],
        items_json(&store.order_items(&id2).await.expect("rust items2")),
        "go reads rust items"
    );

    // --- a third order for admin filters ---------------------------------------
    let mut order3 = sample_order(
        &id3,
        &format!("{prefix}-3"),
        &token3,
        "2026-06-03T10:00:00Z",
    );
    order3.name = format!("Amina {nanos}");
    order3.tracking_number = format!("TRK{nanos}");
    order3.status = "pending".to_string();
    order_ids.push(id3.clone());
    store
        .create_order(store.conn(), &order3, &[])
        .await
        .expect("rust create order3");

    // --- grouped items ---------------------------------------------------------
    let ids = vec![
        id1.clone(),
        id2.clone(),
        id3.clone(),
        format!("{prefix}_missing"),
    ];
    let response = oracle.one(json!({"op": "items_by_order_ids", "ids": ids}));
    let grouped = store
        .order_items_by_order_ids(&ids)
        .await
        .expect("rust grouped items");
    assert_eq!(grouped.len(), 2);
    assert!(!grouped.contains_key(&id3));
    let mut expected_grouped = serde_json::Map::new();
    for (order_id, items) in &grouped {
        expected_grouped.insert(order_id.clone(), items_json(items));
    }
    assert_eq!(
        response["items"],
        Value::Object(expected_grouped),
        "grouped items"
    );

    let response = oracle.one(json!({"op": "items_by_order_ids", "ids": []}));
    assert_eq!(response["items"], json!({}), "empty ids");

    // --- orders by user -------------------------------------------------------
    let response = oracle.one(json!({"op": "orders_by_user", "user_id": user_id}));
    let rust_orders = store
        .orders_by_user(&user_id)
        .await
        .expect("orders by user");
    assert_eq!(
        response["orders"],
        orders_json(&rust_orders),
        "orders by user"
    );
    assert_eq!(
        rust_orders
            .iter()
            .map(|order| order.id.as_str())
            .collect::<Vec<_>>(),
        vec![id2.as_str(), id1.as_str()]
    );

    // --- admin filters --------------------------------------------------------
    let filters = vec![
        AdminOrderFilter {
            status: String::new(),
            query: prefix.clone(),
            limit: 0,
        },
        AdminOrderFilter {
            status: String::new(),
            query: prefix.clone(),
            limit: 2,
        },
        AdminOrderFilter {
            status: format!("it_status_{nanos}"),
            query: String::new(),
            limit: 0,
        },
        AdminOrderFilter {
            status: String::new(),
            query: format!("Amina {nanos}"),
            limit: 0,
        },
        AdminOrderFilter {
            status: String::new(),
            query: format!("{prefix}-2"),
            limit: 0,
        },
        AdminOrderFilter {
            status: String::new(),
            query: format!("055{}", &nanos.to_string()[..7]),
            limit: 0,
        },
        AdminOrderFilter {
            status: String::new(),
            query: format!("TRK{nanos}"),
            limit: 0,
        },
        AdminOrderFilter {
            status: "pending".to_string(),
            query: prefix.clone(),
            limit: 0,
        },
    ];
    for filter in filters {
        let response = oracle.one(json!({
            "op": "admin_orders",
            "filter": {
                "status": filter.status,
                "query": filter.query,
                "limit": filter.limit,
            },
        }));
        let rust = store
            .admin_orders(&filter)
            .await
            .expect("rust admin orders");
        assert_eq!(
            response["orders"],
            orders_json(&rust),
            "admin filter {filter:?}"
        );
    }

    // --- status updates -------------------------------------------------------
    let paid_at = ts("2026-06-04T00:00:00Z");
    let response = oracle.one(json!({
        "op": "update_status",
        "id": id1,
        "from_status": "pending",
        "to_status": "paid",
        "now": format_time(&paid_at),
    }));
    assert_eq!(response["ok"], json!(true), "go status update: {response}");

    store
        .update_order_status(
            store.conn(),
            &id2,
            &format!("it_status_{nanos}"),
            "paid",
            paid_at,
        )
        .await
        .expect("rust status update");
    let rust_order2 = store.order_by_id(&id2).await.expect("reload order2");
    let response = oracle.one(json!({"op": "order_by_id", "id": id2}));
    assert_eq!(
        response["order"],
        order_json(&rust_order2),
        "go reads rust status update"
    );
    assert_eq!(rust_order2.status, "paid");

    // A stale from-status matches no row on either side.
    let stale_at = ts("2026-06-05T00:00:00Z");
    let response = oracle.one(json!({
        "op": "update_status",
        "id": id1,
        "from_status": "pending",
        "to_status": "cancelled",
        "now": format_time(&stale_at),
    }));
    assert_eq!(response["not_found"], json!(true), "go stale status");
    assert!(
        store
            .update_order_status(store.conn(), &id1, "pending", "cancelled", stale_at)
            .await
            .unwrap_err()
            .is_not_found(),
        "rust stale status"
    );
    assert_eq!(
        store.order_by_id(&id1).await.expect("after stale").status,
        "paid"
    );

    // --- shipment and tracking ------------------------------------------------
    let shipment_events = vec![
        CourierEvent {
            status: "created".to_string(),
            raw_status: "CREATED".to_string(),
            timestamp: "2026-06-06T00:00:00Z".to_string(),
        },
        CourierEvent {
            status: "picked_up".to_string(),
            raw_status: "PICKED".to_string(),
            timestamp: "2026-06-06T01:00:00Z".to_string(),
        },
    ];
    let synced_at = ts("2026-06-06T02:00:00Z");
    store
        .update_shipment(
            store.conn(),
            &id1,
            "dzship",
            "TRK-RUST-1",
            "created",
            &shipment_events,
            Some(synced_at),
            synced_at,
        )
        .await
        .expect("rust shipment");
    let rust_order1 = store.order_by_id(&id1).await.expect("reload order1");
    let response = oracle.one(json!({"op": "order_by_id", "id": id1}));
    assert_eq!(
        response["order"],
        order_json(&rust_order1),
        "go reads rust shipment"
    );
    assert_eq!(rust_order1.courier_events, shipment_events);

    let response = oracle.one(json!({
        "op": "update_shipment",
        "id": id3,
        "courier_key": "yaldine",
        "tracking_number": "TRK-GO-3",
        "status": "created",
        "events": [],
        "synced_at": "",
        "now": format_time(&synced_at),
    }));
    assert_eq!(response["ok"], json!(true), "go shipment: {response}");
    let rust_order3 = store.order_by_id(&id3).await.expect("reload order3");
    assert_eq!(rust_order3.courier_key, "yaldine");
    assert_eq!(rust_order3.tracking_number, "TRK-GO-3");
    assert!(rust_order3.courier_events.is_empty());
    assert_eq!(rust_order3.courier_synced_at, None);

    let response = oracle.one(json!({
        "op": "update_shipment",
        "id": format!("{prefix}_missing"),
        "courier_key": "dzship",
        "tracking_number": "TRK",
        "status": "created",
        "events": [],
        "synced_at": "",
        "now": format_time(&synced_at),
    }));
    assert_eq!(response["not_found"], json!(true), "go missing shipment");
    assert!(
        store
            .update_shipment(
                store.conn(),
                &format!("{prefix}_missing"),
                "dzship",
                "TRK",
                "created",
                &[],
                None,
                synced_at,
            )
            .await
            .unwrap_err()
            .is_not_found(),
        "rust missing shipment"
    );

    let tracking_events = vec![CourierEvent {
        status: "in_transit".to_string(),
        raw_status: "IN_TRANSIT".to_string(),
        timestamp: "2026-06-07T00:00:00Z".to_string(),
    }];
    let tracking_at = ts("2026-06-07T03:00:00Z");
    store
        .update_tracking(
            store.conn(),
            &id2,
            "in_transit",
            &tracking_events,
            Some(tracking_at),
            tracking_at,
        )
        .await
        .expect("rust tracking");
    let rust_order2 = store.order_by_id(&id2).await.expect("reload tracking");
    let response = oracle.one(json!({"op": "order_by_id", "id": id2}));
    assert_eq!(
        response["order"],
        order_json(&rust_order2),
        "go reads rust tracking"
    );

    let response = oracle.one(json!({
        "op": "update_tracking",
        "id": id3,
        "status": "delivered",
        "events": [{
            "status": "delivered",
            "rawStatus": "DELIVERED",
            "timestamp": "2026-06-08T00:00:00Z"
        }],
        "synced_at": "2026-06-08T01:00:00Z",
        "now": "2026-06-08T01:00:00Z",
    }));
    assert_eq!(response["ok"], json!(true), "go tracking: {response}");
    let rust_order3 = store.order_by_id(&id3).await.expect("reload go tracking");
    assert_eq!(rust_order3.courier_status, "delivered");
    assert_eq!(rust_order3.courier_events.len(), 1);
    assert_eq!(rust_order3.courier_events[0].raw_status, "DELIVERED");
    assert_eq!(
        rust_order3.courier_synced_at,
        Some(ts("2026-06-08T01:00:00Z"))
    );

    // --- metrics --------------------------------------------------------------
    let mut metric_orders = Vec::new();
    for (id, status, created_at, total) in [
        ("m1", "paid", "2001-03-01T09:00:00Z", 120_000),
        ("m2", "paid", "2001-03-01T15:00:00Z", 80_000),
        ("m3", "cancelled", "2001-03-02T12:00:00Z", 999_999),
        ("m4", "pending", "2001-03-03T23:00:00Z", 50_000),
    ] {
        let mut order = sample_order(
            &format!("{prefix}_{id}"),
            &format!("{prefix}-{id}"),
            &format!("{prefix}_tok_{id}"),
            created_at,
        );
        order.status = status.to_string();
        order.total_cents = total;
        order.subtotal_cents = total;
        order.shipping_cents = 0;
        order.courier_events.clear();
        order.courier_synced_at = None;
        metric_orders.push(order);
    }
    let mut previous_order = sample_order(
        &format!("{prefix}_m0"),
        &format!("{prefix}-m0"),
        &format!("{prefix}_tok_m0"),
        "2001-02-15T10:00:00Z",
    );
    previous_order.status = "paid".to_string();
    previous_order.total_cents = 70_000;
    previous_order.courier_events.clear();
    previous_order.courier_synced_at = None;

    let metric_item =
        |id: &str, order_id: &str, product: &str, name: &str, unit: i64, quantity: i32| OrderItem {
            id: format!("{prefix}_{id}"),
            order_id: order_id.to_string(),
            product_id: product.to_string(),
            name: name.to_string(),
            unit_price_cents: unit,
            quantity,
            line_total_cents: unit * i64::from(quantity),
            ..OrderItem::default()
        };

    // Go writes the first two metric orders, Rust the rest.
    let response = oracle.one(json!({
        "op": "create_order",
        "order": order_input(&metric_orders[0], ""),
        "items": [
            item_input(&metric_item("mi1", &format!("{prefix}_m1"), "prd_alpha", "Alpha", 50_000, 2)),
            item_input(&metric_item("mi2", &format!("{prefix}_m1"), "prd_beta", "Beta", 20_000, 1)),
        ],
    }));
    assert_eq!(response["ok"], json!(true), "go m1: {response}");
    let response = oracle.one(json!({
        "op": "create_order",
        "order": order_input(&metric_orders[1], ""),
        "items": [
            item_input(&metric_item("mi3", &format!("{prefix}_m2"), "prd_alpha", "Alpha", 80_000, 1)),
        ],
    }));
    assert_eq!(response["ok"], json!(true), "go m2: {response}");
    for (index, items) in [
        vec![metric_item(
            "mi4",
            &format!("{prefix}_m3"),
            "prd_gamma",
            "Gamma",
            199_999,
            5,
        )],
        vec![metric_item(
            "mi5",
            &format!("{prefix}_m4"),
            "prd_beta",
            "Beta",
            16_666,
            3,
        )],
        Vec::new(),
    ]
    .into_iter()
    .enumerate()
    {
        let order = if index == 2 {
            &previous_order
        } else {
            &metric_orders[index + 2]
        };
        order_ids.push(order.id.clone());
        store
            .create_order(store.conn(), order, &items)
            .await
            .unwrap_or_else(|err| panic!("rust create {}: {err}", order.id));
    }
    order_ids.push(metric_orders[0].id.clone());
    order_ids.push(metric_orders[1].id.clone());

    let from = ts("2001-03-01T00:00:00Z");
    let to = ts("2001-03-03T00:00:00Z");
    let previous_from = ts("2001-02-01T00:00:00Z");
    let previous_to = ts("2001-02-28T00:00:00Z");
    let rust_metrics = store
        .metrics(from, to, previous_from, previous_to)
        .await
        .expect("rust metrics");
    let response = oracle.one(json!({
        "op": "metrics",
        "from": format_time(&from),
        "to": format_time(&to),
        "previous_from": format_time(&previous_from),
        "previous_to": format_time(&previous_to),
    }));
    assert_eq!(response["ok"], json!(true), "go metrics: {response}");
    assert_eq!(
        response["metrics"],
        metrics_json(&rust_metrics),
        "metrics differ"
    );
    assert_eq!(rust_metrics.revenue_cents, 250_000);
    assert_eq!(rust_metrics.previous_revenue_cents, 70_000);
    assert_eq!(rust_metrics.units_sold, 7);
    assert_eq!(
        rust_metrics
            .revenue_series
            .iter()
            .map(|point| (point.date.as_str(), point.revenue_cents, point.orders))
            .collect::<Vec<_>>(),
        vec![
            ("2001-03-01", 200_000, 2),
            ("2001-03-02", 0, 0),
            ("2001-03-03", 50_000, 1),
        ]
    );

    // --- product images -------------------------------------------------------
    let image1 = ProductImage {
        id: format!("{prefix}_img1"),
        content_type: "image/png".to_string(),
        data: vec![
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0xff, 0x42,
        ],
        created_at: Some(ts("2026-07-01T00:00:00Z")),
    };
    image_ids.push(image1.id.clone());
    store
        .create_product_image(store.conn(), &image1, &user_id)
        .await
        .expect("rust image");
    let response = oracle.one(json!({"op": "product_image", "id": image1.id}));
    assert_eq!(
        response["image"],
        image_json(&image1),
        "go reads rust image"
    );

    let image2 = ProductImage {
        id: format!("{prefix}_img2"),
        content_type: "image/jpeg".to_string(),
        data: vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, 0x4a, 0x46],
        created_at: Some(ts("2026-07-02T00:00:00Z")),
    };
    image_ids.push(image2.id.clone());
    let response = oracle.one(json!({
        "op": "create_image",
        "image": {
            "id": image2.id,
            "mime": image2.content_type,
            "bytes_base64": STANDARD.encode(&image2.data),
            "created_by": user_id,
            "created_at": format_time(&image2.created_at.expect("image time")),
        },
    }));
    assert_eq!(response["ok"], json!(true), "go image: {response}");
    let rust_image2 = store.product_image(&image2.id).await.expect("rust image2");
    assert_eq!(rust_image2, image2);
    let response = oracle.one(json!({"op": "product_image", "id": image2.id}));
    assert_eq!(
        response["image"],
        image_json(&rust_image2),
        "image round trip"
    );

    let response = oracle.one(json!({"op": "product_image", "id": format!("{prefix}_missing")}));
    assert_eq!(response["not_found"], json!(true), "go missing image");
    assert!(
        store
            .product_image(&format!("{prefix}_missing"))
            .await
            .unwrap_err()
            .is_not_found(),
        "rust missing image"
    );

    // --- audit flush ----------------------------------------------------------
    // Rows dated 1900 sort first, so a limit of one moves only our row.
    let audit1 = AuditEntry {
        id: format!("{prefix}_aud1"),
        actor_user_id: user_id.clone(),
        action: "test.differential".to_string(),
        entity_type: "test".to_string(),
        entity_id: format!("{prefix}_entity1"),
        detail: r#"{"source":"rust"}"#.to_string(),
        created_at: Some(ts("1900-01-01T00:00:00Z")),
    };
    audit_ids.push(audit1.id.clone());
    store
        .insert_audit(store.conn(), &audit1)
        .await
        .expect("rust audit row");
    let response = oracle.one(json!({"op": "flush_audit", "limit": 1}));
    assert_eq!(response["ok"], json!(true), "go flush: {response}");
    assert_eq!(response["count"], json!(1), "go flushed our row");
    assert_eq!(audit_state(&store, &audit1.id).await, (1, 0));

    let audit2 = AuditEntry {
        id: format!("{prefix}_aud2"),
        actor_user_id: user_id.clone(),
        action: "test.differential".to_string(),
        entity_type: "test".to_string(),
        entity_id: format!("{prefix}_entity2"),
        detail: r#"{"source":"rust"}"#.to_string(),
        created_at: Some(ts("1900-01-01T00:00:01Z")),
    };
    audit_ids.push(audit2.id.clone());
    store
        .insert_audit(store.conn(), &audit2)
        .await
        .expect("rust audit row 2");
    assert_eq!(store.flush_audit_batch(1).await.expect("rust flush"), 1);
    assert_eq!(audit_state(&store, &audit2.id).await, (1, 0));

    // --- cleanup --------------------------------------------------------------
    for id in &order_ids {
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
    for id in &image_ids {
        store
            .conn()
            .execute(
                "DELETE FROM product_images WHERE id = ?",
                libsql::params![id.clone()],
            )
            .await
            .expect("cleanup image");
    }
    for id in &audit_ids {
        store
            .conn()
            .execute(
                "DELETE FROM audit_log WHERE id = ?",
                libsql::params![id.clone()],
            )
            .await
            .expect("cleanup audit log");
        store
            .conn()
            .execute(
                "DELETE FROM audit_outbox WHERE id = ?",
                libsql::params![id.clone()],
            )
            .await
            .expect("cleanup audit outbox");
    }
    for value in [year, year2] {
        store
            .conn()
            .execute(
                "DELETE FROM order_sequences WHERE year = ?",
                libsql::params![value],
            )
            .await
            .expect("cleanup sequence");
    }
    store
        .conn()
        .execute("DELETE FROM users WHERE id = ?", libsql::params![user_id])
        .await
        .expect("cleanup user");
}

fn sequence_value(number: &str, year: i32) -> i64 {
    let prefix = format!("AZ-{year}-");
    let suffix = number
        .strip_prefix(&prefix)
        .unwrap_or_else(|| panic!("unexpected order number {number:?}"));
    assert!(suffix.len() >= 4, "number is not zero padded: {number:?}");
    suffix.parse().expect("numeric sequence")
}

async fn audit_state(store: &Store, id: &str) -> (i64, i64) {
    let mut rows = store
        .conn()
        .query(
            "SELECT (SELECT COUNT(*) FROM audit_log WHERE id = ?),
                    (SELECT COUNT(*) FROM audit_outbox WHERE id = ?)",
            libsql::params![id, id],
        )
        .await
        .expect("audit state");
    let row = rows.next().await.expect("audit step").expect("audit row");
    (
        row.get(0).expect("log count"),
        row.get(1).expect("outbox count"),
    )
}
