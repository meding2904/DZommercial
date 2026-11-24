//! Differential test against the Go oracle in `backend/cmd/oracle-seed`.
//!
//! The oracle seeds an empty, disposable libSQL server (never the shared demo
//! database) and prints one JSON report. It is ignored by default because it
//! needs that scratch server. Run it with:
//!
//! ```text
//! AZAHAR_SEED_ORACLE_DSN=http://127.0.0.1:8891 \
//!     cargo test -p azahar-seed --test differential -- --ignored
//! ```
//!
//! Order draws differ between Go's `math/rand` and Rust's `StdRng`, so the
//! report is compared structurally: summaries, users, products, categories,
//! phone devices and the deterministic status/day plan must match; item
//! counts, quantities and the number of distinct products are checked against
//! their allowed ranges on both sides.

mod common;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use azahar_auth::verify_password;
use azahar_seed::{Options, Summary, run};
use azahar_store::orders::AdminOrderFilter;
use azahar_store::{Product, Store, parse_time};
use chrono::{DateTime, Datelike, NaiveDate, Timelike, Utc};
use common::*;
use serde_json::{Value, json};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .parent()
        .expect("repository root")
        .to_path_buf()
}

fn run_oracle(dsn: &str) -> Value {
    let output = Command::new("go")
        .args(["run", "./cmd/oracle-seed", "-dsn", dsn])
        .current_dir(repository_root().join("backend"))
        .output()
        .expect("spawn go oracle");
    assert!(
        output.status.success(),
        "oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("oracle JSON")
}

fn summary_json(summary: Summary) -> Value {
    json!({
        "categories": summary.categories,
        "products": summary.products,
        "users": summary.users,
        "orders": summary.orders,
    })
}

fn day_offset(today: NaiveDate, value: Option<DateTime<Utc>>) -> i64 {
    let value = value.expect("seeded timestamp");
    (today - value.date_naive()).num_days()
}

fn hash_algorithm(hash: &str) -> &'static str {
    let parts: Vec<&str> = hash.split('$').collect();
    if parts.len() == 6 && parts[1] == "argon2id" && parts[2] == "v=19" {
        "argon2id-v19"
    } else {
        "other"
    }
}

fn expected_password(options: &Options, role: &str, email: &str) -> String {
    if role == "admin" {
        options.admin_password.clone()
    } else if role == "employee" {
        options.employee_password.clone()
    } else if email == options.customer_email {
        options.customer_password.clone()
    } else {
        SAMPLE_PASSWORD.to_string()
    }
}

async fn users_json(store: &Store, options: &Options, today: NaiveDate) -> Value {
    let mut users = store.list_users().await.expect("users");
    users.sort_by(|left, right| left.id.cmp(&right.id));
    Value::Array(
        users
            .iter()
            .map(|user| {
                let password = expected_password(options, &user.role, &user.email);
                json!({
                    "id": &user.id,
                    "name": &user.name,
                    "email": &user.email,
                    "role": &user.role,
                    "theme_preset": &user.theme_preset,
                    "created_day_offset": day_offset(today, user.created_at),
                    "hash_algorithm": hash_algorithm(&user.password_hash),
                    "password_ok": verify_password(&password, &user.password_hash),
                    "wrong_password_ok": verify_password("definitely-wrong", &user.password_hash),
                })
            })
            .collect(),
    )
}

fn products_json(products: &[Product], today: NaiveDate) -> Value {
    let mut products = products.to_vec();
    products.sort_by(|left, right| left.id.cmp(&right.id));
    Value::Array(
        products
            .iter()
            .map(|product| {
                let sale_window_hours = match (product.sale_starts_at, product.sale_ends_at) {
                    (Some(start), Some(end)) => (end - start).num_hours(),
                    _ => 0,
                };
                json!({
                    "id": &product.id,
                    "slug": &product.slug,
                    "name": &product.name,
                    "subtitle": &product.subtitle,
                    "description": &product.description,
                    "price_cents": product.price_cents,
                    "sale_price_cents": product.sale_price_cents,
                    "has_sale_window": product.sale_starts_at.is_some() || product.sale_ends_at.is_some(),
                    "sale_window_hours": sale_window_hours,
                    "currency": &product.currency,
                    "size": &product.size,
                    "category_id": &product.category.id,
                    "category_slug": &product.category.slug,
                    "category_plugin_id": &product.category.plugin_id,
                    "concerns": &product.concerns,
                    "ingredients": &product.ingredients,
                    "how_to": &product.how_to,
                    "image": &product.image,
                    "gallery": &product.gallery,
                    "stock": product.stock,
                    "featured": product.featured,
                    "active": product.active,
                    "plugin_id": &product.plugin_id,
                    "attributes": &product.attributes,
                    "created_day_offset": day_offset(today, product.created_at),
                    "updated_equals_created": product.updated_at == product.created_at,
                })
            })
            .collect(),
    )
}

async fn categories_json(store: &Store) -> Value {
    let mut categories = store.categories().await.expect("categories");
    categories.sort_by(|left, right| left.slug.cmp(&right.slug));
    Value::Array(
        categories
            .iter()
            .map(|category| {
                json!({
                    "id": &category.id,
                    "slug": &category.slug,
                    "name": &category.name,
                    "plugin_id": &category.plugin_id,
                    "sort_order": category.sort_order,
                })
            })
            .collect(),
    )
}

async fn phone_devices_json(store: &Store, today: NaiveDate) -> Value {
    let mut rows = store
        .conn()
        .query(
            "SELECT product_id, variant_key, config, color, imei, status, order_id, created_at
             FROM phone_devices ORDER BY imei",
            (),
        )
        .await
        .expect("phone devices");
    let mut devices = Vec::new();
    while let Some(row) = rows.next().await.expect("phone row") {
        let created: String = row.get(7).expect("created");
        devices.push(json!({
            "product_id": row.get::<String>(0).expect("product"),
            "variant_key": row.get::<String>(1).expect("variant"),
            "config": row.get::<String>(2).expect("config"),
            "color": row.get::<String>(3).expect("color"),
            "imei": row.get::<String>(4).expect("imei"),
            "status": row.get::<String>(5).expect("status"),
            "order_id": row.get::<String>(6).expect("order"),
            "created_day_offset": day_offset(today, parse_time(&created)),
        }));
    }
    Value::Array(devices)
}

fn valid_order_number(number: &str, year: i32) -> bool {
    let parts: Vec<&str> = number.split('-').collect();
    parts.len() == 3
        && parts[0] == "AZ"
        && parts[1].parse::<i32>() == Ok(year)
        && parts[2].len() >= 4
        && parts[2].parse::<u32>().is_ok_and(|sequence| sequence >= 1)
}

fn valid_token_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn destination(commune: &str) -> Option<(i32, &'static str)> {
    match commune {
        "Alger Centre" => Some((16, "Alger")),
        "Oran" => Some((31, "Oran")),
        "Constantine" => Some((25, "Constantine")),
        "Tlemcen" => Some((13, "Tlemcen")),
        "Annaba" => Some((23, "Annaba")),
        "Blida" => Some((9, "Blida")),
        _ => None,
    }
}

async fn orders_json(store: &Store, products: &[Product], today: NaiveDate) -> (Value, i64) {
    let orders = store
        .admin_orders(&AdminOrderFilter {
            limit: 500,
            ..AdminOrderFilter::default()
        })
        .await
        .expect("orders");
    let products_by_id: HashMap<&str, &Product> = products
        .iter()
        .map(|product| (product.id.as_str(), product))
        .collect();

    let mut status_counts: HashMap<String, i64> = HashMap::new();
    let mut status_offsets: HashMap<String, Vec<i64>> = HashMap::new();
    let mut guest_count = 0i64;
    let mut linked_count = 0i64;
    let mut min_items = -1i64;
    let mut max_items = 0i64;
    let mut min_quantity = -1i64;
    let mut max_quantity = 0i64;
    let mut numbers = HashSet::new();
    let mut tokens = HashSet::new();
    let mut used_products = HashSet::new();
    let mut invariants_ok = true;
    let mut total_items = 0i64;

    for order in &orders {
        *status_counts.entry(order.status.clone()).or_default() += 1;
        let created = order.created_at.expect("order created");
        status_offsets
            .entry(order.status.clone())
            .or_default()
            .push((today - created.date_naive()).num_days());
        if order.user_id.is_none() {
            guest_count += 1;
        } else {
            linked_count += 1;
        }
        invariants_ok &= numbers.insert(order.number.clone());
        invariants_ok &= tokens.insert(order.token_hash.clone());

        let items = store.order_items(&order.id).await.expect("order items");
        total_items += items.len() as i64;
        if min_items < 0 || (items.len() as i64) < min_items {
            min_items = items.len() as i64;
        }
        max_items = max_items.max(items.len() as i64);
        let mut order_products = HashSet::new();
        let mut subtotal = 0i64;
        for item in &items {
            order_products.insert(item.product_id.clone());
            used_products.insert(item.product_id.clone());
            if min_quantity < 0 || i64::from(item.quantity) < min_quantity {
                min_quantity = i64::from(item.quantity);
            }
            max_quantity = max_quantity.max(i64::from(item.quantity));
            let product = products_by_id.get(item.product_id.as_str());
            invariants_ok &= product.is_some_and(|product| {
                item.slug == product.slug
                    && item.name == product.name
                    && item.image == product.image
                    && item.unit_price_cents == product.price_cents
                    && (1..=3).contains(&item.quantity)
                    && item.line_total_cents == item.unit_price_cents * i64::from(item.quantity)
                    && item.plugin_id == "oils"
            });
            subtotal += item.line_total_cents;
        }
        let (code, wilaya) = destination(&order.city).unwrap_or((-1, ""));
        invariants_ok &= (1..=4).contains(&items.len())
            && order_products.len() == items.len()
            && order.subtotal_cents == subtotal
            && order.shipping_cents == if subtotal > 800_000 { 0 } else { 60_000 }
            && order.total_cents == subtotal + order.shipping_cents
            && order.currency == "DZD"
            && order.delivery_type == "home"
            && order.wilaya_code == code
            && order.wilaya_name == wilaya
            && order.commune_name == order.city
            && (9..=18).contains(&created.hour())
            && order.phone.starts_with("0550 ")
            && order.phone.len() == 13
            && order.address.ends_with(" rue des Oliviers")
            && (order.notes.is_empty() || order.notes == "Leave with the concierge")
            && valid_order_number(&order.number, created.year())
            && valid_token_hash(&order.token_hash);
    }
    for offsets in status_offsets.values_mut() {
        offsets.sort_unstable();
    }

    let report = json!({
        "count": orders.len(),
        "status_counts": status_counts,
        "status_day_offsets": status_offsets,
        "guest_count": guest_count,
        "linked_count": linked_count,
        "min_items": min_items,
        "max_items": max_items,
        "min_quantity": min_quantity,
        "max_quantity": max_quantity,
        "distinct_products": used_products.len(),
        "invariants_ok": invariants_ok,
        "numbers_unique": numbers.len() == orders.len(),
        "tokens_unique": tokens.len() == orders.len(),
    });
    (report, total_items)
}

#[tokio::test]
#[ignore = "needs AZAHAR_SEED_ORACLE_DSN pointing at an empty scratch libSQL server"]
async fn go_oracle_and_rust_seed_agree_structurally() {
    let dsn = std::env::var("AZAHAR_SEED_ORACLE_DSN")
        .expect("set AZAHAR_SEED_ORACLE_DSN to an empty scratch libSQL server");
    let oracle = run_oracle(&dsn);

    let store = seeded_store().await;
    let options = demo_options();
    let today = Utc::now().date_naive();
    let first = run(&store, &options).await.expect("first seed");
    let second = run(&store, &options).await.expect("second seed");

    assert_eq!(oracle["first"], summary_json(first));
    assert_eq!(oracle["second"], summary_json(second));

    let counts = oracle["counts"].as_object().expect("counts");
    assert_eq!(counts["users"], 15);
    assert_eq!(counts["products"], 17);
    assert_eq!(counts["orders"], 48);
    assert_eq!(counts["categories"], 12);
    assert_eq!(counts["phone_devices"], 6);
    assert_eq!(counts["audit_log"], 0);
    assert_eq!(counts["sequence_total"], 48);
    let go_items = counts["order_items"].as_i64().expect("order items");
    assert!((48..=192).contains(&go_items));

    let users = store.list_users().await.expect("users");
    assert_eq!(oracle["users"], users_json(&store, &options, today).await);
    let products = store
        .list_products(&all_products_filter())
        .await
        .expect("products");
    assert_eq!(
        oracle["products"],
        products_json(&products, today),
        "product catalogue differs from the Go oracle"
    );
    assert_eq!(oracle["categories"], categories_json(&store).await);
    assert_eq!(
        oracle["phone_devices"],
        phone_devices_json(&store, today).await
    );

    let (orders, rust_items) = orders_json(&store, &products, today).await;
    assert!((48..=192).contains(&rust_items));
    let oracle_orders = oracle["orders"].as_object().expect("orders");
    for key in [
        "count",
        "status_counts",
        "status_day_offsets",
        "guest_count",
        "linked_count",
        "invariants_ok",
        "numbers_unique",
        "tokens_unique",
    ] {
        assert_eq!(oracle_orders[key], orders[key], "{key}");
    }
    assert_eq!(oracle_orders["invariants_ok"], true);
    assert_eq!(orders["invariants_ok"], true);

    // These depend on random draws and legitimately differ between the two
    // generators; check that each side stays inside the seeded ranges.
    for report in [oracle_orders, orders.as_object().expect("rust orders")] {
        assert!((1..=4).contains(&report["min_items"].as_i64().expect("min items")));
        assert!((1..=4).contains(&report["max_items"].as_i64().expect("max items")));
        assert!((1..=3).contains(&report["min_quantity"].as_i64().expect("min quantity")));
        assert!((1..=3).contains(&report["max_quantity"].as_i64().expect("max quantity")));
        assert!((1..=8).contains(&report["distinct_products"].as_i64().expect("distinct")));
    }
    assert_eq!(users.len(), 15);
    assert_eq!(products.len(), 17);
}
