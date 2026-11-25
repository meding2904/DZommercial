//! Differential test for the oils manifest against
//! `backend/cmd/oracle-plugin-data`, plus the registration checks the Go test
//! suite covers.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Arc;

use azahar_plugin_oils::Plugin;
use azahar_plugins::{Plugin as PluginApi, Registry};
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
            .args(["run", "./cmd/oracle-plugin-data"])
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

    fn request(&mut self, request: &Value) -> Value {
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

#[test]
fn manifest_matches_the_go_oracle() {
    let mut oracle = Oracle::spawn();
    let response = oracle.request(&json!({"plugin": "oils"}));
    assert_eq!(response["error"], Value::Null, "oracle error: {response}");
    let manifests = response["manifests"].as_array().expect("manifests");
    assert_eq!(manifests.len(), 1);
    assert_eq!(
        manifests[0],
        serde_json::to_value(PluginApi::manifest(&Plugin)).expect("serialize manifest"),
        "oils manifest differs from Go"
    );

    let response = oracle.request(&json!({"plugin": "all"}));
    let manifests = response["manifests"].as_array().expect("manifests");
    assert_eq!(manifests.len(), 2);
    assert_eq!(
        manifests[0],
        serde_json::to_value(PluginApi::manifest(&Plugin)).expect("serialize manifest"),
        "oils manifest differs inside the combined response"
    );
}

#[test]
fn declares_the_oil_fields() {
    let manifest = PluginApi::manifest(&Plugin);
    assert_eq!(manifest.id, "oils");
    let product_type = manifest.product_type.as_ref().expect("product type");
    let fields: std::collections::HashMap<&str, &str> = product_type
        .fields
        .iter()
        .map(|field| (field.key.as_str(), field.field_type.as_str()))
        .collect();
    for (key, expected) in [
        ("size", "text"),
        ("concerns", "list"),
        ("ingredients", "list"),
        ("how_to", "textarea"),
    ] {
        assert_eq!(fields[key], expected);
    }
    assert_eq!(
        product_type
            .fields
            .iter()
            .find(|field| field.key == "concerns")
            .expect("concerns field")
            .suggestions,
        vec!["dry", "sensitive", "dull", "oily", "tired"]
    );
}

#[test]
fn registers_with_the_registry() {
    Registry::new()
        .register(Arc::new(Plugin))
        .expect("register oils");
}
