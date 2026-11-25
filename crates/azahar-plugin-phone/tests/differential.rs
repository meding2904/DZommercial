//! Differential test for the phone plugin against
//! `backend/cmd/oracle-phone`.
//!
//! Both sides point at `AZAHAR_TEST_DATABASE_URL` (default
//! `http://127.0.0.1:8880`). The pure comparisons cover 300+ generated IMEIs
//! and variant maps; the database half owns an `itph_*` fixture namespace and
//! deletes it before returning.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use azahar_db::{PluginMigration, migrate, migrate_plugins, open};
use azahar_orderintake::{CheckoutHook, CheckoutLine, OrderRef, OrderStatusHook};
use azahar_plugin_phone::{
    Device, DeviceCounts, PLUGIN_ID, PhoneHooks, PhoneService, product_enrichment, valid_imei,
    variant_key, variant_label,
};
use azahar_plugins::Plugin as _;
use azahar_store::{Category, Order, Product, Store, format_time};
use chrono::{Duration, Utc};
use libsql::Connection;
use serde_json::{Map, Value, json};

const DEFAULT_DSN: &str = "http://127.0.0.1:8880";

struct Oracle {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    dsn: String,
}

impl Oracle {
    fn start(dsn: &str) -> Self {
        let backend = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../backend");
        let mut child = Command::new("go")
            .args(["run", "./cmd/oracle-phone"])
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
            dsn: dsn.to_string(),
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
            "Go oracle exited before answering; is AZAHAR_TEST_DATABASE_URL={} reachable? run `make dev-db`",
            self.dsn
        );
        serde_json::from_str(&line)
            .unwrap_or_else(|error| panic!("decode oracle response {line:?}: {error}"))
    }

    fn ok(&mut self, request: Value) -> Value {
        let response = self.call(&request);
        assert_eq!(
            response["ok"],
            json!(true),
            "oracle error for {request}: {}",
            response["error"]
        );
        response["value"].clone()
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

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
    format!("{first14}{}", (10 - sum % 10) % 10)
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

async fn make_product(
    store: &Store,
    category: &Category,
    id: &str,
    slug: &str,
    plugin_id: &str,
    stock: i32,
    attributes: HashMap<String, Vec<String>>,
) -> Product {
    let now = Utc::now();
    let product = Product {
        id: id.to_string(),
        slug: slug.to_string(),
        name: "Test handset".to_string(),
        price_cents: 100_000,
        currency: "DZD".to_string(),
        category: category.clone(),
        image: "/images/atelier-1200.webp".to_string(),
        stock,
        active: true,
        plugin_id: plugin_id.to_string(),
        attributes,
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

async fn make_order(store: &Store, tag: &str, side: &str, index: usize) -> Order {
    let now = Utc::now();
    let id = format!("ord_{tag}_{side}_{index}");
    let order = Order {
        id: id.clone(),
        number: format!("AZ-{tag}-{side}-{index}"),
        name: "Buyer".to_string(),
        email: "buyer@example.test".to_string(),
        phone: "0550000000".to_string(),
        address: "1 Rue de Test".to_string(),
        city: "Alger".to_string(),
        subtotal_cents: 0,
        shipping_cents: 0,
        total_cents: 0,
        currency: "DZD".to_string(),
        status: "pending".to_string(),
        token_hash: format!("tok_{tag}_{side}_{index}"),
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
) {
    let id = format!("phn_{}", imei);
    let key = variant_key(&HashMap::from([
        ("config".to_string(), config.to_string()),
        ("color".to_string(), color.to_string()),
    ]));
    conn.execute(
        "INSERT INTO phone_devices (id, product_id, variant_key, config, color, imei,
         status, order_id, order_item_id, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, '', ?, ?)",
        libsql::params![
            id,
            product.id.clone(),
            key,
            config,
            color,
            imei,
            status,
            order_id,
            created_at,
            created_at
        ],
    )
    .await
    .expect("insert device");
}

async fn product_stock(conn: &Connection, id: &str) -> i64 {
    let mut rows = conn
        .query(
            "SELECT stock FROM products WHERE id = ?",
            libsql::params![id],
        )
        .await
        .expect("stock query");
    let row = rows.next().await.expect("row").expect("product");
    row.get(0).expect("stock")
}

async fn raw_attributes(conn: &Connection, id: &str) -> Value {
    let mut rows = conn
        .query(
            "SELECT attributes FROM products WHERE id = ?",
            libsql::params![id],
        )
        .await
        .expect("attributes query");
    let row = rows.next().await.expect("row").expect("product");
    let raw: String = row.get(0).expect("attributes");
    serde_json::from_str(&raw).expect("attributes json")
}

fn normalize_device(value: &Value) -> Value {
    let mut normalized = Map::new();
    for key in [
        "product_name",
        "variant_key",
        "variant_label",
        "config",
        "color",
        "status",
    ] {
        normalized.insert(key.to_string(), value[key].clone());
    }
    // IMEIs are unique rows, so each side registers its own. The value and its
    // length/checksum are compared by the pure IMEI batch and the per-side
    // assertions instead.
    normalized.insert("imei".to_string(), json!("<imei>"));
    normalized.insert(
        "has_order".to_string(),
        json!(!value["order_id"].as_str().unwrap_or("").is_empty()),
    );
    normalized.insert(
        "has_order_number".to_string(),
        json!(!value["order_number"].as_str().unwrap_or("").is_empty()),
    );
    Value::Object(normalized)
}

fn normalize_oracle_problem(value: &Value) -> Value {
    if value.is_null() {
        return Value::Null;
    }
    json!({
        "status": value["Status"],
        "code": value["Code"],
        "message": value["Message"],
        "fields": value["Fields"],
    })
}

fn normalize_rust_problem(problem: Option<&azahar_plugins::Problem>) -> Value {
    problem.map_or(Value::Null, |problem| {
        json!({
            "status": problem.status,
            "code": problem.code,
            "message": problem.message,
            "fields": problem.fields,
        })
    })
}

fn compare_devices(label: &str, oracle_devices: &Value, rust_devices: &[Device]) {
    let mut expected: Vec<Value> = oracle_devices
        .as_array()
        .expect("oracle device array")
        .iter()
        .map(normalize_device)
        .collect();
    let mut actual: Vec<Value> = rust_devices
        .iter()
        .map(|device| {
            normalize_device(&serde_json::to_value(device).expect("serialize rust device"))
        })
        .collect();
    // List order ends with `d.id DESC`, and the random ids differ per side, so
    // rows created in the same second have no cross-side order. Compare content.
    expected.sort_by_key(Value::to_string);
    actual.sort_by_key(Value::to_string);
    assert_eq!(actual, expected, "{label}");
}

fn compare_counts(label: &str, oracle: &Value, rust: &DeviceCounts) {
    assert_eq!(
        serde_json::to_value(rust).expect("counts json"),
        *oracle,
        "{label}"
    );
}

fn variant_matrix() -> Vec<HashMap<String, String>> {
    let mut variants = Vec::new();
    for index in 0..340u64 {
        let mut options = HashMap::new();
        if index % 7 != 0 {
            options.insert(
                "config".to_string(),
                format!("{} GB RAM + {} GB", 4 + index % 13, 64 + index % 15),
            );
        }
        if index % 5 != 0 {
            options.insert(
                "color".to_string(),
                format!("Finish {index} & {}", index % 3),
            );
        }
        if index % 11 == 0 {
            options.insert("size".to_string(), format!("{index}"));
        }
        variants.push(options);
    }
    variants.push(HashMap::new());
    variants.push(HashMap::from([(
        "config".to_string(),
        "  12 GB RAM + 256 GB  ".to_string(),
    )]));
    variants.push(HashMap::from([(
        "color".to_string(),
        " Café/Noir ".to_string(),
    )]));
    variants.push(HashMap::from([
        (
            "config".to_string(),
            "16 GB RAM + 1 TB ~ plus & equals =".to_string(),
        ),
        ("color".to_string(), "银 · Midnight".to_string()),
    ]));
    variants.push(HashMap::from([
        ("config".to_string(), "a+b&c=d/e?f#g".to_string()),
        ("color".to_string(), "Blue\tMidnight\n".to_string()),
    ]));
    variants
}

#[tokio::test]
async fn phone_matches_the_go_oracle() {
    let dsn = std::env::var("AZAHAR_TEST_DATABASE_URL").unwrap_or_else(|_| DEFAULT_DSN.to_string());
    let database = open(&dsn).await.unwrap_or_else(|error| {
        panic!("open AZAHAR_TEST_DATABASE_URL={dsn}: {error}. run `make dev-db`")
    });
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

    let mut oracle = Oracle::start(&dsn);

    // ---- Pure functions -------------------------------------------------
    let mut imeis = Vec::new();
    for index in 0..340u64 {
        imeis.push(imei(700_000 + index));
    }
    imeis.extend(
        [
            "",
            "49015420323751",
            "4901542032375181",
            "490154203237519",
            "123456789012345",
            "860000000000000",
            "49015420323751a",
            "49015 203237518",
            "4901-5420-3237-51",
            "490154203237518\n",
            "٤٩٠١٥٤٢٠٣٢٣٧٥١٨",
            "49015420323751８",
            "356938035643809",
            "352099001761481",
        ]
        .map(str::to_string),
    );
    assert!(imeis.len() >= 300);
    let oracle_results = oracle.ok(json!({"op": "imei", "imeis": imeis}));
    let expected: Vec<Value> = imeis.iter().map(|value| json!(valid_imei(value))).collect();
    assert_eq!(Value::Array(expected), oracle_results);

    let variants = variant_matrix();
    assert!(variants.len() >= 300);
    let oracle_results = oracle.ok(json!({"op": "variant", "variants": variants}));
    let expected: Vec<Value> = variants
        .iter()
        .map(|options| json!({"key": variant_key(options), "label": variant_label(options)}))
        .collect();
    assert_eq!(Value::Array(expected), oracle_results);

    let manifest = oracle.ok(json!({"op": "manifest"}));
    assert_eq!(
        serde_json::to_value(azahar_plugin_phone::new().manifest()).expect("manifest json"),
        manifest
    );

    // ---- Fixtures -------------------------------------------------------
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let tag = format!("itph{nonce}");
    // IMEIs are globally unique. Derive the fixture range from the run's nonce
    // so a failed run cannot collide with the next one.
    let imei_base = (nonce % 10_000_000_000) as u64 * 100;
    let category = Category {
        slug: format!("{tag}-category"),
        name: "Phone differential".to_string(),
        description: "Temporary".to_string(),
        sort_order: 99,
        ..Default::default()
    };
    store
        .upsert_category(PLUGIN_ID, &category)
        .await
        .expect("category");
    let category = store
        .category_by_slug(&category.slug)
        .await
        .expect("load category");

    let reconcile_go = make_product(
        &store,
        &category,
        &format!("prd_{tag}_go_reconcile"),
        &format!("{tag}-go-reconcile"),
        PLUGIN_ID,
        99,
        attributes(),
    )
    .await;
    let reconcile_rs = make_product(
        &store,
        &category,
        &format!("prd_{tag}_rs_reconcile"),
        &format!("{tag}-rs-reconcile"),
        PLUGIN_ID,
        99,
        attributes(),
    )
    .await;
    let now = format_time(&Utc::now());
    for (side, product) in [("go", &reconcile_go), ("rs", &reconcile_rs)] {
        for index in 0..3u64 {
            insert_device(
                store.conn(),
                product,
                "12 GB RAM + 256 GB",
                "Midnight",
                &imei(imei_base + index + if side == "go" { 0 } else { 100 }),
                if index < 2 { "in_stock" } else { "reserved" },
                &now,
                "",
            )
            .await;
        }
    }
    let oracle_reconcile = oracle.ok(json!({"op": "reconcile", "product_id": reconcile_go.id}));
    PhoneService::new()
        .reconcile_stock(store.conn(), Utc::now())
        .await
        .expect("rust reconcile");
    assert_eq!(oracle_reconcile["stock"], json!(2));
    assert_eq!(product_stock(store.conn(), &reconcile_rs.id).await, 2);

    // ---- Legacy attribute repair ---------------------------------------
    let legacy_attributes = HashMap::from([
        (
            "ram".to_string(),
            vec!["12 GB".to_string(), "16 GB".to_string()],
        ),
        (
            "storage".to_string(),
            vec!["256 GB".to_string(), "512 GB".to_string()],
        ),
        ("color".to_string(), vec!["Midnight".to_string()]),
    ]);
    let legacy_go = make_product(
        &store,
        &category,
        &format!("prd_{tag}_go_legacy"),
        &format!("{tag}-go-legacy"),
        PLUGIN_ID,
        0,
        legacy_attributes.clone(),
    )
    .await;
    let legacy_rs = make_product(
        &store,
        &category,
        &format!("prd_{tag}_rs_legacy"),
        &format!("{tag}-rs-legacy"),
        PLUGIN_ID,
        0,
        legacy_attributes,
    )
    .await;
    let oracle_repair = oracle.ok(json!({"op": "repair", "product_id": legacy_go.id}));
    azahar_plugin_phone::repair_legacy_attributes(store.conn()).await;
    assert_eq!(
        raw_attributes(store.conn(), &legacy_rs.id).await,
        oracle_repair["attributes"]
    );
    assert_eq!(
        raw_attributes(store.conn(), &legacy_rs.id).await["config"],
        json!(["12 GB RAM + 256 GB", "16 GB RAM + 512 GB"])
    );

    // ---- Device service -------------------------------------------------
    let go_product = make_product(
        &store,
        &category,
        &format!("prd_{tag}_go_devices"),
        &format!("{tag}-go-devices"),
        PLUGIN_ID,
        0,
        attributes(),
    )
    .await;
    let rs_product = make_product(
        &store,
        &category,
        &format!("prd_{tag}_rs_devices"),
        &format!("{tag}-rs-devices"),
        PLUGIN_ID,
        0,
        attributes(),
    )
    .await;
    let core_product = make_product(
        &store,
        &category,
        &format!("prd_{tag}_core"),
        &format!("{tag}-core"),
        "core",
        0,
        attributes(),
    )
    .await;

    let service = PhoneService::new();
    let go_imei = imei(imei_base + 10_000);
    let rs_imei = imei(imei_base + 20_000);
    let go_create = oracle.ok(json!({
        "op": "create", "product_id": go_product.id, "config": " 12 GB RAM + 256 GB ",
        "color": " Midnight ", "imei": go_imei,
    }));
    let rs_create = service
        .create_device(
            store.conn(),
            &azahar_plugin_phone::CreateDevicePayload {
                product_id: rs_product.id.clone(),
                config: " 12 GB RAM + 256 GB ".to_string(),
                color: " Midnight ".to_string(),
                imei: rs_imei.clone(),
            },
        )
        .await
        .expect("rust create");
    assert!(go_create["problem"].is_null());
    assert_eq!(go_create["device"]["imei"], json!(go_imei));
    assert!(rs_create.id.starts_with("phn_"));
    assert_eq!(rs_create.imei, rs_imei);
    assert_eq!(
        normalize_device(&serde_json::to_value(&rs_create).expect("device json")),
        normalize_device(&go_create["device"])
    );
    assert_eq!(product_stock(store.conn(), &rs_product.id).await, 1);

    let second_go_imei = imei(imei_base + 10_001);
    let second_rs_imei = imei(imei_base + 20_001);
    let go_second = oracle.ok(json!({
        "op": "create", "product_id": go_product.id, "config": "16 GB RAM + 512 GB",
        "color": "Blue", "imei": second_go_imei,
    }));
    let rs_second = service
        .create_device(
            store.conn(),
            &azahar_plugin_phone::CreateDevicePayload {
                product_id: rs_product.id.clone(),
                config: "16 GB RAM + 512 GB".to_string(),
                color: "Blue".to_string(),
                imei: second_rs_imei.clone(),
            },
        )
        .await
        .expect("rust second");
    assert_eq!(
        normalize_device(&serde_json::to_value(&rs_second).expect("device json")),
        normalize_device(&go_second["device"])
    );

    // Give the two created units a deterministic shelf order. The Go hook
    // selects `ORDER BY created_at, id`; the random ids would otherwise decide
    // which configuration a same-second checkout claims.
    let older = format_time(&(Utc::now() - Duration::seconds(5)));
    let newer = format_time(&(Utc::now() - Duration::seconds(4)));
    let go_second_id = go_second["device"]["id"]
        .as_str()
        .expect("go second id")
        .to_string();
    for (id, created_at) in [
        (
            go_create["device"]["id"]
                .as_str()
                .expect("go first id")
                .to_string(),
            older.clone(),
        ),
        (go_second_id, newer.clone()),
        (rs_create.id.clone(), older),
        (rs_second.id.clone(), newer.clone()),
    ] {
        store
            .conn()
            .execute(
                "UPDATE phone_devices SET created_at = ? WHERE id = ?",
                libsql::params![created_at, id],
            )
            .await
            .expect("order devices");
    }

    // Duplicate IMEI.
    let go_duplicate = oracle.ok(json!({
        "op": "create", "product_id": go_product.id, "config": "16 GB RAM + 512 GB",
        "color": "Blue", "imei": go_imei,
    }));
    let rs_duplicate = service
        .create_device(
            store.conn(),
            &azahar_plugin_phone::CreateDevicePayload {
                product_id: rs_product.id.clone(),
                config: "16 GB RAM + 512 GB".to_string(),
                color: "Blue".to_string(),
                imei: rs_imei.clone(),
            },
        )
        .await
        .expect_err("duplicate");
    assert_eq!(
        normalize_rust_problem(Some(&rs_duplicate)),
        normalize_oracle_problem(&go_duplicate["problem"])
    );

    // Unknown option and bad IMEI.
    let go_bad_option = oracle.ok(json!({
        "op": "create", "product_id": go_product.id, "config": "8 GB RAM + 64 GB",
        "color": "Blue", "imei": imei(imei_base + 10_002),
    }));
    let rs_bad_option = service
        .create_device(
            store.conn(),
            &azahar_plugin_phone::CreateDevicePayload {
                product_id: rs_product.id.clone(),
                config: "8 GB RAM + 64 GB".to_string(),
                color: "Blue".to_string(),
                imei: imei(imei_base + 20_002),
            },
        )
        .await
        .expect_err("bad option");
    assert_eq!(
        normalize_rust_problem(Some(&rs_bad_option)),
        normalize_oracle_problem(&go_bad_option["problem"])
    );

    let go_bad_imei = oracle.ok(json!({
        "op": "create", "product_id": go_product.id, "config": "16 GB RAM + 512 GB",
        "color": "Blue", "imei": "123456789012345",
    }));
    let rs_bad_imei = service
        .create_device(
            store.conn(),
            &azahar_plugin_phone::CreateDevicePayload {
                product_id: rs_product.id.clone(),
                config: "16 GB RAM + 512 GB".to_string(),
                color: "Blue".to_string(),
                imei: "123456789012345".to_string(),
            },
        )
        .await
        .expect_err("bad imei");
    assert_eq!(
        normalize_rust_problem(Some(&rs_bad_imei)),
        normalize_oracle_problem(&go_bad_imei["problem"])
    );

    // Core product.
    let go_core = oracle.ok(json!({
        "op": "create", "product_id": core_product.id, "config": "16 GB RAM + 512 GB",
        "color": "Blue", "imei": imei(imei_base + 10_003),
    }));
    let rs_core = service
        .create_device(
            store.conn(),
            &azahar_plugin_phone::CreateDevicePayload {
                product_id: core_product.id.clone(),
                config: "16 GB RAM + 512 GB".to_string(),
                color: "Blue".to_string(),
                imei: imei(imei_base + 20_003),
            },
        )
        .await
        .expect_err("core product");
    assert_eq!(
        normalize_rust_problem(Some(&rs_core)),
        normalize_oracle_problem(&go_core["problem"])
    );

    // List and counts.
    let go_list = oracle.ok(json!({"op": "list", "product_id": go_product.id}));
    let rs_list = service
        .list_devices(store.conn(), &rs_product.id)
        .await
        .expect("list");
    compare_devices("list devices", &go_list["devices"], &rs_list.devices);
    compare_counts("list counts", &go_list["counts"], &rs_list.counts);
    assert_eq!(rs_list.counts.in_stock, 2);

    let go_core_list = oracle.ok(json!({"op": "list", "product_id": core_product.id}));
    let rs_core_list = service
        .list_devices(store.conn(), &core_product.id)
        .await
        .expect_err("core list");
    assert_eq!(
        normalize_rust_problem(Some(&rs_core_list)),
        normalize_oracle_problem(&go_core_list["problem"])
    );

    let go_exists = oracle.ok(json!({"op": "imei_exists", "imei": go_imei}));
    let rs_exists = service.imei_exists(store.conn(), &rs_imei).await;
    assert_eq!(go_exists["exists"], json!(rs_exists));
    assert!(rs_exists);

    // A second unit in the hook configuration. Both sides need two in-stock
    // handsets for a quantity-two checkout.
    let go_hook_imei = imei(imei_base + 10_005);
    let rs_hook_imei = imei(imei_base + 20_005);
    let go_hook = oracle.ok(json!({
        "op": "create", "product_id": go_product.id, "config": "12 GB RAM + 256 GB",
        "color": "Midnight", "imei": go_hook_imei,
    }));
    let rs_hook = service
        .create_device(
            store.conn(),
            &azahar_plugin_phone::CreateDevicePayload {
                product_id: rs_product.id.clone(),
                config: "12 GB RAM + 256 GB".to_string(),
                color: "Midnight".to_string(),
                imei: rs_hook_imei,
            },
        )
        .await
        .expect("hook device");
    assert!(go_hook["problem"].is_null());
    for (id, created_at) in [
        (
            go_hook["device"]["id"]
                .as_str()
                .expect("go hook id")
                .to_string(),
            newer.clone(),
        ),
        (rs_hook.id.clone(), newer.clone()),
    ] {
        store
            .conn()
            .execute(
                "UPDATE phone_devices SET created_at = ? WHERE id = ?",
                libsql::params![created_at, id],
            )
            .await
            .expect("order hook device");
    }

    // ---- Hooks ----------------------------------------------------------
    let hooks = PhoneHooks::new();
    let go_order_one = make_order(&store, &tag, "go", 1).await;
    let rs_order_one = make_order(&store, &tag, "rs", 1).await;
    let item_id = format!("itm_{tag}");
    let go_line = CheckoutLine {
        product: go_product.clone(),
        options: HashMap::from([
            ("config".to_string(), "12 GB RAM + 256 GB".to_string()),
            ("color".to_string(), "Midnight".to_string()),
        ]),
        quantity: 2,
        order_id: go_order_one.id.clone(),
        item_id: item_id.clone(),
    };
    let rs_line = CheckoutLine {
        product: rs_product.clone(),
        options: go_line.options.clone(),
        quantity: 2,
        order_id: rs_order_one.id.clone(),
        item_id: item_id.clone(),
    };

    let go_validate = oracle.ok(json!({
        "op": "validate", "product_id": go_product.id,
        "options": go_line.options, "quantity": 2,
        "order_id": go_order_one.id, "item_id": item_id,
    }));
    let rs_validate = hooks.validate_checkout_line(store.conn(), &rs_line).await;
    assert_eq!(
        normalize_rust_problem(rs_validate.as_ref()),
        normalize_oracle_problem(&go_validate["problem"])
    );
    assert!(rs_validate.is_none());

    let go_short = oracle.ok(json!({
        "op": "validate", "product_id": go_product.id,
        "options": go_line.options, "quantity": 3,
        "order_id": go_order_one.id, "item_id": item_id,
    }));
    let rs_short = CheckoutLine {
        quantity: 3,
        ..rs_line.clone()
    };
    let rs_short = hooks.validate_checkout_line(store.conn(), &rs_short).await;
    assert_eq!(
        normalize_rust_problem(rs_short.as_ref()),
        normalize_oracle_problem(&go_short["problem"])
    );

    let go_unknown = oracle.ok(json!({
        "op": "validate", "product_id": go_product.id,
        "options": {"config": "8 GB RAM + 64 GB", "color": "Midnight"},
        "quantity": 1, "order_id": go_order_one.id, "item_id": item_id,
    }));
    let rs_unknown_line = CheckoutLine {
        options: HashMap::from([
            ("config".to_string(), "8 GB RAM + 64 GB".to_string()),
            ("color".to_string(), "Midnight".to_string()),
        ]),
        quantity: 1,
        ..rs_line.clone()
    };
    let rs_unknown = hooks
        .validate_checkout_line(store.conn(), &rs_unknown_line)
        .await;
    assert_eq!(
        normalize_rust_problem(rs_unknown.as_ref()),
        normalize_oracle_problem(&go_unknown["problem"])
    );

    let go_created = oracle.ok(json!({
        "op": "order_created", "product_id": go_product.id,
        "options": go_line.options, "quantity": 2,
        "order_id": go_order_one.id, "item_id": item_id,
    }));
    hooks
        .on_order_created(
            store.conn(),
            &OrderRef {
                id: rs_order_one.id.clone(),
                number: rs_order_one.number.clone(),
                status: "pending".to_string(),
            },
            std::slice::from_ref(&rs_line),
        )
        .await
        .expect("reserve");
    assert_eq!(go_created["error"], json!(""));

    let go_counts = oracle.ok(json!({"op": "counts", "product_id": go_product.id}));
    let rs_counts = service
        .count_devices(store.conn(), &rs_product.id)
        .await
        .expect("counts after reserve");
    compare_counts("reserved counts", &go_counts["counts"], &rs_counts);
    assert_eq!(rs_counts.reserved, 2);

    let go_order_devices = oracle.ok(json!({"op": "order_devices", "order_id": go_order_one.id}));
    let rs_order_devices = service
        .order_devices(store.conn(), &rs_order_one.id)
        .await
        .expect("order devices");
    compare_devices(
        "reserved order devices",
        &go_order_devices["devices"],
        &rs_order_devices.devices,
    );
    assert!(
        rs_order_devices
            .devices
            .iter()
            .all(|device| device.order_id == rs_order_one.id && device.status == "reserved")
    );
    for device in &rs_order_devices.devices {
        let mut rows = store
            .conn()
            .query(
                "SELECT order_item_id FROM phone_devices WHERE id = ?",
                libsql::params![device.id.clone()],
            )
            .await
            .expect("item query");
        let row = rows.next().await.expect("row").expect("device");
        let stored: String = row.get(0).expect("item id");
        assert_eq!(stored, item_id);
    }

    let go_cancel = oracle.ok(json!({
        "op": "status", "order_id": go_order_one.id, "from": "pending", "to": "cancelled",
    }));
    hooks
        .on_order_status_changed(store.conn(), &rs_order_one.id, "pending", "cancelled")
        .await
        .expect("cancel");
    assert_eq!(go_cancel["error"], json!(""));
    let go_counts = oracle.ok(json!({"op": "counts", "product_id": go_product.id}));
    let rs_counts = service
        .count_devices(store.conn(), &rs_product.id)
        .await
        .expect("counts after cancel");
    compare_counts("cancelled counts", &go_counts["counts"], &rs_counts);
    assert_eq!(rs_counts.in_stock, 3);

    // A second order reserves the same shelf, then delivery turns it into sold
    // stock and a third attempt has nothing left to claim.
    let go_order_two = make_order(&store, &tag, "go", 2).await;
    let rs_order_two = make_order(&store, &tag, "rs", 2).await;
    let go_line_two = CheckoutLine {
        order_id: go_order_two.id.clone(),
        ..go_line.clone()
    };
    let rs_line_two = CheckoutLine {
        order_id: rs_order_two.id.clone(),
        ..rs_line.clone()
    };
    let go_created = oracle.ok(json!({
        "op": "order_created", "product_id": go_product.id,
        "options": go_line_two.options, "quantity": 2,
        "order_id": go_order_two.id, "item_id": item_id,
    }));
    hooks
        .on_order_created(
            store.conn(),
            &OrderRef {
                id: rs_order_two.id.clone(),
                number: rs_order_two.number.clone(),
                status: "pending".to_string(),
            },
            std::slice::from_ref(&rs_line_two),
        )
        .await
        .expect("reserve second");
    assert_eq!(go_created["error"], json!(""));

    let go_delivered = oracle.ok(json!({
        "op": "status", "order_id": go_order_two.id, "from": "pending", "to": "delivered",
    }));
    hooks
        .on_order_status_changed(store.conn(), &rs_order_two.id, "pending", "delivered")
        .await
        .expect("deliver");
    assert_eq!(go_delivered["error"], json!(""));
    let go_counts = oracle.ok(json!({"op": "counts", "product_id": go_product.id}));
    let rs_counts = service
        .count_devices(store.conn(), &rs_product.id)
        .await
        .expect("counts after delivery");
    compare_counts("delivered counts", &go_counts["counts"], &rs_counts);
    assert_eq!(rs_counts.sold, 2);

    let go_sold_devices = oracle.ok(json!({"op": "order_devices", "order_id": go_order_two.id}));
    let rs_sold_devices = service
        .order_devices(store.conn(), &rs_order_two.id)
        .await
        .expect("sold devices");
    compare_devices(
        "sold order devices",
        &go_sold_devices["devices"],
        &rs_sold_devices.devices,
    );

    let go_empty = oracle.ok(json!({
        "op": "order_created", "product_id": go_product.id,
        "options": go_line.options, "quantity": 1,
        "order_id": go_order_two.id, "item_id": item_id,
    }));
    let rs_empty = hooks
        .on_order_created(
            store.conn(),
            &OrderRef {
                id: rs_order_two.id.clone(),
                number: rs_order_two.number.clone(),
                status: "pending".to_string(),
            },
            std::slice::from_ref(&rs_line),
        )
        .await
        .expect_err("no stock");
    assert_eq!(
        rs_empty.to_string(),
        go_empty["error"].as_str().unwrap_or("")
    );

    // ---- Delete ---------------------------------------------------------
    let go_third_imei = imei(imei_base + 10_004);
    let rs_third_imei = imei(imei_base + 20_004);
    let go_third = oracle.ok(json!({
        "op": "create", "product_id": go_product.id, "config": "12 GB RAM + 256 GB",
        "color": "Midnight", "imei": go_third_imei,
    }));
    let rs_third = service
        .create_device(
            store.conn(),
            &azahar_plugin_phone::CreateDevicePayload {
                product_id: rs_product.id.clone(),
                config: "12 GB RAM + 256 GB".to_string(),
                color: "Midnight".to_string(),
                imei: rs_third_imei.clone(),
            },
        )
        .await
        .expect("third device");
    assert!(go_third["problem"].is_null());
    let go_third_id = go_third["device"]["id"]
        .as_str()
        .expect("go device id")
        .to_string();

    let go_delete = oracle.ok(json!({"op": "delete", "device_id": go_third_id}));
    service
        .delete_device(store.conn(), &rs_third.id)
        .await
        .expect("delete in stock");
    assert_eq!(go_delete["deleted"], json!(true));
    assert!(go_delete["problem"].is_null());
    assert_eq!(product_stock(store.conn(), &rs_product.id).await, 1);

    let sold_go_id = go_sold_devices["devices"][0]["id"]
        .as_str()
        .expect("sold go id")
        .to_string();
    let sold_rs_id = rs_sold_devices.devices[0].id.clone();
    let go_sold_delete = oracle.ok(json!({"op": "delete", "device_id": sold_go_id}));
    let rs_sold_delete = service
        .delete_device(store.conn(), &sold_rs_id)
        .await
        .expect_err("sold delete");
    assert_eq!(
        normalize_rust_problem(Some(&rs_sold_delete)),
        normalize_oracle_problem(&go_sold_delete["problem"])
    );

    let go_missing = oracle.ok(json!({"op": "delete", "device_id": "phn_itph_missing"}));
    let rs_missing = service
        .delete_device(store.conn(), "phn_itph_missing")
        .await
        .expect_err("missing delete");
    assert_eq!(
        normalize_rust_problem(Some(&rs_missing)),
        normalize_oracle_problem(&go_missing["problem"])
    );

    // ---- Enrichment -----------------------------------------------------
    let go_enrich = make_product(
        &store,
        &category,
        &format!("prd_{tag}_go_enrich"),
        &format!("{tag}-go-enrich"),
        PLUGIN_ID,
        0,
        attributes(),
    )
    .await;
    let rs_enrich = make_product(
        &store,
        &category,
        &format!("prd_{tag}_rs_enrich"),
        &format!("{tag}-rs-enrich"),
        PLUGIN_ID,
        0,
        attributes(),
    )
    .await;
    for (offset, product) in [(0u64, &go_enrich), (10_000u64, &rs_enrich)] {
        for index in 0..3u64 {
            insert_device(
                store.conn(),
                product,
                "12 GB RAM + 256 GB",
                "Midnight",
                &imei(imei_base + 30_000 + offset + index),
                "in_stock",
                &now,
                "",
            )
            .await;
        }
        insert_device(
            store.conn(),
            product,
            "16 GB RAM + 512 GB",
            "Blue",
            &imei(imei_base + 31_000 + offset),
            "in_stock",
            &now,
            "",
        )
        .await;
        insert_device(
            store.conn(),
            product,
            "12 GB RAM + 256 GB",
            "Midnight",
            &imei(imei_base + 32_000 + offset),
            "reserved",
            &now,
            "",
        )
        .await;
    }
    let go_enrichment = oracle.ok(json!({"op": "enrich", "product_id": go_enrich.id}));
    let rs_enrichment = product_enrichment(store.conn(), &rs_enrich.id)
        .await
        .expect("enrichment");
    assert_eq!(
        serde_json::to_value(&rs_enrichment).expect("enrichment json"),
        go_enrichment["enrichment"]
    );

    // ---- Cleanup --------------------------------------------------------
    let product_ids = [
        &reconcile_go.id,
        &reconcile_rs.id,
        &legacy_go.id,
        &legacy_rs.id,
        &go_product.id,
        &rs_product.id,
        &core_product.id,
        &go_enrich.id,
        &rs_enrich.id,
    ];
    for product_id in product_ids {
        store
            .conn()
            .execute(
                "DELETE FROM phone_devices WHERE product_id = ?",
                libsql::params![product_id.clone()],
            )
            .await
            .expect("cleanup devices");
        store
            .conn()
            .execute(
                "DELETE FROM products WHERE id = ?",
                libsql::params![product_id.clone()],
            )
            .await
            .expect("cleanup products");
    }
    for order in [&go_order_one, &rs_order_one, &go_order_two, &rs_order_two] {
        store
            .conn()
            .execute(
                "DELETE FROM orders WHERE id = ?",
                libsql::params![order.id.clone()],
            )
            .await
            .expect("cleanup orders");
    }
    store
        .conn()
        .execute(
            "DELETE FROM categories WHERE id = ?",
            libsql::params![category.id.clone()],
        )
        .await
        .expect("cleanup category");
}
