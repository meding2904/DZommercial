//! Differential test for the PC builder parts catalog shaping.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use azahar_plugin_pcbuilder::{Plugin, build_parts_response};
use azahar_plugins::Plugin as PluginTrait;
use azahar_store::{Category, Product};

struct Oracle {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl Oracle {
    fn start() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut child = Command::new("go")
            .args(["run", "-tags", "oracle", "./cmd/oracle-pcbuilder"])
            .current_dir(root.join("backend"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("go run oracle-pcbuilder");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Self {
            child,
            stdin,
            stdout,
        }
    }

    fn call(&mut self, request: &Value) -> Value {
        writeln!(self.stdin, "{request}").expect("write");
        self.stdin.flush().expect("flush");
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("read");
        serde_json::from_str(&line).expect("json")
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn category_json(category: &Category) -> Value {
    json!({
        "ID": category.id, "Slug": category.slug, "Name": category.name,
        "Description": category.description, "SortOrder": category.sort_order,
        "PluginID": category.plugin_id,
    })
}

fn product_json(product: &Product) -> Value {
    json!({
        "ID": product.id, "Slug": product.slug, "Name": product.name,
        "Subtitle": product.subtitle, "Description": product.description,
        "PriceCents": product.price_cents, "CompareAtCents": product.compare_at_cents,
        "SalePriceCents": product.sale_price_cents,
        "Currency": product.currency, "Size": product.size,
        "Category": category_json(&product.category),
        "Concerns": product.concerns, "Ingredients": product.ingredients,
        "HowTo": product.how_to, "Image": product.image, "Gallery": product.gallery,
        "Stock": product.stock, "Featured": product.featured, "Active": product.active,
        "PluginID": product.plugin_id, "Attributes": product.attributes,
    })
}

fn category(slug: &str, name: &str, plugin: &str, order: i32) -> Category {
    Category {
        id: format!("cat_{slug}"),
        slug: slug.to_string(),
        name: name.to_string(),
        plugin_id: plugin.to_string(),
        sort_order: order,
        ..Default::default()
    }
}

fn product(slug: &str, name: &str, plugin: &str, category: &Category, active: bool) -> Product {
    Product {
        id: format!("prd_{slug}"),
        slug: slug.to_string(),
        name: name.to_string(),
        plugin_id: plugin.to_string(),
        active,
        category: category.clone(),
        attributes: HashMap::from([("socket".to_string(), vec!["AM5".to_string()])]),
        ..Default::default()
    }
}

#[test]
fn parts_catalog_matches_the_go_oracle() {
    let mut oracle = Oracle::start();

    let manifest = Plugin.manifest();
    let expected = oracle.call(&json!({"op":"manifest"}));
    assert_eq!(
        serde_json::to_value(&manifest).expect("manifest json"),
        expected["manifest"]
    );

    let cpu = category("cpu", "CPU", "pcparts", 2);
    let gpu = category("gpu", "GPU", "pcparts", 1);
    let oils = category("oils", "Oils", "oils", 0);
    let categories = vec![cpu.clone(), gpu.clone(), oils.clone()];
    let products = vec![
        product("b-cpu", "Beta CPU", "pcparts", &cpu, true),
        product("a-gpu", "Alpha GPU", "pcparts", &gpu, true),
        product("hidden", "Hidden GPU", "pcparts", &gpu, false),
        product("oil", "Oil", "oils", &oils, true),
    ];

    let expected = oracle.call(&json!({
        "op":"parts",
        "categories": categories.iter().map(category_json).collect::<Vec<_>>(),
        "products": products.iter().map(product_json).collect::<Vec<_>>(),
    }));
    let actual = serde_json::to_value(build_parts_response(&categories, &products)).expect("json");
    assert_eq!(actual, expected["response"]);
}
