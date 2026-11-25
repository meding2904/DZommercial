//! Differential test for the validation helpers against the Go oracle.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use azahar_httpapi::validate::{
    normalized_algerian_mobile, rune_len, trim_all, truncate_runes, valid_email, valid_phone,
    valid_slug,
};

struct Oracle {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl Oracle {
    fn start() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let backend = root.join("backend");
        let mut child = Command::new("go")
            .args(["run", "-tags", "oracle", "./cmd/oracle-validate"])
            .current_dir(backend)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("go run oracle-validate");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Self {
            child,
            stdin,
            stdout,
        }
    }

    fn call(&mut self, request: &Value) -> Value {
        writeln!(self.stdin, "{request}").expect("write request");
        self.stdin.flush().expect("flush");
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("read response");
        serde_json::from_str(&line).expect("json response")
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn cases() -> Vec<String> {
    let fixed = [
        "",
        "a@b",
        "a@b.c",
        "a..b@c.d",
        ".a@b",
        "a.@b",
        "a@-b",
        "a@b-",
        "a@b_c",
        "a@[1.2.3.4]",
        "a@[::1]",
        "a@[IPv6:::1]",
        "a@[IPv6:2001:db8::1]",
        "a@[IPv6:1.2.3.4]",
        "a@[::ffff:1.2.3.4]",
        "a@[0:0:0:0:0:ffff:102:304]",
        "a@[IPv6:::ffff:1.2.3.4]",
        "a@[abc]",
        "a+b@c.d",
        "\"ab\"@c.d",
        "a@b@c",
        "a@",
        "@b",
        "a@b.",
        "a@b..c",
        "a@b c",
        "a@b/c",
        "a@b:c",
        "A@B.C",
        "ü@b.c",
        "a@ü.c",
        "a\u{a0}b@c.d",
        "a@b(c)",
        "a@1.2.3.4",
        "123456",
        "      ",
        "+213 555 12 34 56",
        "(06) 12-34-56-78",
        "0712345678",
        "0412345678",
        "07a1234567",
        "abc-123",
        "-a",
        "a--b",
        "héllo",
        " a ",
    ];
    let mut out: Vec<String> = fixed.iter().map(|value| (*value).to_string()).collect();
    let alphabet: Vec<char> = "ab01@.-_+() /[]:é\u{a0}\t".chars().collect();
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    for _ in 0..1200 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let length = (state as usize) % 24;
        let mut value = String::new();
        for _ in 0..length {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            value.push(alphabet[(state as usize) % alphabet.len()]);
        }
        out.push(value);
    }
    out
}

#[test]
fn validation_matches_the_go_oracle() {
    let mut oracle = Oracle::start();
    let mut mismatches = Vec::new();
    for value in cases() {
        let expected = oracle.call(&json!({"op":"email","value":value}));
        if expected["ok"].as_bool() != Some(valid_email(&value)) {
            mismatches.push(format!(
                "email {value:?}: go={expected} rust={}",
                valid_email(&value)
            ));
        }
        let expected = oracle.call(&json!({"op":"slug","value":value}));
        if expected["ok"].as_bool() != Some(valid_slug(&value)) {
            mismatches.push(format!(
                "slug {value:?}: go={expected} rust={}",
                valid_slug(&value)
            ));
        }
        let expected = oracle.call(&json!({"op":"phone","value":value}));
        if expected["ok"].as_bool() != Some(valid_phone(&value)) {
            mismatches.push(format!(
                "phone {value:?}: go={expected} rust={}",
                valid_phone(&value)
            ));
        }
        let expected = oracle.call(&json!({"op":"mobile","value":value}));
        let rust = normalized_algerian_mobile(&value);
        if expected["ok"].as_bool() != Some(rust.is_some())
            || expected["value"].as_str().unwrap_or("") != rust.as_deref().unwrap_or("")
        {
            mismatches.push(format!("mobile {value:?}: go={expected} rust={rust:?}"));
        }
        let expected = oracle.call(&json!({"op":"rune_len","value":value}));
        if expected["value"].as_u64() != Some(rune_len(&value) as u64) {
            mismatches.push(format!("rune_len {value:?}: go={expected}"));
        }
        let expected = oracle.call(&json!({"op":"truncate","value":value,"max":7}));
        if expected["value"].as_str() != Some(truncate_runes(&value, 7).as_str()) {
            mismatches.push(format!("truncate {value:?}: go={expected}"));
        }
        let values = vec![value.clone(), format!(" {value} "), String::new()];
        let expected = oracle.call(&json!({"op":"trim_all","values":values}));
        let rust = trim_all(&values);
        let expected: Vec<String> = expected["values"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if expected != rust {
            mismatches.push(format!(
                "trim_all {values:?}: go={expected:?} rust={rust:?}"
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} mismatches, first 20:\n{}",
        mismatches.len(),
        mismatches
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
