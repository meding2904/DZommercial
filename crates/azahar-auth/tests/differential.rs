//! Differential tests against the Go oracle in `backend/cmd/oracle-ids-auth`.

use std::{
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use azahar_auth::{
    dummy_hash, hash_password, hash_token, new_token, verify_dummy, verify_password,
};
use base64::{Engine, engine::general_purpose::STANDARD_NO_PAD};
use serde::Deserialize;
use serde_json::{Value, json};

const DUMMY_PASSWORD: &str = "azahar-dummy-password-for-timing";

/// Known behaviour difference: Go's argon2 clamps a memory cost below `8*p`
/// up to that minimum, so these Go-generated hashes verify in Go. The Rust
/// argon2 crate rejects the parameters, so `verify_password` deliberately
/// returns `false` for them. `HashPassword` never emits such parameters.
const BELOW_MINIMUM_MEMORY_HASHES: &[(&str, &str)] = &[
    (
        "below-minimum-memory",
        "$argon2id$v=19$m=1,t=1,p=1$YXphaGFyLXRlc3Qtc2FsdA$O2DCMZX2T/w/B1ibEhr+QhmwnWo8AYGljFzp4hDjQOg",
    ),
    (
        "below-minimum-memory",
        "$argon2id$v=19$m=7,t=1,p=1$YXphaGFyLXRlc3Qtc2FsdA$3PVY9ett50zMsALF1nvVQFxtcxvESNP/nNI0u6inw58",
    ),
];

#[derive(Debug, Deserialize)]
struct OracleResponse {
    id: Option<String>,
    hash: Option<String>,
    token: Option<String>,
    ok: Option<bool>,
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
fn run_oracle(lines: &[String]) -> Vec<OracleResponse> {
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
    let mut responses = Vec::with_capacity(lines.len());

    for (index, line) in lines.iter().enumerate() {
        writeln!(stdin, "{line}").expect("write oracle request");
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

#[derive(Default)]
struct Batch {
    lines: Vec<String>,
}

impl Batch {
    fn push(&mut self, request: Value) -> usize {
        let index = self.lines.len();
        self.lines.push(request.to_string());
        index
    }

    fn push_raw(&mut self, line: &str) -> usize {
        let index = self.lines.len();
        self.lines.push(line.to_owned());
        index
    }
}

fn split_hash(hash: &str) -> Vec<String> {
    hash.split('$').map(str::to_owned).collect()
}

fn build_hash(version: &str, params: &str, salt: &str, key: &str) -> String {
    format!("$argon2id${version}${params}${salt}${key}")
}

fn go_style_base64(bytes: &[u8]) -> String {
    STANDARD_NO_PAD.encode(bytes)
}

/// Flips the unused trailing bits of the final base64 character.
fn noncanonical_base64(value: &str) -> String {
    let (head, last) = value.split_at(value.len() - 1);
    let index = base64::alphabet::STANDARD
        .as_str()
        .find(last)
        .expect("base64 character");
    let mutated = base64::alphabet::STANDARD.as_str().as_bytes()[index | 1];
    format!("{head}{}", char::from(mutated))
}

struct VerifyCase {
    label: String,
    password: String,
    hash: String,
    expected: bool,
    response: usize,
}

fn push_verify(
    batch: &mut Batch,
    cases: &mut Vec<VerifyCase>,
    label: &str,
    password: &str,
    hash: &str,
) {
    let expected = verify_password(password, hash);
    let response = batch.push(json!({
        "op": "verify",
        "password": password,
        "hash": hash,
    }));
    cases.push(VerifyCase {
        label: label.to_owned(),
        password: password.to_owned(),
        hash: hash.to_owned(),
        expected,
        response,
    });
}

#[test]
fn differential_passwords_hashes_and_hostile_input() {
    let passwords = [
        "",
        "a long enough password",
        "another password",
        "correct horse battery staple",
        "pässwörd 🔐",
        "with\u{0}nul",
        " leading and trailing ",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ];

    let mut batch = Batch::default();
    let mut verify_cases = Vec::new();

    // Go produces hashes, Rust verifies them afterwards.
    let go_hash_responses: Vec<usize> = passwords
        .iter()
        .map(|password| batch.push(json!({"op": "hash", "password": password})))
        .collect();

    // Rust produces hashes now; the oracle verifies them in the same batch.
    let rust_hashes: Vec<String> = passwords
        .iter()
        .map(|password| hash_password(password).expect("hash"))
        .collect();
    let rust_hash_responses: Vec<usize> = passwords
        .iter()
        .zip(&rust_hashes)
        .map(|(password, hash)| {
            batch.push(json!({"op": "verify", "password": password, "hash": hash}))
        })
        .collect();

    // Wrong passwords against Rust hashes, and against Go hashes after the run.
    let wrong_responses: Vec<usize> = rust_hashes
        .iter()
        .map(|hash| {
            batch.push(json!({
                "op": "verify",
                "password": "definitely the wrong password",
                "hash": hash,
            }))
        })
        .collect();

    let base = &rust_hashes[0];
    let parts = split_hash(base);
    let (version, params, salt, key) = (&parts[2], &parts[3], &parts[4], &parts[5]);

    // Version field quirks.
    for (label, candidate) in [
        ("version canonical", version.clone()),
        (
            "version without v=",
            version.trim_start_matches("v=").to_owned(),
        ),
        ("version leading zero", "v=019".to_owned()),
        ("version plus sign", "v=+19".to_owned()),
        ("version minus", "v=-19".to_owned()),
        ("version 20", "v=20".to_owned()),
        ("version 18", "v=18".to_owned()),
        ("version empty", String::new()),
        ("version space", "v= 19".to_owned()),
        ("version trailing space", "v=19 ".to_owned()),
        ("version uppercase", "V=19".to_owned()),
        ("version hex", "v=0x13".to_owned()),
        ("version float", "v=19.0".to_owned()),
        ("version newline", "v=19\n".to_owned()),
        ("version plus leading zero", "v=+019".to_owned()),
    ] {
        push_verify(
            &mut batch,
            &mut verify_cases,
            label,
            "a long enough password",
            &build_hash(&candidate, params, salt, key),
        );
    }

    // Parameter field quirks.
    for (label, candidate) in [
        ("params canonical", params.clone()),
        ("params reordered", "p=4,m=65536,t=3".to_owned()),
        (
            "params duplicated",
            "m=1,t=1,p=1,m=65536,t=3,p=4".to_owned(),
        ),
        ("params unknown key", "m=65536,t=3,p=4,x=1".to_owned()),
        ("params missing p", "m=65536,t=3".to_owned()),
        ("params empty", String::new()),
        ("params leading comma", ",m=65536,t=3,p=4".to_owned()),
        ("params trailing comma", "m=65536,t=3,p=4,".to_owned()),
        ("params double comma", "m=65536,,t=3,p=4".to_owned()),
        ("params spaces", "m=65536, t=3,p=4".to_owned()),
        ("params plus", "m=+65536,t=3,p=4".to_owned()),
        ("params minus", "m=-65536,t=3,p=4".to_owned()),
        ("params extra equals", "m=65536,t=3,p=4=5".to_owned()),
        ("params uint32 overflow", "m=4294967296,t=3,p=4".to_owned()),
        ("params m zero", "m=0,t=3,p=4".to_owned()),
        ("params t zero", "m=65536,t=0,p=4".to_owned()),
        ("params p zero", "m=65536,t=3,p=0".to_owned()),
        ("params p wraps to zero", "m=65536,t=3,p=256".to_owned()),
        ("params p wraps to four", "m=65536,t=3,p=260".to_owned()),
        ("params p wraps over limit", "m=65536,t=3,p=321".to_owned()),
        ("params m over limit", "m=1048577,t=3,p=4".to_owned()),
        ("params t over limit", "m=65536,t=21,p=4".to_owned()),
        ("params p over limit", "m=65536,t=3,p=65".to_owned()),
        ("params memory below minimum", "m=7,t=1,p=1".to_owned()),
        ("params memory clamped by go", "m=1,t=1,p=4".to_owned()),
        ("params memory exactly minimum", "m=8,t=1,p=1".to_owned()),
        ("params memory just above minimum", "m=9,t=1,p=1".to_owned()),
        ("params uppercase key", "M=65536,t=3,p=4".to_owned()),
    ] {
        push_verify(
            &mut batch,
            &mut verify_cases,
            label,
            "a long enough password",
            &build_hash(version, &candidate, salt, key),
        );
    }

    // Salt quirks. Only the noncanonical spelling decodes to the right salt.
    let salt_cases = [
        ("salt canonical", salt.clone()),
        ("salt with newline", format!("{salt}\n")),
        ("salt with crlf", format!("{salt}\r\n")),
        ("salt noncanonical trailing bits", noncanonical_base64(salt)),
        ("salt empty", String::new()),
        ("salt 4 bytes", go_style_base64(b"four")),
        ("salt 7 bytes", go_style_base64(b"sevenby")),
        ("salt 8 bytes", go_style_base64(b"saltsalt")),
        ("salt 48 bytes", go_style_base64(&[7u8; 48])),
        ("salt padded", format!("{salt}=")),
        ("salt with space", format!("{} {}", &salt[..4], &salt[4..])),
        (
            "salt url-safe alphabet",
            salt.replace('+', "-").replace('/', "_"),
        ),
        ("salt invalid character", format!("{salt}!")),
    ];
    for (label, candidate) in salt_cases {
        push_verify(
            &mut batch,
            &mut verify_cases,
            label,
            "a long enough password",
            &build_hash(version, params, &candidate, key),
        );
    }

    // Key quirks.
    let key_cases = [
        ("key canonical", key.clone()),
        ("key with newline", format!("{key}\n")),
        ("key noncanonical trailing bits", noncanonical_base64(key)),
        ("key empty", String::new()),
        ("key 15 bytes", go_style_base64(&[0u8; 15])),
        ("key 16 bytes", go_style_base64(&[0u8; 16])),
        ("key 128 bytes", go_style_base64(&[0u8; 128])),
        ("key 129 bytes", go_style_base64(&[0u8; 129])),
        ("key padded", format!("{key}=")),
        ("key invalid character", format!("{key}!")),
    ];
    for (label, candidate) in key_cases {
        push_verify(
            &mut batch,
            &mut verify_cases,
            label,
            "a long enough password",
            &build_hash(version, params, salt, &candidate),
        );
    }

    // Field count quirks; `parts[0]` is not validated by Go.
    push_verify(
        &mut batch,
        &mut verify_cases,
        "leading non-empty field",
        "a long enough password",
        &format!("ignored${}", &base[1..]),
    );
    push_verify(
        &mut batch,
        &mut verify_cases,
        "leading non-empty field plus separator",
        "a long enough password",
        &format!("ignored$again${}", &base[1..]),
    );
    push_verify(
        &mut batch,
        &mut verify_cases,
        "trailing extra field",
        "a long enough password",
        &format!("{base}$extra"),
    );
    push_verify(
        &mut batch,
        &mut verify_cases,
        "empty string",
        "a long enough password",
        "",
    );
    push_verify(
        &mut batch,
        &mut verify_cases,
        "not a hash",
        "a long enough password",
        "not-a-hash",
    );

    // Known behaviour difference, pinned on purpose: Go's argon2 clamps
    // `m < 8*p`, so the oracle accepts these Go-generated fixtures; the Rust
    // argon2 crate rejects the parameters, so `verify_password` is false.
    // This must not silently change in either direction.
    let below_minimum_responses: Vec<usize> = BELOW_MINIMUM_MEMORY_HASHES
        .iter()
        .map(|(password, hash)| {
            batch.push(json!({
                "op": "verify",
                "password": password,
                "hash": hash,
            }))
        })
        .collect();

    // Deterministic hostile batch, including structured near-misses.
    for (index, candidate) in generated_hostile_hashes(120).into_iter().enumerate() {
        push_verify(
            &mut batch,
            &mut verify_cases,
            &format!("generated {index}"),
            "generated password",
            &candidate,
        );
    }

    // The oracle must survive an invalid JSON line.
    let invalid_json = batch.push_raw("this is not json");
    let after_invalid = batch.push(json!({
        "op": "hash_token",
        "token": "still-alive",
    }));
    let unknown_op = batch.push(json!({"op": "no-such-operation"}));

    let responses = run_oracle(&batch.lines);
    assert_eq!(responses.len(), batch.lines.len());

    // Go hashes verify in Rust, and Rust hashes verify in Go.
    for (password, response) in passwords.iter().zip(&go_hash_responses) {
        let hash = responses[*response].hash.as_deref().expect("Go hash");
        assert!(hash.starts_with("$argon2id$v=19$m=65536,t=3,p=4$"));
        assert!(verify_password(password, hash), "Go hash for {password:?}");
        assert!(!verify_password("definitely the wrong password", hash));
    }
    for (index, response) in rust_hash_responses.iter().enumerate() {
        assert_eq!(
            responses[*response].ok,
            Some(true),
            "Go rejected Rust hash {index}: {:?}",
            responses[*response].error
        );
    }
    for response in wrong_responses {
        assert_eq!(responses[response].ok, Some(false));
    }

    for case in &verify_cases {
        let response = &responses[case.response];
        match (response.ok, &response.error) {
            (Some(actual), None) => assert_eq!(
                actual, case.expected,
                "verdict mismatch for {} ({:?} / {:?})",
                case.label, case.password, case.hash
            ),
            (None, Some(error)) => {
                assert!(!case.expected, "Go failed but Rust accepted {}", case.label);
                assert!(
                    error.starts_with("panic:"),
                    "unexpected oracle error for {}: {error}",
                    case.label
                );
            }
            other => panic!("malformed oracle response for {}: {other:?}", case.label),
        }
    }

    for ((password, hash), response) in BELOW_MINIMUM_MEMORY_HASHES
        .iter()
        .zip(&below_minimum_responses)
    {
        let response = &responses[*response];
        assert_eq!(
            response.ok,
            Some(true),
            "Go must keep accepting m < 8*p fixture {hash}: {response:?}"
        );
        assert!(
            !verify_password(password, hash),
            "Rust deliberately rejects m < 8*p (known behaviour difference): {hash}"
        );
    }

    assert!(
        responses[invalid_json]
            .error
            .as_deref()
            .is_some_and(|error| error.starts_with("invalid JSON:"))
    );
    assert_eq!(
        responses[after_invalid].hash.as_deref(),
        Some(hash_token("still-alive").as_str())
    );
    assert!(responses[unknown_op].error.is_some());
}

#[test]
fn differential_tokens_ids_dummy_and_base64_key_lengths() {
    let mut batch = Batch::default();

    let tokens = ["", "abc", "token-value", "pässwörd 🔐", &"a".repeat(200)];
    let token_responses: Vec<usize> = tokens
        .iter()
        .map(|token| batch.push(json!({"op": "hash_token", "token": token})))
        .collect();

    let new_token_responses: Vec<usize> = (0..4)
        .map(|_| batch.push(json!({"op": "new_token"})))
        .collect();

    let rust_dummy = dummy_hash();
    let rust_dummy_response = batch.push(json!({
        "op": "verify",
        "password": DUMMY_PASSWORD,
        "hash": rust_dummy,
    }));
    let rust_dummy_wrong_response = batch.push(json!({
        "op": "verify",
        "password": "not the dummy password",
        "hash": rust_dummy,
    }));
    let go_dummy_response = batch.push(json!({"op": "dummy_hash"}));
    let verify_dummy_responses: Vec<usize> = [DUMMY_PASSWORD, "not the dummy password"]
        .iter()
        .map(|password| batch.push(json!({"op": "verify_dummy", "password": password})))
        .collect();

    // A 128-byte key is legal for Go's VerifyPassword and outside the PHC
    // crate's 64-byte output type, so build it directly with the argon2 KDF.
    let long_key_hash = long_key_hash("long key", &[9u8; 40], 128);
    let long_key_response = batch.push(json!({
        "op": "verify",
        "password": "long key",
        "hash": long_key_hash,
    }));

    let id_prefixes = ["", "prd", "ses", "ord", "a_b$c", "héllo", &"p".repeat(200)];
    let id_responses: Vec<usize> = id_prefixes
        .iter()
        .map(|prefix| batch.push(json!({"op": "id", "prefix": prefix})))
        .collect();

    let responses = run_oracle(&batch.lines);
    assert_eq!(responses.len(), batch.lines.len());

    for (token, response) in tokens.iter().zip(&token_responses) {
        assert_eq!(
            responses[*response].hash.as_deref(),
            Some(hash_token(token).as_str()),
            "HashToken mismatch for {token:?}"
        );
    }

    let mut seen = std::collections::HashSet::new();
    for response in new_token_responses {
        let token = responses[response].token.as_deref().expect("token");
        assert_eq!(token.len(), 43);
        assert!(
            token
                .bytes()
                .all(|byte| { byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' })
        );
        assert_eq!(
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(token)
                .expect("base64url")
                .len(),
            32
        );
        assert!(seen.insert(token.to_owned()), "duplicate oracle token");
    }
    let mut seen = std::collections::HashSet::new();
    for _ in 0..4 {
        let token = new_token().expect("token");
        assert_eq!(token.len(), 43);
        assert!(seen.insert(token), "duplicate Rust token");
    }

    assert_eq!(responses[rust_dummy_response].ok, Some(true));
    assert_eq!(responses[rust_dummy_wrong_response].ok, Some(false));

    let go_dummy = responses[go_dummy_response]
        .hash
        .as_deref()
        .expect("dummy hash");
    assert!(go_dummy.starts_with("$argon2id$v=19$m=65536,t=3,p=4$"));
    assert!(verify_password(DUMMY_PASSWORD, go_dummy));
    assert!(!verify_password("not the dummy password", go_dummy));

    for response in verify_dummy_responses {
        assert_eq!(
            responses[response].ok, None,
            "verify_dummy returns no fields"
        );
        assert!(responses[response].error.is_none());
    }
    verify_dummy(DUMMY_PASSWORD);
    verify_dummy("not the dummy password");

    assert_eq!(
        responses[long_key_response].ok,
        Some(true),
        "Go rejected the 128-byte key hash: {:?}",
        responses[long_key_response].error
    );
    assert!(verify_password("long key", &long_key_hash));

    for (prefix, response) in id_prefixes.iter().zip(&id_responses) {
        let id = responses[*response].id.as_deref().expect("id");
        assert_id_shape(prefix, id);
    }
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
        "{id:?} is not lowercase hex"
    );
}

fn long_key_hash(password: &str, salt: &[u8], key_len: usize) -> String {
    use argon2::{Algorithm, Argon2, Params, Version};

    let params = Params::new(64 * 1024, 3, 4, Some(key_len)).expect("params");
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = vec![0u8; key_len];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .expect("key");
    build_hash(
        "v=19",
        "m=65536,t=3,p=4",
        &go_style_base64(salt),
        &go_style_base64(&key),
    )
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

fn generated_hostile_hashes(count: usize) -> Vec<String> {
    const CHARSET: &[u8] = b"$=,+-_/AZaz09 \r\n!@#%^&*()[]{}:;'\"<>?\\|~\x7f\xc3\xa9";
    const VERSIONS: &[&str] = &[
        "",
        "19",
        "v=19",
        "v=019",
        "v=+19",
        "v=-19",
        "v=20",
        "v=18",
        "V=19",
        "v=1 9",
        "v=19x",
        "v=4294967315",
    ];
    const PARAMS: &[&str] = &[
        "",
        "m=0,t=0,p=0",
        "m=1,t=1,p=1",
        "m=1,t=1,p=4",
        "m=7,t=1,p=1",
        "m=8,t=1,p=1",
        "m=9,t=1,p=1",
        "m=32,t=1,p=4",
        "m=31,t=1,p=4",
        "m=65536,t=3,p=4",
        "m=65536,t=3,p=4,x=1",
        "m=65536,t=3",
        "p=4,t=3,m=65536",
        "m=65536,t=21,p=4",
        "m=65536,t=3,p=65",
        "m=1048577,t=3,p=4",
        "m=4294967296,t=3,p=4",
        "m=+8,t=1,p=1",
        "m=08,t=01,p=01",
        "m=8,t=1,p=1,",
        ",m=8,t=1,p=1",
        "m=8,,t=1,p=1",
        "m=8,t=1,p=256",
        "m=8,t=1,p=260",
        "m=1,t=1,p=4,m=8",
    ];
    const SALTS: &[&str] = &[
        "",
        "c2FsdA",
        "c2FsdHNhbHQ",
        "c2FsdHNhbHRzYWx0",
        "c2FsdHNhbHRzYWx0c2FsdA",
        "c2FsdHNhbHQ=",
        "c2FsdHNhbH RzYWx0",
        "c2FsdHNhbHRzYWx0\n",
        "YQ",
        "!!!!",
        "_-_-",
    ];
    const KEYS: &[&str] = &[
        "",
        "aGFzaA",
        "aGFzaGhhc2hoYXNo",
        "aGFzaGhhc2hoYXNoaGFzaA",
        "aGFzaGhhc2hoYXNoaGFzaA=",
        "aGFzaGhhc2hoYXNoaGFzaA!",
        "!!!!!!!!!!!!!!!!",
    ];

    let mut lcg = Lcg(0x5eed_1d5a_0bad_cafe);
    let mut cases = Vec::with_capacity(count);

    for _ in 0..count {
        match lcg.below(10) {
            0..=2 => {
                let salt = SALTS[lcg.below(SALTS.len())];
                let key = KEYS[lcg.below(KEYS.len())];
                let version = VERSIONS[lcg.below(VERSIONS.len())];
                cases.push(format!("$argon2id${version}${salt}${key}"));
            }
            3..=6 => {
                let version = VERSIONS[lcg.below(VERSIONS.len())];
                let params = PARAMS[lcg.below(PARAMS.len())];
                let salt = SALTS[lcg.below(SALTS.len())];
                let key = KEYS[lcg.below(KEYS.len())];
                cases.push(build_hash(version, params, salt, key));
            }
            7 | 8 => {
                let length = lcg.below(160);
                let mut candidate = String::with_capacity(length);
                for _ in 0..length {
                    candidate.push(char::from(CHARSET[lcg.below(CHARSET.len())]));
                }
                cases.push(candidate);
            }
            _ => {
                let memory = lcg.below(40);
                let iterations = lcg.below(4);
                let threads = lcg.below(10);
                let salt_len = 8 + lcg.below(20);
                let salt_byte = lcg.next() as u8;
                let salt = go_style_base64(&vec![salt_byte; salt_len]);
                let key_len = 15 + lcg.below(20);
                let key_byte = lcg.next() as u8;
                let key = go_style_base64(&vec![key_byte; key_len]);
                cases.push(build_hash(
                    "v=19",
                    &format!("m={memory},t={iterations},p={threads}"),
                    &salt,
                    &key,
                ));
            }
        }
    }

    cases
}
