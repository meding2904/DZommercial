//! Differential tests against the Go `internal/secrets` oracle.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use azahar_secrets::{INVALID_MESSAGE, NONCE_LEN, PREFIX, SecretBox};
use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use proptest::prelude::{Strategy, any};
use proptest::strategy::ValueTree;
use proptest::test_runner::{Config, TestRunner};
use serde_json::{Value, json};

const INVALID: &str = INVALID_MESSAGE;
const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

struct Oracle {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Oracle {
    fn spawn() -> Self {
        let backend = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../backend");
        let mut child = Command::new("go")
            .args(["run", "./cmd/oracle-orderstatus-secrets"])
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

    fn batch(&mut self, requests: &[Value]) -> Vec<Value> {
        let mut responses = Vec::with_capacity(requests.len());
        for request in requests {
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
            responses.push(serde_json::from_str(&line).expect("decode oracle response"));
        }
        responses
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

fn encode(bytes: &[u8]) -> String {
    STANDARD_NO_PAD.encode(bytes)
}

fn decode(text: &str) -> Vec<u8> {
    STANDARD_NO_PAD.decode(text).expect("base64 from oracle")
}

/// Proptest-generated key strings and plaintexts covering lengths 0..=4096.
fn generated_cases() -> Vec<(String, Vec<u8>)> {
    let case = (
        any::<String>(),
        proptest::collection::vec(any::<u8>(), 0..=4096),
    );
    let strategy = proptest::collection::vec(case, 220..=220);
    let mut runner = TestRunner::new(Config::default());
    let mut cases = strategy
        .new_tree(&mut runner)
        .expect("generate differential cases")
        .current();
    cases.push((String::new(), Vec::new()));
    cases.push(("boundary key".to_owned(), vec![0xA5; 4096]));
    cases
}

#[test]
fn cross_decrypt_matches_go() {
    let cases = generated_cases();
    assert!(cases.len() >= 200, "need at least 200 differential cases");
    let mut oracle = Oracle::spawn();

    let seal_requests: Vec<Value> = cases
        .iter()
        .map(|(key, plain)| json!({"op": "seal", "key": key, "plain": encode(plain)}))
        .collect();
    let sealed_by_go = oracle.batch(&seal_requests);

    let mut go_values = Vec::with_capacity(cases.len());
    for ((key, plain), response) in cases.iter().zip(&sealed_by_go) {
        assert!(
            response.get("error").is_none(),
            "Go failed to seal a case: {response}"
        );
        let value = response["value"].as_str().expect("Go sealed value");
        assert!(
            value.starts_with(PREFIX),
            "Go value has the wrong prefix: {value}"
        );
        let opened = SecretBox::new(key)
            .decrypt(value)
            .unwrap_or_else(|error| panic!("Rust failed to open Go value: {error}"));
        assert_eq!(&opened, plain, "Rust opened a different plaintext");
        go_values.push(value.to_owned());
    }

    let unique: HashSet<&str> = go_values.iter().map(String::as_str).collect();
    assert_eq!(unique.len(), go_values.len(), "nonces must not repeat");

    let rust_values: Vec<String> = cases
        .iter()
        .map(|(key, plain)| {
            SecretBox::new(key)
                .encrypt(plain)
                .expect("Rust encrypts a case")
        })
        .collect();
    let open_requests: Vec<Value> = cases
        .iter()
        .zip(&rust_values)
        .map(|((key, _), value)| json!({"op": "open", "key": key, "value": value}))
        .collect();
    let opened_by_go = oracle.batch(&open_requests);

    for (((key, plain), value), response) in cases.iter().zip(&rust_values).zip(&opened_by_go) {
        assert_eq!(
            response["error"], "",
            "Go rejected a Rust ciphertext sealed with key {key:?}: {response}"
        );
        let opened = decode(response["plain"].as_str().expect("Go opened plaintext"));
        assert_eq!(&opened, plain, "Go opened a different plaintext: {value}");
    }

    println!(
        "secrets cross-decrypt: {} Go->Rust cases, {} Rust->Go cases",
        cases.len(),
        cases.len()
    );
}

#[test]
fn malformed_values_match_go() {
    let key = "malformed-value key";
    let secret_box = SecretBox::new(key);
    let sealed = secret_box.encrypt(b"value").expect("encrypt");
    let payload = &sealed[PREFIX.len()..];

    let trailing_source = secret_box.encrypt(b"123456").expect("encrypt");
    let trailing_payload = &trailing_source[PREFIX.len()..];
    assert_eq!(
        trailing_payload.len() % 4,
        2,
        "the source must have unused trailing bits"
    );
    let last = *trailing_payload.as_bytes().last().expect("payload");
    let last_index = BASE64_ALPHABET
        .iter()
        .position(|&candidate| candidate == last)
        .expect("base64 character");
    let mut non_canonical = trailing_payload.as_bytes().to_vec();
    *non_canonical.last_mut().expect("payload") = BASE64_ALPHABET[last_index ^ 1];

    let mut middle_newline = payload.to_owned();
    middle_newline.insert(payload.len() / 2, '\n');

    let values = vec![
        String::new(),
        "not encrypted".to_owned(),
        PREFIX.to_owned(),
        format!("{PREFIX}AAAA"),
        format!("{PREFIX}{}", "A".repeat(1024)),
        format!("{PREFIX}!!!!!!!!"),
        format!("{PREFIX}{payload}="),
        format!("{PREFIX}{payload}=="),
        format!("{PREFIX} {payload}"),
        format!("{PREFIX}{payload} "),
        format!("enc:v2:{payload}"),
        format!("enc:v1{payload}"),
        format!("{PREFIX}{}", encode(&[0_u8; NONCE_LEN - 1])),
        format!("{PREFIX}{}", encode(&[0_u8; NONCE_LEN])),
        format!("{PREFIX}{}", encode(&[0_u8; NONCE_LEN + 1])),
        format!("{PREFIX}{payload}"),
        format!("{PREFIX}{payload}\n"),
        format!("{PREFIX}{payload}\r\n"),
        format!("\n{sealed}"),
        format!("{PREFIX}{middle_newline}"),
        format!(
            "{PREFIX}{}",
            String::from_utf8(non_canonical).expect("base64 is ASCII")
        ),
    ];

    let requests: Vec<Value> = values
        .iter()
        .map(|value| json!({"op": "open", "key": key, "value": value}))
        .collect();
    let responses = oracle_batch(&requests);

    let mut accepted = 0_usize;
    let mut rejected = 0_usize;
    for (value, response) in values.iter().zip(&responses) {
        let go_error = response["error"]
            .as_str()
            .expect("Go open response always has an error field");
        match SecretBox::new(key).decrypt(value) {
            Ok(plain) => {
                assert_eq!(go_error, "", "Rust accepted a value Go rejected: {value:?}");
                assert_eq!(
                    decode(response["plain"].as_str().expect("Go plaintext")),
                    plain,
                    "decrypted plaintext differs for {value:?}"
                );
                accepted += 1;
            }
            Err(error) => {
                assert_eq!(
                    go_error, INVALID,
                    "unexpected Go error for {value:?}: {go_error}"
                );
                assert_eq!(
                    error.to_string(),
                    INVALID,
                    "Rust error differs from Go for {value:?}"
                );
                rejected += 1;
            }
        }
    }

    assert_eq!(accepted, 5, "expected the well-formed variants to open");
    assert!(rejected >= 15, "expected malformed variants to be rejected");
    println!("secrets malformed parity: {accepted} accepted, {rejected} rejected");
}

fn oracle_batch(requests: &[Value]) -> Vec<Value> {
    let mut oracle = Oracle::spawn();
    oracle.batch(requests)
}

fn mutated_values(value: &str) -> Vec<String> {
    let payload = &value[PREFIX.len()..];
    let mut flipped = payload.as_bytes().to_vec();
    let index = payload.len() / 2;
    let position = BASE64_ALPHABET
        .iter()
        .position(|&candidate| candidate == flipped[index])
        .expect("base64 character");
    flipped[index] = BASE64_ALPHABET[(position + 1) % BASE64_ALPHABET.len()];

    let mut invalid = payload.as_bytes().to_vec();
    invalid[index] = b'!';

    vec![
        format!(
            "{PREFIX}{}",
            String::from_utf8(flipped).expect("base64 is ASCII")
        ),
        format!("{PREFIX}{}", &payload[..payload.len() - 1]),
        format!("{PREFIX}{payload}A"),
        format!("enc:v2:{payload}"),
        format!("{PREFIX}{}", &payload[1..]),
        format!("{PREFIX}{payload}="),
        format!(
            "{PREFIX}{}",
            String::from_utf8(invalid).expect("base64 is ASCII")
        ),
        payload.to_owned(),
    ]
}

#[test]
fn tamper_and_wrong_key_match_go() {
    let cases: Vec<(String, Vec<u8>)> = generated_cases().into_iter().take(40).collect();
    let mut oracle = Oracle::spawn();

    let seal_requests: Vec<Value> = cases
        .iter()
        .map(|(key, plain)| json!({"op": "seal", "key": key, "plain": encode(plain)}))
        .collect();
    let sealed_by_go = oracle.batch(&seal_requests);

    let mut tampered_requests = Vec::new();
    let mut tampered_pairs = Vec::new();
    let mut wrong_key_requests = Vec::new();
    let mut wrong_key_pairs = Vec::new();
    let mut rust_wrong_key_requests = Vec::new();
    let mut rust_wrong_key_pairs = Vec::new();

    for ((key, plain), response) in cases.iter().zip(&sealed_by_go) {
        let value = response["value"].as_str().expect("Go sealed value");
        for tampered in mutated_values(value) {
            tampered_requests.push(json!({"op": "open", "key": key, "value": tampered}));
            tampered_pairs.push((key.clone(), tampered));
        }
        let wrong_key = format!("{key}-wrong");
        wrong_key_requests.push(json!({"op": "open", "key": wrong_key, "value": value}));
        wrong_key_pairs.push((wrong_key.clone(), value.to_owned()));

        let rust_value = SecretBox::new(key).encrypt(plain).expect("encrypt");
        rust_wrong_key_requests.push(json!({"op": "open", "key": wrong_key, "value": rust_value}));
        rust_wrong_key_pairs.push((wrong_key, rust_value));
    }

    let tampered_responses = oracle.batch(&tampered_requests);
    for ((key, value), response) in tampered_pairs.iter().zip(&tampered_responses) {
        let go_error = response["error"]
            .as_str()
            .expect("Go open response always has an error field");
        assert_eq!(go_error, INVALID, "Go accepted a tampered value: {value:?}");
        let error = SecretBox::new(key)
            .decrypt(value)
            .expect_err("Rust accepted a tampered value");
        assert_eq!(error.to_string(), INVALID);
    }

    let wrong_key_responses = oracle.batch(&wrong_key_requests);
    for ((key, value), response) in wrong_key_pairs.iter().zip(&wrong_key_responses) {
        let go_error = response["error"]
            .as_str()
            .expect("Go open response always has an error field");
        assert_eq!(go_error, INVALID, "Go accepted the wrong key");
        let error = SecretBox::new(key)
            .decrypt(value)
            .expect_err("Rust accepted the wrong key");
        assert_eq!(error.to_string(), INVALID);
    }

    let rust_wrong_key_responses = oracle.batch(&rust_wrong_key_requests);
    for ((key, value), response) in rust_wrong_key_pairs.iter().zip(&rust_wrong_key_responses) {
        assert_eq!(
            response["error"], INVALID,
            "Go accepted a Rust ciphertext under the wrong key"
        );
        assert_eq!(
            SecretBox::new(key).decrypt(value),
            Err(azahar_secrets::Error::Invalid)
        );
    }

    println!(
        "secrets tamper/wrong-key: {} sealed values, {} tamper cases, {} Go->Rust wrong-key cases, {} Rust->Go wrong-key cases",
        cases.len(),
        tampered_requests.len(),
        wrong_key_requests.len(),
        rust_wrong_key_requests.len()
    );
}
