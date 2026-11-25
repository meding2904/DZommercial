//! Differential test against the Go oracle (`backend/cmd/oracle-courier`).
//!
//! Both sides point `base_url` at an unreachable local port, so the delivery
//! map exercises the bundled snapshot and the sandbox exercises its local
//! shipment/tracking placeholders. Nothing here contacts the real courier.
//!
//! Only the fallback path is oracle-compared. The cache TTL, stale-cache and
//! offline-window branches need a controllable clock, so they are covered by
//! the unit tests in `client.rs`: `fresh_wilayas_are_served_from_the_cache`,
//! `an_expired_map_refetches_and_refreshes_its_ttl`,
//! `a_stale_map_beats_the_snapshot_when_the_courier_fails`,
//! `sandbox_desks_fall_back_locally_and_stay_cached_for_five_minutes`,
//! `sandbox_falls_back_when_the_courier_is_offline` and
//! `the_offline_window_expires_and_the_network_is_tried_again`.
//!
//! Tracking timestamps come from each implementation's own clock. They cannot
//! be byte-identical, so the test compares the stable fields (tracking number,
//! status, event count, event statuses and raw statuses) and checks that each
//! timestamp parses as RFC 3339 within five minutes of the other side's.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use azahar_courier::{Client, Courier, Error, Options, OrderRequest, Recipient};
use chrono::DateTime;
use serde_json::{Value, json};

const UNREACHABLE: &str = "http://127.0.0.1:1";

struct Oracle {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Oracle {
    fn start() -> Self {
        match Command::new("go").arg("version").output() {
            Ok(output) if output.status.success() => {}
            Ok(_) | Err(_) => panic!(
                "the Go toolchain is required for the courier differential test; \
                 install go and re-run `cargo test -p azahar-courier`"
            ),
        }

        let backend = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../backend");
        let mut child = Command::new("go")
            .args(["run", "./cmd/oracle-courier"])
            .current_dir(backend)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn go oracle");
        let stdin = child.stdin.take().expect("oracle stdin");
        let stdout = child.stdout.take().expect("oracle stdout");
        Self {
            child,
            stdin: BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
        }
    }

    fn command(&mut self, command: &Value) -> Value {
        writeln!(self.stdin, "{command}").expect("write oracle command");
        self.stdin.flush().expect("flush oracle command");
        let mut line = String::new();
        let read = self
            .stdout
            .read_line(&mut line)
            .expect("read oracle response");
        assert!(
            read > 0,
            "the oracle exited before answering; check the go build output"
        );
        serde_json::from_str(&line).expect("decode oracle response")
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn option_value<'a>(command: &'a Value, key: &str) -> Option<&'a Value> {
    command
        .get("options")
        .and_then(|options| options.get(key))
        .or_else(|| command.get(key))
        .filter(|value| !value.is_null())
}

fn option_str<'a>(command: &'a Value, key: &str) -> Option<&'a str> {
    option_value(command, key).and_then(Value::as_str)
}

fn option_i64(command: &Value, key: &str) -> Option<i64> {
    option_value(command, key).and_then(Value::as_i64)
}

fn option_bool(command: &Value, key: &str) -> Option<bool> {
    option_value(command, key).and_then(Value::as_bool)
}

fn options_from(command: &Value, notified: &Arc<Mutex<Vec<String>>>) -> Options {
    let recorder = Arc::clone(notified);
    Options {
        base_url: option_str(command, "base_url")
            .unwrap_or(UNREACHABLE)
            .to_owned(),
        courier_key: option_str(command, "courier_key")
            .unwrap_or("sandbox")
            .to_owned(),
        credentials: BTreeMap::new(),
        from_wilaya: option_i64(command, "from_wilaya").unwrap_or(16) as i32,
        timeout: Duration::from_millis(option_i64(command, "timeout_ms").unwrap_or(0).max(0) as u64),
        cache_ttl: Duration::from_millis(
            option_i64(command, "cache_ttl_ms").unwrap_or(0).max(0) as u64
        ),
        allow_local_sandbox: option_bool(command, "allow_local_sandbox").unwrap_or(true),
        on_fallback: Some(Arc::new(move |resource| {
            recorder
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(resource.to_owned());
        })),
    }
}

fn order_request(command: &Value) -> OrderRequest {
    let recipient = command.get("recipient");
    let field = |key: &str| -> String {
        recipient
            .and_then(|value| value.get(key))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    OrderRequest {
        reference: command
            .get("reference")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        recipient: Recipient {
            full_name: field("full_name"),
            phone: field("phone"),
            phone_alt: field("phone_alt"),
            wilaya_code: recipient
                .and_then(|value| value.get("wilaya_code"))
                .and_then(Value::as_i64)
                .unwrap_or_default() as i32,
            commune_name: field("commune_name"),
            address_line: field("address_line"),
        },
        delivery_type: command
            .get("delivery_type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        stop_desk_id: command
            .get("stop_desk_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        product_list: command
            .get("product_list")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        cod_amount: command
            .get("cod_amount")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        free_shipping: command
            .get("free_shipping")
            .and_then(Value::as_bool)
            .unwrap_or_default(),
        notes: command
            .get("notes")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    }
}

fn error_kind(error: &Error) -> &'static str {
    match error {
        Error::UnknownWilaya => "unknown_wilaya",
        Error::UnknownCommune => "unknown_commune",
        Error::UnknownDesk => "unknown_desk",
        Error::Transport(_) => "transport",
        Error::Api(_) => "api",
        Error::NoSnapshot => "no_snapshot",
        Error::Encode(_) => "encode",
        Error::BuildRequest(_) => "build_request",
    }
}

fn error_json(error: &Error) -> Value {
    if let Some(api) = error.api() {
        let mut body = serde_json::Map::new();
        body.insert("kind".into(), json!("api"));
        body.insert("message".into(), json!(api.to_string()));
        if api.status != 0 {
            body.insert("status".into(), json!(api.status));
        }
        if !api.code.is_empty() {
            body.insert("code".into(), json!(api.code));
        }
        if !api.fields.is_empty() {
            body.insert("fields".into(), json!(api.fields));
        }
        let retry_after = api.retry_after.as_millis() as i64;
        if retry_after > 0 {
            body.insert("retry_after_ms".into(), json!(retry_after));
        }
        Value::Object(body)
    } else {
        json!({"kind": error_kind(error), "message": error.to_string()})
    }
}

async fn run_rust(command: &Value) -> Value {
    let notified = Arc::new(Mutex::new(Vec::new()));
    let options = options_from(command, &notified);
    let client = Client::new(options);
    let op = command
        .get("op")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let result: Result<Value, Error> = match op {
        "wilayas" => client.wilayas().await.map(|value| json!(value)),
        "wilaya" => {
            let code = command.get("code").and_then(Value::as_i64).unwrap_or(0) as i32;
            client.wilaya(code).await.map(|value| json!(value))
        }
        "communes" => {
            let wilaya = command.get("wilaya").and_then(Value::as_i64).unwrap_or(0) as i32;
            client.communes(wilaya).await.map(|value| json!(value))
        }
        "commune" => {
            let wilaya = command.get("wilaya").and_then(Value::as_i64).unwrap_or(0) as i32;
            let name = command
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            client.commune(wilaya, name).await.map(Value::String)
        }
        "desks" => {
            let wilaya = command.get("wilaya").and_then(Value::as_i64).unwrap_or(0) as i32;
            client.desks(wilaya).await.map(|value| json!(value))
        }
        "desk" => {
            let wilaya = command.get("wilaya").and_then(Value::as_i64).unwrap_or(0) as i32;
            let id = command
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            client.desk(wilaya, id).await.map(|value| json!(value))
        }
        "create_order" => client
            .create_order(order_request(command))
            .await
            .map(|value| json!(value)),
        "track" => {
            let tracking_number = command
                .get("tracking_number")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let courier_key = option_str(command, "courier_key").unwrap_or("sandbox");
            client
                .track(courier_key, tracking_number)
                .await
                .map(|value| json!(value))
        }
        other => Err(Error::BuildRequest(format!("unknown op {other}"))),
    };

    let fallbacks = notified
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    match result {
        Ok(value) => json!({"ok": true, "value": value, "fallbacks": fallbacks}),
        Err(error) => json!({
            "ok": false,
            "error": error_json(&error),
            "fallbacks": fallbacks,
        }),
    }
}

fn compare_tracking(go: &Value, rust: &Value, problems: &mut Vec<String>) {
    let go_value = &go["value"];
    let rust_value = &rust["value"];
    for field in ["trackingNumber", "status"] {
        if go_value.get(field) != rust_value.get(field) {
            problems.push(format!(
                "track.{field}: go={} rust={}",
                go_value
                    .get(field)
                    .map(Value::to_string)
                    .unwrap_or_default(),
                rust_value
                    .get(field)
                    .map(Value::to_string)
                    .unwrap_or_default()
            ));
        }
    }

    let go_events = go_value["events"].as_array().cloned().unwrap_or_default();
    let rust_events = rust_value["events"].as_array().cloned().unwrap_or_default();
    if go_events.len() != rust_events.len() {
        problems.push(format!(
            "track.events: go={} rust={}",
            go_events.len(),
            rust_events.len()
        ));
        return;
    }

    for (index, (go_event, rust_event)) in go_events.iter().zip(&rust_events).enumerate() {
        for field in ["status", "rawStatus"] {
            if go_event.get(field) != rust_event.get(field) {
                problems.push(format!(
                    "track.events[{index}].{field}: go={} rust={}",
                    go_event
                        .get(field)
                        .map(Value::to_string)
                        .unwrap_or_default(),
                    rust_event
                        .get(field)
                        .map(Value::to_string)
                        .unwrap_or_default()
                ));
            }
        }
        let go_timestamp = go_event
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok());
        let rust_timestamp = rust_event
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok());
        match (go_timestamp, rust_timestamp) {
            (Some(go_timestamp), Some(rust_timestamp)) => {
                let drift = (go_timestamp - rust_timestamp).num_seconds().abs();
                if drift > 300 {
                    problems.push(format!("track.events[{index}].timestamp: drift {drift}s"));
                }
            }
            _ => problems.push(format!(
                "track.events[{index}].timestamp is not RFC 3339: go={} rust={}",
                go_event
                    .get("timestamp")
                    .map(Value::to_string)
                    .unwrap_or_default(),
                rust_event
                    .get("timestamp")
                    .map(Value::to_string)
                    .unwrap_or_default()
            )),
        }
    }
}

fn compare(command: &Value, go: &Value, rust: &Value, problems: &mut Vec<String>) {
    if go["fallbacks"] != rust["fallbacks"] {
        problems.push(format!(
            "fallbacks: go={} rust={}",
            go["fallbacks"], rust["fallbacks"]
        ));
    }
    if go["ok"] != rust["ok"] {
        problems.push(format!("ok: go={} rust={}", go["ok"], rust["ok"]));
        return;
    }
    if go["ok"] == json!(false) {
        let go_kind = go["error"]["kind"].as_str().unwrap_or_default();
        let rust_kind = rust["error"]["kind"].as_str().unwrap_or_default();
        if go_kind != rust_kind {
            problems.push(format!(
                "error.kind: go={go_kind} rust={rust_kind} ({})",
                command
            ));
            return;
        }
        // Transport messages come from the two standard libraries and cannot
        // match word for word; the typed kind is the contract.
        if go_kind != "transport" && go["error"] != rust["error"] {
            problems.push(format!("error: go={} rust={}", go["error"], rust["error"]));
        }
        return;
    }

    if command["op"] == json!("track") {
        compare_tracking(go, rust, problems);
    } else if go["value"] != rust["value"] {
        problems.push(format!("value: go={} rust={}", go["value"], rust["value"]));
    }
}

fn base_cases() -> Vec<Value> {
    let mut cases = Vec::new();

    cases.push(json!({"op": "wilayas"}));
    for code in 1..=58 {
        cases.push(json!({"op": "wilaya", "code": code}));
    }
    for code in [0, 59, 999] {
        cases.push(json!({"op": "wilaya", "code": code}));
    }

    for wilaya in 1..=58 {
        cases.push(json!({"op": "communes", "wilaya": wilaya}));
        cases.push(json!({"op": "desks", "wilaya": wilaya}));
    }

    let communes: Vec<Value> =
        serde_json::from_str(include_str!("../data/communes.json")).expect("commune snapshot");
    for (index, commune) in communes.iter().enumerate() {
        if index % 17 != 0 && index != communes.len() - 1 {
            continue;
        }
        let wilaya = commune["wilayaCode"].as_i64().unwrap_or_default();
        let name = commune["nameFr"].as_str().unwrap_or_default();
        cases.push(json!({"op": "commune", "wilaya": wilaya, "name": name}));
        if index % 51 == 0 {
            cases.push(json!({
                "op": "commune",
                "wilaya": wilaya,
                "name": name.to_lowercase(),
            }));
            cases.push(json!({
                "op": "commune",
                "wilaya": wilaya,
                "name": format!("  {name}  "),
            }));
        }
    }
    cases.push(json!({"op": "commune", "wilaya": 16, "name": "Nowhere"}));
    cases.push(json!({"op": "commune", "wilaya": 999, "name": "Bab Ezzouar"}));

    for id in ["SBX-1", "SBX-16", "SBX-58", "SBX-999", ""] {
        cases.push(json!({"op": "desk", "wilaya": 16, "id": id}));
    }
    cases.push(json!({"op": "desks", "wilaya": 999}));
    cases.push(json!({"op": "communes", "wilaya": 999}));

    cases
}

fn order_cases() -> Vec<Value> {
    let mut cases = Vec::new();
    cases.push(json!({"op": "create_order", "reference": "AZ-2026-0041"}));
    cases.push(json!({"op": "create_order", "reference": ""}));
    cases.push(json!({"op": "create_order", "reference": " az/2026-0041 é! "}));
    cases.push(json!({"op": "create_order", "reference": "ß-ﬀ"}));
    cases.push(json!({"op": "create_order", "reference": "x".repeat(80)}));
    cases.push(json!({
        "op": "create_order",
        "reference": "AZ-2026-0042",
        "recipient": {
            "full_name": "Amina Bouzid",
            "phone": "0551234567",
            "phone_alt": "0661234567",
            "wilaya_code": 16,
            "commune_name": "Bab Ezzouar",
            "address_line": "12 rue des Oliviers",
        },
        "delivery_type": "home",
        "stop_desk_id": "SBX-16",
        "product_list": "Orange blossom cleansing oil x1",
        "cod_amount": 5400,
        "free_shipping": true,
        "notes": "Call before delivery",
    }));
    cases.push(json!({
        "op": "create_order",
        "reference": "AZ-2026-0043",
        "options": {"courier_key": "yalidine"},
    }));
    cases.push(json!({
        "op": "create_order",
        "reference": "AZ-2026-0044",
        "options": {"courier_key": "sandbox", "allow_local_sandbox": false},
    }));
    cases.push(json!({
        "op": "create_order",
        "reference": "AZ-2026-0045",
        "options": {"from_wilaya": 0},
    }));
    cases
}

fn track_cases() -> Vec<Value> {
    vec![
        json!({"op": "track", "tracking_number": "SBX-AZ-2026-0041"}),
        json!({"op": "track", "tracking_number": ""}),
        json!({"op": "track", "tracking_number": " weird/编号 "}),
        json!({
            "op": "track",
            "tracking_number": "SBX-AZ-2026-0041",
            "options": {"courier_key": "yalidine"},
        }),
        json!({
            "op": "track",
            "tracking_number": "SBX-AZ-2026-0041",
            "options": {"courier_key": "sandbox", "allow_local_sandbox": false},
        }),
    ]
}

fn policy_cases() -> Vec<Value> {
    vec![
        json!({"op": "wilayas", "options": {"courier_key": "yalidine"}}),
        json!({"op": "wilaya", "code": 16, "options": {"courier_key": "yalidine"}}),
        json!({
            "op": "communes",
            "wilaya": 16,
            "options": {"courier_key": "yalidine"},
        }),
        json!({
            "op": "commune",
            "wilaya": 16,
            "name": "Bab Ezzouar",
            "options": {"courier_key": "yalidine"},
        }),
        json!({
            "op": "desks",
            "wilaya": 16,
            "options": {"courier_key": "yalidine"},
        }),
        json!({
            "op": "desk",
            "wilaya": 16,
            "id": "SBX-16",
            "options": {"courier_key": "yalidine"},
        }),
        json!({
            "op": "desks",
            "wilaya": 16,
            "options": {"courier_key": "sandbox", "allow_local_sandbox": false},
        }),
        json!({
            "op": "wilaya",
            "code": 999,
            "options": {"courier_key": "yalidine"},
        }),
        json!({
            "op": "commune",
            "wilaya": 16,
            "name": "Nowhere",
            "options": {"courier_key": "yalidine"},
        }),
        json!({
            "op": "desk",
            "wilaya": 16,
            "id": "NOPE",
            "options": {"courier_key": "yalidine"},
        }),
    ]
}

#[tokio::test]
async fn rust_matches_the_go_oracle() {
    let mut oracle = Oracle::start();

    let mut cases = base_cases();
    cases.extend(order_cases());
    cases.extend(track_cases());
    cases.extend(policy_cases());
    assert!(
        cases.len() >= 250,
        "the differential suite should stay broad, got {} cases",
        cases.len()
    );

    let mut problems = Vec::new();
    for (index, command) in cases.iter().enumerate() {
        let go = oracle.command(command);
        let rust = run_rust(command).await;
        let mut case_problems = Vec::new();
        compare(command, &go, &rust, &mut case_problems);
        for problem in case_problems {
            problems.push(format!("case {index} {command}: {problem}"));
        }
    }

    assert!(
        problems.is_empty(),
        "{} differential mismatch(es):\n{}",
        problems.len(),
        problems.join("\n")
    );
    println!("courier differential: {} cases matched", cases.len());
}
