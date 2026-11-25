//! Differential tests against the Go oracle in `backend/cmd/oracle-codemode`.
//!
//! Pure sections compare plan parsing (300+ generated and fixed documents),
//! shape validation, theme/product patches, drafts, attributes, fingerprints,
//! prompts, settings and the manifest. The database section seeds unique
//! `itcm_*` fixtures, runs the real Go apply handler, then re-seeds the same
//! rows and runs the Rust service, comparing the HTTP body and the resulting
//! rows in both directions.
//!
//! The oracle needs the local libSQL test server (`make dev-db`). An
//! unreachable database fails the test with a clear message instead of being
//! skipped.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, RwLock};

use azahar_orderintake::NoopHooks;
use azahar_plugin_codemode::{
    ApplyInput, Codemode, Plan, ProductDraft, ProductPatch, build_prompt, config_from_settings,
    encode_plan, encode_product_draft, first_field_message, manifest, migrations,
    normalize_attributes, parse_plan, parse_product_patch, plan_baseline, product_fingerprint,
    prompt_plugins, validate_plan_shape, validate_product_attributes, validate_product_draft,
    validate_settings,
};
use azahar_plugins::{Manifest, OptionField, Plugin as PluginTrait, ProductType, Registry};
use azahar_store::{Category, Order, OrderItem, Product, ProductFilter, Store, parse_time};
use serde_json::{Map, Value, json};

// ---------------------------------------------------------------------------
// Oracle process
// ---------------------------------------------------------------------------

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn dsn() -> String {
    std::env::var("AZAHAR_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8880".to_string())
}

struct Oracle {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Oracle {
    fn start() -> Self {
        let backend = repository_root().join("backend");
        let mut child = Command::new("go")
            .args(["run", "-tags", "oracle", "./cmd/oracle-codemode"])
            .current_dir(&backend)
            .env("AZAHAR_TEST_DATABASE_URL", dsn())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap_or_else(|error| {
                panic!(
                    "start Go code-mode oracle in {}: {error}",
                    backend.display()
                )
            });
        let stdin = child.stdin.take().expect("oracle stdin");
        let stdout = child.stdout.take().expect("oracle stdout");
        Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        }
    }

    fn call(&mut self, request: &Value) -> Value {
        writeln!(self.stdin, "{request}").expect("write oracle request");
        self.stdin.flush().expect("flush oracle request");
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("read oracle reply");
        assert!(!line.is_empty(), "the oracle exited before replying");
        serde_json::from_str(&line).expect("oracle reply is JSON")
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct DefPlugin {
    id: String,
    name: String,
    product_type: Option<ProductType>,
}

impl PluginTrait for DefPlugin {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: self.id.clone(),
            name: self.name.clone(),
            version: "1.0.0".to_string(),
            product_type: self.product_type.clone(),
            ..Manifest::default()
        }
    }
}

fn field(key: &str, label: &str, field_type: &str, required: bool) -> OptionField {
    OptionField {
        key: key.to_string(),
        label: label.to_string(),
        field_type: field_type.to_string(),
        required,
        ..OptionField::default()
    }
}

fn oils_product_type() -> ProductType {
    ProductType {
        label: "Oil".to_string(),
        fields: vec![
            field("size", "Size", "text", true),
            field("concerns", "Concerns", "list", false),
        ],
        ..ProductType::default()
    }
}

fn plugin_defs() -> Vec<Value> {
    vec![
        json!({
            "id": "oils",
            "label": "Oil",
            "fields": [
                {"key":"size","label":"Size","type":"text","required":true},
                {"key":"concerns","label":"Concerns","type":"list","required":false}
            ]
        }),
        json!({"id": "plain", "label": ""}),
    ]
}

fn registry_from_defs(defs: &[Value]) -> RwLock<Registry> {
    let mut registry = Registry::new();
    for def in defs {
        let id = def.get("id").and_then(Value::as_str).unwrap_or_default();
        let label = def.get("label").and_then(Value::as_str).unwrap_or_default();
        let product_type = if label.is_empty() {
            None
        } else {
            let fields = def
                .get("fields")
                .and_then(Value::as_array)
                .map(|fields| {
                    fields
                        .iter()
                        .map(|field| OptionField {
                            key: field
                                .get("key")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            label: field
                                .get("label")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            field_type: field
                                .get("type")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            required: field
                                .get("required")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                            ..OptionField::default()
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(ProductType {
                label: label.to_string(),
                fields,
                ..ProductType::default()
            })
        };
        registry
            .register(Arc::new(DefPlugin {
                id: id.to_string(),
                name: id.to_string(),
                product_type,
            }))
            .expect("register oracle definition");
    }
    RwLock::new(registry)
}

fn empty_theme() -> Value {
    json!({})
}

fn encoded_theme(theme: &azahar_storefront::Theme) -> Value {
    serde_json::from_str(&azahar_storefront::encode(theme).expect("theme")).expect("theme json")
}

fn encoded_theme_text(theme: &azahar_storefront::Theme) -> Value {
    Value::String(azahar_storefront::encode(theme).expect("theme"))
}

fn json_products(products: &[Product]) -> Vec<Value> {
    products
        .iter()
        .map(|product| {
            json!({
                "id": product.id,
                "slug": product.slug,
                "name": product.name,
                "subtitle": product.subtitle,
                "description": product.description,
                "price_cents": product.price_cents,
                "stock": product.stock,
                "active": product.active,
                "plugin_id": product.plugin_id,
                "category_slug": product.category.slug,
            })
        })
        .collect()
}

fn wire_product(id: &str, name: &str, price_cents: i64, stock: i32) -> Product {
    Product {
        id: id.to_string(),
        slug: format!("slug-{id}"),
        name: name.to_string(),
        subtitle: "Sub".to_string(),
        description: "Desc".to_string(),
        price_cents,
        stock,
        active: true,
        plugin_id: "oils".to_string(),
        category: Category {
            slug: "body".to_string(),
            plugin_id: "oils".to_string(),
            ..Category::default()
        },
        ..Product::default()
    }
}

// ---------------------------------------------------------------------------
// Plan parsing: 300+ documents
// ---------------------------------------------------------------------------

fn plan_documents() -> Vec<Value> {
    let mut documents = Vec::new();
    let summaries = [
        "Change the shelf.",
        "Été à Alger: update the hero.",
        "A <b> & c",
    ];
    let action_pool = vec![
        json!({"type":"theme.update","patch":{"hero_heading":"A shorter shelf."}}),
        json!({"type":"theme.update","patch":{ "radius" : 4 , "light" : {"accent":"#123456"} }}),
        json!({"type":"theme.update","patch":{"concerns_enabled":false,"footer_note":""}}),
        json!({"type":"product.update","product_id":"prd_1","patch":{"price_cents":4200}}),
        json!({"type":"product.update","product_id":"prd_1","patch":{"stock":9,"active":false,"name":"New name","subtitle":"S","description":"D"}}),
        json!({"type":"product.update","product_id":null,"patch":{"stock":1}}),
        json!({"type":"product.archive","product_id":"prd_2"}),
        json!({"type":"product.create","product":{"name":"Rose balm","slug":"rose-balm","price_cents":3200,"category_slug":"body","plugin_id":"oils","stock":4,"image":"/images/rose.webp","description":"A balm.","attributes":{"size":["50 ml"]}}}),
        json!({"type":"product.create","product":{"name":"Plain & Simple","slug":"plain","price_cents":1,"category_slug":"core","plugin_id":"","stock":0,"image":"/api/images/img_0123456789abcdef01234567","description":"","attributes":null}}),
        json!({"type":"product.create","product":{"name":"Minimal","slug":"minimal","price_cents":10,"category_slug":"body","plugin_id":"oils","stock":1,"image":"/images/a.webp","description":"D","attributes":{"size":[],"concerns":["dry",null]}}}),
        json!({"type":"product.create","product":{"Name":"Case Fields","Slug":"case-fields","Price_Cents":10,"Category_Slug":"body","Plugin_ID":"oils","Stock":1,"Image":"/images/a.webp","Description":"D","Attributes":{"Size":["50 ml"]}}}),
        json!({"type":"product.create","product":{"name":"Zero","slug":"zero","price_cents":-0,"category_slug":"body","plugin_id":"oils","stock":-0,"image":"/images/a.webp","description":"D"}}),
        json!({"type":"order.status","order_id":"ord_1","status":"shipped"}),
        json!({"type":"order.status","order_id":"ord_1","status":"cancelled"}),
        json!({"type":"order.status","order_id":"ord_1","status":" confirmed "}),
    ];

    for summary in summaries {
        for count in 1..=10 {
            for offset in 0..10 {
                let actions: Vec<Value> = (0..count)
                    .map(|index| action_pool[(index + offset) % action_pool.len()].clone())
                    .collect();
                documents.push(json!({"summary": summary, "actions": actions}));
            }
        }
    }

    // Fixed valid documents with edge shapes.
    for raw in [
        r#"{"summary":"A.","actions":[]}"#,
        r#"{"summary":"A.","actions":null}"#,
        r#"{"summary":null,"actions":[]}"#,
        r#"{"summary":" leading and trailing ","actions":[{"type":"product.archive","product_id":"p"}]}"#,
        r#"{"Summary":"A.","actions":[]}"#,
        r#"{"summary":"A.","actions":[],"summary":"last"}"#,
        r#"{"summary":"A.","actions":[{"type":"theme.update","patch":{"radius":4,"radius":6}}]}"#,
        r#"{"summary":"A.","actions":[{"type":"product.create","product":{"name":"Zero","slug":"zero","price_cents":-0,"category_slug":"body","plugin_id":"oils","stock":-0,"image":"/images/a.webp","description":"D"}}]}"#,
        r#"{"summary":"A.","actions":[{"type":"product.update","product_id":"p","patch":{"price_cents":-0}}]}"#,
        r#"```json
{"summary":"A.","actions":[]}
```"#,
        r#"```{"summary":"A.","actions":[]}```"#,
        "null",
    ] {
        documents.push(Value::String(raw.to_string()));
    }

    // Fixed invalid documents.
    for raw in [
        "",
        "[]",
        "\"text\"",
        "{",
        "{}",
        r#"{"summary":"A.","actions":[],"code":"rm -rf"}"#,
        r#"{"summary":"A.","actions":[],"Summary":"B"}"#,
        r#"{"summary":1,"actions":[]}"#,
        r#"{"summary":"A.","actions":{}}"#,
        r#"{"summary":"A.","actions":[null]}"#,
        r#"{"summary":"A.","actions":[1]}"#,
        r#"{"summary":"A.","actions":["x"]}"#,
        r#"{"summary":"A.","actions":[{"type":"order.cancel","order_id":"o"}]}"#,
        r#"{"summary":"A.","actions":[{"type":"theme.update","patch":{},"product_id":"p"}]}"#,
        r#"{"summary":"A.","actions":[{"type":"theme.update","patch":null}]}"#,
        r#"{"summary":"A.","actions":[{"type":"theme.update","patch":[]}]}"#,
        r#"{"summary":"A.","actions":[{"type":"theme.update"}]}"#,
        r#"{"summary":"A.","actions":[{"type":"product.update","product_id":"p"}]}"#,
        r#"{"summary":"A.","actions":[{"type":"product.update","patch":{"stock":1}}]}"#,
        r#"{"summary":"A.","actions":[{"type":"product.update","product_id":1,"patch":{"stock":1}}]}"#,
        r#"{"summary":"A.","actions":[{"type":"product.update","product_id":"p","patch":5}]}"#,
        r#"{"summary":"A.","actions":[{"type":"product.create"}]}"#,
        r#"{"summary":"A.","actions":[{"type":"product.create","product":null}]}"#,
        r#"{"summary":"A.","actions":[{"type":"product.create","product":[]}]}"#,
        r#"{"summary":"A.","actions":[{"type":"product.create","product":{"name":"x","extra":1}}]}"#,
        r#"{"summary":"A.","actions":[{"type":"product.create","product":{"price_cents":1.5}}]}"#,
        r#"{"summary":"A.","actions":[{"type":"product.create","product":{"price_cents":1e2}}]}"#,
        r#"{"summary":"A.","actions":[{"type":"product.create","product":{"stock":9223372036854775808}}]}"#,
        r#"{"summary":"A.","actions":[{"type":"product.create","product":{"attributes":{"size":"50 ml"}}}]}"#,
        r#"{"summary":"A.","actions":[{"type":"product.create","product":{"attributes":{"size":["a",1]}}}]}"#,
        r#"{"summary":"A.","actions":[{"type":"order.status","order_id":"o"}]}"#,
        r#"{"summary":"A.","actions":[{"type":"order.status","status":"shipped"}]}"#,
        r#"{"summary":"A.","actions":[{"type":"order.status","order_id":"","status":"shipped"}]}"#,
        r#"{"summary":"A.","actions":[{"type":"order.status","order_id":"o","status":null}]}"#,
        r#"{"summary":"A.","actions":[{"type":"order.status","order_id":"o","status":"lost"}]}"#,
        r#"{"summary":"A.","actions":[{"type":"order.status","order_id":"o","status":"shipped","price_cents":1}]}"#,
        r#"{"summary":"A.","actions":[{"type":null}]}"#,
        r#"{"summary":"A.","actions":[{"type":1}]}"#,
        r#"{"summary":"A.","actions":[{"type":""}]}"#,
        r#"{"summary":"A.","actions":[{"type":"theme.update","patch":{"radius":4}}]} {}"#,
        r#"{"summary":"A.","actions":[]} trailing"#,
        r#"{"summary":"A.","actions":[{"type":"theme.update","patch":{"radius":4}},]}"#,
    ] {
        documents.push(Value::String(raw.to_string()));
    }
    documents
}

#[test]
fn plan_parsing_matches_the_oracle() {
    let mut oracle = Oracle::start();
    let documents = plan_documents();
    assert!(
        documents.len() >= 300,
        "expected at least 300 plan documents, got {}",
        documents.len()
    );

    let mut compared = 0;
    for document in documents {
        let raw = match &document {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        let oracle_reply = oracle.call(&json!({"op": "parse_plan", "raw": raw}));
        let rust = parse_plan(&raw);
        if oracle_reply
            .get("ok")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            let plan = rust.unwrap_or_else(|error| panic!("Rust rejected {raw}: {error}"));
            let encoded = encode_plan(&plan);
            assert_eq!(
                encoded,
                oracle_reply
                    .get("plan")
                    .and_then(Value::as_str)
                    .expect("oracle plan"),
                "encoded plan differs for {raw}"
            );
        } else {
            assert!(
                rust.is_err(),
                "Rust accepted a plan the oracle rejected: {raw}"
            );
        }
        compared += 1;
    }
    println!("differential: compared {compared} plan documents");
    assert!(compared >= 300, "compared only {compared} documents");
}

// ---------------------------------------------------------------------------
// Shape validation
// ---------------------------------------------------------------------------

#[test]
fn plan_shape_validation_matches_the_oracle() {
    let themes = [
        empty_theme(),
        json!({"hero_heading":"A shorter shelf.","radius":4}),
        encoded_theme(&azahar_storefront::Theme::default()),
    ];
    let plans = [
        r#"{"summary":"Change.","actions":[{"type":"theme.update","patch":{"hero_heading":"A shorter shelf."}}]}"#,
        r#"{"summary":"Change.","actions":[{"type":"product.update","product_id":"prd_1","patch":{"price_cents":4200}}]}"#,
        r#"{"summary":"Change.","actions":[{"type":"product.archive","product_id":"prd_1"}]}"#,
        r#"{"summary":"Change.","actions":[{"type":"product.create","product":{"name":"Rose balm","slug":"rose-balm","price_cents":3200,"category_slug":"body","plugin_id":"oils","stock":4,"image":"/images/rose.webp","description":"A balm.","attributes":{"size":["50 ml"]}}}]}"#,
        r#"{"summary":"Change.","actions":[{"type":"order.status","order_id":"ord_1","status":"shipped"}]}"#,
        r#"{"summary":"","actions":[{"type":"product.archive","product_id":"prd_1"}]}"#,
        r#"{"summary":"Change.","actions":[]}"#,
        r#"{"summary":"Change.","actions":[{"type":"theme.update","patch":{}}]}"#,
        r#"{"summary":"Change.","actions":[{"type":"theme.update","patch":{"css":"x"}}]}"#,
        r#"{"summary":"Change.","actions":[{"type":"theme.update","patch":{"hero_heading":null}}]}"#,
        r#"{"summary":"Change.","actions":[{"type":"theme.update","patch":{"font_scale":4}}]}"#,
        r#"{"summary":"Change.","actions":[{"type":"product.update","product_id":" ","patch":{"price_cents":4}}]}"#,
        r#"{"summary":"Change.","actions":[{"type":"product.update","product_id":"p","patch":{}}]}"#,
        r#"{"summary":"Change.","actions":[{"type":"product.update","product_id":"p","patch":{"slug":"x"}}]}"#,
        r#"{"summary":"Change.","actions":[{"type":"product.archive","product_id":" "}]}"#,
        r#"{"summary":"Change.","actions":[{"type":"product.create","product":{"name":"x","slug":"x","price_cents":1,"category_slug":"body","plugin_id":"oils","stock":1,"image":"/images/a.webp"}}]}"#,
        r#"{"summary":"Change.","actions":[{"type":"product.create","product":{"name":"Rose balm","slug":"rose-balm","price_cents":3200,"category_slug":"body","plugin_id":"oils","stock":4,"image":"/images/rose.webp"}}]}"#,
        r#"{"summary":"Change.","actions":[{"type":"product.create","product":{"name":"Rose balm","slug":"rose-balm","price_cents":3200,"category_slug":"body","plugin_id":"missing","stock":4,"image":"/images/rose.webp"}}]}"#,
        r#"{"summary":"Change.","actions":[{"type":"product.create","product":{"name":"Rose balm","slug":"rose-balm","price_cents":3200,"category_slug":"body","plugin_id":"core","stock":4,"image":"/images/rose.webp","attributes":{"size":["50 ml"]}}}]}"#,
        r#"{"summary":"Change.","actions":[{"type":"order.status","order_id":"ord_1","status":" shipped "}]}"#,
        r#"{"summary":"Change.","actions":[{"type":"theme.update","patch":{"radius":1}},{"type":"theme.update","patch":{"radius":2}},{"type":"theme.update","patch":{"radius":3}},{"type":"theme.update","patch":{"radius":4}},{"type":"theme.update","patch":{"radius":5}},{"type":"theme.update","patch":{"radius":6}},{"type":"theme.update","patch":{"radius":7}},{"type":"theme.update","patch":{"radius":8}},{"type":"theme.update","patch":{"radius":9}},{"type":"theme.update","patch":{"radius":10}},{"type":"theme.update","patch":{"radius":11}}]}"#,
    ];

    let defs = plugin_defs();
    let registry = registry_from_defs(&defs);
    let mut oracle = Oracle::start();
    let mut generated = Vec::new();
    generated.push(format!(
        r#"{{"summary":"{}","actions":[{{"type":"product.archive","product_id":"p"}}]}}"#,
        "a".repeat(300)
    ));
    generated.push(format!(
        r#"{{"summary":"{}","actions":[{{"type":"product.archive","product_id":"p"}}]}}"#,
        "a".repeat(301)
    ));
    generated.push(
        r#"{"summary":"bell\u0007","actions":[{"type":"product.archive","product_id":"p"}]}"#
            .to_string(),
    );
    let generated_refs: Vec<&str> = generated.iter().map(String::as_str).collect();
    let mut compared = 0;
    for theme in &themes {
        for plan_doc in plans.iter().copied().chain(generated_refs.iter().copied()) {
            let oracle_reply = oracle.call(&json!({
                "op": "validate_shape",
                "plan": plan_doc,
                "theme": theme,
                "plugins": defs,
            }));
            let mut plan = parse_plan(plan_doc).expect("fixture plan parses");
            let theme_value = parse_go_theme(theme);
            let result = validate_plan_shape(&mut plan, &theme_value, Some(&registry));
            let oracle_ok = oracle_reply
                .get("ok")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            match result {
                Ok(next) => {
                    assert!(
                        oracle_ok,
                        "Rust accepted, oracle rejected {plan_doc}: {oracle_reply}"
                    );
                    assert_eq!(
                        encoded_theme_text(&next),
                        oracle_reply.get("theme").cloned().expect("theme"),
                        "theme differs for {plan_doc}"
                    );
                    assert_eq!(
                        encode_plan(&plan),
                        oracle_reply
                            .get("plan")
                            .and_then(Value::as_str)
                            .expect("plan"),
                        "mutated plan differs for {plan_doc}"
                    );
                }
                Err(error) => {
                    assert!(!oracle_ok, "Rust rejected, oracle accepted {plan_doc}");
                    assert_eq!(
                        error.to_string(),
                        oracle_reply
                            .get("error")
                            .and_then(Value::as_str)
                            .expect("error"),
                        "error text differs for {plan_doc}"
                    );
                }
            }
            compared += 1;
        }
    }
    println!("differential: compared {compared} shape-validation cases");
    assert!(compared >= 60);
}

/// Parses a theme document the way the oracle does. `{}` is the default.
fn parse_go_theme(value: &Value) -> azahar_storefront::Theme {
    azahar_storefront::parse(&value.to_string()).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Theme and product patches
// ---------------------------------------------------------------------------

#[test]
fn theme_patches_match_the_oracle() {
    let mut oracle = Oracle::start();
    let themes = [empty_theme(), json!({"hero_heading":"Current."})];
    let patches = [
        r#"{"hero_heading":"A shorter shelf."}"#,
        r##"{"hero_heading":"A shorter shelf.","light":{"accent":"#123456"}}"##,
        r#"{"radius":4,"font_scale":1.05,"concerns_enabled":false}"#,
        r#"{"brand_name":"Nour","contact_email":"salon@example.com","hero_image":"/images/x.webp"}"#,
        r##"{"light":{"ground":"#f6f0eb","surface":"#ebe0d9","ink":"#2c2325","muted":"#6a5d5e","accent":"#9a3c4c","accent_ink":"#ffffff"}}"##,
        r#"{}"#,
        r#"null"#,
        r#"[]"#,
        r#"{"css":"body{}"}"#,
        r#"{"updated_at":"2026-01-01T00:00:00Z"}"#,
        r##"{"light":{"background":"#ffffff"}}"##,
        r#"{"light":{}}"#,
        r#"{"light":null}"#,
        r#"{"light":{"accent":null}}"#,
        r#"{"hero_heading":null}"#,
        r#"{"hero_heading":""}"#,
        r#"{"font_scale":4}"#,
        r#"{"font_scale":"1"}"#,
        r#"{"radius":13}"#,
        r#"{"radius":4.0}"#,
        r#"{"contact_email":"not-an-email"}"#,
        r#"{"hero_image":"/etc/passwd"}"#,
        r#"{"font_display":"comic"}"#,
        r##"{"accent":"#ffffff"}"##,
        r#"{"radius":2,"radius":13}"#,
    ];
    for theme in &themes {
        let rust_theme = parse_go_theme(theme);
        for patch in patches {
            let oracle_reply = oracle.call(&json!({
                "op": "theme_patch",
                "theme": theme,
                "patch": serde_json::from_str::<Value>(patch).expect("patch value"),
            }));
            let raw = serde_json::value::RawValue::from_string(patch.to_string()).expect("raw");
            let result = azahar_plugin_codemode::apply_theme_patch(&rust_theme, Some(&raw));
            let oracle_ok = oracle_reply
                .get("ok")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            match result {
                Ok(next) => {
                    assert!(oracle_ok, "Rust accepted {patch}: {oracle_reply}");
                    assert_eq!(
                        encoded_theme_text(&next),
                        oracle_reply.get("theme").cloned().expect("theme"),
                        "{patch}"
                    );
                }
                Err(error) => {
                    assert!(!oracle_ok, "Rust rejected {patch}, oracle accepted");
                    assert_eq!(
                        error,
                        oracle_reply
                            .get("error")
                            .and_then(Value::as_str)
                            .expect("error"),
                        "{patch}"
                    );
                }
            }
        }
    }
}

/// Numbers compare by value: Go writes `font_scale` as `1`, serde as `1.0`.
fn numeric_eq(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, value)| b.get(key).is_some_and(|other| numeric_eq(value, other)))
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(value, other)| numeric_eq(value, other))
        }
        _ => left == right,
    }
}

fn patch_from_json(value: &Value) -> Box<serde_json::value::RawValue> {
    serde_json::value::RawValue::from_string(value.to_string()).expect("raw patch")
}

fn rust_product_patch(value: &Value) -> Result<ProductPatch, String> {
    parse_product_patch(Some(&patch_from_json(value)))
}

fn product_patch_json(patch: &ProductPatch) -> Value {
    let mut map = Map::new();
    if let Some(name) = &patch.name {
        map.insert("name".to_string(), Value::String(name.clone()));
    }
    if let Some(subtitle) = &patch.subtitle {
        map.insert("subtitle".to_string(), Value::String(subtitle.clone()));
    }
    if let Some(description) = &patch.description {
        map.insert(
            "description".to_string(),
            Value::String(description.clone()),
        );
    }
    if let Some(price) = patch.price_cents {
        map.insert("price_cents".to_string(), Value::from(price));
    }
    if let Some(stock) = patch.stock {
        map.insert("stock".to_string(), Value::from(stock));
    }
    if let Some(active) = patch.active {
        map.insert("active".to_string(), Value::Bool(active));
    }
    Value::Object(map)
}

#[test]
fn theme_objects_match_the_oracle() {
    let mut oracle = Oracle::start();
    let themes = [
        azahar_storefront::Theme::default(),
        azahar_storefront::Theme {
            brand_name: "Nour".to_string(),
            font_scale: 1.05,
            radius: 9,
            concerns_enabled: false,
            light: azahar_storefront::Palette {
                accent: "#123456".to_string(),
                ..azahar_storefront::Theme::default().light
            },
            ..azahar_storefront::Theme::default()
        },
    ];
    for theme in &themes {
        let oracle_reply =
            oracle.call(&json!({"op": "theme_object", "theme": encoded_theme(theme)}));
        assert_eq!(oracle_reply.get("ok").and_then(Value::as_bool), Some(true));
        let rust = serde_json::to_value(
            azahar_plugin_codemode::theme_object(theme).expect("theme object"),
        )
        .expect("json");
        assert!(
            numeric_eq(&rust, oracle_reply.get("object").expect("object")),
            "theme object differs: {rust} vs {}",
            oracle_reply["object"]
        );
        assert!(rust.get("updated_at").is_none());
    }
}

#[test]
fn product_patches_match_the_oracle() {
    let patches = [
        r#"{"price_cents":4200,"stock":9,"active":false,"name":"New name","subtitle":"Short","description":"Long enough."}"#,
        r#"{"name":"  padded  "}"#,
        r#"{"subtitle":"  padded  "}"#,
        r#"{"description":"  padded  "}"#,
        r#"{"price_cents":1}"#,
        r#"{"price_cents":99999999}"#,
        r#"{"price_cents":0}"#,
        r#"{"price_cents":100000000}"#,
        r#"{"price_cents":1.5}"#,
        r#"{"price_cents":1e2}"#,
        r#"{"stock":0}"#,
        r#"{"stock":100000}"#,
        r#"{"stock":-1}"#,
        r#"{"stock":100001}"#,
        r#"{"active":true}"#,
        r#"{"active":null}"#,
        r#"{"active":"true"}"#,
        r#"{"name":"x"}"#,
        r#"{"name":""}"#,
        r#"{"name":"a\u0007b"}"#,
        r#"{"name":"éé"}"#,
        r#"{"subtitle":""}"#,
        r#"{"description":""}"#,
        r#"{}"#,
        r#"null"#,
        r#"[]"#,
        r#"{"slug":"new-slug"}"#,
        r#"{"price_cents":null}"#,
        r#"{"price_cents":4,"price_cents":5}"#,
    ];
    let mut oracle = Oracle::start();
    for patch in patches {
        let value: Value = serde_json::from_str(patch).expect("patch value");
        let oracle_reply = oracle.call(&json!({"op": "product_patch", "patch": value}));
        let rust = rust_product_patch(&value);
        let oracle_ok = oracle_reply
            .get("ok")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        match rust {
            Ok(parsed) => {
                assert!(oracle_ok, "Rust accepted {patch}: {oracle_reply}");
                assert_eq!(
                    product_patch_json(&parsed),
                    oracle_reply.get("patch").cloned().expect("patch"),
                    "{patch}"
                );
            }
            Err(error) => {
                assert!(!oracle_ok, "Rust rejected {patch}, oracle accepted");
                assert_eq!(
                    error,
                    oracle_reply
                        .get("error")
                        .and_then(Value::as_str)
                        .expect("error"),
                    "{patch}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Drafts and attributes
// ---------------------------------------------------------------------------

fn draft_json(draft: &ProductDraft) -> Value {
    let mut map = Map::new();
    map.insert("name".to_string(), Value::String(draft.name.clone()));
    map.insert("slug".to_string(), Value::String(draft.slug.clone()));
    map.insert("price_cents".to_string(), Value::from(draft.price_cents));
    map.insert(
        "category_slug".to_string(),
        Value::String(draft.category_slug.clone()),
    );
    map.insert(
        "plugin_id".to_string(),
        Value::String(draft.plugin_id.clone()),
    );
    map.insert("stock".to_string(), Value::from(draft.stock));
    map.insert("image".to_string(), Value::String(draft.image.clone()));
    map.insert(
        "description".to_string(),
        Value::String(draft.description.clone()),
    );
    let mut attributes = Map::new();
    for (key, values) in &draft.attributes {
        attributes.insert(
            key.clone(),
            Value::Array(values.iter().cloned().map(Value::String).collect()),
        );
    }
    map.insert("attributes".to_string(), Value::Object(attributes));
    Value::Object(map)
}

fn base_draft() -> ProductDraft {
    ProductDraft {
        name: "Rose balm".to_string(),
        slug: "rose-balm".to_string(),
        price_cents: 3200,
        category_slug: "body".to_string(),
        plugin_id: "oils".to_string(),
        stock: 4,
        image: "/images/rose.webp".to_string(),
        description: "A balm.".to_string(),
        attributes: HashMap::from([("size".to_string(), vec!["50 ml".to_string()])]),
    }
}

#[test]
fn product_drafts_match_the_oracle() {
    let defs = plugin_defs();
    let registry = registry_from_defs(&defs);
    let mut cases = Vec::new();
    cases.push(base_draft());
    let mut core = base_draft();
    core.plugin_id = String::new();
    core.attributes.clear();
    cases.push(core);
    let mut core_options = base_draft();
    core_options.plugin_id = "core".to_string();
    cases.push(core_options);
    let mut missing_size = base_draft();
    missing_size.attributes.clear();
    cases.push(missing_size);
    let mut trimmed = base_draft();
    trimmed.name = "  Rose balm  ".to_string();
    trimmed.slug = "  rose-balm  ".to_string();
    trimmed.category_slug = " body ".to_string();
    trimmed.plugin_id = " oils ".to_string();
    trimmed.description = "  A balm.  ".to_string();
    trimmed.image = "  /images/rose.webp  ".to_string();
    trimmed.attributes = HashMap::from([(" size ".to_string(), vec![" 50 ml ".to_string()])]);
    cases.push(trimmed);
    for (name, slug, price, plugin, stock, image) in [
        ("x", "rose-balm", 3200, "oils", 4, "/images/rose.webp"),
        (
            "Rose balm",
            "Rose-Balm",
            3200,
            "oils",
            4,
            "/images/rose.webp",
        ),
        (
            "Rose balm",
            "rose--balm",
            3200,
            "oils",
            4,
            "/images/rose.webp",
        ),
        ("Rose balm", "rose-balm", 0, "oils", 4, "/images/rose.webp"),
        (
            "Rose balm",
            "rose-balm",
            100_000_000,
            "oils",
            4,
            "/images/rose.webp",
        ),
        (
            "Rose balm",
            "rose-balm",
            3200,
            "oils",
            -1,
            "/images/rose.webp",
        ),
        (
            "Rose balm",
            "rose-balm",
            3200,
            "oils",
            100_001,
            "/images/rose.webp",
        ),
        ("Rose balm", "rose-balm", 3200, "oils", 4, "/etc/passwd"),
        (
            "Rose balm",
            "rose-balm",
            3200,
            "OILS",
            4,
            "/images/rose.webp",
        ),
        ("Rose balm", "rose-balm", 3200, "o", 4, "/images/rose.webp"),
        ("a\u{7}b", "rose-balm", 3200, "oils", 4, "/images/rose.webp"),
    ] {
        let mut draft = base_draft();
        draft.name = name.to_string();
        draft.slug = slug.to_string();
        draft.price_cents = price;
        draft.plugin_id = plugin.to_string();
        draft.stock = stock;
        draft.image = image.to_string();
        cases.push(draft);
    }
    let mut duplicate = base_draft();
    duplicate.attributes = HashMap::from([
        ("size".to_string(), vec!["50 ml".to_string()]),
        (" size ".to_string(), vec!["30 ml".to_string()]),
    ]);
    cases.push(duplicate);
    let mut empty_key = base_draft();
    empty_key.attributes = HashMap::from([("  ".to_string(), vec!["x".to_string()])]);
    cases.push(empty_key);

    let mut oracle = Oracle::start();
    for case in cases {
        let mut draft = case.clone();
        let oracle_reply = oracle.call(&json!({
            "op": "product_draft",
            "patch": draft_json(&case),
            "plugins": defs,
        }));
        let rust = validate_product_draft(&mut draft, Some(&registry));
        let oracle_ok = oracle_reply
            .get("ok")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        match rust {
            Ok(()) => {
                assert!(oracle_ok, "Rust accepted {case:?}: {oracle_reply}");
                assert_eq!(
                    Value::String(encode_product_draft(&draft)),
                    oracle_reply.get("draft").cloned().expect("draft"),
                    "{case:?}"
                );
            }
            Err(error) => {
                assert!(!oracle_ok, "Rust rejected {case:?}, oracle accepted");
                assert_eq!(
                    error,
                    oracle_reply
                        .get("error")
                        .and_then(Value::as_str)
                        .expect("error"),
                    "{case:?}"
                );
            }
        }
    }
}

#[test]
fn attributes_and_product_rules_match_the_oracle() {
    let mut oracle = Oracle::start();
    let attribute_cases = [
        HashMap::from([("size".to_string(), vec!["50 ml".to_string()])]),
        HashMap::from([(" size ".to_string(), vec![" 50 ml ".to_string()])]),
        HashMap::from([
            ("size".to_string(), vec!["50 ml".to_string()]),
            (" size ".to_string(), vec!["30 ml".to_string()]),
        ]),
        HashMap::from([("  ".to_string(), vec!["x".to_string()])]),
        HashMap::new(),
    ];
    for attributes in &attribute_cases {
        let oracle_reply =
            oracle.call(&json!({"op": "normalize_attributes", "attributes": attributes}));
        let rust = normalize_attributes(attributes);
        let oracle_ok = oracle_reply
            .get("ok")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        match rust {
            Ok(normalized) => {
                assert!(oracle_ok, "Rust accepted {attributes:?}: {oracle_reply}");
                assert_eq!(
                    serde_json::to_value(&normalized).expect("json"),
                    oracle_reply.get("attributes").cloned().expect("attributes")
                );
            }
            Err(error) => {
                assert!(!oracle_ok, "Rust rejected {attributes:?}, oracle accepted");
                assert_eq!(
                    error,
                    oracle_reply
                        .get("error")
                        .and_then(Value::as_str)
                        .expect("error")
                );
            }
        }
    }

    let product_type = oils_product_type();
    let product_type_json = json!({
        "label": "Oil",
        "fields": [
            {"key":"size","label":"Size","type":"text","required":true},
            {"key":"concerns","label":"Concerns","type":"list","required":false}
        ]
    });
    let validation_cases = [
        HashMap::from([("size".to_string(), vec!["50 ml".to_string()])]),
        HashMap::new(),
        HashMap::from([
            ("size".to_string(), vec!["50 ml".to_string()]),
            ("extra".to_string(), vec!["x".to_string()]),
        ]),
        HashMap::from([(
            "size".to_string(),
            vec!["50 ml".to_string(), "30 ml".to_string()],
        )]),
        HashMap::from([("size".to_string(), vec![String::new()])]),
        HashMap::from([("size".to_string(), vec!["x".repeat(121)])]),
        HashMap::from([(
            "concerns".to_string(),
            vec!["dry".to_string(), " dry ".to_string()],
        )]),
        HashMap::from([(
            "concerns".to_string(),
            (0..13).map(|index| format!("c{index}")).collect(),
        )]),
        HashMap::from([("concerns".to_string(), vec!["x".repeat(81)])]),
    ];
    for attributes in &validation_cases {
        let oracle_reply = oracle.call(&json!({
            "op": "product_attributes",
            "product_type": product_type_json,
            "attributes": attributes,
        }));
        let rust = validate_product_attributes(&product_type, attributes);
        let oracle_ok = oracle_reply
            .get("ok")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        match rust {
            Ok(()) => assert!(oracle_ok, "Rust accepted {attributes:?}: {oracle_reply}"),
            Err(error) => {
                assert!(!oracle_ok, "Rust rejected {attributes:?}, oracle accepted");
                assert_eq!(
                    error,
                    oracle_reply
                        .get("error")
                        .and_then(Value::as_str)
                        .expect("error")
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Fingerprints, baselines, first field, settings
// ---------------------------------------------------------------------------

#[test]
fn fingerprints_and_baselines_match_the_oracle() {
    let products = [
        wire_product("prd_1", "A <name> & more", 4200, 9),
        wire_product("prd_2", "Été", 1, 0),
        Product {
            active: false,
            subtitle: String::new(),
            description: "line\nbreak".to_string(),
            sale_price_cents: Some(1),
            ..wire_product("prd_3", "Name", 100, 3)
        },
    ];
    let mut oracle = Oracle::start();
    for product in &products {
        let oracle_reply = oracle.call(&json!({
            "op": "fingerprint",
            "products": json_products(std::slice::from_ref(product)),
        }));
        assert_eq!(
            product_fingerprint(product),
            oracle_reply
                .get("fingerprint")
                .and_then(Value::as_str)
                .expect("fingerprint")
        );
    }

    let theme = azahar_storefront::Theme::default();
    let orders = [
        json!({"id":"ord_1","number":"AZ-1","status":"pending"}),
        json!({"id":"ord_2","number":"AZ-2","status":"shipped"}),
    ];
    let plans = [
        r#"{"summary":"A.","actions":[{"type":"theme.update","patch":{"radius":4}}]}"#,
        r#"{"summary":"A.","actions":[{"type":"product.update","product_id":"prd_1","patch":{"stock":1}}]}"#,
        r#"{"summary":"A.","actions":[{"type":"product.archive","product_id":"prd_3"}]}"#,
        r#"{"summary":"A.","actions":[{"type":"order.status","order_id":"ord_2","status":"delivered"}]}"#,
        r#"{"summary":"A.","actions":[{"type":"product.update","product_id":"missing","patch":{"stock":1}}]}"#,
        r#"{"summary":"A.","actions":[{"type":"order.status","order_id":"missing","status":"confirmed"}]}"#,
        r#"{"summary":"A.","actions":[{"type":"product.create","product":{"name":"Rose balm","slug":"rose-balm","price_cents":3200,"category_slug":"body","plugin_id":"oils","stock":4,"image":"/images/rose.webp","description":"A balm.","attributes":{"size":["50 ml"]}}}]}"#,
    ];
    for plan_doc in plans {
        let plan = parse_plan(plan_doc).expect("plan");
        let prompt_orders: Vec<azahar_plugin_codemode::PromptOrder> = orders
            .iter()
            .map(|order| azahar_plugin_codemode::PromptOrder {
                id: order["id"].as_str().unwrap_or_default().to_string(),
                number: order["number"].as_str().unwrap_or_default().to_string(),
                status: order["status"].as_str().unwrap_or_default().to_string(),
                updated_at: String::new(),
            })
            .collect();
        let oracle_reply = oracle.call(&json!({
            "op": "baseline",
            "plan": plan_doc,
            "theme": encoded_theme(&theme),
            "products": json_products(&products),
            "orders": orders,
        }));
        let rust = plan_baseline(&plan, &products, &prompt_orders, &theme);
        let oracle_ok = oracle_reply
            .get("ok")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        match rust {
            Ok(baseline) => {
                assert!(oracle_ok, "Rust accepted {plan_doc}: {oracle_reply}");
                assert_eq!(
                    baseline,
                    oracle_reply
                        .get("baseline")
                        .and_then(Value::as_str)
                        .expect("baseline"),
                    "{plan_doc}"
                );
            }
            Err(error) => {
                assert!(!oracle_ok, "Rust rejected {plan_doc}, oracle accepted");
                assert_eq!(
                    error.to_string(),
                    oracle_reply
                        .get("error")
                        .and_then(Value::as_str)
                        .expect("error"),
                    "{plan_doc}"
                );
            }
        }
    }

    let fields = HashMap::from([
        ("zeta".to_string(), "Z".to_string()),
        ("alpha".to_string(), "A".to_string()),
    ]);
    let oracle_reply = oracle.call(&json!({"op": "first_field", "fields": fields}));
    assert_eq!(
        first_field_message(&std::collections::BTreeMap::from([
            ("zeta".to_string(), "Z".to_string()),
            ("alpha".to_string(), "A".to_string()),
        ]))
        .expect("message"),
        oracle_reply
            .get("message")
            .and_then(Value::as_str)
            .expect("message")
    );
}

#[test]
fn settings_and_manifest_match_the_oracle() {
    let mut oracle = Oracle::start();
    let settings_cases = [
        json!({"api_key":"sk-test","model":"gpt-4o-mini","enabled":"true"}),
        json!({"api_key":"","model":"gpt 4o","enabled":"yes"}),
        json!({"api_key":" sk ","model":"gpt-4o-mini"}),
        json!({"api_key":"sk","model":"gpt-4o-mini","enabled":"false"}),
        json!({"api_key":"sk","model":"a/b:c.d-e_f","enabled":"true"}),
    ];
    for settings in &settings_cases {
        let map: HashMap<String, String> = settings
            .as_object()
            .expect("object")
            .iter()
            .map(|(key, value)| (key.clone(), value.as_str().unwrap_or_default().to_string()))
            .collect();
        let oracle_reply = oracle.call(&json!({"op": "validate_settings", "settings": settings}));
        assert_eq!(
            serde_json::to_value(validate_settings(&map)).expect("json"),
            oracle_reply.get("fields").cloned().expect("fields")
        );
        let oracle_reply = oracle.call(&json!({"op": "config", "settings": settings}));
        assert_eq!(
            oracle_reply.get("enabled").and_then(Value::as_bool),
            Some(azahar_plugin_codemode::enabled_from_settings(&map))
        );
        match config_from_settings(&map) {
            Ok(config) => {
                assert_eq!(oracle_reply.get("ok").and_then(Value::as_bool), Some(true));
                assert_eq!(
                    oracle_reply.get("api_key").and_then(Value::as_str),
                    Some(config.api_key.as_str())
                );
                assert_eq!(
                    oracle_reply.get("model").and_then(Value::as_str),
                    Some(config.model.as_str())
                );
            }
            Err(error) => {
                assert_eq!(oracle_reply.get("ok").and_then(Value::as_bool), Some(false));
                assert_eq!(
                    oracle_reply.get("error").and_then(Value::as_str),
                    Some(error.to_string().as_str())
                );
            }
        }
    }

    let oracle_reply = oracle.call(&json!({"op": "manifest"}));
    assert_eq!(
        serde_json::to_value(manifest()).expect("manifest"),
        oracle_reply.get("manifest").cloned().expect("manifest")
    );
    let oracle_reply = oracle.call(&json!({"op": "migrations"}));
    assert_eq!(
        serde_json::to_value(migrations()).expect("migrations"),
        oracle_reply.get("migrations").cloned().expect("migrations")
    );
}

// ---------------------------------------------------------------------------
// Prompt
// ---------------------------------------------------------------------------

#[test]
fn prompts_match_the_oracle_byte_for_byte() {
    let defs = plugin_defs();
    let registry = registry_from_defs(&defs);
    let mut oracle = Oracle::start();

    let mut product = wire_product("prd_1", "Rose <oil> & co", 4200, 9);
    product.category = Category {
        slug: "body".to_string(),
        plugin_id: "oils".to_string(),
        ..Category::default()
    };
    let products = [
        product.clone(),
        Product {
            plugin_id: String::new(),
            ..wire_product("prd_2", "Été", 1, 0)
        },
    ];
    let categories = [
        Category {
            slug: "body".to_string(),
            plugin_id: "oils".to_string(),
            ..Category::default()
        },
        Category {
            slug: "core".to_string(),
            plugin_id: String::new(),
            ..Category::default()
        },
    ];
    let orders = [
        json!({"id":"ord_1","number":"AZ-1","status":"pending"}),
        json!({"id":"ord_2","number":"AZ-2","status":"shipped"}),
    ];
    let prompt_orders: Vec<azahar_plugin_codemode::PromptOrder> = orders
        .iter()
        .map(|order| azahar_plugin_codemode::PromptOrder {
            id: order["id"].as_str().unwrap_or_default().to_string(),
            number: order["number"].as_str().unwrap_or_default().to_string(),
            status: order["status"].as_str().unwrap_or_default().to_string(),
            updated_at: String::new(),
        })
        .collect();

    let themes = [
        azahar_storefront::Theme::default(),
        azahar_storefront::Theme {
            hero_heading: "A <shorter> shelf.".to_string(),
            light: azahar_storefront::Palette {
                accent: "#123456".to_string(),
                ..azahar_storefront::Theme::default().light
            },
            ..azahar_storefront::Theme::default()
        },
    ];
    for theme in &themes {
        let oracle_reply = oracle.call(&json!({
            "op": "prompt",
            "theme": encoded_theme(theme),
            "products": json_products(&products),
            "categories": [
                {"slug":"body","plugin_id":"oils"},
                {"slug":"core","plugin_id":""}
            ],
            "orders": orders,
            "plugins": defs,
        }));
        let rust = build_prompt(
            theme,
            &products,
            &categories,
            &prompt_orders,
            &prompt_plugins(Some(&registry)),
        );
        assert_eq!(
            rust,
            oracle_reply
                .get("prompt")
                .and_then(Value::as_str)
                .expect("prompt")
        );
    }

    let oracle_reply = oracle.call(&json!({"op": "prompt_plugins", "plugins": defs}));
    assert_eq!(
        serde_json::to_value(prompt_plugins(Some(&registry))).expect("plugins"),
        oracle_reply.get("plugins").cloned().expect("plugins")
    );
}

// ---------------------------------------------------------------------------
// Database apply: both directions
// ---------------------------------------------------------------------------

const ACTOR: &str = "usr_itcm_admin";
const PLUGIN_SECRET: &str = "azahar-itcm-differential-key";
const FIXED_CREATED: &str = "2026-11-24T10:00:00Z";
const FIXED_NOW: &str = "2026-11-24T11:00:00Z";

async fn database_store() -> Store {
    let dsn = dsn();
    let database = azahar_db::open(&dsn).await.unwrap_or_else(|error| {
        panic!(
            "code-mode differential needs the libSQL test server at {dsn}: {error}\nrun `make dev-db` first"
        )
    });
    let store = Store::new(database).expect("store");
    azahar_db::migrate(store.conn())
        .await
        .unwrap_or_else(|error| panic!("migrate test database at {dsn}: {error}"));
    let migrations: Vec<azahar_db::PluginMigration> = migrations()
        .into_iter()
        .map(|migration| azahar_db::PluginMigration {
            plugin_id: "codemode".to_string(),
            version: migration.version,
            sql: migration.sql,
        })
        .collect();
    azahar_db::migrate_plugins(store.conn(), migrations)
        .await
        .expect("code-mode migrations");
    store
}

async fn execute(store: &Store, sql: &str, params: impl libsql::params::IntoParams) {
    store
        .conn()
        .execute(sql, params)
        .await
        .unwrap_or_else(|error| panic!("execute {sql}: {error}"));
}

async fn reset_fixtures(store: &Store) {
    execute(
        store,
        "DELETE FROM order_items WHERE order_id LIKE 'itcm_%'",
        (),
    )
    .await;
    execute(store, "DELETE FROM orders WHERE id LIKE 'itcm_%'", ()).await;
    execute(store, "DELETE FROM products WHERE slug LIKE 'itcm-%'", ()).await;
    execute(store, "DELETE FROM categories WHERE slug LIKE 'itcm-%'", ()).await;
    execute(
        store,
        "DELETE FROM audit_outbox WHERE detail LIKE '%cmp_itcm_%'",
        (),
    )
    .await;
    execute(
        store,
        "DELETE FROM codemode_plans WHERE id LIKE 'cmp_itcm_%'",
        (),
    )
    .await;
    execute(
        store,
        "DELETE FROM storefront_settings WHERE id = 'default'",
        (),
    )
    .await;
}

async fn seed_base(store: &Store, with_sale: bool) -> (Category, Product, Order) {
    execute(
        store,
        "INSERT INTO categories (id, slug, name, description, sort_order, plugin_id)
         VALUES ('cat_itcm_body', 'itcm-body', 'Itcm Body', '', 1, 'oils')",
        (),
    )
    .await;
    let now = parse_time("2099-11-20T10:00:00Z");
    let category = Category {
        id: "cat_itcm_body".to_string(),
        slug: "itcm-body".to_string(),
        name: "Itcm Body".to_string(),
        plugin_id: "oils".to_string(),
        ..Category::default()
    };
    let product = Product {
        id: "prd_itcm_oil".to_string(),
        slug: "itcm-rose-oil".to_string(),
        name: "Rose oil".to_string(),
        subtitle: "A rose oil".to_string(),
        description: "Pressed roses.".to_string(),
        price_cents: 4200,
        sale_price_cents: if with_sale { Some(4000) } else { None },
        sale_starts_at: if with_sale {
            parse_time("2026-11-01T00:00:00Z")
        } else {
            None
        },
        sale_ends_at: if with_sale {
            parse_time("2026-12-01T00:00:00Z")
        } else {
            None
        },
        currency: "DZD".to_string(),
        category: category.clone(),
        image: "/images/rose.webp".to_string(),
        stock: 5,
        active: true,
        plugin_id: "oils".to_string(),
        attributes: HashMap::from([("size".to_string(), vec!["50 ml".to_string()])]),
        created_at: now,
        updated_at: now,
        ..Product::default()
    };
    store
        .create_product(store.conn(), &product)
        .await
        .expect("create fixture product");

    let order = Order {
        id: "itcm_ord_1".to_string(),
        number: "AZ-ITCM-1".to_string(),
        name: "Buyer".to_string(),
        email: "buyer@example.test".to_string(),
        phone: "0550123456".to_string(),
        address: "1 Rue de Test".to_string(),
        city: "Alger".to_string(),
        subtotal_cents: 8400,
        total_cents: 8400,
        currency: "DZD".to_string(),
        status: "pending".to_string(),
        token_hash: "itcm-token".to_string(),
        created_at: parse_time("2099-11-21T10:00:00Z"),
        updated_at: parse_time("2099-11-21T10:00:00Z"),
        ..Order::default()
    };
    let item = OrderItem {
        id: "oit_itcm_1".to_string(),
        order_id: order.id.clone(),
        product_id: product.id.clone(),
        slug: product.slug.clone(),
        name: product.name.clone(),
        image: product.image.clone(),
        unit_price_cents: product.price_cents,
        quantity: 2,
        line_total_cents: 8400,
        ..OrderItem::default()
    };
    store
        .create_order(store.conn(), &order, &[item])
        .await
        .expect("create fixture order");
    (category, product, order)
}

async fn read_default_theme(store: &Store) -> Option<(String, String)> {
    let mut rows = store
        .conn()
        .query(
            "SELECT data, updated_at FROM storefront_settings WHERE id = 'default'",
            (),
        )
        .await
        .expect("read theme row");
    rows.next().await.expect("theme row").map(|row| {
        (
            row.get::<String>(0).unwrap_or_default(),
            row.get::<String>(1).unwrap_or_default(),
        )
    })
}

async fn restore_default_theme(store: &Store, saved: Option<(String, String)>) {
    execute(
        store,
        "DELETE FROM storefront_settings WHERE id = 'default'",
        (),
    )
    .await;
    if let Some((data, updated_at)) = saved {
        execute(
            store,
            "INSERT INTO storefront_settings (id, data, updated_at) VALUES ('default', ?, ?)",
            libsql::params![data, updated_at],
        )
        .await;
    }
}

async fn seed_theme(store: &Store, theme: &azahar_storefront::Theme) {
    let payload = azahar_storefront::encode(theme).expect("theme");
    store
        .save_storefront_settings(
            store.conn(),
            &payload,
            parse_time("2026-11-24T09:00:00Z").expect("time"),
        )
        .await
        .expect("save theme");
}

async fn seed_plan(store: &Store, plan_id: &str, plan: &Plan, baseline: &str) {
    execute(
        store,
        "INSERT INTO codemode_plans (id, actor_user_id, instruction, plan, status, result, baseline, created_at)
         VALUES (?, ?, 'test', ?, 'preview', '', ?, ?)",
        libsql::params![plan_id, ACTOR, encode_plan(plan), baseline, FIXED_CREATED],
    )
    .await;
}

async fn baseline_from_store(store: &Store, plan: &Plan) -> String {
    let service = Codemode::new(store, None, &NoopHooks);
    let theme = service.load_theme().await.expect("theme");
    let products = store
        .list_products(&ProductFilter {
            include_inactive: true,
            limit: 500,
            ..ProductFilter::default()
        })
        .await
        .expect("products");
    let orders = service.recent_orders().await.expect("orders");
    plan_baseline(plan, &products, &orders, &theme).expect("baseline")
}

fn plugin_defs_value() -> Value {
    json!([{
        "id": "oils",
        "label": "Oil",
        "fields": [
            {"key":"size","label":"Size","type":"text","required":true},
            {"key":"concerns","label":"Concerns","type":"list","required":false}
        ]
    }])
}

fn oils_registry() -> RwLock<Registry> {
    let mut registry = Registry::new();
    registry
        .register(Arc::new(DefPlugin {
            id: "oils".to_string(),
            name: "Oil shelf".to_string(),
            product_type: Some(oils_product_type()),
        }))
        .expect("register oils");
    RwLock::new(registry)
}

async fn dump_state(store: &Store, plan_id: &str) -> Value {
    let (plan_status, plan_result) = {
        let mut rows = store
            .conn()
            .query(
                "SELECT status, result FROM codemode_plans WHERE id = ?",
                libsql::params![plan_id],
            )
            .await
            .expect("plan row");
        match rows.next().await.expect("row") {
            Some(row) => (
                row.get::<String>(0).unwrap_or_default(),
                row.get::<String>(1).unwrap_or_default(),
            ),
            None => (String::new(), String::new()),
        }
    };

    let theme = {
        let mut rows = store
            .conn()
            .query(
                "SELECT data FROM storefront_settings WHERE id = 'default'",
                (),
            )
            .await
            .expect("theme row");
        match rows.next().await.expect("row") {
            Some(row) => row.get::<String>(0).unwrap_or_default(),
            None => String::new(),
        }
    };

    let mut products = Vec::new();
    {
        let mut rows = store
            .conn()
            .query(
                "SELECT p.id, p.slug, p.name, p.subtitle, p.description, p.price_cents, p.stock,
                        p.active, p.plugin_id, p.image, p.attributes, c.slug
                 FROM products p JOIN categories c ON c.id = p.category_id
                 WHERE p.slug LIKE 'itcm-%' ORDER BY p.slug",
                (),
            )
            .await
            .expect("products");
        while let Some(row) = rows.next().await.expect("row") {
            let attributes: String = row.get(10).unwrap_or_default();
            products.push(json!({
                "id": row.get::<String>(0).unwrap_or_default(),
                "slug": row.get::<String>(1).unwrap_or_default(),
                "name": row.get::<String>(2).unwrap_or_default(),
                "subtitle": row.get::<String>(3).unwrap_or_default(),
                "description": row.get::<String>(4).unwrap_or_default(),
                "price_cents": row.get::<i64>(5).unwrap_or_default(),
                "stock": row.get::<i64>(6).unwrap_or_default(),
                "active": row.get::<i64>(7).unwrap_or_default() != 0,
                "plugin_id": row.get::<String>(8).unwrap_or_default(),
                "image": row.get::<String>(9).unwrap_or_default(),
                "attributes": serde_json::from_str::<Value>(&attributes).unwrap_or(Value::Null),
                "category_slug": row.get::<String>(11).unwrap_or_default(),
            }));
        }
    }

    let mut orders = Vec::new();
    {
        let mut rows = store
            .conn()
            .query(
                "SELECT id, number, status FROM orders WHERE id LIKE 'itcm_%' ORDER BY id",
                (),
            )
            .await
            .expect("orders");
        while let Some(row) = rows.next().await.expect("row") {
            orders.push(json!({
                "id": row.get::<String>(0).unwrap_or_default(),
                "number": row.get::<String>(1).unwrap_or_default(),
                "status": row.get::<String>(2).unwrap_or_default(),
            }));
        }
    }

    let mut audits = Vec::new();
    {
        let mut rows = store
            .conn()
            .query(
                "SELECT action, entity_type, entity_id, detail FROM audit_outbox
                 WHERE detail LIKE '%cmp_itcm_%' ORDER BY action, entity_id",
                (),
            )
            .await
            .expect("audits");
        while let Some(row) = rows.next().await.expect("row") {
            audits.push(json!({
                "action": row.get::<String>(0).unwrap_or_default(),
                "entity_type": row.get::<String>(1).unwrap_or_default(),
                "entity_id": row.get::<String>(2).unwrap_or_default(),
                "detail": row.get::<String>(3).unwrap_or_default(),
            }));
        }
    }

    json!({
        "plan_status": plan_status,
        "plan_result": plan_result,
        "theme": theme,
        "products": products,
        "orders": orders,
        "audits": audits,
    })
}

/// Generated ids differ between the two implementations; normalise them so
/// the rest of the row can be compared.
fn normalize_state(mut state: Value) -> Value {
    if let Some(products) = state.get_mut("products").and_then(Value::as_array_mut) {
        for product in products {
            let generated = product
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| !id.starts_with("prd_itcm_"));
            if generated {
                product["id"] = Value::String("<generated>".to_string());
            }
        }
    }
    if let Some(audits) = state.get_mut("audits").and_then(Value::as_array_mut) {
        for audit in audits {
            let id = audit
                .get("entity_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if id.starts_with("prd_") && !id.starts_with("prd_itcm_") {
                audit["entity_id"] = Value::String("<generated>".to_string());
            }
        }
    }
    state
}

struct ScenarioResult {
    status: u16,
    body: Value,
    state: Value,
}

async fn run_rust(store: &Store, plan_id: &str) -> ScenarioResult {
    let registry = oils_registry();
    let hooks = NoopHooks;
    let service = Codemode::new(store, Some(&registry), &hooks);
    let settings = HashMap::new();
    let now = parse_time(FIXED_NOW).expect("now");
    let outcome = service
        .apply(&ApplyInput {
            plan_id,
            actor_user_id: ACTOR,
            settings: Some(&settings),
            now,
        })
        .await;
    let (status, body) = match outcome {
        Ok(body) => (200, serde_json::from_str(&body).expect("body")),
        Err(problem) => {
            let mut payload = Map::new();
            let mut error = Map::new();
            error.insert("code".to_string(), Value::String(problem.code.clone()));
            error.insert(
                "message".to_string(),
                Value::String(problem.message.clone()),
            );
            if let Some(fields) = problem.fields {
                error.insert(
                    "fields".to_string(),
                    serde_json::to_value(fields).expect("fields"),
                );
            }
            payload.insert("error".to_string(), Value::Object(error));
            (problem.status.as_u16(), Value::Object(payload))
        }
    };
    ScenarioResult {
        status,
        body,
        state: normalize_state(dump_state(store, plan_id).await),
    }
}

async fn run_go(oracle: &mut Oracle, plan_id: &str) -> ScenarioResult {
    let reply = oracle.call(&json!({
        "op": "apply",
        "actor": ACTOR,
        "plan_id": plan_id,
        "secret_key": PLUGIN_SECRET,
        "plugins": plugin_defs_value(),
    }));
    if let Some(error) = reply.get("error") {
        panic!("Go apply oracle failed: {error}");
    }
    ScenarioResult {
        status: u16::try_from(
            reply
                .get("status")
                .and_then(Value::as_u64)
                .unwrap_or_default(),
        )
        .unwrap_or_default(),
        body: serde_json::from_str(
            reply
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
        .expect("Go body"),
        state: normalize_state(reply.get("state").cloned().expect("state")),
    }
}

async fn seed_scenario(
    store: &Store,
    plan_id: &str,
    plan_doc: &str,
    theme: &azahar_storefront::Theme,
    with_sale: bool,
    baseline_override: Option<String>,
) -> Plan {
    seed_base(store, with_sale).await;
    seed_theme(store, theme).await;
    let plan = parse_plan(plan_doc).expect("plan");
    let baseline = match baseline_override {
        Some(baseline) => baseline,
        None => baseline_from_store(store, &plan).await,
    };
    seed_plan(store, plan_id, &plan, &baseline).await;
    plan
}

#[tokio::test]
async fn apply_matches_the_oracle_in_both_directions() {
    let store = database_store().await;
    let mut oracle = Oracle::start();
    let saved_theme = read_default_theme(&store).await;
    reset_fixtures(&store).await;

    let default_theme = azahar_storefront::Theme::default();
    let plans = [
        (
            "cmp_itcm_theme",
            r##"{"summary":"Theme.","actions":[{"type":"theme.update","patch":{"hero_heading":"A smaller shelf.","light":{"accent":"#123456"}}}]}"##,
            false,
            false,
        ),
        (
            "cmp_itcm_product",
            r#"{"summary":"Product.","actions":[{"type":"product.update","product_id":"prd_itcm_oil","patch":{"price_cents":4500,"stock":9,"name":"New rose oil"}}]}"#,
            false,
            false,
        ),
        (
            "cmp_itcm_archive",
            r#"{"summary":"Archive.","actions":[{"type":"product.archive","product_id":"prd_itcm_oil"}]}"#,
            false,
            false,
        ),
        (
            "cmp_itcm_create",
            r#"{"summary":"Create.","actions":[{"type":"product.create","product":{"name":"Saffron balm","slug":"itcm-saffron-balm","price_cents":3200,"category_slug":"itcm-body","plugin_id":"oils","stock":6,"image":"/images/saffron.webp","description":"A balm.","attributes":{"size":["30 ml"],"concerns":["dry"]}}}]}"#,
            false,
            false,
        ),
        (
            "cmp_itcm_archive_create",
            r#"{"summary":"Replace.","actions":[{"type":"product.archive","product_id":"prd_itcm_oil"},{"type":"product.create","product":{"name":"Saffron balm","slug":"itcm-saffron-balm","price_cents":3200,"category_slug":"itcm-body","plugin_id":"oils","stock":6,"image":"/images/saffron.webp","description":"A balm.","attributes":{"size":["30 ml"]}}}]}"#,
            false,
            false,
        ),
        (
            "cmp_itcm_confirm",
            r#"{"summary":"Confirm.","actions":[{"type":"order.status","order_id":"itcm_ord_1","status":"confirmed"}]}"#,
            false,
            false,
        ),
        (
            "cmp_itcm_cancel",
            r#"{"summary":"Cancel.","actions":[{"type":"order.status","order_id":"itcm_ord_1","status":"cancelled"}]}"#,
            false,
            false,
        ),
        (
            "cmp_itcm_bad_transition",
            r#"{"summary":"Bad.","actions":[{"type":"order.status","order_id":"itcm_ord_1","status":"shipped"}]}"#,
            false,
            false,
        ),
        (
            "cmp_itcm_missing_product",
            r#"{"summary":"Missing.","actions":[{"type":"product.update","product_id":"prd_itcm_missing","patch":{"stock":1}}]}"#,
            false,
            false,
        ),
        (
            "cmp_itcm_rollback",
            r#"{"summary":"Rollback.","actions":[{"type":"product.archive","product_id":"prd_itcm_oil"},{"type":"product.update","product_id":"prd_itcm_oil","patch":{"price_cents":3000}}]}"#,
            true,
            false,
        ),
        (
            "cmp_itcm_stale_theme",
            r#"{"summary":"Theme.","actions":[{"type":"theme.update","patch":{"radius":6}}]}"#,
            false,
            false,
        ),
        (
            "cmp_itcm_already",
            r#"{"summary":"Applied.","actions":[{"type":"theme.update","patch":{"radius":6}}]}"#,
            false,
            false,
        ),
    ];

    let mut compared = 0;
    let mut observed = Vec::new();
    for (plan_id, plan_doc, with_sale, _) in plans {
        // Stale-theme scenario: seed a baseline from a different theme.
        let (baseline_override, applied) = if plan_id == "cmp_itcm_stale_theme" {
            let mut other = default_theme.clone();
            other.hero_heading = "Some other heading.".to_string();
            let plan = parse_plan(plan_doc).expect("plan");
            let baseline = {
                let service = Codemode::new(&store, None, &NoopHooks);
                let products = store
                    .list_products(&ProductFilter {
                        include_inactive: true,
                        limit: 500,
                        ..ProductFilter::default()
                    })
                    .await
                    .expect("products");
                let orders = service.recent_orders().await.expect("orders");
                plan_baseline(&plan, &products, &orders, &other).expect("baseline")
            };
            (Some(baseline), false)
        } else if plan_id == "cmp_itcm_missing_product" || plan_id == "cmp_itcm_bad_transition" {
            // Both fail at the reference check, before the baseline is read.
            (Some("{}".to_string()), false)
        } else {
            (None, plan_id == "cmp_itcm_already")
        };

        // Go side.
        seed_scenario(
            &store,
            plan_id,
            plan_doc,
            &default_theme,
            with_sale,
            baseline_override.clone(),
        )
        .await;
        if applied {
            execute(
                &store,
                "UPDATE codemode_plans SET status = 'applied' WHERE id = ?",
                libsql::params![plan_id],
            )
            .await;
        }
        let go = run_go(&mut oracle, plan_id).await;
        reset_fixtures(&store).await;

        // Rust side.
        seed_scenario(
            &store,
            plan_id,
            plan_doc,
            &default_theme,
            with_sale,
            baseline_override,
        )
        .await;
        if applied {
            execute(
                &store,
                "UPDATE codemode_plans SET status = 'applied' WHERE id = ?",
                libsql::params![plan_id],
            )
            .await;
        }
        let rust = run_rust(&store, plan_id).await;
        reset_fixtures(&store).await;

        observed.push((plan_id, go, rust));
        compared += 1;
    }
    restore_default_theme(&store, saved_theme).await;

    for (plan_id, go, rust) in observed {
        assert_eq!(go.status, rust.status, "{plan_id}: status");
        assert_eq!(go.body, rust.body, "{plan_id}: body");
        assert_eq!(go.state, rust.state, "{plan_id}: state");
    }
    println!("differential: compared {compared} database apply scenarios");
    assert!(compared >= 12);
}
