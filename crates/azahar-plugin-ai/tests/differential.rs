//! Differential test for the AI plugin against `backend/cmd/oracle-ai`.
//!
//! Covers the manifest, migrations, route metadata, settings validation,
//! config/enabled rules, the `UpstreamError` mapping and 120 generated prompt
//! fixtures (60 store metric sets, 60 products).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Arc;

use azahar_plugin_ai::{
    DEFAULT_LANGUAGE, Plugin, ProductSales, UpstreamError, analysis_instruction,
    build_product_prompt, build_store_prompt, config_from_settings, enabled_from_settings,
    format_dzd, new,
};
use azahar_plugins::Plugin as PluginApiTrait;
use azahar_plugins::SettingsValidator;
use azahar_store::metrics::Metrics;
use azahar_store::{Category, Product, format_time};
use chrono::{Duration, TimeZone, Utc};
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
            .args(["run", "-tags", "oracle", "./cmd/oracle-ai"])
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
        assert!(read > 0, "Go oracle exited before answering");
        serde_json::from_str(&line).expect("decode oracle response")
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

/// Deterministic xorshift generator so a failing case is reproducible.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }

    fn below(&mut self, limit: u64) -> u64 {
        self.next() % limit
    }

    fn pick<'a, T>(&mut self, values: &'a [T]) -> &'a T {
        &values[usize::try_from(self.below(values.len() as u64)).unwrap_or(0)]
    }

    fn signed(&mut self, magnitude: i64) -> i64 {
        let value = i64::try_from(self.below(u64::try_from(magnitude).unwrap_or(1))).unwrap_or(0);
        if self.below(3) == 0 { -value } else { value }
    }
}

const NAMES: &[&str] = &[
    "Serum",
    "Crème hydratante",
    "زيت الأرغان",
    "Toner <daily> & light",
    "  Spaced name  ",
    "Quote \" and \\ slash",
    "Line\nbreak",
    "DZD 100% pure",
    "",
    "مجموعة العناية بالبشرة",
];

const STATUSES: &[&str] = &[
    "pending",
    "paid",
    "shipped",
    "delivered",
    "cancelled",
    "returned",
    "paid",
    "Pending",
];

fn utc(year: i32, month: u32, day: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, 0, 0, 0)
        .single()
        .expect("valid test date")
}

fn generated_metrics(rng: &mut Rng) -> Metrics {
    let month = 1 + u32::try_from(rng.below(12)).unwrap_or(0);
    let day = 1 + u32::try_from(rng.below(28)).unwrap_or(0);
    let from = utc(2026, month, day);
    let to = from + Duration::days(i64::try_from(rng.below(30)).unwrap_or(0));

    let top_count = usize::try_from(rng.below(8)).unwrap_or(0);
    let mut top_products = Vec::with_capacity(top_count);
    for _ in 0..top_count {
        top_products.push(azahar_store::metrics::TopProduct {
            product_id: format!("prd_{}", rng.below(1000)),
            name: (*rng.pick(NAMES)).to_string(),
            units: rng.signed(20),
            revenue_cents: rng.signed(2_000_000),
        });
    }

    let low_count = usize::try_from(rng.below(8)).unwrap_or(0);
    let mut low_stock = Vec::with_capacity(low_count);
    for _ in 0..low_count {
        low_stock.push(azahar_store::metrics::LowStockItem {
            id: format!("prd_{}", rng.below(1000)),
            name: (*rng.pick(NAMES)).to_string(),
            stock: i32::try_from(rng.below(10)).unwrap_or(0),
            image: String::new(),
        });
    }

    let status_count = usize::try_from(rng.below(7)).unwrap_or(0);
    let mut status_split = HashMap::new();
    for _ in 0..status_count {
        let status = (*rng.pick(STATUSES)).to_string();
        let count = i64::try_from(rng.below(200)).unwrap_or(0);
        status_split.insert(status, count);
    }

    Metrics {
        demo: false,
        from,
        to,
        revenue_cents: rng.signed(100_000_000),
        previous_revenue_cents: rng.signed(100_000_000),
        orders_count: rng.signed(500),
        previous_orders_count: rng.signed(500),
        units_sold: rng.signed(2_000),
        new_customers: rng.signed(200),
        low_stock,
        revenue_series: Vec::new(),
        status_split,
        top_products,
        recent_orders: Vec::new(),
    }
}

fn generated_product(rng: &mut Rng) -> Product {
    let concern_count = usize::try_from(rng.below(3)).unwrap_or(0);
    let ingredient_count = usize::try_from(rng.below(3)).unwrap_or(0);
    let attribute_count = usize::try_from(rng.below(4)).unwrap_or(0);
    let mut attributes = HashMap::new();
    for index in 0..attribute_count {
        let value_count = usize::try_from(rng.below(3)).unwrap_or(0);
        let values = (0..value_count)
            .map(|_| (*rng.pick(NAMES)).to_string())
            .collect();
        attributes.insert(
            format!(
                "{}{index}",
                rng.pick(&["scent", "color", "size", "زيت", "empty"])
            ),
            values,
        );
    }

    Product {
        name: (*rng.pick(NAMES)).to_string(),
        subtitle: if rng.below(2) == 0 {
            String::new()
        } else {
            (*rng.pick(NAMES)).to_string()
        },
        description: if rng.below(2) == 0 {
            String::new()
        } else {
            format!("{} {}", rng.pick(NAMES), rng.below(10_000))
        },
        price_cents: rng.signed(5_000_000),
        size: if rng.below(2) == 0 {
            String::new()
        } else {
            (*rng.pick(&["30 ml", " 1 L ", "زيت 100 مل", "XL"])).to_string()
        },
        stock: rng.signed(100) as i32,
        category: Category {
            name: (*rng.pick(NAMES)).to_string(),
            ..Category::default()
        },
        concerns: (0..concern_count)
            .map(|_| (*rng.pick(NAMES)).to_string())
            .collect(),
        ingredients: (0..ingredient_count)
            .map(|_| (*rng.pick(NAMES)).to_string())
            .collect(),
        attributes,
        ..Product::default()
    }
}

fn metrics_json(metrics: &Metrics) -> Value {
    json!({
        "from": format_time(&metrics.from),
        "to": format_time(&metrics.to),
        "revenue_cents": metrics.revenue_cents,
        "previous_revenue_cents": metrics.previous_revenue_cents,
        "orders_count": metrics.orders_count,
        "units_sold": metrics.units_sold,
        "new_customers": metrics.new_customers,
        "top_products": metrics.top_products.iter().map(|item| json!({
            "name": &item.name,
            "units": item.units,
            "revenue_cents": item.revenue_cents,
        })).collect::<Vec<_>>(),
        "low_stock": metrics.low_stock.iter().map(|item| json!({
            "name": &item.name,
            "stock": item.stock,
        })).collect::<Vec<_>>(),
        "status_split": &metrics.status_split,
    })
}

fn product_json(product: &Product) -> Value {
    json!({
        "name": &product.name,
        "subtitle": &product.subtitle,
        "description": &product.description,
        "price_cents": product.price_cents,
        "size": &product.size,
        "stock": product.stock,
        "category_name": &product.category.name,
        "concerns": &product.concerns,
        "ingredients": &product.ingredients,
        "attributes": &product.attributes,
    })
}

fn settings(values: &[(&str, &str)]) -> HashMap<String, String> {
    values
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

fn oracle_settings(oracle: &mut Oracle, values: &HashMap<String, String>) -> Value {
    let response = oracle.call(&json!({"op": "settings", "settings": values}));
    assert_eq!(response["error"], Value::Null, "oracle error: {response}");
    response
}

#[test]
fn manifest_migrations_routes_and_validation_match() {
    let mut oracle = Oracle::spawn();
    let plugin = new();

    let expected = oracle.call(&json!({"op": "manifest"}));
    assert_eq!(
        serde_json::to_value(plugin.manifest()).expect("manifest json"),
        expected["manifest"],
        "manifest differs from Go"
    );

    let expected = oracle.call(&json!({"op": "migrations"}));
    assert_eq!(
        serde_json::to_value(plugin.migrations()).expect("migrations json"),
        expected["migrations"],
        "migrations differ from Go"
    );

    let expected = oracle.call(&json!({"op": "routes"}));
    assert_eq!(
        serde_json::to_value(plugin.routes()).expect("routes json"),
        expected["routes"],
        "route metadata differs from Go"
    );

    let long_model = "a".repeat(64);
    let too_long_model = "a".repeat(65);
    let cases = [
        settings(&[("model", "gpt-4o-mini"), ("language", "en")]),
        settings(&[("model", "Qwen/Qwen2.5-7B-Instruct"), ("language", "fr")]),
        settings(&[("model", "llama3.1:8b"), ("language", "ar")]),
        settings(&[("model", ""), ("language", "")]),
        settings(&[("model", "gpt 4o"), ("language", "de")]),
        settings(&[("model", "modèle"), ("language", "EN")]),
        settings(&[("model", "gpt;4o"), ("language", "fr-FR")]),
        settings(&[("model", "  gpt-4o  "), ("language", "  en  ")]),
        settings(&[("model", long_model.as_str()), ("language", "en")]),
        settings(&[("model", too_long_model.as_str()), ("language", "en")]),
        HashMap::new(),
    ];
    for case in &cases {
        let expected = oracle.call(&json!({"op": "validate", "settings": case}));
        assert_eq!(
            serde_json::to_value(plugin.validate_settings(case)).expect("validate json"),
            expected["fields"],
            "validation differs for {case:?}"
        );
    }
}

#[test]
fn settings_and_upstream_errors_match() {
    let mut oracle = Oracle::spawn();
    let cases = [
        settings(&[
            ("enabled", "true"),
            ("api_key", "sk-key"),
            ("model", "gpt-4o-mini"),
            ("language", "en"),
        ]),
        settings(&[
            ("api_key", "sk-key"),
            ("model", "gpt-4o-mini"),
            ("language", "fr"),
        ]),
        settings(&[
            ("enabled", ""),
            ("api_key", " sk-key "),
            ("model", " m "),
            ("language", "ar"),
        ]),
        settings(&[
            ("enabled", "false"),
            ("api_key", "sk-key"),
            ("model", "gpt-4o-mini"),
            ("language", "en"),
        ]),
        settings(&[
            ("enabled", "TRUE"),
            ("api_key", "sk-key"),
            ("model", "gpt-4o-mini"),
            ("language", "en"),
        ]),
        settings(&[
            ("enabled", "0"),
            ("api_key", "sk-key"),
            ("model", "gpt-4o-mini"),
            ("language", "en"),
        ]),
        settings(&[
            ("enabled", "true"),
            ("model", "gpt-4o-mini"),
            ("language", "en"),
        ]),
        settings(&[
            ("enabled", "true"),
            ("api_key", "sk-key"),
            ("language", "en"),
        ]),
        settings(&[
            ("enabled", "true"),
            ("api_key", "   "),
            ("model", "   "),
            ("language", "de"),
        ]),
        HashMap::new(),
    ];
    for case in &cases {
        let expected = oracle_settings(&mut oracle, case);
        let ours = config_from_settings(case);
        assert_eq!(
            expected["enabled"].as_bool(),
            Some(enabled_from_settings(case)),
            "enabled differs for {case:?}"
        );
        match ours {
            Ok(config) => {
                assert_eq!(expected["class"], "", "class differs for {case:?}");
                assert_eq!(expected["config"]["api_key"], config.api_key);
                assert_eq!(expected["config"]["model"], config.model);
                assert_eq!(expected["config"]["language"], config.language);
            }
            Err(error) => {
                let expected_class = match error {
                    azahar_plugin_ai::ConfigError::Disabled => "disabled",
                    azahar_plugin_ai::ConfigError::NotConfigured => "not_configured",
                };
                assert_eq!(
                    expected["class"], expected_class,
                    "class differs for {case:?}"
                );
            }
        }
    }

    for status in [
        0u16, 100, 199, 200, 204, 299, 302, 400, 404, 429, 500, 503, 599, 999,
    ] {
        for reason in ["", "connection refused", "provider secret detail"] {
            let expected = oracle.call(&json!({
                "op": "upstream_error",
                "status_code": status,
                "reason": reason,
            }));
            let error = UpstreamError::new(status, reason);
            assert_eq!(
                expected["error"],
                error.to_string(),
                "error text for {status}"
            );
            assert_eq!(
                expected["status_class"],
                error.status_class(),
                "status class for {status}"
            );
            assert_eq!(expected["detail"], error.detail(), "detail for {status}");
        }
    }
}

#[test]
fn generated_prompts_match_the_go_builder() {
    let mut oracle = Oracle::spawn();
    let mut rng = Rng::new(0x005e_eda1);

    for index in 0..60 {
        let metrics = generated_metrics(&mut rng);
        let language = (*rng.pick(&["en", "fr", "ar", "de", ""])).to_string();
        let days = *rng.pick(&[7, 30, 90, 0, -3, 365]);

        let expected = oracle.call(&json!({
            "op": "store_prompt",
            "language": language,
            "days": days,
            "metrics": metrics_json(&metrics),
        }));
        assert_eq!(expected["error"], Value::Null, "oracle error: {expected}");
        let (system, user) = build_store_prompt(&language, days, &metrics);
        assert_eq!(expected["system"], system, "store system prompt {index}");
        assert_eq!(expected["user"], user, "store user prompt {index}");
    }

    for language in ["en", "fr", "ar", "de", "", "AR"] {
        let expected = oracle.call(&json!({
            "op": "instruction",
            "language": language,
        }));
        assert_eq!(
            analysis_instruction(language),
            expected["instruction"],
            "instruction for {language:?}"
        );
    }

    for index in 0..60 {
        let product = generated_product(&mut rng);
        let sales = ProductSales {
            days: *rng.pick(&[30, 7, 0, -1]),
            units: rng.signed(500),
            orders: rng.signed(200),
            revenue_cents: rng.signed(3_000_000),
        };
        let language = (*rng.pick(&["en", "fr", "ar", "es", ""])).to_string();

        let expected = oracle.call(&json!({
            "op": "product_prompt",
            "language": language,
            "product": product_json(&product),
            "sales": {
                "days": sales.days,
                "units": sales.units,
                "orders": sales.orders,
                "revenue_cents": sales.revenue_cents,
            },
        }));
        assert_eq!(expected["error"], Value::Null, "oracle error: {expected}");
        let (system, user) = build_product_prompt(&language, &product, &sales);
        assert_eq!(expected["system"], system, "product system prompt {index}");
        assert_eq!(expected["user"], user, "product user prompt {index}");
    }

    let cents = [
        0,
        1,
        5,
        99,
        100,
        101,
        12_345,
        -1,
        -99,
        -100,
        -12_345,
        1_000_000,
        -1_000_000,
        i64::MAX,
        i64::MIN,
        i64::from(i32::MAX),
        -i64::from(i32::MAX),
    ];
    for value in cents {
        let expected = oracle.call(&json!({"op": "format_dzd", "cents": value}));
        assert_eq!(expected["formatted"], format_dzd(value), "DZD for {value}");
    }
    let mut rng = Rng::new(0xd2d);
    for _ in 0..40 {
        let value = rng.next() as i64;
        let expected = oracle.call(&json!({"op": "format_dzd", "cents": value}));
        assert_eq!(expected["formatted"], format_dzd(value), "DZD for {value}");
    }
}

#[test]
fn registers_and_keeps_the_default_language() {
    let mut registry = azahar_plugins::Registry::new();
    registry
        .register(Arc::new(Plugin))
        .expect("register the AI plugin");
    assert_eq!(DEFAULT_LANGUAGE, "en");
    assert_eq!(
        config_from_settings(&settings(&[
            ("api_key", "sk"),
            ("model", "gpt-4o-mini"),
            ("language", "xx"),
        ]))
        .expect("config")
        .language,
        "en"
    );
}
