//! Behavioural tests for the demo seed against a local in-memory libSQL
//! database. Generated order values are deliberately not compared byte for
//! byte: Go and Rust draw from different generators after the same fixed seed,
//! so the tests assert the shared plan, ranges and invariants.

mod common;

use std::collections::{HashMap, HashSet};

use azahar_auth::verify_password;
use azahar_seed::{Options, Summary, run};
use azahar_store::orders::AdminOrderFilter;
use azahar_store::{Category, Product};
use azahar_storefront::{Theme, parse};
use chrono::{Datelike, TimeDelta, Timelike, Utc};
use common::*;

const EXPECTED_OILS_SLUGS: [&str; 8] = [
    "orange-blossom-cleansing-oil",
    "rosehip-and-olive-face-serum",
    "rose-clay-mask",
    "fig-and-sesame-body-balm",
    "prickly-pear-face-oil",
    "cedar-and-myrrh-hand-cream",
    "green-fig-body-oil",
    "rose-petal-bath-soak",
];

const EXPECTED_PART_SLUGS: [(&str, &str); 8] = [
    ("demo-ryzen-7", "cpus"),
    ("demo-b650m", "motherboards"),
    ("demo-ddr5-32", "memory"),
    ("demo-nvme-1tb", "storage"),
    ("demo-psu-750", "psus"),
    ("demo-mesh-atx", "cases"),
    ("demo-rx-9070", "gpus"),
    ("demo-am5-tower", "cooling"),
];

fn admin_filter() -> AdminOrderFilter {
    AdminOrderFilter {
        limit: 500,
        ..AdminOrderFilter::default()
    }
}

fn attributes(product: &Product, key: &str) -> Vec<String> {
    product.attributes.get(key).cloned().unwrap_or_default()
}

#[tokio::test]
async fn demo_seed_fills_every_stage_and_second_run_is_a_noop() {
    let store = seeded_store().await;
    seed_framework_rows(&store).await;
    let opts = demo_options();
    let today = Utc::now().date_naive();

    let first = run(&store, &opts).await.expect("first seed");
    assert_eq!(
        first,
        Summary {
            categories: 3,
            products: 17,
            users: 15,
            orders: 48,
        }
    );

    // --- users ---------------------------------------------------------
    assert_eq!(store.count_users().await.expect("count users"), 15);
    for (id, email, password, role) in [
        (
            "usr_admin",
            "admin@azahar.example",
            "azahar-dev-admin-2026",
            "admin",
        ),
        (
            "usr_staff",
            "staff@azahar.example",
            "azahar-dev-staff-2026",
            "employee",
        ),
        (
            "usr_client",
            "client@azahar.example",
            "azahar-dev-client-2026",
            "customer",
        ),
    ] {
        let user = store.user_by_id(id).await.expect("demo user");
        assert_eq!(user.email, email);
        assert_eq!(user.role, role);
        assert_eq!(user.theme_preset, "cellar");
        assert!(verify_password(password, &user.password_hash), "{id}");
        assert!(
            !verify_password("wrong-password", &user.password_hash),
            "{id}"
        );
    }
    let sample = store
        .user_by_id("usr_sample_01")
        .await
        .expect("sample user");
    assert_eq!(sample.email, "sample01@azahar.example");
    assert_eq!(sample.role, "customer");
    assert!(verify_password(SAMPLE_PASSWORD, &sample.password_hash));
    assert_eq!(
        store
            .user_by_id("usr_sample_12")
            .await
            .expect("last sample")
            .email,
        "sample12@azahar.example"
    );
    assert_eq!(
        store
            .user_by_email("admin@azahar.example")
            .await
            .expect("admin")
            .created_at
            .expect("created")
            .date_naive(),
        today - TimeDelta::days(60)
    );
    assert_eq!(
        sample.created_at.expect("created").date_naive(),
        today - TimeDelta::days(3)
    );

    // --- catalogue -----------------------------------------------------
    assert_eq!(store.count_products().await.expect("count products"), 17);
    let products = store
        .list_products(&all_products_filter())
        .await
        .expect("list products");
    assert_eq!(products.len(), 17);
    let by_slug: HashMap<&str, &Product> = products
        .iter()
        .map(|product| (product.slug.as_str(), product))
        .collect();

    for (index, slug) in EXPECTED_OILS_SLUGS.iter().enumerate() {
        let product = by_slug
            .get(slug)
            .unwrap_or_else(|| panic!("missing {slug}"));
        assert_eq!(product.id, format!("prd_{:02}", index + 1));
        assert_eq!(product.plugin_id, "oils");
        assert!(product.active);
        assert_eq!(product.currency, "DZD");
        assert_eq!(product.gallery, vec![product.image.clone()]);
        assert_eq!(attributes(product, "size"), vec![product.size.clone()]);
        assert!(!attributes(product, "concerns").is_empty());
        assert!(!attributes(product, "how_to").is_empty());
        assert_eq!(
            product.created_at.expect("created").date_naive(),
            today - TimeDelta::days(8 - index as i64)
        );
    }

    let mask = by_slug["rose-clay-mask"];
    assert_eq!(mask.price_cents, 360_000);
    assert_eq!(mask.sale_price_cents, Some(288_000));
    assert_eq!(mask.effective_price_cents(Utc::now()), 288_000);
    assert_eq!(mask.size, "75 g");
    assert_eq!(mask.image, "/images/products/clay-mask-1200.webp");
    assert_eq!(mask.stock, 4);
    assert!(!mask.featured);
    assert_eq!(mask.category.slug, "face");
    assert_eq!(mask.category.plugin_id, "oils");
    assert_eq!(
        attributes(mask, "ingredients"),
        vec!["Rose clay", "Kaolin", "Chamomile"]
    );
    assert!(mask.sale_starts_at.expect("sale start") < Utc::now());
    assert!(mask.sale_ends_at.expect("sale end") > Utc::now());
    let cleansing = by_slug["orange-blossom-cleansing-oil"];
    assert_eq!(cleansing.price_cents, 480_000);
    assert!(cleansing.sale_price_cents.is_none());
    assert!(cleansing.sale_starts_at.is_none());
    assert!(cleansing.featured);

    let phone = by_slug["aster-5"];
    assert_eq!(phone.id, "prd_demo_phone");
    assert_eq!(phone.plugin_id, "phone");
    assert_eq!(phone.category.id, "cat_phone_phones");
    assert_eq!(phone.stock, 6);
    assert!(phone.featured);
    assert_eq!(phone.image, "/images/demo/phone.svg");
    assert_eq!(attributes(phone, "config").len(), 2);
    assert_eq!(attributes(phone, "color"), vec!["Midnight", "Silver"]);

    let mut phone_rows = store
        .conn()
        .query(
            "SELECT product_id, variant_key, imei, config, color, status, order_id
             FROM phone_devices ORDER BY imei",
            (),
        )
        .await
        .expect("phone devices");
    let mut devices = Vec::new();
    while let Some(row) = phone_rows.next().await.expect("phone row") {
        devices.push((
            row.get::<String>(0).expect("product"),
            row.get::<String>(1).expect("variant"),
            row.get::<String>(2).expect("imei"),
            row.get::<String>(3).expect("config"),
            row.get::<String>(4).expect("color"),
            row.get::<String>(5).expect("status"),
            row.get::<String>(6).expect("order"),
        ));
    }
    assert_eq!(devices.len(), 6);
    let imeis: HashSet<&str> = devices.iter().map(|device| device.2.as_str()).collect();
    assert_eq!(imeis.len(), 6);
    assert!(imeis.contains("356938035640003"));
    for device in &devices {
        assert_eq!(device.0, "prd_demo_phone");
        assert_eq!(device.1, "");
        assert_eq!(device.5, "in_stock");
        assert_eq!(device.6, "");
    }

    for (slug, category) in EXPECTED_PART_SLUGS {
        let part = by_slug[slug];
        assert_eq!(part.id, format!("prd_demo_{category}"));
        assert_eq!(part.plugin_id, "pcparts");
        assert_eq!(part.category.slug, category);
        assert!(part.active);
        assert!(!part.featured);
        assert_eq!(part.image, format!("/images/demo/{category}.svg"));
        assert_eq!(part.subtitle, "Builder sample part");
        assert_eq!(
            part.description,
            "A sample PC part used by the Build your PC demo."
        );
    }
    let cpu = by_slug["demo-ryzen-7"];
    assert_eq!(cpu.name, "Ryzen 7 9700X");
    assert_eq!(cpu.price_cents, 5_490_000);
    assert_eq!(cpu.stock, 6);
    assert_eq!(attributes(cpu, "socket"), vec!["AM5"]);
    assert_eq!(attributes(cpu, "tdp_watts"), vec!["120"]);
    assert_eq!(
        attributes(by_slug["demo-mesh-atx"], "supported_form_factor"),
        vec!["ATX, micro-ATX"]
    );

    let categories = store.categories().await.expect("categories");
    assert_eq!(categories.len(), 12);
    let face = categories
        .iter()
        .find(|category| category.slug == "face")
        .expect("face category");
    assert_eq!(face.id, "cat_oils_face");
    assert_eq!(face.plugin_id, "oils");
    assert_eq!(face.name, "Face");
    assert_eq!(face.sort_order, 1);
    assert_eq!(
        categories
            .iter()
            .filter(|category| category.plugin_id == "oils")
            .count(),
        3
    );
    assert_eq!(
        categories
            .iter()
            .filter(|category| category.plugin_id == "pcparts")
            .count(),
        8
    );

    // The Go seed never writes audit rows; order intake does that at checkout.
    assert_eq!(
        scalar_i64(store.conn(), "SELECT COUNT(*) FROM audit_log").await,
        0
    );

    // --- framework rows are left untouched -----------------------------
    let (theme_raw, _) = store
        .storefront_settings()
        .await
        .expect("storefront settings");
    assert_eq!(parse(&theme_raw).expect("theme parses"), Theme::default());
    assert_eq!(
        parse(&theme_raw).expect("theme parses").brand_name,
        "Azahar"
    );
    let states = store.plugin_states().await.expect("plugin states");
    assert_eq!(states.get("oils"), Some(&true));
    assert_eq!(states.get("phone"), Some(&false));
    assert_eq!(states.get("pcparts"), Some(&true));
    let (plugin_settings, _) = store.plugin_settings("ai").await.expect("plugin settings");
    assert_eq!(plugin_settings, r#"{"enabled":false}"#);

    // --- orders --------------------------------------------------------
    assert_eq!(store.count_orders().await.expect("count orders"), 48);
    let orders = store
        .admin_orders(&admin_filter())
        .await
        .expect("admin orders");
    assert_eq!(orders.len(), 48);

    let mut status_counts: HashMap<&str, usize> = HashMap::new();
    let mut status_offsets: HashMap<&str, Vec<i64>> = HashMap::new();
    let mut numbers = HashSet::new();
    let mut token_hashes = HashSet::new();
    let mut expected_numbers_by_year: HashMap<i32, Vec<u32>> = HashMap::new();
    for order in &orders {
        *status_counts.entry(order.status.as_str()).or_default() += 1;
        let created = order.created_at.expect("order created");
        status_offsets
            .entry(order.status.as_str())
            .or_default()
            .push((today - created.date_naive()).num_days());
        assert!((9..=18).contains(&created.hour()));
        assert!(created.minute() < 60);

        assert!(numbers.insert(order.number.clone()));
        let parts: Vec<&str> = order.number.split('-').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "AZ");
        let year: i32 = parts[1].parse().expect("number year");
        assert_eq!(year, created.year());
        let sequence: u32 = parts[2].parse().expect("number sequence");
        assert!(parts[2].len() >= 4);
        expected_numbers_by_year
            .entry(year)
            .or_default()
            .push(sequence);

        assert!(token_hashes.insert(order.token_hash.clone()));
        assert_eq!(order.token_hash.len(), 64);
        assert!(
            order
                .token_hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
        assert_eq!(order.currency, "DZD");
        assert_eq!(order.delivery_type, "home");
        assert!(order.phone.starts_with("0550 "));
        assert_eq!(order.phone.len(), 13);
        assert!(order.address.ends_with(" rue des Oliviers"));
        let house_number: i32 = order
            .address
            .split(' ')
            .next()
            .expect("house number")
            .parse()
            .expect("number");
        assert!((4..=83).contains(&house_number));
        assert!(order.notes.is_empty() || order.notes == "Leave with the concierge");
        let (code, wilaya) = match order.city.as_str() {
            "Alger Centre" => (16, "Alger"),
            "Oran" => (31, "Oran"),
            "Constantine" => (25, "Constantine"),
            "Tlemcen" => (13, "Tlemcen"),
            "Annaba" => (23, "Annaba"),
            "Blida" => (9, "Blida"),
            other => panic!("unexpected city {other}"),
        };
        assert_eq!(order.wilaya_code, code);
        assert_eq!(order.wilaya_name, wilaya);
        assert_eq!(order.commune_name, order.city);
    }
    for (status, count) in [
        ("pending", 6),
        ("confirmed", 6),
        ("packed", 5),
        ("shipped", 8),
        ("delivered", 20),
        ("cancelled", 3),
    ] {
        assert_eq!(status_counts.get(status), Some(&count), "{status}");
    }
    for (status, expected) in [
        ("pending", vec![0, 0, 1, 2, 3, 4]),
        ("confirmed", vec![1, 2, 3, 4, 5, 7]),
        ("packed", vec![2, 4, 6, 8, 10]),
        ("shipped", vec![4, 6, 8, 10, 13, 15, 17, 20]),
        (
            "delivered",
            vec![
                8, 9, 11, 13, 15, 17, 19, 21, 23, 25, 26, 28, 30, 32, 34, 36, 38, 40, 42, 44,
            ],
        ),
        ("cancelled", vec![5, 17, 30]),
    ] {
        let mut actual = status_offsets.remove(status).expect("status offsets");
        actual.sort_unstable();
        assert_eq!(actual, expected, "{status}");
    }
    for (_, mut sequences) in expected_numbers_by_year {
        sequences.sort_unstable();
        assert_eq!(sequences, (1..=sequences.len() as u32).collect::<Vec<_>>());
    }
    assert_eq!(
        scalar_i64(
            store.conn(),
            "SELECT COALESCE(SUM(last_value), 0) FROM order_sequences"
        )
        .await,
        48
    );

    let guests: Vec<_> = orders
        .iter()
        .filter(|order| order.user_id.is_none())
        .collect();
    assert_eq!(guests.len(), 6);
    let guest_emails: HashSet<&str> = guests.iter().map(|order| order.email.as_str()).collect();
    for index in 1..=6 {
        let email = format!("guest{index:02}@azahar.example");
        assert!(guest_emails.contains(email.as_str()), "{email}");
        let guest = guests
            .iter()
            .find(|order| order.email == email)
            .expect("guest order");
        assert_eq!(guest.name, format!("Sample guest {index:02}"));
    }

    let users = store.list_users().await.expect("users");
    let users_by_id: HashMap<&str, _> = users.iter().map(|user| (user.id.as_str(), user)).collect();
    let customer_ids: HashSet<&str> = users
        .iter()
        .filter(|user| user.role == "customer")
        .map(|user| user.id.as_str())
        .collect();
    for order in orders.iter().filter(|order| order.user_id.is_some()) {
        let user_id = order.user_id.as_deref().expect("user id");
        assert!(customer_ids.contains(user_id), "{user_id}");
        let user = users_by_id[user_id];
        assert_eq!(order.name, user.name);
        assert_eq!(order.email, user.email);
    }
    assert_eq!(
        orders
            .iter()
            .filter(|order| order.user_id.is_some())
            .count(),
        42
    );

    let mut total_items = 0;
    for order in &orders {
        let items = store.order_items(&order.id).await.expect("order items");
        assert!((1..=4).contains(&items.len()));
        total_items += items.len();
        let mut product_ids = HashSet::new();
        let mut subtotal = 0i64;
        for item in &items {
            assert_eq!(item.order_id, order.id);
            assert!(product_ids.insert(item.product_id.clone()));
            assert!((1..=3).contains(&item.quantity));
            assert_eq!(
                item.line_total_cents,
                item.unit_price_cents * i64::from(item.quantity)
            );
            assert_eq!(item.plugin_id, "oils");
            assert!(item.options.is_empty());
            assert!(item.options_label.is_empty());
            let product = by_slug[item.slug.as_str()];
            assert_eq!(item.product_id, product.id);
            assert_eq!(item.name, product.name);
            assert_eq!(item.image, product.image);
            assert_eq!(item.unit_price_cents, product.price_cents);
            subtotal += item.line_total_cents;
        }
        assert_eq!(order.subtotal_cents, subtotal);
        assert_eq!(
            order.shipping_cents,
            if subtotal > 800_000 { 0 } else { 60_000 }
        );
        assert_eq!(order.total_cents, subtotal + order.shipping_cents);
    }
    assert!((48..=192).contains(&total_items));

    // --- second run ----------------------------------------------------
    let admin_before = store
        .user_by_id("usr_admin")
        .await
        .expect("admin")
        .password_hash;
    let second = run(&store, &opts).await.expect("second seed");
    assert_eq!(second, Summary::default());
    assert_eq!(store.count_products().await.expect("products"), 17);
    assert_eq!(store.count_users().await.expect("users"), 15);
    assert_eq!(store.count_orders().await.expect("orders"), 48);
    assert_eq!(store.categories().await.expect("categories").len(), 12);
    assert_eq!(
        scalar_i64(store.conn(), "SELECT COUNT(*) FROM order_items").await,
        total_items as i64
    );
    assert_eq!(
        scalar_i64(
            store.conn(),
            "SELECT COALESCE(SUM(last_value), 0) FROM order_sequences"
        )
        .await,
        48
    );
    assert_eq!(
        store
            .user_by_id("usr_admin")
            .await
            .expect("admin")
            .password_hash,
        admin_before
    );
    let (theme_after, _) = store
        .storefront_settings()
        .await
        .expect("storefront settings");
    assert_eq!(theme_after, theme_raw);
    assert_eq!(store.plugin_states().await.expect("states"), states);
    assert_eq!(
        store.plugin_settings("ai").await.expect("settings").0,
        plugin_settings
    );
}

#[tokio::test]
async fn existing_users_are_left_alone_and_orders_attach_to_them() {
    let store = seeded_store().await;
    let existing = demo_customer();
    store
        .create_user(store.conn(), &existing)
        .await
        .expect("existing user");

    let summary = run(&store, &demo_options()).await.expect("seed");
    assert_eq!(
        summary,
        Summary {
            categories: 3,
            products: 17,
            users: 0,
            orders: 48,
        }
    );
    assert_eq!(store.count_users().await.expect("users"), 1);
    let loaded = store
        .user_by_id("usr_existing")
        .await
        .expect("existing user");
    assert_eq!(loaded.password_hash, "not-used-during-seed");
    assert!(store.user_by_email("admin@azahar.example").await.is_err());

    let orders = store.admin_orders(&admin_filter()).await.expect("orders");
    assert_eq!(
        orders
            .iter()
            .filter(|order| order.user_id.as_deref() == Some("usr_existing"))
            .count(),
        42
    );
    assert_eq!(
        orders
            .iter()
            .filter(|order| order.user_id.is_none())
            .count(),
        6
    );
}

#[tokio::test]
async fn custom_credentials_hash_and_verify() {
    let store = seeded_store().await;
    let opts = Options {
        admin_email: "owner@example.test".to_string(),
        admin_password: "owner-secret-1".to_string(),
        customer_email: "buyer@example.test".to_string(),
        customer_password: "buyer-secret-2".to_string(),
        employee_email: "team@example.test".to_string(),
        employee_password: "team-secret-3".to_string(),
    };

    let summary = run(&store, &opts).await.expect("seed");
    assert_eq!(summary.users, 15);
    for (id, email, password, role) in [
        ("usr_admin", "owner@example.test", "owner-secret-1", "admin"),
        (
            "usr_staff",
            "team@example.test",
            "team-secret-3",
            "employee",
        ),
        (
            "usr_client",
            "buyer@example.test",
            "buyer-secret-2",
            "customer",
        ),
    ] {
        let user = store.user_by_id(id).await.expect("user");
        assert_eq!(user.email, email);
        assert_eq!(user.role, role);
        assert_eq!(user.theme_preset, "cellar");
        assert!(verify_password(password, &user.password_hash), "{id}");
        assert!(
            !verify_password("azahar-dev-admin-2026", &user.password_hash),
            "{id}"
        );
    }
    let sample = store
        .user_by_id("usr_sample_01")
        .await
        .expect("sample user");
    assert!(verify_password(SAMPLE_PASSWORD, &sample.password_hash));
    assert!(!verify_password("buyer-secret-2", &sample.password_hash));
}

#[tokio::test]
async fn a_short_oils_catalogue_does_not_panic_the_order_stage() {
    let store = seeded_store().await;
    let existing = demo_customer();
    store
        .create_user(store.conn(), &existing)
        .await
        .expect("existing user");
    let category = store.category_by_slug("face").await.expect("face");
    let now = Utc::now();
    for index in 1..=2 {
        let product = Product {
            id: format!("prd_short_{index}"),
            slug: format!("short-{index}"),
            name: format!("Short shelf {index}"),
            price_cents: 100_000,
            category: category.clone(),
            plugin_id: "oils".to_string(),
            active: true,
            created_at: Some(now),
            updated_at: Some(now),
            ..Product::default()
        };
        store
            .create_product(store.conn(), &product)
            .await
            .expect("short product");
    }

    let summary = run(&store, &demo_options()).await.expect("seed");
    assert_eq!(
        summary,
        Summary {
            categories: 0,
            products: 9,
            users: 0,
            orders: 48,
        }
    );
    let orders = store.admin_orders(&admin_filter()).await.expect("orders");
    for order in orders {
        let items = store.order_items(&order.id).await.expect("items");
        assert!((1..=2).contains(&items.len()));
        let product_ids: HashSet<&str> =
            items.iter().map(|item| item.product_id.as_str()).collect();
        assert_eq!(product_ids.len(), items.len());
    }
}

#[tokio::test]
async fn missing_oils_category_reports_the_catalog_prefix() {
    let store = memory_store().await;
    let error = run(&store, &demo_options())
        .await
        .expect_err("missing category");
    assert_eq!(
        error.to_string(),
        "seed catalog: category face is not registered by the oils plugin"
    );
    assert_eq!(store.count_products().await.expect("products"), 0);
}

#[tokio::test]
async fn missing_phone_category_reports_the_plugin_catalog_prefix() {
    let store = memory_store().await;
    for (slug, name, description, sort_order) in [
        ("face", "Face", "Oils, serums and masks for the face.", 1),
        ("body", "Body", "Balms, oils and creams for the body.", 2),
        ("ritual", "Ritual", "Soaks for the bath.", 3),
    ] {
        store
            .upsert_category(
                "oils",
                &Category {
                    slug: slug.to_string(),
                    name: name.to_string(),
                    description: description.to_string(),
                    sort_order,
                    ..Category::default()
                },
            )
            .await
            .expect("oils category");
    }

    let error = run(&store, &demo_options())
        .await
        .expect_err("missing phone category");
    assert!(
        error
            .to_string()
            .starts_with("seed plugin catalog: phone category: "),
        "{error}"
    );
    assert_eq!(store.count_products().await.expect("products"), 8);
    assert_eq!(
        scalar_i64(store.conn(), "SELECT COUNT(*) FROM audit_log").await,
        0
    );
}

#[tokio::test]
async fn no_customers_reports_the_order_prefix() {
    let store = seeded_store().await;
    let mut admin = demo_customer();
    admin.id = "usr_admin_only".to_string();
    admin.name = "Solo admin".to_string();
    admin.email = "solo@example.test".to_string();
    admin.role = "admin".to_string();
    store
        .create_user(store.conn(), &admin)
        .await
        .expect("admin user");

    let error = run(&store, &demo_options())
        .await
        .expect_err("no customers");
    assert_eq!(
        error.to_string(),
        "seed orders: no customers to seed orders with"
    );
    assert_eq!(store.count_orders().await.expect("orders"), 0);
}

#[tokio::test]
async fn no_active_oils_product_reports_the_order_prefix() {
    let store = seeded_store().await;
    let existing = demo_customer();
    store
        .create_user(store.conn(), &existing)
        .await
        .expect("existing user");
    let category = store.category_by_slug("face").await.expect("face");
    let now = Utc::now();
    let product = Product {
        id: "prd_inactive".to_string(),
        slug: "inactive-sample".to_string(),
        name: "Inactive sample".to_string(),
        price_cents: 100_000,
        category,
        plugin_id: "oils".to_string(),
        active: false,
        created_at: Some(now),
        updated_at: Some(now),
        ..Product::default()
    };
    store
        .create_product(store.conn(), &product)
        .await
        .expect("inactive product");

    let error = run(&store, &demo_options())
        .await
        .expect_err("no active product");
    assert_eq!(
        error.to_string(),
        "seed orders: no active products to seed orders with"
    );
    assert_eq!(store.count_products().await.expect("products"), 10);
    assert_eq!(store.count_orders().await.expect("orders"), 0);
}
