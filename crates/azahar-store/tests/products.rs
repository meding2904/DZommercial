//! Product, category and stock behaviour against an in-memory libSQL
//! database migrated with the shared schema.

mod common;

use std::collections::HashMap;

use azahar_store::products::ProductFilter;
use azahar_store::{Category, Product, Store};
use chrono::{DateTime, Duration, TimeZone, Utc};
use libsql::params;

fn at(month: u32, day: u32, hour: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, month, day, hour, 0, 0).unwrap()
}

async fn setup() -> Store {
    let database = libsql::Builder::new_local(":memory:")
        .build()
        .await
        .expect("open in-memory database");
    let store = Store::new(database).expect("store");
    azahar_db::migrate(store.conn()).await.expect("migrate");
    store
}

async fn insert_category(
    store: &Store,
    id: &str,
    slug: &str,
    name: &str,
    sort_order: i32,
    plugin_id: &str,
) -> Category {
    store
        .conn()
        .execute(
            "INSERT INTO categories (id, slug, name, description, sort_order, plugin_id)
             VALUES (?, ?, ?, ?, ?, ?)",
            params![
                id,
                slug,
                name,
                format!("{name} description."),
                sort_order,
                plugin_id
            ],
        )
        .await
        .expect("insert category");
    Category {
        id: id.to_string(),
        slug: slug.to_string(),
        name: name.to_string(),
        description: format!("{name} description."),
        sort_order,
        plugin_id: plugin_id.to_string(),
    }
}

struct Fixture {
    category_a: Category,
    category_b: Category,
    category_x: Category,
    products: Vec<Product>,
}

fn base_product(category: &Category, number: i32, created: DateTime<Utc>) -> Product {
    Product {
        id: format!("prd_{number}"),
        slug: format!("prd-{number}"),
        name: format!("Product {number}"),
        category: category.clone(),
        price_cents: 10_000,
        stock: 1,
        active: true,
        currency: "DZD".to_string(),
        created_at: Some(created),
        updated_at: Some(created),
        ..Product::default()
    }
}

async fn seed(store: &Store) -> Fixture {
    let category_a = insert_category(store, "cat_a", "cat-a", "Alpha Shelf", 10, "plug").await;
    let category_b = insert_category(store, "cat_b", "cat-b", "Beta Shelf", 20, "plug").await;
    let category_x = insert_category(store, "cat_x", "cat-x", "Foreign Shelf", 30, "other").await;

    let now = Utc::now();
    let base = at(1, 1, 0);

    let mut p1 = base_product(&category_a, 1, base + Duration::seconds(1));
    p1.name = "Argan Day Oil".to_string();
    p1.subtitle = "Bright".to_string();
    p1.description = "Light everyday oil.".to_string();
    p1.price_cents = 100_000;
    p1.compare_at_cents = Some(120_000);
    p1.size = "30 ml".to_string();
    p1.concerns = vec!["dry".to_string(), "mature".to_string()];
    p1.ingredients = vec!["Argan".to_string()];
    p1.how_to = "Massage.".to_string();
    p1.image = "/images/a.webp".to_string();
    p1.gallery = vec!["/images/a.webp".to_string(), "/images/a2.webp".to_string()];
    p1.stock = 5;
    p1.featured = true;
    p1.plugin_id = "plug".to_string();
    p1.attributes = HashMap::from([
        ("size".to_string(), vec!["30 ml".to_string()]),
        ("concerns".to_string(), vec!["dry".to_string()]),
    ]);

    let mut p2 = base_product(&category_a, 2, base + Duration::seconds(2));
    p2.name = "Rose Night Oil".to_string();
    p2.subtitle = "Calm".to_string();
    p2.description = "Night repair oil.".to_string();
    p2.price_cents = 200_000;
    p2.sale_price_cents = Some(95_000);
    p2.sale_starts_at = Some(now - Duration::hours(1));
    p2.sale_ends_at = Some(now + Duration::hours(1));
    p2.size = "20 ml".to_string();
    p2.concerns = vec!["sensitive".to_string()];
    p2.stock = 2;
    p2.plugin_id = "plug".to_string();

    let mut p3 = base_product(&category_a, 3, base + Duration::seconds(3));
    p3.name = "Clay Mask".to_string();
    p3.subtitle = "Purifying clay mask".to_string();
    p3.description = "Weekly clay ritual.".to_string();
    p3.price_cents = 50_000;
    p3.sale_price_cents = Some(40_000);
    p3.sale_starts_at = Some(now - Duration::hours(48));
    p3.sale_ends_at = Some(now - Duration::hours(24));
    p3.size = "100 g".to_string();
    p3.stock = 0;
    p3.plugin_id = "plug".to_string();

    let mut p4 = base_product(&category_a, 4, base + Duration::seconds(4));
    p4.name = "Future Serum".to_string();
    p4.subtitle = "Soon".to_string();
    p4.description = "Not on sale yet.".to_string();
    p4.price_cents = 300_000;
    p4.sale_price_cents = Some(250_000);
    p4.sale_starts_at = Some(now + Duration::hours(24));
    p4.sale_ends_at = Some(now + Duration::hours(48));
    p4.size = "15 ml".to_string();
    p4.stock = 7;
    p4.featured = true;
    p4.active = false;
    p4.plugin_id = "plug".to_string();

    let mut p5 = base_product(&category_b, 5, base + Duration::seconds(5));
    p5.name = "Hidden Balm".to_string();
    p5.subtitle = "Store room".to_string();
    p5.description = "Kept out of the storefront.".to_string();
    p5.price_cents = 120_000;
    p5.size = "50 ml".to_string();
    p5.stock = 3;
    p5.active = false;
    p5.plugin_id = "plug".to_string();

    let mut p6 = base_product(&category_a, 6, base + Duration::seconds(6));
    p6.name = "Citrus Mist".to_string();
    p6.subtitle = "Fresh".to_string();
    p6.description = "A verbena finishing mist.".to_string();
    p6.price_cents = 90_000;
    p6.size = "60 ml".to_string();
    p6.stock = 10;
    p6.plugin_id = "plug".to_string();
    p6.attributes = HashMap::from([("concerns".to_string(), vec!["dry".to_string()])]);

    let products = vec![p1, p2, p3, p4, p5, p6];
    for product in &products {
        store
            .create_product(store.conn(), product)
            .await
            .expect("seed product");
    }

    Fixture {
        category_a,
        category_b,
        category_x,
        products,
    }
}

fn numbers(products: &[Product]) -> Vec<i32> {
    products
        .iter()
        .map(|product| {
            product
                .id
                .trim_start_matches("prd_")
                .parse()
                .expect("numeric id")
        })
        .collect()
}

async fn list(store: &Store, filter: ProductFilter) -> Vec<Product> {
    store.list_products(&filter).await.expect("list products")
}

fn filter(category: &str, plugin_id: &str) -> ProductFilter {
    ProductFilter {
        category: category.to_string(),
        plugin_id: plugin_id.to_string(),
        ..ProductFilter::default()
    }
}

#[tokio::test]
async fn list_products_applies_filters_and_order() {
    let store = setup().await;
    seed(&store).await;

    let active = list(&store, filter("cat-a", "")).await;
    assert_eq!(numbers(&active), vec![1, 6, 3, 2]);

    let mut all = filter("cat-a", "");
    all.include_inactive = true;
    assert_eq!(numbers(&list(&store, all).await), vec![4, 1, 6, 3, 2]);

    assert!(list(&store, filter("cat-b", "")).await.is_empty());
    let mut hidden = filter("cat-b", "");
    hidden.include_inactive = true;
    assert_eq!(numbers(&list(&store, hidden).await), vec![5]);

    let mut plugin_all = filter("", "plug");
    plugin_all.include_inactive = true;
    assert_eq!(
        numbers(&list(&store, plugin_all).await),
        vec![4, 1, 6, 5, 3, 2]
    );

    let mut plugin_new = filter("", "plug");
    plugin_new.include_inactive = true;
    plugin_new.sort = "new".to_string();
    assert_eq!(
        numbers(&list(&store, plugin_new).await),
        vec![6, 5, 4, 3, 2, 1]
    );

    let search = |query: &str| ProductFilter {
        plugin_id: "plug".to_string(),
        query: query.to_string(),
        ..ProductFilter::default()
    };
    assert_eq!(numbers(&list(&store, search("Oil")).await), vec![1, 2]);
    assert_eq!(numbers(&list(&store, search("clay")).await), vec![3]);
    assert_eq!(numbers(&list(&store, search("verbena")).await), vec![6]);
    assert!(list(&store, search("zzz")).await.is_empty());

    let concern = ProductFilter {
        plugin_id: "plug".to_string(),
        concern: "dry".to_string(),
        ..ProductFilter::default()
    };
    assert_eq!(numbers(&list(&store, concern).await), vec![1, 6]);

    let sorted = |sort: &str| ProductFilter {
        plugin_id: "plug".to_string(),
        sort: sort.to_string(),
        ..ProductFilter::default()
    };
    assert_eq!(
        numbers(&list(&store, sorted("price_asc")).await),
        vec![3, 6, 2, 1]
    );
    assert_eq!(
        numbers(&list(&store, sorted("price_desc")).await),
        vec![1, 2, 6, 3]
    );
    assert_eq!(
        numbers(&list(&store, sorted("new")).await),
        vec![6, 3, 2, 1]
    );
    assert_eq!(
        numbers(&list(&store, sorted("bogus")).await),
        vec![1, 6, 3, 2]
    );

    let limited = ProductFilter {
        plugin_id: "plug".to_string(),
        sort: "new".to_string(),
        limit: 2,
        ..ProductFilter::default()
    };
    assert_eq!(numbers(&list(&store, limited).await), vec![6, 3]);

    let one = ProductFilter {
        plugin_id: "plug".to_string(),
        sort: "price_asc".to_string(),
        limit: 1,
        ..ProductFilter::default()
    };
    assert_eq!(numbers(&list(&store, one).await), vec![3]);

    let zero = ProductFilter {
        plugin_id: "plug".to_string(),
        sort: "new".to_string(),
        limit: 0,
        ..ProductFilter::default()
    };
    assert_eq!(numbers(&list(&store, zero).await), vec![6, 3, 2, 1]);

    let oversized = ProductFilter {
        plugin_id: "plug".to_string(),
        sort: "new".to_string(),
        limit: 501,
        ..ProductFilter::default()
    };
    assert_eq!(numbers(&list(&store, oversized).await), vec![6, 3, 2, 1]);
}

#[tokio::test]
async fn product_and_category_lookups_respect_active() {
    let store = setup().await;
    let fixture = seed(&store).await;

    let product = store
        .product_by_slug("prd-1", true)
        .await
        .expect("active product");
    assert_eq!(product.id, "prd_1");
    assert_eq!(product.category.id, fixture.category_a.id);
    assert_eq!(product.category.sort_order, 0);

    assert!(
        store
            .product_by_slug("prd-4", true)
            .await
            .is_err_and(|err| err.is_not_found())
    );
    assert!(store.product_by_slug("prd-4", false).await.is_ok());

    assert!(store.product_by_id("prd_1", true).await.is_ok());
    assert!(
        store
            .product_by_id("prd_4", true)
            .await
            .is_err_and(|err| err.is_not_found())
    );
    assert!(
        store
            .product_by_id("missing", false)
            .await
            .is_err_and(|err| err.is_not_found())
    );
    assert!(
        store
            .product_by_slug("missing", false)
            .await
            .is_err_and(|err| err.is_not_found())
    );

    let category = store.category_by_slug("cat-a").await.expect("category");
    assert_eq!(category.id, "cat_a");
    assert_eq!(category.name, "Alpha Shelf");
    assert_eq!(category.description, "Alpha Shelf description.");
    assert_eq!(category.sort_order, 10);
    assert_eq!(category.plugin_id, "plug");
    assert!(
        store
            .category_by_slug("missing")
            .await
            .is_err_and(|err| err.is_not_found())
    );
}

#[tokio::test]
async fn categories_and_public_categories_omit_empty_shelves() {
    let store = setup().await;
    let fixture = seed(&store).await;

    let categories = store.categories().await.expect("categories");
    let ids: Vec<&str> = categories.iter().map(|row| row.id.as_str()).collect();
    assert_eq!(ids, vec!["cat_x", "cat_a", "cat_b"]);

    let public = store.public_categories().await.expect("public categories");
    assert_eq!(public.len(), 1);
    assert_eq!(public[0].id, fixture.category_a.id);

    let mut empty = fixture.category_b.clone();
    let mut inactive = fixture.products[4].clone();
    inactive.id = "prd_only".to_string();
    inactive.slug = "prd-only".to_string();
    store
        .create_product(store.conn(), &inactive)
        .await
        .expect("inactive product");
    empty.id = "unused".to_string();
    empty.slug = "unused".to_string();
    store
        .upsert_category("plug", &empty)
        .await
        .expect("unused category");
    assert_eq!(store.public_categories().await.expect("public").len(), 1);
}

#[tokio::test]
async fn upsert_category_keeps_plugin_ownership() {
    let store = setup().await;
    seed(&store).await;

    let category = Category {
        id: String::new(),
        slug: "fresh-shelf".to_string(),
        name: "Fresh".to_string(),
        description: "Fresh shelf.".to_string(),
        sort_order: 40,
        plugin_id: String::new(),
    };
    store
        .upsert_category("plug", &category)
        .await
        .expect("insert category");

    let inserted = store
        .category_by_slug("fresh-shelf")
        .await
        .expect("category");
    assert_eq!(inserted.id, "cat_plug_fresh_shelf");
    assert_eq!(inserted.name, "Fresh");
    assert_eq!(inserted.description, "Fresh shelf.");
    assert_eq!(inserted.sort_order, 40);
    assert_eq!(inserted.plugin_id, "plug");

    let changed = Category {
        name: "Fresher".to_string(),
        description: "Updated shelf.".to_string(),
        sort_order: 41,
        ..category.clone()
    };
    store
        .upsert_category("plug", &changed)
        .await
        .expect("update category");
    let updated = store
        .category_by_slug("fresh-shelf")
        .await
        .expect("category");
    assert_eq!(updated.id, "cat_plug_fresh_shelf");
    assert_eq!(updated.name, "Fresher");
    assert_eq!(updated.description, "Updated shelf.");
    assert_eq!(updated.sort_order, 41);

    let error = store
        .upsert_category("other", &changed)
        .await
        .expect_err("foreign plugin must fail");
    assert_eq!(
        error.to_string(),
        "category \"fresh-shelf\" belongs to another plugin"
    );
    let untouched = store
        .category_by_slug("fresh-shelf")
        .await
        .expect("category");
    assert_eq!(untouched.name, "Fresher");
    assert_eq!(untouched.plugin_id, "plug");

    let dashed = Category {
        id: String::new(),
        slug: "two-dash-shelf".to_string(),
        name: "Dashed".to_string(),
        description: String::new(),
        sort_order: 0,
        plugin_id: String::new(),
    };
    store
        .upsert_category("plug", &dashed)
        .await
        .expect("dashed category");
    assert_eq!(
        store
            .category_by_slug("two-dash-shelf")
            .await
            .expect("category")
            .id,
        "cat_plug_two_dash_shelf"
    );
}

#[tokio::test]
async fn reconcile_moves_only_foreign_categories() {
    let store = setup().await;
    let fixture = seed(&store).await;

    let mut mismatch = base_product(&fixture.category_x, 7, at(1, 1, 7));
    mismatch.plugin_id = "plug".to_string();
    store
        .create_product(store.conn(), &mismatch)
        .await
        .expect("mismatch product");

    let mut valid = base_product(&fixture.category_b, 8, at(1, 1, 8));
    valid.plugin_id = "plug".to_string();
    store
        .create_product(store.conn(), &valid)
        .await
        .expect("valid product");

    let mut loner = base_product(&fixture.category_a, 9, at(1, 1, 9));
    loner.plugin_id = "lonely".to_string();
    store
        .create_product(store.conn(), &loner)
        .await
        .expect("plugin without categories");

    store
        .reconcile_product_categories()
        .await
        .expect("reconcile");

    let moved = store.product_by_id("prd_7", false).await.expect("moved");
    assert_eq!(moved.category.id, fixture.category_a.id);
    assert_eq!(moved.category.slug, "cat-a");

    let kept = store.product_by_id("prd_8", false).await.expect("kept");
    assert_eq!(kept.category.id, fixture.category_b.id);

    let untouched = store.product_by_id("prd_9", false).await.expect("loner");
    assert_eq!(untouched.category.id, fixture.category_a.id);
}

#[tokio::test]
async fn stock_changes_stop_at_zero() {
    let store = setup().await;
    seed(&store).await;

    assert!(
        store
            .decrement_stock(store.conn(), "prd_1", 2)
            .await
            .expect("decrement")
    );
    assert_eq!(
        store
            .product_by_id("prd_1", false)
            .await
            .expect("product")
            .stock,
        3
    );

    assert!(
        store
            .decrement_stock(store.conn(), "prd_1", 3)
            .await
            .expect("decrement")
    );
    assert_eq!(
        store
            .product_by_id("prd_1", false)
            .await
            .expect("product")
            .stock,
        0
    );

    assert!(
        !store
            .decrement_stock(store.conn(), "prd_1", 1)
            .await
            .expect("short stock")
    );
    assert_eq!(
        store
            .product_by_id("prd_1", false)
            .await
            .expect("product")
            .stock,
        0
    );

    assert!(
        !store
            .decrement_stock(store.conn(), "missing", 1)
            .await
            .expect("missing product")
    );

    store
        .increment_stock(store.conn(), "prd_1", 4)
        .await
        .expect("increment");
    assert_eq!(
        store
            .product_by_id("prd_1", false)
            .await
            .expect("product")
            .stock,
        4
    );

    store
        .increment_stock(store.conn(), "prd_1", 0)
        .await
        .expect("zero increment");
    store
        .increment_stock(store.conn(), "prd_1", -2)
        .await
        .expect("negative increment");
    store
        .increment_stock(store.conn(), "missing", 1)
        .await
        .expect("missing product");
    assert_eq!(
        store
            .product_by_id("prd_1", false)
            .await
            .expect("product")
            .stock,
        4
    );
}

#[tokio::test]
async fn related_products_follow_category_and_stock_rules() {
    let store = setup().await;
    let fixture = seed(&store).await;

    let related = store
        .related_products(&fixture.category_a.id, "prd_1", 0)
        .await
        .expect("related");
    assert_eq!(numbers(&related), vec![6, 3, 2]);

    let one = store
        .related_products(&fixture.category_a.id, "prd_1", 1)
        .await
        .expect("related");
    assert_eq!(numbers(&one), vec![6]);

    let without_six = store
        .related_products(&fixture.category_a.id, "prd_6", 10)
        .await
        .expect("related");
    assert_eq!(numbers(&without_six), vec![1, 3, 2]);

    let inactive_only = store
        .related_products(&fixture.category_b.id, "prd_5", 4)
        .await
        .expect("related");
    assert!(inactive_only.is_empty());

    let missing = store
        .related_products("missing", "prd_1", 4)
        .await
        .expect("related");
    assert!(missing.is_empty());
}

#[tokio::test]
async fn create_update_and_archive_round_trip() {
    let store = setup().await;
    let fixture = seed(&store).await;

    let created_at = at(2, 1, 0);
    let updated_at = at(2, 1, 1);
    let sale_start = at(2, 2, 0);
    let sale_end = at(2, 5, 0);

    let product = Product {
        id: "prd_new".to_string(),
        slug: "new-oil".to_string(),
        name: "New Oil".to_string(),
        subtitle: "Sub".to_string(),
        description: "Description.".to_string(),
        price_cents: 123_400,
        compare_at_cents: Some(150_000),
        sale_price_cents: Some(99_000),
        sale_starts_at: Some(sale_start),
        sale_ends_at: Some(sale_end),
        currency: String::new(),
        size: "10 ml".to_string(),
        category: fixture.category_a.clone(),
        concerns: vec!["dry".to_string()],
        ingredients: vec!["Olive".to_string()],
        how_to: "Apply.".to_string(),
        image: "/img.webp".to_string(),
        gallery: vec!["/img.webp".to_string(), "/img2.webp".to_string()],
        stock: 4,
        featured: true,
        active: true,
        plugin_id: String::new(),
        attributes: HashMap::from([("color".to_string(), vec!["gold".to_string()])]),
        created_at: Some(created_at),
        updated_at: Some(updated_at),
    };
    store
        .create_product(store.conn(), &product)
        .await
        .expect("create");

    let loaded = store.product_by_id("prd_new", true).await.expect("load");
    assert_eq!(loaded.slug, "new-oil");
    assert_eq!(loaded.price_cents, 123_400);
    assert_eq!(loaded.compare_at_cents, Some(150_000));
    assert_eq!(loaded.sale_price_cents, Some(99_000));
    assert_eq!(loaded.sale_starts_at, Some(sale_start));
    assert_eq!(loaded.sale_ends_at, Some(sale_end));
    assert_eq!(loaded.currency, "DZD");
    assert_eq!(loaded.plugin_id, "core");
    assert_eq!(loaded.concerns, vec!["dry".to_string()]);
    assert_eq!(loaded.ingredients, vec!["Olive".to_string()]);
    assert_eq!(loaded.gallery, product.gallery);
    assert_eq!(loaded.attributes, product.attributes);
    assert_eq!(loaded.created_at, Some(created_at));
    assert_eq!(loaded.updated_at, Some(updated_at));
    assert_eq!(loaded.category.id, fixture.category_a.id);
    assert_eq!(loaded.category.sort_order, 0);

    let mut changed = loaded.clone();
    changed.slug = "new-oil-v2".to_string();
    changed.name = "Updated Oil".to_string();
    changed.subtitle = "Updated".to_string();
    changed.description = "Changed.".to_string();
    changed.price_cents = 222_000;
    changed.compare_at_cents = None;
    changed.sale_price_cents = None;
    changed.sale_starts_at = None;
    changed.sale_ends_at = None;
    changed.currency = "EUR".to_string();
    changed.size = "12 ml".to_string();
    changed.category = fixture.category_b.clone();
    changed.concerns = vec!["calm".to_string()];
    changed.ingredients = vec!["Rose".to_string()];
    changed.how_to = "Pat.".to_string();
    changed.image = "/img3.webp".to_string();
    changed.gallery = vec!["/img3.webp".to_string()];
    changed.stock = 0;
    changed.featured = false;
    changed.plugin_id = "oils".to_string();
    changed.attributes = HashMap::from([("finish".to_string(), vec!["dewy".to_string()])]);
    changed.updated_at = Some(at(3, 1, 0));
    store
        .update_product(store.conn(), &changed)
        .await
        .expect("update");

    let reloaded = store.product_by_id("prd_new", false).await.expect("reload");
    assert_eq!(reloaded.slug, "new-oil-v2");
    assert_eq!(reloaded.name, "Updated Oil");
    assert_eq!(reloaded.price_cents, 222_000);
    assert_eq!(reloaded.compare_at_cents, None);
    assert_eq!(reloaded.sale_price_cents, None);
    assert_eq!(reloaded.sale_starts_at, None);
    assert_eq!(reloaded.currency, "EUR");
    assert_eq!(reloaded.category.id, fixture.category_b.id);
    assert_eq!(reloaded.gallery, vec!["/img3.webp".to_string()]);
    assert_eq!(reloaded.attributes, changed.attributes);
    assert_eq!(reloaded.updated_at, changed.updated_at);

    let mut missing = changed.clone();
    missing.id = "missing".to_string();
    assert!(
        store
            .update_product(store.conn(), &missing)
            .await
            .is_err_and(|err| err.is_not_found())
    );

    store
        .set_product_active(store.conn(), "prd_new", false, at(3, 2, 0))
        .await
        .expect("archive");
    let archived = store
        .product_by_id("prd_new", false)
        .await
        .expect("archived");
    assert!(!archived.active);
    assert!(
        store
            .product_by_slug("new-oil-v2", true)
            .await
            .is_err_and(|err| err.is_not_found())
    );
    assert!(store.product_by_slug("new-oil-v2", false).await.is_ok());
    assert!(
        store
            .set_product_active(store.conn(), "missing", false, at(3, 2, 0))
            .await
            .is_err_and(|err| err.is_not_found())
    );

    let mut undated = base_product(&fixture.category_a, 10, at(1, 1, 10));
    undated.created_at = None;
    undated.updated_at = None;
    store
        .create_product(store.conn(), &undated)
        .await
        .expect("undated create");
    let read = store.product_by_id("prd_10", false).await.expect("read");
    assert_eq!(
        read.created_at.map(|time| azahar_store::format_time(&time)),
        Some("0001-01-01T00:00:00Z".to_string())
    );
}

#[tokio::test]
async fn count_products_tracks_inserts() {
    let store = setup().await;
    assert_eq!(store.count_products().await.expect("count"), 0);
    seed(&store).await;
    assert_eq!(store.count_products().await.expect("count"), 6);
}

#[tokio::test]
async fn store_errors_expose_not_found_and_unique_helpers() {
    let store = setup().await;
    seed(&store).await;

    let error = store
        .product_by_slug("missing", false)
        .await
        .expect_err("missing slug");
    assert!(error.is_not_found());

    let duplicate = Product {
        slug: "prd-1".to_string(),
        ..base_product(&Category::default(), 99, at(1, 1, 0))
    };
    let error = store
        .create_product(store.conn(), &duplicate)
        .await
        .expect_err("duplicate slug");
    assert!(!error.is_not_found());
    assert!(error.is_unique_violation());
}
