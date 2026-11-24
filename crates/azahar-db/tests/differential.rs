//! Differential test for the migration statement splitter.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

fn oracle_call(
    stdin: &mut std::process::ChildStdin,
    stdout: &mut BufReader<std::process::ChildStdout>,
    body: &str,
) -> Vec<String> {
    writeln!(stdin, "{}", json!({"body": body})).expect("write");
    stdin.flush().expect("flush");
    let mut line = String::new();
    stdout.read_line(&mut line).expect("read");
    let value: Value = serde_json::from_str(&line).expect("json");
    value["statements"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn split_statements_matches_go() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut child = Command::new("go")
        .args(["run", "-tags", "oracle", "./cmd/oracle-db"])
        .current_dir(root.join("backend"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("go run oracle-db");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));

    let mut bodies = vec![
        String::new(),
        "  ;  ".to_string(),
        "CREATE TABLE a (id TEXT);".to_string(),
        "CREATE TABLE a (id TEXT); ; SELECT 1;".to_string(),
        ";;;".to_string(),
        "SELECT ';' AS semi;".to_string(),
        "-- comment\nCREATE INDEX i ON t(a);\n".to_string(),
        " \n\t; \n".to_string(),
    ];
    let alphabet = ['a', ';', ' ', '\n', '\t', '-', '\'', '1'];
    let mut state: u64 = 0x1234_5678_9abc_def0;
    for _ in 0..600 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let length = (state as usize) % 80;
        let mut body = String::new();
        for _ in 0..length {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            body.push(alphabet[(state as usize) % alphabet.len()]);
        }
        bodies.push(body);
    }

    let mut mismatches = Vec::new();
    for body in bodies {
        let expected = oracle_call(&mut stdin, &mut stdout, &body);
        let actual = azahar_db::split_statements(&body);
        if expected != actual {
            mismatches.push(format!("{body:?}: go={expected:?} rust={actual:?}"));
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        mismatches.is_empty(),
        "{} mismatches, first 5: {:#?}",
        mismatches.len(),
        &mismatches[..mismatches.len().min(5)]
    );
}
