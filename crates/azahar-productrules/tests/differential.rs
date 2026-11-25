//! Differential test for product write rules.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use azahar_plugins::{OptionField, ProductType};
use azahar_productrules::{
    Kind, Problem, normalize_attributes, plain_text, valid_image_id, valid_image_path, valid_name,
    valid_price, valid_stock, validate_attributes,
};

struct Oracle {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl Oracle {
    fn start() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut child = Command::new("go")
            .args(["run", "./cmd/oracle-productrules"])
            .current_dir(root.join("backend"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("go run oracle-productrules");
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

fn schema_json(fields: &[(&str, &str, &str, bool)]) -> Value {
    Value::Array(
        fields
            .iter()
            .map(|(key, label, kind, required)| {
                json!({"key": key, "label": label, "type": kind, "required": required})
            })
            .collect(),
    )
}

fn schema(fields: &[(&str, &str, &str, bool)]) -> ProductType {
    ProductType {
        label: "oracle".into(),
        summary: String::new(),
        fields: fields
            .iter()
            .map(|(key, label, kind, required)| OptionField {
                key: (*key).to_string(),
                label: (*label).to_string(),
                field_type: (*kind).to_string(),
                required: *required,
                help: String::new(),
                suggestions: Vec::new(),
            })
            .collect(),
    }
}

fn attributes_json(attributes: &HashMap<String, Vec<String>>) -> Value {
    let mut map = serde_json::Map::new();
    for (key, values) in attributes {
        map.insert(key.clone(), json!(values));
    }
    Value::Object(map)
}

fn problem_json(problem: &Problem) -> Value {
    let kind = match problem.kind {
        Kind::MissingRequired => 1,
        Kind::SingleValue => 2,
        Kind::TooManyValues => 3,
        Kind::InvalidText => 4,
        Kind::InvalidValue => 5,
        Kind::DuplicateValue => 6,
        Kind::UnknownField => 7,
        Kind::EmptyOptionName => 8,
        Kind::DuplicateOptionName => 9,
    };
    json!({"field": problem.field, "kind": kind, "label": problem.label, "limit": problem.limit})
}

#[test]
fn product_rules_match_the_go_oracle() {
    let mut oracle = Oracle::start();

    let strings = [
        "",
        "a",
        "é",
        "éé",
        "olive oil",
        "  olive  ",
        "img_0123456789abcdef01234567",
        "img_0123456789ABCDEF01234567",
        "/images/products/cleansing-oil.webp",
        "/api/images/img_0123456789abcdef01234567",
        "/images/../secret",
        "/images/%2e%2e/secret.webp",
        "javascript:alert(1)",
        "bell\u{7}",
        "Two\nlines",
        "Tab\there",
        "del\u{7f}",
        &"a".repeat(121),
        &"a".repeat(500),
    ];
    for value in strings {
        for (op, rust) in [
            ("name", valid_name(value)),
            ("image_id", valid_image_id(value)),
            ("image_path", valid_image_path(value)),
            ("plain_text", plain_text(value)),
        ] {
            let expected = oracle.call(&json!({"op": op, "value": value}));
            assert_eq!(expected["ok"].as_bool(), Some(rust), "{op} {value:?}");
        }
    }
    for cents in [i64::MIN, -1, 0, 1, 99_999_999, 100_000_000, i64::MAX] {
        let expected = oracle.call(&json!({"op": "price", "cents": cents}));
        assert_eq!(
            expected["ok"].as_bool(),
            Some(valid_price(cents)),
            "price {cents}"
        );
    }
    for stock in [i64::MIN, -1, 0, 100_000, 100_001, i64::MAX] {
        let expected = oracle.call(&json!({"op": "stock", "stock": stock}));
        assert_eq!(
            expected["ok"].as_bool(),
            Some(valid_stock(stock)),
            "stock {stock}"
        );
    }

    let fields = [
        ("storage", "Storage", "select", true),
        ("size", "Size", "text", false),
        ("notes", "Notes", "textarea", false),
        ("concerns", "Concerns", "list", false),
    ];
    let rust_schema = schema(&fields);
    let mut state: u64 = 0x0123_4567_89ab_cdef;
    let mut cases: Vec<HashMap<String, Vec<String>>> = vec![
        HashMap::new(),
        HashMap::from([("storage".to_string(), vec!["128 GB".to_string()])]),
    ];
    let keys = [
        "storage",
        "size",
        "notes",
        "concerns",
        "battery",
        " storage ",
    ];
    let long = "a".repeat(2001);
    let values = [
        "",
        "128 GB",
        " 128 GB ",
        "dry",
        "dry",
        long.as_str(),
        "bell\u{7}",
    ];
    for _ in 0..300 {
        let entries = (state as usize) % 6;
        let mut attributes = HashMap::new();
        for _ in 0..entries {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let key = keys[(state as usize) % keys.len()].to_string();
            let count = 1 + ((state >> 8) as usize % 3);
            let mut list = Vec::new();
            for _ in 0..count {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                list.push(values[(state as usize) % values.len()].to_string());
            }
            attributes.insert(key, list);
        }
        cases.push(attributes);
    }

    for attributes in cases {
        let expected = oracle.call(&json!({"op":"validate","fields":schema_json(&fields),"attributes":attributes_json(&attributes)}));
        let got: Vec<Value> = validate_attributes(&rust_schema, &attributes)
            .iter()
            .map(problem_json)
            .collect();
        assert_eq!(
            expected["problems"].as_array(),
            Some(&got),
            "validate {attributes:?}"
        );

        let expected =
            oracle.call(&json!({"op":"normalize","attributes":attributes_json(&attributes)}));
        let (normalized, problems) = normalize_attributes(&attributes);
        let expected_normalized: HashMap<String, Vec<String>> =
            serde_json::from_value(expected["attributes"].clone()).unwrap_or_default();
        assert_eq!(expected_normalized, normalized, "normalize {attributes:?}");
        let got: Vec<Value> = problems.iter().map(problem_json).collect();
        assert_eq!(
            expected["problems"].as_array(),
            Some(&got),
            "normalize problems {attributes:?}"
        );
    }
}
