//! Differential tests against the Go `internal/store` product oracle.

mod common;

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use azahar_store::products::ProductFilter;
use azahar_store::{Category, Product, Store, parse_time};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const DEFAULT_DSN: &str = "http://127.0.0.1:8880";

fn dsn() -> String {
    std::env::var("AZAHAR_TEST_DATABASE_URL").unwrap_or_else(|_| DEFAULT_DSN.to_string())
}

struct Oracle {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    dsn: String,
}

impl Oracle {
    fn spawn() -> Self {
        let backend = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../backend");
        let mut child = Command::new("go")
            .args(["run", "./cmd/oracle-store-products"])
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
            dsn: dsn(),
        }
    }

    fn call(&mut self, request: Value) -> Value {
        {
            let stdin = self.stdin.as_mut().expect("oracle stdin open");
            serde_json::to_writer(&mut *stdin, &request).expect("encode oracle request");
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
            "Go oracle exited before answering; is Turso reachable at {}? \
             start it or set AZAHAR_TEST_DATABASE_URL",
            self.dsn
        );
        serde_json::from_str(&line)
            .unwrap_or_else(|error| panic!("decode oracle response {line:?}: {error}"))
    }

    fn ok(&mut self, request: Value) -> Value {
        let response = self.call(request);
        assert_eq!(
            response["ok"],
            json!(true),
            "oracle error: {}",
            response["error"]
        );
        response["value"].clone()
    }

    fn error(&mut self, request: Value) -> String {
        let response = self.call(request);
        assert_eq!(
            response["ok"],
            json!(false),
            "expected an oracle error, got {response}"
        );
        response["error"].as_str().expect("error text").to_string()
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        if let Some(mut stdin) = self.stdin.take() {
            let _ = serde_json::to_writer(&mut stdin, &json!({"op": "cleanup"}));
            let _ = stdin.write_all(b"\n");
            let _ = stdin.flush();
        }
        let _ = self.child.wait();
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct WireCategory {
    id: String,
    slug: String,
    name: String,
    description: String,
    sort_order: i32,
    plugin_id: String,
}

impl WireCategory {
    fn to_store(&self) -> Category {
        Category {
            id: self.id.clone(),
            slug: self.slug.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            sort_order: self.sort_order,
            plugin_id: self.plugin_id.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct WireProduct {
    id: String,
    slug: String,
    name: String,
    subtitle: String,
    description: String,
    price_cents: i64,
    compare_at_cents: Option<i64>,
    sale_price_cents: Option<i64>,
    sale_starts_at: String,
    sale_ends_at: String,
    currency: String,
    size: String,
    category_id: String,
    concerns: Vec<String>,
    ingredients: Vec<String>,
    how_to: String,
    image: String,
    gallery: Vec<String>,
    stock: i32,
    featured: bool,
    active: bool,
    plugin_id: String,
    attributes: BTreeMap<String, Vec<String>>,
    created_at: String,
    updated_at: String,
}

impl WireProduct {
    fn to_store(&self) -> Product {
        Product {
            id: self.id.clone(),
            slug: self.slug.clone(),
            name: self.name.clone(),
            subtitle: self.subtitle.clone(),
            description: self.description.clone(),
            price_cents: self.price_cents,
            compare_at_cents: self.compare_at_cents,
            sale_price_cents: self.sale_price_cents,
            sale_starts_at: parse_time(&self.sale_starts_at),
            sale_ends_at: parse_time(&self.sale_ends_at),
            currency: self.currency.clone(),
            size: self.size.clone(),
            category: Category {
                id: self.category_id.clone(),
                ..Category::default()
            },
            concerns: self.concerns.clone(),
            ingredients: self.ingredients.clone(),
            how_to: self.how_to.clone(),
            image: self.image.clone(),
            gallery: self.gallery.clone(),
            stock: self.stock,
            featured: self.featured,
            active: self.active,
            plugin_id: self.plugin_id.clone(),
            attributes: self.attributes.clone().into_iter().collect(),
            created_at: parse_time(&self.created_at),
            updated_at: parse_time(&self.updated_at),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Seed {
    prefix: String,
    slots: BTreeMap<String, SlotFixtures>,
}

#[derive(Debug, Deserialize)]
struct SlotFixtures {
    plugin_id: String,
    other_plugin_id: String,
    category_a: WireCategory,
    category_b: WireCategory,
    category_x: WireCategory,
    products: Vec<WireProduct>,
}

fn wire_filter(filter: &ProductFilter) -> Value {
    json!({
        "category": filter.category,
        "plugin_id": filter.plugin_id,
        "concern": filter.concern,
        "query": filter.query,
        "sort": filter.sort,
        "include_inactive": filter.include_inactive,
        "limit": filter.limit,
    })
}

fn product_filter(
    category: &str,
    plugin_id: &str,
    concern: &str,
    query: &str,
    sort: &str,
    include_inactive: bool,
    limit: i32,
) -> ProductFilter {
    ProductFilter {
        category: category.to_string(),
        plugin_id: plugin_id.to_string(),
        concern: concern.to_string(),
        query: query.to_string(),
        sort: sort.to_string(),
        include_inactive,
        limit,
    }
}

fn slot_map(seed: &Seed) -> Vec<(String, String)> {
    let g = &seed.slots["g"];
    let r = &seed.slots["r"];
    let mut pairs = Vec::new();
    let mut push = |from: &str, to: &str| pairs.push((from.to_string(), to.to_string()));

    push(&g.plugin_id, &r.plugin_id);
    push(&g.other_plugin_id, &r.other_plugin_id);
    for (from, to) in [
        (&g.category_a, &r.category_a),
        (&g.category_b, &r.category_b),
        (&g.category_x, &r.category_x),
    ] {
        push(&from.id, &to.id);
        push(&from.slug, &to.slug);
    }
    for (from, to) in g.products.iter().zip(r.products.iter()) {
        push(&from.id, &to.id);
        push(&from.slug, &to.slug);
    }
    pairs.sort_by_key(|pair| Reverse(pair.0.len()));
    pairs
}

fn normalize(value: &Value, map: &[(String, String)]) -> Value {
    match value {
        Value::String(text) => {
            let mut out = text.clone();
            for (from, to) in map {
                out = out.replace(from, to);
            }
            Value::String(out)
        }
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| normalize(item, map)).collect())
        }
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), normalize(value, map)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn compare(label: &str, go: &Value, rust: &Value, map: &[(String, String)]) {
    let go = normalize(go, map);
    assert_eq!(go, *rust, "{label}");
}

fn filter_slot_categories(value: &Value, prefix: &str, slot: &str) -> Value {
    let wanted = format!("-{slot}");
    let rows: Vec<Value> = value
        .as_array()
        .expect("category array")
        .iter()
        .filter(|row| {
            row["slug"]
                .as_str()
                .is_some_and(|slug| slug.starts_with(prefix) && slug.ends_with(&wanted))
        })
        .cloned()
        .collect();
    Value::Array(rows)
}

async fn check_list(
    oracle: &mut Oracle,
    store: &Store,
    map: &[(String, String)],
    label: &str,
    go_filter: ProductFilter,
    rust_filter: ProductFilter,
) -> usize {
    let go = oracle.ok(json!({"op": "list_products", "filter": wire_filter(&go_filter)}));
    let rust = common::products_json(
        &store
            .list_products(&rust_filter)
            .await
            .expect("list products"),
    );
    compare(label, &go, &rust, map);
    1
}

#[tokio::test]
async fn products_match_go() {
    let mut oracle = Oracle::spawn();
    let seed_value = oracle.ok(json!({"op": "seed"}));
    let seed: Seed = serde_json::from_value(seed_value).expect("seed value");
    let g = &seed.slots["g"];
    let r = &seed.slots["r"];
    let mut map = slot_map(&seed);

    let database = azahar_db::open(&dsn()).await.unwrap_or_else(|error| {
        panic!(
            "Turso is unreachable at {} ({error}); start the test database or set \
                 AZAHAR_TEST_DATABASE_URL",
            dsn()
        )
    });
    let store = Store::new(database).expect("store");
    let mut cases = 0usize;

    cases += check_list(
        &mut oracle,
        &store,
        &map,
        "default active order",
        product_filter(&g.category_a.slug, "", "", "", "", false, 0),
        product_filter(&r.category_a.slug, "", "", "", "", false, 0),
    )
    .await;

    cases += check_list(
        &mut oracle,
        &store,
        &map,
        "include inactive",
        product_filter(&g.category_a.slug, "", "", "", "", true, 0),
        product_filter(&r.category_a.slug, "", "", "", "", true, 0),
    )
    .await;

    cases += check_list(
        &mut oracle,
        &store,
        &map,
        "inactive shelf hidden",
        product_filter(&g.category_b.slug, "", "", "", "", false, 0),
        product_filter(&r.category_b.slug, "", "", "", "", false, 0),
    )
    .await;

    cases += check_list(
        &mut oracle,
        &store,
        &map,
        "inactive shelf visible to admin",
        product_filter(&g.category_b.slug, "", "", "", "", true, 0),
        product_filter(&r.category_b.slug, "", "", "", "", true, 0),
    )
    .await;

    cases += check_list(
        &mut oracle,
        &store,
        &map,
        "plugin filter with inactive",
        product_filter("", &g.plugin_id, "", "", "", true, 0),
        product_filter("", &r.plugin_id, "", "", "", true, 0),
    )
    .await;

    cases += check_list(
        &mut oracle,
        &store,
        &map,
        "plugin filter new sort",
        product_filter("", &g.plugin_id, "", "", "new", true, 0),
        product_filter("", &r.plugin_id, "", "", "new", true, 0),
    )
    .await;

    for (label, query) in [
        ("search name", "Oil"),
        ("search subtitle", "clay"),
        ("search description", "verbena"),
        ("search miss", "zzz"),
    ] {
        cases += check_list(
            &mut oracle,
            &store,
            &map,
            label,
            product_filter("", &g.plugin_id, "", query, "", false, 0),
            product_filter("", &r.plugin_id, "", query, "", false, 0),
        )
        .await;
    }

    cases += check_list(
        &mut oracle,
        &store,
        &map,
        "concern from concerns and attributes",
        product_filter("", &g.plugin_id, "dry", "", "", false, 0),
        product_filter("", &r.plugin_id, "dry", "", "", false, 0),
    )
    .await;

    for sort in ["price_asc", "price_desc", "new", "bogus"] {
        cases += check_list(
            &mut oracle,
            &store,
            &map,
            &format!("sort {sort}"),
            product_filter("", &g.plugin_id, "", "", sort, false, 0),
            product_filter("", &r.plugin_id, "", "", sort, false, 0),
        )
        .await;
    }

    cases += check_list(
        &mut oracle,
        &store,
        &map,
        "limit 2 newest",
        product_filter("", &g.plugin_id, "", "", "new", false, 2),
        product_filter("", &r.plugin_id, "", "", "new", false, 2),
    )
    .await;

    cases += check_list(
        &mut oracle,
        &store,
        &map,
        "limit 1 cheapest",
        product_filter("", &g.plugin_id, "", "", "price_asc", false, 1),
        product_filter("", &r.plugin_id, "", "", "price_asc", false, 1),
    )
    .await;

    cases += check_list(
        &mut oracle,
        &store,
        &map,
        "non-positive limit defaults",
        product_filter("", &g.plugin_id, "", "", "new", false, -1),
        product_filter("", &r.plugin_id, "", "", "new", false, -1),
    )
    .await;

    cases += check_list(
        &mut oracle,
        &store,
        &map,
        "oversized limit defaults",
        product_filter("", &g.plugin_id, "", "", "new", false, 501),
        product_filter("", &r.plugin_id, "", "", "new", false, 501),
    )
    .await;

    for (label, go_slug, rust_slug, active_only) in [
        (
            "by slug active",
            g.products[0].slug.as_str(),
            r.products[0].slug.as_str(),
            true,
        ),
        (
            "by slug inactive admin",
            g.products[3].slug.as_str(),
            r.products[3].slug.as_str(),
            false,
        ),
    ] {
        let go = oracle.ok(json!({
            "op": "product_by_slug", "slug": go_slug, "active_only": active_only
        }));
        let rust = common::product_json(
            &store
                .product_by_slug(rust_slug, active_only)
                .await
                .expect("product by slug"),
        );
        compare(label, &go, &rust, &map);
        cases += 1;
    }

    let missing = oracle.call(json!({
        "op": "product_by_slug", "slug": g.products[3].slug, "active_only": true
    }));
    assert_eq!(missing["error"], json!("not_found"));
    assert!(
        store
            .product_by_slug(&r.products[3].slug, true)
            .await
            .expect_err("inactive slug")
            .is_not_found()
    );
    cases += 1;

    for (label, go_slug, rust_slug, active_only) in [
        (
            "by id active",
            g.products[0].id.as_str(),
            r.products[0].id.as_str(),
            true,
        ),
        (
            "by id inactive admin",
            g.products[4].id.as_str(),
            r.products[4].id.as_str(),
            false,
        ),
    ] {
        let go = oracle.ok(json!({
            "op": "product_by_id", "id": go_slug, "active_only": active_only
        }));
        let rust = common::product_json(
            &store
                .product_by_id(rust_slug, active_only)
                .await
                .expect("product by id"),
        );
        compare(label, &go, &rust, &map);
        cases += 1;
    }

    let missing = oracle.call(json!({
        "op": "product_by_id", "id": g.products[4].id, "active_only": true
    }));
    assert_eq!(missing["error"], json!("not_found"));
    assert!(
        store
            .product_by_id(&r.products[4].id, true)
            .await
            .expect_err("inactive id")
            .is_not_found()
    );
    cases += 1;

    let go = oracle.ok(json!({"op": "category_by_slug", "slug": g.category_a.slug}));
    let rust = common::category_json(
        &store
            .category_by_slug(&r.category_a.slug)
            .await
            .expect("category"),
    );
    compare("category by slug", &go, &rust, &map);
    cases += 1;

    let missing = oracle.call(json!({"op": "category_by_slug", "slug": "missing-category"}));
    assert_eq!(missing["error"], json!("not_found"));
    assert!(
        store
            .category_by_slug("missing-category")
            .await
            .expect_err("missing category")
            .is_not_found()
    );
    cases += 1;

    let go = filter_slot_categories(&oracle.ok(json!({"op": "categories"})), &seed.prefix, "g");
    let rust = filter_slot_categories(
        &common::categories_json(&store.categories().await.expect("categories")),
        &seed.prefix,
        "r",
    );
    compare("categories", &go, &rust, &map);
    cases += 1;

    let go = filter_slot_categories(
        &oracle.ok(json!({"op": "public_categories"})),
        &seed.prefix,
        "g",
    );
    let rust = filter_slot_categories(
        &common::categories_json(&store.public_categories().await.expect("public categories")),
        &seed.prefix,
        "r",
    );
    compare("public categories", &go, &rust, &map);
    cases += 1;

    for (label, exclude, limit) in [
        ("related default", 0, 0),
        ("related one", 0, 1),
        ("related ten", 5, 10),
    ] {
        let go = oracle.ok(json!({
            "op": "related_products",
            "category_id": g.category_a.id,
            "exclude_id": g.products[exclude].id,
            "limit": limit,
        }));
        let rust = common::products_json(
            &store
                .related_products(&r.category_a.id, &r.products[exclude].id, limit)
                .await
                .expect("related"),
        );
        compare(label, &go, &rust, &map);
        cases += 1;
    }

    let go = oracle.ok(json!({
        "op": "related_products",
        "category_id": g.category_b.id,
        "exclude_id": g.products[4].id,
        "limit": 4,
    }));
    let rust = common::products_json(
        &store
            .related_products(&r.category_b.id, &r.products[4].id, 4)
            .await
            .expect("related"),
    );
    compare("related inactive shelf", &go, &rust, &map);
    cases += 1;

    let go_count = oracle.ok(json!({"op": "count_products"}));
    let rust_count = store.count_products().await.expect("count");
    assert_eq!(go_count, json!(rust_count));
    cases += 1;

    let create_g = WireProduct {
        id: format!("{}_new_g", seed.prefix),
        slug: format!("{}-new-g", seed.prefix),
        name: "Created Oil".to_string(),
        subtitle: "Created".to_string(),
        description: "Created by the differential test.".to_string(),
        price_cents: 140_000,
        compare_at_cents: Some(160_000),
        sale_price_cents: None,
        sale_starts_at: String::new(),
        sale_ends_at: String::new(),
        currency: String::new(),
        size: "40 ml".to_string(),
        category_id: g.category_a.id.clone(),
        concerns: vec!["dry".to_string()],
        ingredients: vec!["Argan".to_string()],
        how_to: "Use daily.".to_string(),
        image: "/images/new.webp".to_string(),
        gallery: vec![
            "/images/new.webp".to_string(),
            "/images/new2.webp".to_string(),
        ],
        stock: 8,
        featured: true,
        active: true,
        plugin_id: String::new(),
        attributes: BTreeMap::from([(
            "color".to_string(),
            vec!["gold".to_string(), "amber".to_string()],
        )]),
        created_at: "2026-02-01T00:00:00Z".to_string(),
        updated_at: "2026-02-01T00:00:01Z".to_string(),
    };
    let create_r = WireProduct {
        id: format!("{}_new_r", seed.prefix),
        slug: format!("{}-new-r", seed.prefix),
        category_id: r.category_a.id.clone(),
        ..create_g.clone()
    };
    map.push((create_g.id.clone(), create_r.id.clone()));
    map.push((create_g.slug.clone(), create_r.slug.clone()));
    map.sort_by_key(|pair| Reverse(pair.0.len()));

    let go_created = oracle.ok(json!({"op": "create_product", "product": create_g}));
    store
        .create_product(store.conn(), &create_r.to_store())
        .await
        .expect("create product");
    let rust_created = common::product_json(
        &store
            .product_by_id(&create_r.id, false)
            .await
            .expect("created product"),
    );
    compare("create product", &go_created, &rust_created, &map);
    cases += 1;
    let go_read = oracle.ok(json!({"op": "product_by_id", "id": create_r.id}));
    assert_eq!(go_read, rust_created, "Go read of the Rust-created product");
    cases += 1;

    let mut update_g = g.products[1].clone();
    update_g.name = "Updated Night Oil".to_string();
    update_g.subtitle = "Repaired".to_string();
    update_g.description = "Updated description.".to_string();
    update_g.price_cents = 210_000;
    update_g.compare_at_cents = Some(220_000);
    update_g.currency = String::new();
    update_g.size = "25 ml".to_string();
    update_g.concerns = vec!["calm".to_string()];
    update_g.ingredients = vec!["Rose".to_string()];
    update_g.how_to = "Pat.".to_string();
    update_g.image = "/images/upd.webp".to_string();
    update_g.gallery = vec!["/images/upd.webp".to_string()];
    update_g.stock = 9;
    update_g.featured = true;
    update_g.active = true;
    update_g.plugin_id = String::new();
    update_g.attributes = BTreeMap::from([("finish".to_string(), vec!["dewy".to_string()])]);
    update_g.updated_at = "2026-02-02T00:00:00Z".to_string();
    let update_r = WireProduct {
        id: r.products[1].id.clone(),
        slug: r.products[1].slug.clone(),
        category_id: r.category_a.id.clone(),
        ..update_g.clone()
    };

    let go_updated = oracle.ok(json!({"op": "update_product", "product": update_g}));
    store
        .update_product(store.conn(), &update_r.to_store())
        .await
        .expect("update product");
    let rust_updated = common::product_json(
        &store
            .product_by_id(&update_r.id, false)
            .await
            .expect("updated product"),
    );
    compare("update product", &go_updated, &rust_updated, &map);
    cases += 1;
    let go_read = oracle.ok(json!({"op": "product_by_id", "id": update_r.id}));
    assert_eq!(go_read, rust_updated, "Go read of the Rust-updated product");
    cases += 1;

    let now = "2026-03-02T03:04:05Z";
    let go_archived = oracle.ok(json!({
        "op": "set_active", "id": g.products[0].id, "active": false, "now": now
    }));
    store
        .set_product_active(
            store.conn(),
            &r.products[0].id,
            false,
            parse_time(now).expect("now"),
        )
        .await
        .expect("archive");
    let rust_archived = common::product_json(
        &store
            .product_by_id(&r.products[0].id, false)
            .await
            .expect("archived product"),
    );
    compare("archive product", &go_archived, &rust_archived, &map);
    cases += 1;
    let go_read = oracle.ok(json!({"op": "product_by_id", "id": r.products[0].id}));
    assert_eq!(
        go_read, rust_archived,
        "Go read of the Rust-archived product"
    );
    cases += 1;

    for (quantity, label) in [
        (2, "decrement 2"),
        (2, "decrement to one"),
        (2, "decrement short"),
        (1, "decrement to zero"),
        (1, "decrement empty"),
    ] {
        let go = oracle.ok(json!({
            "op": "decrement_stock", "id": g.products[0].id, "quantity": quantity
        }));
        let sufficient = store
            .decrement_stock(store.conn(), &r.products[0].id, quantity)
            .await
            .expect("decrement");
        let stock = store
            .product_by_id(&r.products[0].id, false)
            .await
            .expect("stock read")
            .stock;
        assert_eq!(
            go,
            json!({"sufficient": sufficient, "stock": stock}),
            "{label}"
        );
        cases += 1;
    }

    for (quantity, label) in [
        (4, "increment 4"),
        (0, "increment zero"),
        (-2, "increment negative"),
    ] {
        let go = oracle.ok(json!({
            "op": "increment_stock", "id": g.products[0].id, "quantity": quantity
        }));
        store
            .increment_stock(store.conn(), &r.products[0].id, quantity)
            .await
            .expect("increment");
        let stock = store
            .product_by_id(&r.products[0].id, false)
            .await
            .expect("stock read")
            .stock;
        assert_eq!(go, json!({"sufficient": true, "stock": stock}), "{label}");
        cases += 1;
    }

    let missing_id_g = format!("{}_missing_g", seed.prefix);
    let missing_id_r = format!("{}_missing_r", seed.prefix);
    let missing = oracle.call(json!({
        "op": "increment_stock", "id": missing_id_g, "quantity": 3
    }));
    assert_eq!(missing["error"], json!("not_found"));
    store
        .increment_stock(store.conn(), &missing_id_r, 3)
        .await
        .expect("increment missing product");
    assert!(
        store
            .product_by_id(&missing_id_r, false)
            .await
            .expect_err("missing product")
            .is_not_found()
    );
    cases += 1;

    let go_read = oracle.ok(json!({"op": "product_by_id", "id": r.products[0].id}));
    let rust_read = common::product_json(
        &store
            .product_by_id(&r.products[0].id, false)
            .await
            .expect("stock read"),
    );
    assert_eq!(go_read, rust_read, "Go read of the Rust stock rows");
    cases += 1;

    let upsert_g = WireCategory {
        id: String::new(),
        slug: format!("{}-upsert-g", seed.prefix),
        name: "Upserted Shelf".to_string(),
        description: "Upsert once.".to_string(),
        sort_order: 40,
        plugin_id: format!("{}_up_g", seed.prefix),
    };
    let upsert_r = WireCategory {
        slug: format!("{}-upsert-r", seed.prefix),
        plugin_id: format!("{}_up_r", seed.prefix),
        ..upsert_g.clone()
    };
    map.push((upsert_g.plugin_id.clone(), upsert_r.plugin_id.clone()));
    map.push((upsert_g.slug.clone(), upsert_r.slug.clone()));
    map.push((
        upsert_g.slug.replace('-', "_"),
        upsert_r.slug.replace('-', "_"),
    ));
    map.sort_by_key(|pair| Reverse(pair.0.len()));

    let go_upsert = oracle.ok(json!({
        "op": "upsert_category", "plugin_id": &upsert_g.plugin_id, "category": &upsert_g
    }));
    store
        .upsert_category(&upsert_r.plugin_id, &upsert_r.to_store())
        .await
        .expect("upsert category");
    let rust_upsert = common::category_json(
        &store
            .category_by_slug(&upsert_r.slug)
            .await
            .expect("upserted category"),
    );
    compare("upsert category", &go_upsert, &rust_upsert, &map);
    cases += 1;

    let changed_g = WireCategory {
        name: "Upserted Again".to_string(),
        description: "Upsert twice.".to_string(),
        sort_order: 41,
        ..upsert_g.clone()
    };
    let changed_r = WireCategory {
        slug: upsert_r.slug.clone(),
        plugin_id: upsert_r.plugin_id.clone(),
        ..changed_g.clone()
    };
    let go_changed = oracle.ok(json!({
        "op": "upsert_category", "plugin_id": &changed_g.plugin_id, "category": &changed_g
    }));
    store
        .upsert_category(&changed_r.plugin_id, &changed_r.to_store())
        .await
        .expect("update category");
    let rust_changed = common::category_json(
        &store
            .category_by_slug(&changed_r.slug)
            .await
            .expect("updated category"),
    );
    compare("upsert category again", &go_changed, &rust_changed, &map);
    cases += 1;

    let conflict_g = WireCategory {
        slug: g.category_a.slug.clone(),
        name: "Stolen".to_string(),
        description: String::new(),
        sort_order: 0,
        plugin_id: format!("{}_up2_g", seed.prefix),
        id: String::new(),
    };
    let conflict_r = WireCategory {
        slug: r.category_a.slug.clone(),
        plugin_id: format!("{}_up2_r", seed.prefix),
        ..conflict_g.clone()
    };
    map.push((
        format!("{}_up2_g", seed.prefix),
        format!("{}_up2_r", seed.prefix),
    ));
    map.sort_by_key(|pair| Reverse(pair.0.len()));

    let go_error = oracle.error(json!({
        "op": "upsert_category", "plugin_id": &conflict_g.plugin_id, "category": &conflict_g
    }));
    let rust_error = store
        .upsert_category(&conflict_r.plugin_id, &conflict_r.to_store())
        .await
        .expect_err("conflicting category")
        .to_string();
    assert_eq!(
        normalize(&Value::String(go_error), &map),
        Value::String(rust_error),
        "upsert ownership error"
    );
    cases += 1;

    let _go_mismatch = oracle.ok(json!({"op": "seed_mismatch", "slot": "g"}));
    oracle.ok(json!({"op": "reconcile"}));
    map.push((
        format!("{}_rec_g", seed.prefix),
        format!("{}_rec_r", seed.prefix),
    ));
    map.push((
        format!("{}-rec-g", seed.prefix),
        format!("{}-rec-r", seed.prefix),
    ));
    map.sort_by_key(|pair| Reverse(pair.0.len()));
    oracle.ok(json!({"op": "seed_mismatch", "slot": "r"}));
    store
        .reconcile_product_categories()
        .await
        .expect("reconcile");

    let go_reconciled = oracle.ok(json!({
        "op": "product_by_id", "id": format!("{}_rec_g", seed.prefix)
    }));
    let rust_reconciled = common::product_json(
        &store
            .product_by_id(&format!("{}_rec_r", seed.prefix), false)
            .await
            .expect("reconciled product"),
    );
    compare("reconcile product", &go_reconciled, &rust_reconciled, &map);
    cases += 1;
    assert_eq!(go_reconciled["category"]["id"], json!(&g.category_a.id));
    assert_eq!(rust_reconciled["category"]["id"], json!(&r.category_a.id));
    let go_read = oracle.ok(json!({
        "op": "product_by_id", "id": format!("{}_rec_r", seed.prefix)
    }));
    assert_eq!(
        go_read, rust_reconciled,
        "Go read of the Rust-reconciled product"
    );
    cases += 1;

    assert_eq!(oracle.ok(json!({"op": "cleanup"})), json!(true));
    let removed = oracle.call(json!({
        "op": "product_by_id", "id": g.products[0].id
    }));
    assert_eq!(removed["error"], json!("not_found"), "fixtures cleaned up");
    assert!(
        store
            .product_by_id(&r.products[0].id, false)
            .await
            .expect_err("cleaned product")
            .is_not_found()
    );
    cases += 1;

    println!("products differential: {cases} comparisons against Go");
}
