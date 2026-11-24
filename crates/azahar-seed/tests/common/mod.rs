//! Shared setup for the seed integration tests: a migrated in-memory libSQL
//! store, the plugin categories that `cmd/api` syncs before seeding, and the
//! phone table contributed by the phone plugin migration.

#![allow(dead_code)]

use azahar_seed::Options;
use azahar_store::products::ProductFilter;
use azahar_store::{Category, Store, User};
use azahar_storefront::Theme;
use chrono::Utc;
use libsql::{Builder, Connection};

pub const SAMPLE_PASSWORD: &str = "azahar-sample-customer-2026";

pub async fn memory_store() -> Store {
    let database = Builder::new_local(":memory:")
        .build()
        .await
        .expect("open in-memory database");
    let store = Store::new(database).expect("store");
    azahar_db::migrate(store.conn()).await.expect("migrate");
    create_phone_devices_table(&store).await;
    store
}

async fn create_phone_devices_table(store: &Store) {
    // The final shape of the phone plugin's two migrations.
    store
        .conn()
        .execute(
            "CREATE TABLE IF NOT EXISTS phone_devices (
                id TEXT PRIMARY KEY,
                product_id TEXT NOT NULL REFERENCES products(id),
                variant_key TEXT NOT NULL,
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
        .expect("create phone_devices");
}

pub async fn seeded_store() -> Store {
    let store = memory_store().await;
    register_categories(&store).await;
    store
}

pub async fn register_categories(store: &Store) {
    for (plugin_id, slug, name, description, sort_order) in [
        (
            "oils",
            "face",
            "Face",
            "Oils, serums and masks for the face.",
            1,
        ),
        (
            "oils",
            "body",
            "Body",
            "Balms, oils and creams for the body.",
            2,
        ),
        ("oils", "ritual", "Ritual", "Soaks for the bath.", 3),
        (
            "phone",
            "phones",
            "Phones",
            "Handsets with registered IMEI stock.",
            1,
        ),
        (
            "pcparts",
            "cpus",
            "CPU",
            "Processors with their socket and power draw.",
            1,
        ),
        (
            "pcparts",
            "gpus",
            "Graphics cards",
            "Graphics cards with their length and power draw.",
            2,
        ),
        (
            "pcparts",
            "motherboards",
            "Motherboards",
            "Boards with their socket, memory type and form factor.",
            3,
        ),
        (
            "pcparts",
            "memory",
            "Memory",
            "Memory kits matched to the board's memory type.",
            4,
        ),
        (
            "pcparts",
            "storage",
            "Storage",
            "Drives with their interface and capacity.",
            5,
        ),
        (
            "pcparts",
            "psus",
            "Power supplies",
            "Power supplies with their rated wattage.",
            6,
        ),
        (
            "pcparts",
            "cases",
            "Cases",
            "Cases with their board support and graphics card clearance.",
            7,
        ),
        (
            "pcparts",
            "cooling",
            "Cooling",
            "Air and liquid coolers with the sockets they fit.",
            8,
        ),
    ] {
        store
            .upsert_category(
                plugin_id,
                &Category {
                    slug: slug.to_string(),
                    name: name.to_string(),
                    description: description.to_string(),
                    sort_order,
                    ..Category::default()
                },
            )
            .await
            .expect("upsert plugin category");
    }
}

pub async fn seed_framework_rows(store: &Store) {
    let now = Utc::now();
    store
        .save_storefront_settings(
            store.conn(),
            &azahar_storefront::encode(&Theme::default()).expect("encode theme"),
            now,
        )
        .await
        .expect("save storefront settings");
    for (plugin_id, enabled) in [("oils", true), ("phone", false), ("pcparts", true)] {
        store
            .set_plugin_state(store.conn(), plugin_id, enabled, now)
            .await
            .expect("save plugin state");
    }
    store
        .save_plugin_settings(store.conn(), "ai", r#"{"enabled":false}"#, now)
        .await
        .expect("save plugin settings");
}

pub fn demo_options() -> Options {
    Options {
        admin_email: "admin@azahar.example".to_string(),
        admin_password: "azahar-dev-admin-2026".to_string(),
        customer_email: "client@azahar.example".to_string(),
        customer_password: "azahar-dev-client-2026".to_string(),
        employee_email: "staff@azahar.example".to_string(),
        employee_password: "azahar-dev-staff-2026".to_string(),
    }
}

pub fn demo_customer() -> User {
    User {
        id: "usr_existing".to_string(),
        name: "Existing client".to_string(),
        email: "existing@example.test".to_string(),
        password_hash: "not-used-during-seed".to_string(),
        role: "customer".to_string(),
        theme_preset: "cellar".to_string(),
        created_at: Some(Utc::now()),
        updated_at: Some(Utc::now()),
    }
}

pub fn all_products_filter() -> ProductFilter {
    ProductFilter {
        include_inactive: true,
        limit: 500,
        ..ProductFilter::default()
    }
}

pub async fn scalar_i64(conn: &Connection, sql: &str) -> i64 {
    let mut rows = conn.query(sql, ()).await.expect("scalar query");
    rows.next()
        .await
        .expect("scalar step")
        .expect("scalar row")
        .get(0)
        .expect("scalar value")
}
