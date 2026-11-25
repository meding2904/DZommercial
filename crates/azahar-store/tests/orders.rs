//! Order, metrics, image and audit-flush behaviour against an in-memory libSQL
//! database.

use std::collections::HashMap;

use azahar_db::migrate;
use azahar_store::orders::AdminOrderFilter;
use azahar_store::{
    AuditEntry, CourierEvent, Order, OrderItem, ProductImage, Store, StoreError, User,
};
use chrono::{DateTime, Utc};
use libsql::{Builder, Connection, Value, params_from_iter};

async fn memory_store() -> Store {
    let database = Builder::new_local(":memory:")
        .build()
        .await
        .expect("open in-memory database");
    let store = Store::new(database).expect("store");
    migrate(store.conn()).await.expect("migrate");
    store
}

fn ts(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .expect("RFC3339 timestamp")
        .with_timezone(&Utc)
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
        notes: "Leave at the desk.".to_string(),
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
            timestamp: "2026-01-01T09:00:00Z".to_string(),
        }],
        courier_synced_at: Some(ts("2026-01-01T09:00:00Z")),
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
        options: HashMap::from([
            ("size".to_string(), "50 ml".to_string()),
            ("finish".to_string(), "matt".to_string()),
        ]),
        options_label: "Size: 50 ml · Finish: matt".to_string(),
        plugin_id: plugin_id.to_string(),
    }
}

async fn scalar(conn: &Connection, sql: &str, args: Vec<Value>) -> i64 {
    let mut rows = conn
        .query(sql, params_from_iter(args))
        .await
        .expect("query scalar");
    rows.next()
        .await
        .expect("step scalar")
        .expect("scalar row")
        .get(0)
        .expect("scalar column")
}

#[tokio::test]
async fn order_numbers_are_zero_padded_and_monotonic() {
    let store = memory_store().await;
    assert_eq!(store.count_orders().await.expect("count"), 0);

    let first = store
        .next_order_number(store.conn(), 2026)
        .await
        .expect("first number");
    let second = store
        .next_order_number(store.conn(), 2026)
        .await
        .expect("second number");
    let other_year = store
        .next_order_number(store.conn(), 2027)
        .await
        .expect("other year");

    assert_eq!(first, "AZ-2026-0001");
    assert_eq!(second, "AZ-2026-0002");
    assert_eq!(other_year, "AZ-2027-0001");

    // The sequence row is reused across calls rather than duplicated.
    assert_eq!(
        scalar(
            store.conn(),
            "SELECT COUNT(*) FROM order_sequences WHERE year = ?",
            vec![Value::from(2026_i64)],
        )
        .await,
        1
    );
    assert_eq!(
        scalar(
            store.conn(),
            "SELECT last_value FROM order_sequences WHERE year = ?",
            vec![Value::from(2026_i64)],
        )
        .await,
        2
    );
}

#[tokio::test]
async fn create_order_round_trips_every_column() {
    let store = memory_store().await;
    let order = sample_order(
        "ord_round_1",
        "AZ-2026-9001",
        "tok_round_1",
        "2026-01-02T03:04:05Z",
    );
    let first_item = sample_item("item_round_1", &order.id, "prd_1", "");
    let second_item = sample_item("item_round_2", &order.id, "prd_2", "phone");

    store
        .create_order(
            store.conn(),
            &order,
            &[first_item.clone(), second_item.clone()],
        )
        .await
        .expect("create order");
    assert_eq!(store.count_orders().await.expect("count"), 1);

    let by_id = store.order_by_id(&order.id).await.expect("order by id");
    assert_eq!(by_id, order);
    assert_eq!(
        store
            .order_by_token_hash(&order.token_hash)
            .await
            .expect("order by token"),
        order
    );
    assert_eq!(
        store
            .order_by_id_tx(store.conn(), &order.id)
            .await
            .expect("tx read"),
        order
    );

    let items = store.order_items(&order.id).await.expect("items");
    assert_eq!(items.len(), 2);
    let mut expected_first = first_item;
    expected_first.plugin_id = "core".to_string();
    assert_eq!(items[0], expected_first);
    assert_eq!(items[1], second_item);
    assert_eq!(
        store
            .order_items_tx(store.conn(), &order.id)
            .await
            .expect("tx items"),
        items
    );

    let stored_options: String = {
        let mut rows = store
            .conn()
            .query(
                "SELECT options FROM order_items WHERE id = ?",
                libsql::params!["item_round_1"],
            )
            .await
            .expect("query options");
        rows.next()
            .await
            .expect("step options")
            .expect("options row")
            .get(0)
            .expect("options column")
    };
    assert_eq!(
        azahar_store::decode_string_map(&stored_options),
        HashMap::from([
            ("size".to_string(), "50 ml".to_string()),
            ("finish".to_string(), "matt".to_string()),
        ])
    );

    // Missing rows and empty item lists keep the Go shapes.
    assert!(
        store
            .order_by_id("ord_missing")
            .await
            .unwrap_err()
            .is_not_found()
    );
    assert!(
        store
            .order_by_token_hash("tok_missing")
            .await
            .unwrap_err()
            .is_not_found()
    );
    assert!(
        store
            .order_items("ord_missing")
            .await
            .expect("missing items")
            .is_empty()
    );
}

#[tokio::test]
async fn create_order_defaults_delivery_type_and_currency() {
    let store = memory_store().await;
    let mut order = sample_order(
        "ord_defaults",
        "AZ-2026-9002",
        "tok_defaults",
        "2026-01-02T04:00:00Z",
    );
    order.delivery_type.clear();
    order.currency.clear();
    order.courier_synced_at = None;

    store
        .create_order(store.conn(), &order, &[])
        .await
        .expect("create order");
    let loaded = store.order_by_id(&order.id).await.expect("order");
    assert_eq!(loaded.delivery_type, "home");
    assert_eq!(loaded.currency, "DZD");
    assert_eq!(loaded.courier_synced_at, None);
}

#[tokio::test]
async fn orders_by_user_and_admin_filters() {
    let store = memory_store().await;
    let mut first = sample_order("ord_a", "AZ-2026-1001", "tok_a", "2026-01-01T10:00:00Z");
    first.user_id = Some("usr_1".to_string());
    first.name = "Amina Test".to_string();
    first.phone = "0550111222".to_string();
    first.tracking_number = "TRK-AAA".to_string();

    let mut second = sample_order("ord_b", "AZ-2026-1002", "tok_b", "2026-01-02T10:00:00Z");
    second.user_id = Some("usr_1".to_string());
    second.name = "Bilal Test".to_string();
    second.phone = "0550333444".to_string();
    second.status = "paid".to_string();

    let mut third = sample_order("ord_c", "AZ-2026-1003", "tok_c", "2026-01-03T10:00:00Z");
    third.user_id = Some("usr_2".to_string());
    third.name = "Amina Other".to_string();
    third.phone = "0660555666".to_string();
    third.status = "shipped".to_string();
    third.tracking_number = "TRK-CCC".to_string();

    let mut fourth = sample_order("ord_d", "AZ-2026-1004", "tok_d", "2026-01-04T10:00:00Z");
    fourth.user_id = None;

    for (id, email) in [
        ("usr_1", "usr_1@example.test"),
        ("usr_2", "usr_2@example.test"),
    ] {
        store
            .create_user(
                store.conn(),
                &User {
                    id: id.to_string(),
                    name: id.to_string(),
                    email: email.to_string(),
                    password_hash: "hash".to_string(),
                    role: "customer".to_string(),
                    theme_preset: String::new(),
                    created_at: Some(ts("2025-12-01T00:00:00Z")),
                    updated_at: Some(ts("2025-12-01T00:00:00Z")),
                },
            )
            .await
            .expect("user");
    }

    store
        .create_order(
            store.conn(),
            &first,
            &[
                sample_item("item_a1", &first.id, "prd_1", ""),
                sample_item("item_a2", &first.id, "prd_2", "oils"),
            ],
        )
        .await
        .expect("create first order with items");
    store
        .create_order(
            store.conn(),
            &second,
            &[sample_item("item_b1", &second.id, "prd_1", "")],
        )
        .await
        .expect("create second order with items");
    for order in [&third, &fourth] {
        store
            .create_order(store.conn(), order, &[])
            .await
            .expect("create order");
    }

    let by_user = store.orders_by_user("usr_1").await.expect("orders by user");
    assert_eq!(
        by_user
            .iter()
            .map(|order| order.id.as_str())
            .collect::<Vec<_>>(),
        vec!["ord_b", "ord_a"]
    );

    let status = |status: &str, query: &str, limit: i64| AdminOrderFilter {
        status: status.to_string(),
        query: query.to_string(),
        limit,
    };

    let pending = store
        .admin_orders(&status("pending", "", 0))
        .await
        .expect("status filter");
    assert_eq!(
        pending
            .iter()
            .map(|order| order.id.as_str())
            .collect::<Vec<_>>(),
        vec!["ord_d", "ord_a"]
    );

    for (query, expected) in [
        ("Amina", vec!["ord_c", "ord_a"]),
        ("AZ-2026-1002", vec!["ord_b"]),
        ("0550111222", vec!["ord_a"]),
        ("TRK-CCC", vec!["ord_c"]),
        ("ord_b@example.test", vec!["ord_b"]),
        ("nothing-matches-this", vec![]),
    ] {
        let found = store
            .admin_orders(&status("", query, 0))
            .await
            .expect("query filter");
        assert_eq!(
            found
                .iter()
                .map(|order| order.id.as_str())
                .collect::<Vec<_>>(),
            expected,
            "query {query:?}"
        );
    }

    let all_matching = store
        .admin_orders(&status("", "AZ-2026-10", 0))
        .await
        .expect("default limit");
    assert_eq!(all_matching.len(), 4);

    let limited = store
        .admin_orders(&status("", "AZ-2026-10", 2))
        .await
        .expect("limit filter");
    assert_eq!(
        limited
            .iter()
            .map(|order| order.id.as_str())
            .collect::<Vec<_>>(),
        vec!["ord_d", "ord_c"]
    );

    let clamped = store
        .admin_orders(&status("", "AZ-2026-10", 1000))
        .await
        .expect("clamped limit");
    assert_eq!(clamped.len(), 4);

    // Items are grouped per order, and orders without items have no key.
    let ids = vec![
        "ord_a".to_string(),
        "ord_b".to_string(),
        "ord_d".to_string(),
        "ord_missing".to_string(),
    ];
    let grouped = store
        .order_items_by_order_ids(&ids)
        .await
        .expect("grouped items");
    assert_eq!(grouped.len(), 2);
    assert_eq!(
        grouped["ord_a"]
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["item_a1", "item_a2"]
    );
    assert_eq!(grouped["ord_b"].len(), 1);
    assert!(!grouped.contains_key("ord_d"));
    assert!(!grouped.contains_key("ord_missing"));

    let empty = store
        .order_items_by_order_ids(&[])
        .await
        .expect("empty ids");
    assert!(empty.is_empty());
}

#[tokio::test]
async fn status_shipment_and_tracking_updates() {
    let store = memory_store().await;
    let order = sample_order(
        "ord_updates",
        "AZ-2026-2001",
        "tok_updates",
        "2026-01-02T03:04:05Z",
    );
    store
        .create_order(store.conn(), &order, &[])
        .await
        .expect("create order");

    let paid_at = ts("2026-01-03T00:00:00Z");
    store
        .update_order_status(store.conn(), &order.id, "pending", "paid", paid_at)
        .await
        .expect("status update");
    let paid = store.order_by_id(&order.id).await.expect("reload");
    assert_eq!(paid.status, "paid");
    assert_eq!(paid.updated_at, Some(paid_at));

    // A stale from-status matches no row and leaves the order untouched.
    let stale = store
        .update_order_status(store.conn(), &order.id, "pending", "shipped", paid_at)
        .await
        .expect_err("stale status");
    assert!(stale.is_not_found());
    assert_eq!(
        store
            .order_by_id(&order.id)
            .await
            .expect("unchanged")
            .status,
        "paid"
    );
    assert!(
        store
            .update_order_status(store.conn(), "ord_missing", "pending", "paid", paid_at)
            .await
            .unwrap_err()
            .is_not_found()
    );

    let events = vec![
        CourierEvent {
            status: "created".to_string(),
            raw_status: "CREATED".to_string(),
            timestamp: "2026-01-04T00:00:00Z".to_string(),
        },
        CourierEvent {
            status: "picked_up".to_string(),
            raw_status: "PICKED".to_string(),
            timestamp: "2026-01-04T01:00:00Z".to_string(),
        },
    ];
    let synced_at = ts("2026-01-04T02:00:00Z");
    store
        .update_shipment(
            store.conn(),
            &order.id,
            "dzship",
            "TRK-999",
            "created",
            &events,
            Some(synced_at),
            synced_at,
        )
        .await
        .expect("shipment update");
    let shipped = store.order_by_id(&order.id).await.expect("reload shipment");
    assert_eq!(shipped.courier_key, "dzship");
    assert_eq!(shipped.tracking_number, "TRK-999");
    assert_eq!(shipped.courier_status, "created");
    assert_eq!(shipped.courier_events, events);
    assert_eq!(shipped.courier_synced_at, Some(synced_at));
    assert_eq!(shipped.updated_at, Some(synced_at));
    assert!(
        store
            .update_shipment(
                store.conn(),
                "ord_missing",
                "dzship",
                "TRK",
                "created",
                &[],
                None,
                synced_at,
            )
            .await
            .unwrap_err()
            .is_not_found()
    );

    let tracking_events = vec![CourierEvent {
        status: "in_transit".to_string(),
        raw_status: "IN_TRANSIT".to_string(),
        timestamp: "2026-01-05T00:00:00Z".to_string(),
    }];
    let tracking_at = ts("2026-01-05T03:00:00Z");
    store
        .update_tracking(
            store.conn(),
            &order.id,
            "in_transit",
            &tracking_events,
            Some(tracking_at),
            tracking_at,
        )
        .await
        .expect("tracking update");
    let tracked = store.order_by_id(&order.id).await.expect("reload tracking");
    assert_eq!(tracked.courier_status, "in_transit");
    assert_eq!(tracked.courier_events, tracking_events);
    assert_eq!(tracked.courier_synced_at, Some(tracking_at));
    assert_eq!(tracked.updated_at, Some(tracking_at));
    assert!(
        store
            .update_tracking(
                store.conn(),
                "ord_missing",
                "in_transit",
                &[],
                None,
                tracking_at,
            )
            .await
            .unwrap_err()
            .is_not_found()
    );

    // A missing sync time is stored as the empty string and reads back as None.
    store
        .update_shipment(
            store.conn(),
            &order.id,
            "dzship",
            "TRK-999",
            "created",
            &[],
            None,
            tracking_at,
        )
        .await
        .expect("shipment without sync time");
    let unsynced = store.order_by_id(&order.id).await.expect("reload");
    assert_eq!(unsynced.courier_synced_at, None);
}

#[tokio::test]
async fn attach_shipment_wins_only_the_first_write() {
    let store = memory_store().await;
    let mut order = sample_order(
        "ord_attach",
        "AZ-2026-2002",
        "tok_attach",
        "2026-01-02T03:04:05Z",
    );
    order.tracking_number.clear();
    order.courier_status.clear();
    order.courier_events.clear();
    order.courier_synced_at = None;
    store
        .create_order(store.conn(), &order, &[])
        .await
        .expect("create order");

    let attached_at = ts("2026-01-06T00:00:00Z");
    store
        .attach_shipment(
            store.conn(),
            &order.id,
            "dzship",
            "TRK-FIRST",
            "created",
            &[],
            Some(attached_at),
            attached_at,
        )
        .await
        .expect("first attach");

    let second = store
        .attach_shipment(
            store.conn(),
            &order.id,
            "other",
            "TRK-SECOND",
            "created",
            &[],
            Some(attached_at),
            attached_at,
        )
        .await
        .expect_err("second attach loses the race");
    assert!(second.is_not_found());

    let stored = store.order_by_id(&order.id).await.expect("reload");
    assert_eq!(stored.tracking_number, "TRK-FIRST");
    assert_eq!(stored.courier_key, "dzship");
}

#[tokio::test]
async fn metrics_aggregate_the_requested_range() {
    let store = memory_store().await;
    let conn = store.conn();

    conn.execute(
        "INSERT INTO categories (id, slug, name, description, sort_order) VALUES (?, ?, ?, ?, ?)",
        libsql::params!["cat_metrics", "cat-metrics", "Metrics", "", 0],
    )
    .await
    .expect("category");
    for (id, name, stock, image, active) in [
        ("prd_alpha", "Alpha", 2, "alpha.webp", 1),
        ("prd_beta", "Beta", 5, "beta.webp", 1),
        ("prd_gamma", "Gamma", 10, "gamma.webp", 1),
        ("prd_hidden", "Hidden", 1, "hidden.webp", 0),
    ] {
        conn.execute(
            "INSERT INTO products (id, slug, name, price_cents, category_id, image, stock, active, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            libsql::params![
                id,
                format!("{id}-slug"),
                name,
                10_000,
                "cat_metrics",
                image,
                stock,
                active,
                "2026-01-01T00:00:00Z",
                "2026-01-01T00:00:00Z"
            ],
        )
        .await
        .expect("product");
    }

    for (id, role, created_at) in [
        ("usr_new", "customer", "2026-02-02T10:00:00Z"),
        ("usr_old", "customer", "2025-12-01T10:00:00Z"),
        ("usr_admin", "admin", "2026-02-02T11:00:00Z"),
    ] {
        let user = User {
            id: id.to_string(),
            name: id.to_string(),
            email: format!("{id}@example.test"),
            password_hash: "hash".to_string(),
            role: role.to_string(),
            theme_preset: String::new(),
            created_at: Some(ts(created_at)),
            updated_at: Some(ts(created_at)),
        };
        store.create_user(conn, &user).await.expect("user");
    }

    let mut orders = Vec::new();
    for (id, status, created_at, total) in [
        ("ord_m1", "paid", "2026-02-01T09:00:00Z", 120_000),
        ("ord_m2", "paid", "2026-02-01T15:00:00Z", 80_000),
        ("ord_m3", "cancelled", "2026-02-02T12:00:00Z", 999_999),
        ("ord_m4", "pending", "2026-02-03T23:59:59Z", 50_000),
    ] {
        let mut order = sample_order(
            id,
            &format!("AZ-2026-{id}"),
            &format!("tok_{id}"),
            created_at,
        );
        order.status = status.to_string();
        order.total_cents = total;
        order.subtotal_cents = total;
        order.shipping_cents = 0;
        order.courier_events.clear();
        order.courier_synced_at = None;
        orders.push(order);
    }
    let mut previous = sample_order(
        "ord_prev",
        "AZ-2026-prev",
        "tok_prev",
        "2026-01-15T09:00:00Z",
    );
    previous.status = "paid".to_string();
    previous.total_cents = 70_000;

    let item =
        |id: &str, order_id: &str, product_id: &str, name: &str, unit: i64, quantity: i32| {
            OrderItem {
                id: id.to_string(),
                order_id: order_id.to_string(),
                product_id: product_id.to_string(),
                name: name.to_string(),
                unit_price_cents: unit,
                quantity,
                line_total_cents: unit * i64::from(quantity),
                ..OrderItem::default()
            }
        };

    store
        .create_order(
            conn,
            &orders[0],
            &[
                item("item_m1a", "ord_m1", "prd_alpha", "Alpha", 50_000, 2),
                item("item_m1b", "ord_m1", "prd_beta", "Beta", 20_000, 1),
            ],
        )
        .await
        .expect("m1");
    store
        .create_order(
            conn,
            &orders[1],
            &[item("item_m2a", "ord_m2", "prd_alpha", "Alpha", 80_000, 1)],
        )
        .await
        .expect("m2");
    store
        .create_order(
            conn,
            &orders[2],
            &[item("item_m3a", "ord_m3", "prd_gamma", "Gamma", 199_999, 5)],
        )
        .await
        .expect("m3");
    store
        .create_order(
            conn,
            &orders[3],
            &[item("item_m4a", "ord_m4", "prd_beta", "Beta", 16_666, 3)],
        )
        .await
        .expect("m4");
    store
        .create_order(conn, &previous, &[])
        .await
        .expect("previous");

    let from = ts("2026-02-01T00:00:00Z");
    let to = ts("2026-02-03T00:00:00Z");
    let metrics = store
        .metrics(
            from,
            to,
            ts("2026-01-01T00:00:00Z"),
            ts("2026-01-31T00:00:00Z"),
        )
        .await
        .expect("metrics");

    assert_eq!(metrics.from, from);
    assert_eq!(metrics.to, to);
    assert_eq!(metrics.revenue_cents, 250_000);
    assert_eq!(metrics.orders_count, 3);
    assert_eq!(metrics.previous_revenue_cents, 70_000);
    assert_eq!(metrics.previous_orders_count, 1);
    assert_eq!(metrics.units_sold, 7);
    assert_eq!(metrics.new_customers, 1);
    assert!(!metrics.demo);
    assert_eq!(
        metrics.status_split,
        HashMap::from([
            ("paid".to_string(), 2),
            ("cancelled".to_string(), 1),
            ("pending".to_string(), 1),
        ])
    );
    assert_eq!(
        metrics
            .revenue_series
            .iter()
            .map(|point| (point.date.as_str(), point.revenue_cents, point.orders))
            .collect::<Vec<_>>(),
        vec![
            ("2026-02-01", 200_000, 2),
            ("2026-02-02", 0, 0),
            ("2026-02-03", 50_000, 1)
        ]
    );
    assert_eq!(
        metrics
            .top_products
            .iter()
            .map(|product| (
                product.product_id.as_str(),
                product.units,
                product.revenue_cents
            ))
            .collect::<Vec<_>>(),
        vec![("prd_beta", 4, 69_998), ("prd_alpha", 3, 180_000)]
    );
    assert_eq!(
        metrics
            .low_stock
            .iter()
            .map(|item| (item.id.as_str(), item.stock, item.image.as_str()))
            .collect::<Vec<_>>(),
        vec![("prd_alpha", 2, "alpha.webp"), ("prd_beta", 5, "beta.webp")]
    );
    assert_eq!(
        metrics
            .recent_orders
            .iter()
            .map(|order| order.id.as_str())
            .collect::<Vec<_>>(),
        vec!["ord_m4", "ord_m3", "ord_m2", "ord_m1"]
    );
    assert_eq!(
        metrics.recent_orders[0].created_at,
        Some(ts("2026-02-03T23:59:59Z"))
    );
    let recent = metrics
        .recent_orders
        .iter()
        .find(|order| order.id == "ord_m2")
        .expect("recent order");
    assert_eq!(recent.number, "AZ-2026-ord_m2");
    assert_eq!(recent.name, "Customer ord_m2");
    assert_eq!(recent.total_cents, 80_000);
    assert_eq!(recent.status, "paid");

    // Demo mode is the presence of the sample account, in users or orders.
    let demo_user = User {
        id: "usr_demo".to_string(),
        name: "Demo".to_string(),
        email: "sample01@azahar.example".to_string(),
        password_hash: "hash".to_string(),
        role: "customer".to_string(),
        theme_preset: String::new(),
        created_at: Some(ts("2020-01-01T00:00:00Z")),
        updated_at: Some(ts("2020-01-01T00:00:00Z")),
    };
    store
        .create_user(conn, &demo_user)
        .await
        .expect("demo user");
    let demo_metrics = store
        .metrics(from, to, from, to)
        .await
        .expect("demo metrics");
    assert!(demo_metrics.demo);
}

#[tokio::test]
async fn product_images_round_trip() {
    let store = memory_store().await;
    let image = ProductImage {
        id: "img_round".to_string(),
        content_type: "image/png".to_string(),
        data: vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0xff],
        created_at: Some(ts("2026-01-02T03:04:05Z")),
    };
    store
        .create_product_image(store.conn(), &image, "usr_uploader")
        .await
        .expect("create image");
    assert_eq!(
        store.product_image(&image.id).await.expect("read image"),
        image
    );

    let created_by: String = {
        let mut rows = store
            .conn()
            .query(
                "SELECT created_by FROM product_images WHERE id = ?",
                libsql::params![image.id],
            )
            .await
            .expect("query created_by");
        rows.next()
            .await
            .expect("step")
            .expect("row")
            .get(0)
            .expect("column")
    };
    assert_eq!(created_by, "usr_uploader");
    assert!(
        store
            .product_image("img_missing")
            .await
            .unwrap_err()
            .is_not_found()
    );
}

#[tokio::test]
async fn audit_flush_moves_the_oldest_batch() {
    let store = memory_store().await;
    let entries = (0..3)
        .map(|index| AuditEntry {
            id: format!("aud_test_{index}"),
            actor_user_id: "usr_actor".to_string(),
            action: "test.audit_batch".to_string(),
            entity_type: "test".to_string(),
            entity_id: format!("entity-{index}"),
            detail: format!(r#"{{"index":{index}}}"#),
            created_at: Some(ts(&format!("2026-01-0{}T00:00:00Z", index + 1))),
        })
        .collect::<Vec<_>>();
    for entry in &entries {
        store
            .insert_audit(store.conn(), entry)
            .await
            .expect("insert audit");
    }

    assert_eq!(
        scalar(
            store.conn(),
            "SELECT COUNT(*) FROM audit_outbox",
            Vec::new()
        )
        .await,
        3
    );
    assert_eq!(store.flush_audit_batch(1).await.expect("first flush"), 1);
    assert_eq!(
        scalar(
            store.conn(),
            "SELECT COUNT(*) FROM audit_log WHERE id = ?",
            vec![Value::from("aud_test_0")],
        )
        .await,
        1
    );
    assert_eq!(
        scalar(
            store.conn(),
            "SELECT COUNT(*) FROM audit_outbox WHERE id = ?",
            vec![Value::from("aud_test_0")],
        )
        .await,
        0
    );

    // limit <= 0 uses the default batch size.
    assert_eq!(store.flush_audit_batch(0).await.expect("default flush"), 2);
    assert_eq!(
        store.flush_audit_batch(5000).await.expect("clamped flush"),
        0
    );
    assert_eq!(
        scalar(store.conn(), "SELECT COUNT(*) FROM audit_log", Vec::new()).await,
        3
    );

    let detail: String = {
        let mut rows = store
            .conn()
            .query(
                "SELECT actor_user_id, action, entity_type, entity_id, detail FROM audit_log WHERE id = ?",
                libsql::params!["aud_test_2"],
            )
            .await
            .expect("query log");
        let row = rows.next().await.expect("step").expect("row");
        assert_eq!(row.get::<String>(0).expect("actor"), "usr_actor");
        assert_eq!(row.get::<String>(1).expect("action"), "test.audit_batch");
        assert_eq!(row.get::<String>(2).expect("type"), "test");
        assert_eq!(row.get::<String>(3).expect("entity"), "entity-2");
        row.get(4).expect("detail")
    };
    assert_eq!(detail, r#"{"index":2}"#);

    // Re-queuing a flushed id keeps the existing log row and drops the copy.
    store
        .insert_audit(store.conn(), &entries[0])
        .await
        .expect("requeue audit");
    assert_eq!(store.flush_audit_batch(10).await.expect("requeue flush"), 1);
    assert_eq!(
        scalar(
            store.conn(),
            "SELECT COUNT(*) FROM audit_log WHERE id = ?",
            vec![Value::from("aud_test_0")],
        )
        .await,
        1
    );
}

#[tokio::test]
async fn store_error_not_found_marker_is_stable() {
    assert!(StoreError::NotFound.is_not_found());
    assert!(!StoreError::Database("x".to_string()).is_not_found());
}
