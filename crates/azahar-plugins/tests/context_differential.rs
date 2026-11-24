//! Differential test for the plugin SDK context helpers.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use azahar_plugins::Problem;
use azahar_plugins::{Context, decode_body, error_body, write_json};

struct Oracle {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl Oracle {
    fn start() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut child = Command::new("go")
            .args(["run", "./cmd/oracle-plugin-context"])
            .current_dir(root.join("backend"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("go run oracle-plugin-context");
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

#[derive(Debug, Default, serde::Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Payload {
    name: String,
}

#[test]
fn context_helpers_match_go() {
    let mut oracle = Oracle::start();

    for role in ["admin", "employee", "customer", ""] {
        let expected = oracle.call(&json!({"op":"is_admin","role":role}));
        let context = Context {
            env: None,
            plugin_id: "test".into(),
            user_id: String::new(),
            user_role: role.to_string(),
        };
        assert_eq!(expected["ok"].as_bool(), Some(context.is_admin()), "{role}");
    }

    for payload in [
        json!({"name": "A <b> & c"}),
        json!({"list": [1, 2, 3]}),
        json!(null),
    ] {
        let expected = oracle.call(&json!({"op":"json","status":200,"payload":payload}));
        let rust = if payload.is_null() {
            write_json(None::<&Value>)
        } else {
            write_json(Some(&payload))
        };
        assert_eq!(expected["body"].as_str().unwrap_or(""), rust, "{payload}");
        assert_eq!(expected["status"].as_i64(), Some(200));
    }

    for (status, code, message, fields) in [
        (400, "bad_request", "Bad.", None),
        (
            422,
            "validation_error",
            "Check.",
            Some(serde_json::json!({"name":"Required."})),
        ),
    ] {
        let expected = oracle.call(&json!({
            "op":"problem","status":status,"code":code,"message":message,"fields":fields
        }));
        let fields = fields
            .as_ref()
            .and_then(|value| serde_json::from_value(value.clone()).ok());
        let problem = Problem::new(status, code, message, fields);
        assert_eq!(
            expected["body"].as_str().unwrap_or(""),
            error_body(&problem)
        );
    }

    for (content_type, body) in [
        ("text/plain", "{}"),
        ("application/json", "  "),
        ("application/json", "[]"),
        ("application/json", r#"{"name":"a","x":1}"#),
        ("application/json", r#"{"name":"a"} {}"#),
        ("application/json; charset=utf-8", r#"{"name":"a"}"#),
        ("APPLICATION/JSON", r#"{"name":"b"}"#),
        ("application/json", r#"{"NAME":"a"}"#),
        ("application/json", r#"{"name":"a","NAME":"b"}"#),
        ("application/json", "null"),
    ] {
        let expected = oracle.call(&json!({
            "op":"decode","content_type":content_type,"body":body
        }));
        let rust = decode_body::<Payload>(content_type, body.as_bytes(), &["name"]);
        match expected.get("problem") {
            Some(expected_problem) => {
                let problem = rust.expect_err("expected refusal");
                assert_eq!(
                    expected_problem["status"].as_i64(),
                    Some(problem.status as i64),
                    "{content_type} {body}"
                );
                assert_eq!(
                    expected_problem["code"].as_str(),
                    Some(problem.code.as_str())
                );
                assert_eq!(
                    expected_problem["message"].as_str(),
                    Some(problem.message.as_str())
                );
            }
            None => {
                let expected_value = &expected["value"];
                let value = rust.expect("expected success");
                assert_eq!(expected_value["name"].as_str(), Some(value.name.as_str()));
            }
        }
    }
}
