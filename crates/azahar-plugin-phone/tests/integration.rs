//! Phone device storage, startup repairs and order hooks against an in-memory
//! libSQL database.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use azahar_db::{PluginMigration, migrate, migrate_plugins};
use azahar_orderintake::{CheckoutHook, CheckoutLine, HookProvider, OrderRef, OrderStatusHook};
use azahar_plugin_phone::{
    CreateDevicePayload, PLUGIN_ID, PhoneHooks, PhoneService, product_enrichment, valid_imei,
    variant_key,
};
use azahar_plugins::Plugin as _;
use azahar_store::{Category, Order, Product, Store, format_time};
use chrono::{Duration, Utc};
use libsql::{Builder, Connection};
use serde_json::Value;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

async fn setup() -> Store {
    let database = Builder::new_local(":memory:")
        .build()
        .await
        .expect("open in-memory database");
    let store = Store::new(database).expect("store");
    migrate(store.conn()).await.expect("migrate");
    let migrations: Vec<PluginMigration> = azahar_plugin_phone::new()
        .migrations()
        .into_iter()
        .map(|migration| PluginMigration {
            plugin_id: PLUGIN_ID.to_string(),
            version: migration.version,
            sql: migration.sql,
        })
        .collect();
    migrate_plugins(store.conn(), migrations)
        .await
        .expect("plugin migrations");
    store
}

fn fixture_id(prefix: &str) -> String {
    format!("{prefix}_{}", NEXT_ID.fetch_add(1, Ordering::SeqCst))
}

fn attributes() -> HashMap<String, Vec<String>> {
    HashMap::from([
        (
            "config".to_string(),
            vec![
                "12 GB RAM + 256 GB".to_string(),
                "16 GB RAM + 512 GB".to_string(),
            ],
        ),
        (
            "color".to_string(),
            vec!["Midnight".to_string(), "Blue".to_string()],
        ),
    ])
}

async fn category(store: &Store, slug: &str) -> Category {
    store
        .upsert_category(
            PLUGIN_ID,
            &Category {
                slug: slug.to_string(),
                name: "Phone test".to_string(),
                description: "Temporary".to_string(),
                sort_order: 99,
                ..Default::default()
            },
        )
        .await
        .expect("create category");
    store.category_by_slug(slug).await.expect("load category")
}

async fn make_product(store: &Store, category: &Category, plugin_id: &str, stock: i32) -> Product {
    let now = Utc::now();
    let product = Product {
        id: fixture_id("prd_itph"),
        slug: fixture_id("itph-phone"),
        name: "Test handset".to_string(),
        price_cents: 100_000,
        currency: "DZD".to_string(),
        category: category.clone(),
        image: "/images/atelier-1200.webp".to_string(),
        stock,
        active: true,
        plugin_id: plugin_id.to_string(),
        attributes: attributes(),
        created_at: Some(now),
        updated_at: Some(now),
        ..Default::default()
    };
    store
        .create_product(store.conn(), &product)
        .await
        .expect("create product");
    product
}

async fn order(store: &Store, status: &str) -> Order {
    let now = Utc::now();
    let id = fixture_id("ord_itph");
    let order = Order {
        id: id.clone(),
        number: fixture_id("AZ-ITPH"),
        name: "Buyer".to_string(),
        email: "buyer@example.test".to_string(),
        phone: "0550000000".to_string(),
        address: "1 Rue de Test".to_string(),
        city: "Alger".to_string(),
        subtotal_cents: 0,
        shipping_cents: 0,
        total_cents: 0,
        currency: "DZD".to_string(),
        status: status.to_string(),
        token_hash: fixture_id("tok_itph"),
        created_at: Some(now),
        updated_at: Some(now),
        ..Default::default()
    };
    store
        .create_order(store.conn(), &order, &[])
        .await
        .expect("create order");
    order
}

/// Builds a valid 15-digit IMEI from a numeric seed.
fn imei(seed: u64) -> String {
    let first14 = format!("{:014}", seed % 100_000_000_000_000);
    let mut sum = 0u32;
    for (index, byte) in first14.bytes().enumerate() {
        let mut digit = u32::from(byte - b'0');
        if index % 2 == 1 {
            digit *= 2;
            if digit > 9 {
                digit -= 9;
            }
        }
        sum += digit;
    }
    let check = (10 - sum % 10) % 10;
    format!("{first14}{check}")
}

#[allow(clippy::too_many_arguments)]
async fn insert_device(
    conn: &Connection,
    product: &Product,
    config: &str,
    color: &str,
    imei: &str,
    status: &str,
    created_at: &str,
    order_id: &str,
    order_item_id: &str,
) -> String {
    let id = fixture_id("phn_it");
    let key = variant_key(&HashMap::from([
        ("config".to_string(), config.to_string()),
        ("color".to_string(), color.to_string()),
    ]));
    conn.execute(
        "INSERT INTO phone_devices (id, product_id, variant_key, config, color, imei,
         status, order_id, order_item_id, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        libsql::params![
            id.clone(),
            product.id.clone(),
            key,
            config,
            color,
            imei,
            status,
            order_id,
            order_item_id,
            created_at,
            created_at
        ],
    )
    .await
    .expect("insert device");
    id
}

async fn product_stock(conn: &Connection, id: &str) -> i64 {
    let mut rows = conn
        .query(
            "SELECT stock FROM products WHERE id = ?",
            libsql::params![id],
        )
        .await
        .expect("stock query");
    let row = rows
        .next()
        .await
        .expect("stock row")
        .expect("stock present");
    row.get(0).expect("stock value")
}

async fn device_row(conn: &Connection, id: &str) -> (String, String, String) {
    let mut rows = conn
        .query(
            "SELECT status, order_id, order_item_id FROM phone_devices WHERE id = ?",
            libsql::params![id],
        )
        .await
        .expect("device query");
    let row = rows
        .next()
        .await
        .expect("device row")
        .expect("device present");
    (
        row.get(0).expect("status"),
        row.get(1).expect("order id"),
        row.get(2).expect("order item id"),
    )
}

async fn raw_attributes(conn: &Connection, id: &str) -> Value {
    let mut rows = conn
        .query(
            "SELECT attributes FROM products WHERE id = ?",
            libsql::params![id],
        )
        .await
        .expect("attributes query");
    let row = rows.next().await.expect("row").expect("product present");
    let raw: String = row.get(0).expect("attributes");
    serde_json::from_str(&raw).expect("attributes json")
}

fn options(config: &str, color: &str) -> HashMap<String, String> {
    HashMap::from([
        ("config".to_string(), config.to_string()),
        ("color".to_string(), color.to_string()),
    ])
}

fn payload(product: &Product, config: &str, color: &str, imei: &str) -> CreateDevicePayload {
    CreateDevicePayload {
        product_id: product.id.clone(),
        config: config.to_string(),
        color: color.to_string(),
        imei: imei.to_string(),
    }
}

#[tokio::test]
async fn startup_reconciliation_counts_in_stock_devices() {
    let store = setup().await;
    let category = category(&store, "itph-reconcile").await;
    let product = make_product(&store, &category, PLUGIN_ID, 9).await;

    let now = Utc::now();
    for index in 0..3 {
        insert_device(
            store.conn(),
            &product,
            "12 GB RAM + 256 GB",
            "Midnight",
            &imei(100 + index),
            "in_stock",
            &format_time(&now),
            "",
            "",
        )
        .await;
    }
    insert_device(
        store.conn(),
        &product,
        "12 GB RAM + 256 GB",
        "Midnight",
        &imei(200),
        "reserved",
        &format_time(&now),
        "",
        "",
    )
    .await;
    insert_device(
        store.conn(),
        &product,
        "12 GB RAM + 256 GB",
        "Midnight",
        &imei(201),
        "sold",
        &format_time(&now),
        "",
        "",
    )
    .await;

    PhoneService::new()
        .reconcile_stock(store.conn(), now)
        .await
        .expect("reconcile");
    assert_eq!(product_stock(store.conn(), &product.id).await, 3);
}

#[tokio::test]
async fn legacy_attributes_are_repaired_once() {
    let store = setup().await;
    let category = category(&store, "itph-legacy").await;

    let now = Utc::now();
    let legacy = Product {
        id: fixture_id("prd_itph_legacy"),
        slug: fixture_id("itph-legacy"),
        name: "Legacy handset".to_string(),
        price_cents: 100_000,
        currency: "DZD".to_string(),
        category: category.clone(),
        stock: 0,
        active: true,
        plugin_id: PLUGIN_ID.to_string(),
        attributes: HashMap::from([
            (
                "ram".to_string(),
                vec!["12 GB".to_string(), "16 GB".to_string()],
            ),
            (
                "storage".to_string(),
                vec!["256 GB".to_string(), "512 GB".to_string()],
            ),
            ("color".to_string(), vec!["Midnight".to_string()]),
        ]),
        created_at: Some(now),
        updated_at: Some(now),
        ..Default::default()
    };
    store
        .create_product(store.conn(), &legacy)
        .await
        .expect("create legacy product");

    let mut current = Product {
        attributes: attributes(),
        ..legacy.clone()
    };
    current.id = fixture_id("prd_itph_current");
    current.slug = fixture_id("itph-current");
    store
        .create_product(store.conn(), &current)
        .await
        .expect("create current product");
    let before = raw_attributes(store.conn(), &current.id).await;

    azahar_plugin_phone::repair_legacy_attributes(store.conn()).await;

    let repaired = raw_attributes(store.conn(), &legacy.id).await;
    assert_eq!(
        repaired["config"],
        serde_json::json!(["12 GB RAM + 256 GB", "16 GB RAM + 512 GB"])
    );
    assert_eq!(repaired["color"], serde_json::json!(["Midnight"]));
    assert!(repaired.get("ram").is_none());
    assert!(repaired.get("storage").is_none());

    assert_eq!(raw_attributes(store.conn(), &current.id).await, before);

    // A second pass leaves the repaired row byte-identical.
    let once = raw_attributes(store.conn(), &legacy.id).await;
    azahar_plugin_phone::repair_legacy_attributes(store.conn()).await;
    assert_eq!(raw_attributes(store.conn(), &legacy.id).await, once);
}

#[tokio::test]
async fn device_create_list_delete_and_counts() {
    let store = setup().await;
    let service = PhoneService::new();
    let category = category(&store, "itph-devices").await;
    let product = make_product(&store, &category, PLUGIN_ID, 0).await;
    let other = make_product(&store, &category, "core", 0).await;

    assert_eq!(product_stock(store.conn(), &product.id).await, 0);

    let first_imei = imei(1_000);
    assert!(valid_imei(&first_imei));
    let created = service
        .create_device(
            store.conn(),
            &payload(
                &product,
                " 12 GB RAM + 256 GB ",
                " Midnight ",
                &format!(" {first_imei} "),
            ),
        )
        .await
        .expect("create device");
    assert!(created.id.starts_with("phn_"));
    assert_eq!(created.product_id, product.id);
    assert_eq!(created.product_name, "Test handset");
    assert_eq!(created.config, "12 GB RAM + 256 GB");
    assert_eq!(created.color, "Midnight");
    assert_eq!(created.imei, first_imei);
    assert_eq!(created.status, "in_stock");
    assert_eq!(created.order_id, "");
    assert_eq!(
        created.variant_key,
        "config=12+GB+RAM+%2B+256+GB&color=Midnight"
    );
    assert_eq!(
        created.variant_label,
        "Memory and storage: 12 GB RAM + 256 GB · Color: Midnight"
    );
    assert!(!created.created_at.is_empty());
    assert_eq!(product_stock(store.conn(), &product.id).await, 1);

    let duplicate = service
        .create_device(
            store.conn(),
            &payload(&product, "16 GB RAM + 512 GB", "Blue", &first_imei),
        )
        .await
        .expect_err("duplicate IMEI");
    assert_eq!(duplicate.status, 409);
    assert_eq!(duplicate.code, "conflict");
    assert_eq!(duplicate.message, "That IMEI is already registered.");
    assert_eq!(
        duplicate.fields.as_ref().and_then(|f| f.get("imei")),
        Some(&"This IMEI is already registered.".to_string())
    );

    let bad_config = service
        .create_device(
            store.conn(),
            &payload(&product, "8 GB RAM + 64 GB", "Blue", &imei(1_001)),
        )
        .await
        .expect_err("unknown config");
    assert_eq!(bad_config.status, 422);
    assert_eq!(
        bad_config.fields.as_ref().and_then(|f| f.get("config")),
        Some(&"Choose an available memory and storage option.".to_string())
    );

    let bad_color = service
        .create_device(
            store.conn(),
            &payload(&product, "16 GB RAM + 512 GB", "Green", &imei(1_002)),
        )
        .await
        .expect_err("unknown color");
    assert_eq!(
        bad_color.fields.as_ref().and_then(|f| f.get("color")),
        Some(&"Choose an available color option.".to_string())
    );

    let bad_imei = service
        .create_device(
            store.conn(),
            &payload(&product, "16 GB RAM + 512 GB", "Blue", "123456789012345"),
        )
        .await
        .expect_err("bad checksum");
    assert_eq!(
        bad_imei.fields.as_ref().and_then(|f| f.get("imei")),
        Some(&"Enter a 15-digit IMEI that passes the checksum.".to_string())
    );

    let wrong_product = service
        .create_device(
            store.conn(),
            &payload(&other, "16 GB RAM + 512 GB", "Blue", &imei(1_003)),
        )
        .await
        .expect_err("core product");
    assert_eq!(wrong_product.status, 422);
    assert_eq!(
        wrong_product
            .fields
            .as_ref()
            .and_then(|f| f.get("product_id")),
        Some(&"Choose a phone product.".to_string())
    );

    let mut missing = payload(&product, "16 GB RAM + 512 GB", "Blue", &imei(1_004));
    missing.product_id = "prd_missing".to_string();
    assert_eq!(
        service
            .create_device(store.conn(), &missing)
            .await
            .expect_err("missing product")
            .status,
        422
    );

    let second = service
        .create_device(
            store.conn(),
            &payload(&product, "16 GB RAM + 512 GB", "Blue", &imei(1_005)),
        )
        .await
        .expect("create second");
    assert_eq!(second.variant_key, "config=16+GB+RAM+%2B+512+GB&color=Blue");
    assert_eq!(product_stock(store.conn(), &product.id).await, 2);

    let listed = service
        .list_devices(store.conn(), &product.id)
        .await
        .expect("list");
    assert_eq!(listed.devices.len(), 2);
    assert_eq!(listed.counts.in_stock, 2);
    assert_eq!(listed.counts.reserved, 0);
    assert_eq!(listed.counts.sold, 0);
    assert!(listed.devices.iter().any(|device| device.id == created.id));

    let all = service
        .list_devices(store.conn(), " ")
        .await
        .expect("list all");
    assert!(all.devices.len() >= 2);

    assert_eq!(
        service
            .list_devices(store.conn(), &other.id)
            .await
            .expect_err("core list")
            .status,
        422
    );

    store
        .conn()
        .execute(
            "UPDATE phone_devices SET status = 'reserved' WHERE id = ?",
            libsql::params![second.id.clone()],
        )
        .await
        .expect("reserve for counts");
    let counts = service
        .count_devices(store.conn(), &product.id)
        .await
        .expect("counts");
    assert_eq!(counts.in_stock, 1);
    assert_eq!(counts.reserved, 1);

    assert_eq!(
        service
            .delete_device(store.conn(), &second.id)
            .await
            .expect_err("reserved delete")
            .status,
        409
    );
    store
        .conn()
        .execute(
            "UPDATE phone_devices SET status = 'sold' WHERE id = ?",
            libsql::params![second.id.clone()],
        )
        .await
        .expect("sell for delete refusal");
    let sold_refusal = service
        .delete_device(store.conn(), &second.id)
        .await
        .expect_err("sold delete");
    assert_eq!(sold_refusal.status, 409);
    assert_eq!(
        sold_refusal.message,
        "Only in-stock handsets can be deleted."
    );
    assert_eq!(
        service
            .delete_device(store.conn(), "phn_missing")
            .await
            .expect_err("missing delete")
            .status,
        404
    );

    service
        .delete_device(store.conn(), &created.id)
        .await
        .expect("delete in stock");
    assert_eq!(product_stock(store.conn(), &product.id).await, 0);
    assert!(!service.imei_exists(store.conn(), "").await);
}

#[tokio::test]
async fn order_created_reserves_and_status_moves() {
    let store = setup().await;
    let hooks = PhoneHooks::new();
    let category = category(&store, "itph-hooks").await;
    let product = make_product(&store, &category, PLUGIN_ID, 0).await;
    let start = Utc::now() - Duration::seconds(10);
    let mut devices = Vec::new();
    for index in 0..4 {
        devices.push(
            insert_device(
                store.conn(),
                &product,
                "12 GB RAM + 256 GB",
                "Midnight",
                &imei(2_000 + index),
                "in_stock",
                &format_time(&(start + Duration::seconds(index as i64))),
                "",
                "",
            )
            .await,
        );
    }

    let first_order = order(&store, "pending").await;
    let line = CheckoutLine {
        product: product.clone(),
        options: options("12 GB RAM + 256 GB", "Midnight"),
        quantity: 2,
        order_id: first_order.id.clone(),
        item_id: first_order.id.clone(),
    };
    let write = store.begin_write().await.expect("begin");
    hooks
        .on_order_created(
            write.conn(),
            &OrderRef {
                id: first_order.id.clone(),
                number: first_order.number.clone(),
                status: first_order.status.clone(),
            },
            std::slice::from_ref(&line),
        )
        .await
        .expect("reserve");
    write.commit().await.expect("commit");

    for device in &devices[..2] {
        let (status, order_id, item_id) = device_row(store.conn(), device).await;
        assert_eq!(status, "reserved");
        assert_eq!(order_id, first_order.id);
        assert_eq!(item_id, first_order.id);
    }
    for device in &devices[2..] {
        assert_eq!(device_row(store.conn(), device).await.0, "in_stock");
    }

    let write = store.begin_write().await.expect("begin cancel");
    hooks
        .on_order_status_changed(write.conn(), &first_order.id, "pending", "cancelled")
        .await
        .expect("cancel");
    write.commit().await.expect("commit cancel");
    for device in &devices {
        let (status, order_id, item_id) = device_row(store.conn(), device).await;
        assert_eq!(status, "in_stock");
        assert_eq!(order_id, "");
        assert_eq!(item_id, "");
    }

    let second_order = order(&store, "pending").await;
    let second_line = CheckoutLine {
        product: product.clone(),
        options: options("12 GB RAM + 256 GB", "Midnight"),
        quantity: 2,
        order_id: second_order.id.clone(),
        item_id: fixture_id("itm_itph"),
    };
    let write = store.begin_write().await.expect("begin second");
    hooks
        .on_order_created(
            write.conn(),
            &OrderRef {
                id: second_order.id.clone(),
                number: second_order.number.clone(),
                status: second_order.status.clone(),
            },
            std::slice::from_ref(&second_line),
        )
        .await
        .expect("reserve second");
    hooks
        .on_order_status_changed(write.conn(), &second_order.id, "pending", "delivered")
        .await
        .expect("deliver");
    write.commit().await.expect("commit second");

    let counts = PhoneService::new()
        .count_devices(store.conn(), &product.id)
        .await
        .expect("counts");
    assert_eq!(counts.in_stock, 2);
    assert_eq!(counts.sold, 2);

    let attached = PhoneService::new()
        .order_devices(store.conn(), &second_order.id)
        .await
        .expect("order devices");
    assert_eq!(attached.devices.len(), 2);
    assert!(
        attached
            .devices
            .iter()
            .all(|device| device.status == "sold" && device.order_number == second_order.number)
    );
    assert!(
        attached
            .devices
            .iter()
            .all(|device| device.order_id == second_order.id)
    );

    let empty = PhoneService::new()
        .order_devices(store.conn(), "ord_missing")
        .await
        .expect("missing order devices");
    assert!(empty.devices.is_empty());

    let short = CheckoutLine {
        quantity: 3,
        item_id: fixture_id("itm_itph"),
        ..second_line
    };
    let write = store.begin_write().await.expect("begin short");
    let error = hooks
        .on_order_created(
            write.conn(),
            &OrderRef {
                id: fixture_id("ord_itph"),
                number: fixture_id("AZ-ITPH"),
                status: "pending".to_string(),
            },
            &[short],
        )
        .await
        .expect_err("short reservation");
    write.rollback().await.expect("rollback short");
    assert_eq!(
        error.to_string(),
        "claim handset for Test handset: sql: no rows in result set"
    );
    let counts = PhoneService::new()
        .count_devices(store.conn(), &product.id)
        .await
        .expect("counts after rollback");
    assert_eq!(counts.in_stock, 2);
    assert_eq!(counts.sold, 2);
}

#[tokio::test]
async fn checkout_hook_validates_options_and_stock() {
    let store = setup().await;
    let hooks = PhoneHooks::new();
    let category = category(&store, "itph-checkout").await;
    let product = make_product(&store, &category, PLUGIN_ID, 0).await;
    let now = format_time(&Utc::now());
    for index in 0..2 {
        insert_device(
            store.conn(),
            &product,
            "12 GB RAM + 256 GB",
            "Midnight",
            &imei(3_000 + index),
            "in_stock",
            &now,
            "",
            "",
        )
        .await;
    }

    let line = CheckoutLine {
        product: product.clone(),
        options: options("12 GB RAM + 256 GB", "Midnight"),
        quantity: 2,
        order_id: fixture_id("ord_itph"),
        item_id: fixture_id("itm_itph"),
    };
    assert!(
        hooks
            .validate_checkout_line(store.conn(), &line)
            .await
            .is_none()
    );

    let short = CheckoutLine {
        quantity: 3,
        ..line.clone()
    };
    let problem = hooks
        .validate_checkout_line(store.conn(), &short)
        .await
        .expect("shortage problem");
    assert_eq!(problem.status, 409);
    assert_eq!(problem.code, "conflict");
    assert_eq!(problem.message, "Some products do not have enough stock.");
    assert_eq!(
        problem.fields.as_ref().and_then(|f| f.get("items")),
        Some(
            &"Not enough IMEI-registered handsets for Test handset in that configuration."
                .to_string()
        )
    );

    let no_stock = CheckoutLine {
        options: options("16 GB RAM + 512 GB", "Blue"),
        ..line.clone()
    };
    let problem = hooks
        .validate_checkout_line(store.conn(), &no_stock)
        .await
        .expect("no stock problem");
    assert_eq!(problem.status, 409);

    let unknown_option = CheckoutLine {
        options: options("8 GB RAM + 64 GB", "Midnight"),
        ..line.clone()
    };
    let problem = hooks
        .validate_checkout_line(store.conn(), &unknown_option)
        .await
        .expect("option problem");
    assert_eq!(problem.status, 422);
    assert_eq!(
        problem.fields.as_ref().and_then(|f| f.get("items")),
        Some(&"Choose an available memory and storage option for Test handset.".to_string())
    );

    let mut other_plugin = line.clone();
    other_plugin.product.plugin_id = "core".to_string();
    other_plugin.options = HashMap::from([("color".to_string(), "Blue".to_string())]);
    assert!(
        hooks
            .validate_checkout_line(store.conn(), &other_plugin)
            .await
            .is_none()
    );

    assert!(hooks.checkout_hook(PLUGIN_ID).is_some());
    assert!(hooks.checkout_hook("core").is_none());
    assert_eq!(hooks.status_hooks().len(), 1);
}

#[tokio::test]
async fn enrichment_reports_in_stock_counts_and_variants() {
    let store = setup().await;
    let category = category(&store, "itph-enrich").await;
    let product = make_product(&store, &category, PLUGIN_ID, 0).await;
    let now = format_time(&Utc::now());
    for index in 0..3 {
        insert_device(
            store.conn(),
            &product,
            "12 GB RAM + 256 GB",
            "Midnight",
            &imei(4_000 + index),
            "in_stock",
            &now,
            "",
            "",
        )
        .await;
    }
    insert_device(
        store.conn(),
        &product,
        "16 GB RAM + 512 GB",
        "Blue",
        &imei(4_100),
        "in_stock",
        &now,
        "",
        "",
    )
    .await;
    insert_device(
        store.conn(),
        &product,
        "12 GB RAM + 256 GB",
        "Midnight",
        &imei(4_101),
        "reserved",
        &now,
        "",
        "",
    )
    .await;

    let enrichment = product_enrichment(store.conn(), &product.id)
        .await
        .expect("enrichment");
    assert_eq!(enrichment["devices_in_stock"], serde_json::json!(4));
    assert_eq!(
        enrichment["variants"],
        serde_json::json!([
            {"config": "12 GB RAM + 256 GB", "color": "Midnight", "in_stock": 3},
            {"config": "16 GB RAM + 512 GB", "color": "Blue", "in_stock": 1}
        ])
    );

    let empty = product_enrichment(store.conn(), "prd_missing")
        .await
        .expect("empty enrichment");
    assert_eq!(empty["devices_in_stock"], serde_json::json!(0));
    assert_eq!(empty["variants"], serde_json::json!([]));
}
