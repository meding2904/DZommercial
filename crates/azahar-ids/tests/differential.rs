//! Differential test against the Go oracle in `backend/cmd/oracle-ids-auth`.
//!
//! Identifiers are random, so parity is structural: both sides must return the
//! prefix, an underscore, and 24 lowercase hex characters.

use std::{
    collections::HashSet,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
struct OracleResponse {
    id: Option<String>,
    error: Option<String>,
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .parent()
        .expect("repository root")
        .to_path_buf()
}

/// Runs one oracle process, writing a request and reading its response one at
/// a time so neither pipe can fill regardless of batch size.
fn run_oracle(requests: &[String]) -> Vec<OracleResponse> {
    let mut child = Command::new("go")
        .args(["run", "./cmd/oracle-ids-auth"])
        .current_dir(repository_root().join("backend"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn go run");

    let stderr = child.stderr.take().expect("oracle stderr");
    let stderr_reader = std::thread::spawn(move || {
        let mut buffer = String::new();
        BufReader::new(stderr)
            .read_to_string(&mut buffer)
            .expect("read oracle stderr");
        buffer
    });

    let mut stdin = child.stdin.take().expect("oracle stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("oracle stdout"));
    let mut responses = Vec::with_capacity(requests.len());

    for (index, request) in requests.iter().enumerate() {
        writeln!(stdin, "{request}").expect("write oracle request");
        stdin.flush().expect("flush oracle request");

        let mut response = String::new();
        let read = stdout
            .read_line(&mut response)
            .expect("read oracle response");
        assert!(read > 0, "oracle stopped before response {index}");
        responses.push(serde_json::from_str(&response).expect("oracle response JSON"));
    }

    drop(stdin);
    let status = child.wait().expect("wait for oracle");
    let stderr = stderr_reader.join().expect("join oracle stderr reader");
    assert!(status.success(), "oracle failed: {stderr}");
    responses
}

fn assert_id_shape(prefix: &str, id: &str) {
    let expected_prefix = format!("{prefix}_");
    assert!(
        id.starts_with(&expected_prefix),
        "{id:?} does not start with {expected_prefix:?}"
    );
    let random = &id[expected_prefix.len()..];
    assert_eq!(random.len(), 24, "{id:?}");
    assert!(
        random
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "{id:?} is not 24 lowercase hex characters"
    );
}

fn prefixes() -> Vec<String> {
    let mut prefixes: Vec<String> = vec![
        String::new(),
        "prd".to_owned(),
        "ses".to_owned(),
        "ord".to_owned(),
        "itm".to_owned(),
        "aud".to_owned(),
        "img".to_owned(),
        "cmp".to_owned(),
        "phn".to_owned(),
        "a_b$c".to_owned(),
        "héllo".to_owned(),
        "🔐".to_owned(),
        "with\nnewline".to_owned(),
        "with\0nul".to_owned(),
        " leading".to_owned(),
        "trailing ".to_owned(),
        "p".repeat(200),
    ];

    // Deterministic pseudo-random prefixes, including multi-byte characters.
    let mut state: u64 = 0x1234_5678_9abc_def0;
    for _ in 0..64 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let length = (state as usize) % 48;
        let mut prefix = String::with_capacity(length);
        for _ in 0..length {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let character = match state % 5 {
                0 => char::from(b'a' + (state % 26) as u8),
                1 => char::from(b'0' + (state % 10) as u8),
                2 => ['_', '-', '$', '.', '/', ' ', '\n', '\r'][(state % 8) as usize],
                3 => 'é',
                _ => '🔐',
            };
            prefix.push(character);
        }
        prefixes.push(prefix);
    }

    prefixes
}

#[test]
fn ids_match_the_go_shape() {
    let prefixes = prefixes();

    let requests: Vec<String> = prefixes
        .iter()
        .map(|prefix| json!({"op": "id", "prefix": prefix}).to_string())
        .collect();
    let responses = run_oracle(&requests);
    assert_eq!(responses.len(), requests.len());

    let mut oracle_ids = HashSet::new();
    for (prefix, response) in prefixes.iter().zip(&responses) {
        assert!(
            response.error.is_none(),
            "oracle error for {prefix:?}: {:?}",
            response.error
        );
        let id = response.id.as_deref().expect("id");
        assert_id_shape(prefix, id);
        assert!(oracle_ids.insert(id.to_owned()), "duplicate oracle id {id}");
    }

    let mut rust_ids = HashSet::new();
    for prefix in &prefixes {
        let id = azahar_ids::new(prefix);
        assert_id_shape(prefix, &id);
        assert!(rust_ids.insert(id), "duplicate Rust id");
    }

    // The same prefix repeated must still produce distinct identifiers.
    for prefix in ["prd", ""] {
        let first = azahar_ids::new(prefix);
        let second = azahar_ids::new(prefix);
        assert_ne!(first, second);
    }
}
