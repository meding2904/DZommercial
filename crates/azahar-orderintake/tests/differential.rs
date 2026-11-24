//! Differential test for basket lines and delivery rules.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Map, Value, json};

use azahar_orderintake::{Line, build_lines, no_shipping, shipping_cents};

struct Oracle {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl Oracle {
    fn start() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut child = Command::new("go")
            .args(["run", "./cmd/oracle-orderintake-pure"])
            .current_dir(root.join("backend"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("go run oracle-orderintake-pure");
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

fn rust_line(line: &Line) -> Value {
    let mut options = Map::new();
    for (key, value) in &line.options {
        options.insert(key.clone(), Value::String(value.clone()));
    }
    json!({
        "product_id": line.product_id,
        "quantity": line.quantity,
        "options": options,
        "expected_price_cents": line.expected_price_cents,
    })
}

#[test]
fn lines_and_shipping_match_the_go_oracle() {
    let mut oracle = Oracle::start();

    for subtotal in [
        i64::MIN,
        -1,
        0,
        59_999,
        60_000,
        799_999,
        800_000,
        800_001,
        i64::MAX,
    ] {
        let expected = oracle.call(&json!({"op":"shipping","subtotal_cents":subtotal}));
        assert_eq!(
            expected["cents"].as_i64(),
            Some(shipping_cents(subtotal)),
            "shipping {subtotal}"
        );
        let expected = oracle.call(&json!({"op":"no_shipping","subtotal_cents":subtotal}));
        assert_eq!(
            expected["cents"].as_i64(),
            Some(no_shipping(subtotal)),
            "no_shipping {subtotal}"
        );
    }

    let product_ids = ["prd_a", " prd_b ", "", "  ", "prd_a "];
    let option_sets = [
        vec![],
        vec![(" colour ", " rose ")],
        vec![("colour", "rose")],
        vec![("a", "x y"), ("b", "é&")],
        vec![("a", "x"), ("a", "y")],
    ];
    let mut state: u64 = 0xdead_beef_1234_5678;
    let mut cases: Vec<Vec<Line>> = Vec::new();
    for _ in 0..250 {
        let count = 1 + (state as usize % 5);
        let mut lines = Vec::new();
        for _ in 0..count {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let product_id = product_ids[(state as usize) % product_ids.len()].to_string();
            let quantity = ((state >> 8) % 12) as i64;
            let options: HashMap<String, String> = option_sets
                [(state as usize) % option_sets.len()]
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect();
            let expected = if state.is_multiple_of(3) {
                Some((state % 1000) as i64)
            } else {
                None
            };
            lines.push(Line {
                product_id,
                quantity,
                options,
                expected_price_cents: expected,
            });
        }
        cases.push(lines);
    }

    let mut mismatches = Vec::new();
    for lines in cases {
        let payload: Vec<Value> = lines
            .iter()
            .map(|line| {
                let mut options = Map::new();
                for (key, value) in &line.options {
                    options.insert(key.clone(), Value::String(value.clone()));
                }
                json!({
                    "product_id": line.product_id,
                    "quantity": line.quantity,
                    "options": options,
                    "expected_price_cents": line.expected_price_cents,
                })
            })
            .collect();
        let expected = oracle.call(&json!({"op":"build_lines","items":payload}));
        let (built, fields) = build_lines(lines);
        let actual_lines: Vec<Value> = built.iter().map(rust_line).collect();
        let expected_lines = expected["lines"].as_array().cloned().unwrap_or_default();
        let expected_fields: HashMap<String, String> =
            serde_json::from_value(expected["fields"].clone()).unwrap_or_default();
        if expected_lines != actual_lines || expected_fields != fields {
            mismatches.push(format!(
                "lines differ:\n go={expected_lines:?} fields={expected_fields:?}\n rust={actual_lines:?} fields={fields:?}"
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} mismatches, first 3:\n{}",
        mismatches.len(),
        mismatches
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
