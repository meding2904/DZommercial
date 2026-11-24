//! Differential tests against the Go `internal/config` oracle.
//!
//! The oracle is `backend/cmd/oracle-config`, run with `go run`. Each request
//! clears every configuration environment variable, applies the request's
//! environment, then calls `config.Load` or one of its methods.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use azahar_config::Config;
use proptest::prelude::*;
use serde_json::{Map, Value, json};

const CONFIG_KEYS: &[&str] = &[
    "APP_ENV",
    "API_ADDR",
    "APP_ORIGIN",
    "TURSO_DATABASE_URL",
    "TURSO_AUTH_TOKEN",
    "ADMIN_EMAIL",
    "ADMIN_PASSWORD",
    "CUSTOMER_EMAIL",
    "CUSTOMER_PASSWORD",
    "EMPLOYEE_EMAIL",
    "EMPLOYEE_PASSWORD",
    "SESSION_TTL_HOURS",
    "COURIER_API_URL",
    "COURIER_KEY",
    "COURIER_CREDENTIALS",
    "COURIER_FROM_WILAYA",
    "COURIER_TIMEOUT_SECONDS",
    "TURNSTILE_SECRET_KEY",
    "PLUGIN_SECRET_KEY",
    "AI_BASE_URL",
    "AI_TIMEOUT_SECONDS",
    "AUDIT_BATCH_INTERVAL_MS",
    "SEED_DEMO",
    "COOKIE_SECURE",
    "TRUST_PROXY",
];

struct Oracle {
    _child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Oracle {
    fn start() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repository root");
        let child = Command::new("go")
            .arg("run")
            .arg("./cmd/oracle-config")
            .current_dir(root.join("backend"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn go oracle");

        let mut child = child;
        let stdin = BufWriter::new(child.stdin.take().expect("oracle stdin"));
        let stdout = BufReader::new(child.stdout.take().expect("oracle stdout"));
        Self {
            _child: child,
            stdin,
            stdout,
        }
    }

    fn request(&mut self, request: &Value) -> Value {
        let line = serde_json::to_string(request).expect("serialize oracle request");
        writeln!(self.stdin, "{line}").expect("write oracle request");
        self.stdin.flush().expect("flush oracle request");

        let mut response = String::new();
        let read = self
            .stdout
            .read_line(&mut response)
            .expect("read oracle response");
        assert!(read > 0, "oracle exited before responding to {line}");
        serde_json::from_str(&response)
            .unwrap_or_else(|error| panic!("invalid oracle response {response:?}: {error}"))
    }
}

fn oracle() -> MutexGuard<'static, Oracle> {
    static ORACLE: OnceLock<Mutex<Oracle>> = OnceLock::new();
    ORACLE
        .get_or_init(|| Mutex::new(Oracle::start()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

fn with(mut base: HashMap<String, String>, key: &str, value: &str) -> HashMap<String, String> {
    base.insert(key.to_string(), value.to_string());
    base
}

fn without(mut base: HashMap<String, String>, key: &str) -> HashMap<String, String> {
    base.remove(key);
    base
}

fn development_env() -> HashMap<String, String> {
    env(&[("APP_ENV", "development")])
}

fn production_env() -> HashMap<String, String> {
    env(&[
        ("APP_ENV", "production"),
        ("TURSO_DATABASE_URL", "http://127.0.0.1:8880"),
        ("TURSO_AUTH_TOKEN", "test-token"),
        ("APP_ORIGIN", "https://shop.example"),
        ("ADMIN_EMAIL", "admin@shop.example"),
        ("ADMIN_PASSWORD", "a-unique-production-password"),
        ("CUSTOMER_PASSWORD", "a-unique-customer-password"),
        ("SEED_DEMO", "false"),
        ("COOKIE_SECURE", "false"),
        ("SESSION_TTL_HOURS", "168"),
        ("COURIER_API_URL", "https://freeship.dzbuild.com"),
        ("COURIER_KEY", "sandbox"),
        ("COURIER_FROM_WILAYA", "16"),
        ("COURIER_TIMEOUT_SECONDS", "12"),
        ("TURNSTILE_SECRET_KEY", "test-turnstile-secret"),
        ("PLUGIN_SECRET_KEY", "a-long-production-plugin-key"),
        ("AI_BASE_URL", "https://api.openai.com/v1"),
        ("AI_TIMEOUT_SECONDS", "25"),
    ])
}

fn json_env(environment: &HashMap<String, String>) -> Value {
    Value::Object(
        environment
            .iter()
            .map(|(key, value)| (key.clone(), Value::String(value.clone())))
            .collect::<Map<_, _>>(),
    )
}

fn request(environment: &HashMap<String, String>, op: &str) -> Value {
    json!({"env": json_env(environment), "op": op})
}

fn method_request(
    environment: &HashMap<String, String>,
    op: &str,
    overrides: &[(&str, Value)],
) -> Value {
    let mut request = request(environment, op);
    let object = request.as_object_mut().expect("request object");
    for (key, value) in overrides {
        object.insert((*key).to_string(), value.clone());
    }
    request
}

fn duration_ns(duration: Duration) -> i64 {
    i64::try_from(duration.as_nanos()).expect("duration fits i64 nanoseconds")
}

fn rust_config_value(config: &Config) -> Value {
    json!({
        "app_env": &config.app_env,
        "api_addr": &config.api_addr,
        "app_origin": &config.app_origin,
        "turso_url": &config.turso_url,
        "turso_token": &config.turso_token,
        "cookie_secure": config.cookie_secure,
        "trust_proxy": config.trust_proxy,
        "seed_demo": config.seed_demo,
        "admin_email": &config.admin_email,
        "admin_password": &config.admin_password,
        "customer_email": &config.customer_email,
        "customer_password": &config.customer_password,
        "employee_email": &config.employee_email,
        "employee_password": &config.employee_password,
        "session_ttl_ns": duration_ns(config.session_ttl),
        "courier_api_url": &config.courier_api_url,
        "courier_key": &config.courier_key,
        "courier_credentials": &config.courier_credentials,
        "courier_from_wilaya": config.courier_from_wilaya,
        "courier_timeout_ns": duration_ns(config.courier_timeout),
        "turnstile_secret": &config.turnstile_secret,
        "plugin_secret_key": &config.plugin_secret_key,
        "ai_base_url": &config.ai_base_url,
        "ai_timeout_ns": duration_ns(config.ai_timeout),
        "audit_batch_interval_ns": duration_ns(config.audit_batch_interval),
    })
}

fn load_rust(environment: &HashMap<String, String>) -> Result<Config, azahar_config::ConfigError> {
    Config::load_from(|key| environment.get(key).cloned())
}

fn check_load(environment: &HashMap<String, String>) {
    let rust = load_rust(environment);
    let response = oracle().request(&request(environment, "load"));

    match rust {
        Ok(config) => {
            assert_eq!(
                response["ok"],
                json!(true),
                "oracle failed for env {environment:?}: {response}"
            );
            assert_eq!(
                response["config"],
                rust_config_value(&config),
                "configuration mismatch for env {environment:?}"
            );
        }
        Err(error) => {
            assert_eq!(
                response["ok"],
                json!(false),
                "oracle unexpectedly loaded env {environment:?}: {response}"
            );
            assert_eq!(
                response["error"].as_str(),
                Some(error.message()),
                "error mismatch for env {environment:?}"
            );
        }
    }
}

fn check_dsn(environment: &HashMap<String, String>, url: &str, token: &str) {
    let response = oracle().request(&method_request(
        environment,
        "dsn",
        &[("turso_url", json!(url)), ("turso_token", json!(token))],
    ));
    let config = load_rust(environment).expect("dsn base environment must load");
    let config = Config {
        turso_url: url.to_string(),
        turso_token: token.to_string(),
        ..config
    };

    assert_eq!(response["ok"], json!(true), "{response}");
    assert_eq!(
        response["dsn"].as_str(),
        Some(config.dsn().as_str()),
        "dsn mismatch for url={url:?} token={token:?}"
    );
}

fn check_credentials(environment: &HashMap<String, String>, raw: &str) {
    let response = oracle().request(&method_request(
        environment,
        "credentials",
        &[("courier_credentials", json!(raw))],
    ));
    let config = load_rust(environment).expect("credentials base environment must load");
    let config = Config {
        courier_credentials: raw.to_string(),
        ..config
    };
    let (values, valid) = config.courier_credential_values();

    assert_eq!(response["ok"], json!(true), "{response}");
    assert_eq!(response["valid"], json!(valid), "raw={raw:?}");
    let expected = match values {
        Some(values) => json!(values),
        None => Value::Null,
    };
    assert_eq!(response["credentials"], expected, "raw={raw:?}");
}

fn check_flags(environment: &HashMap<String, String>, app_env: &str) {
    let response = oracle().request(&method_request(
        environment,
        "flags",
        &[("app_env", json!(app_env))],
    ));
    let config = load_rust(environment).expect("flags base environment must load");
    let config = Config {
        app_env: app_env.to_string(),
        ..config
    };

    assert_eq!(response["ok"], json!(true), "{response}");
    assert_eq!(response["is_production"], json!(config.is_production()));
    assert_eq!(response["is_development"], json!(config.is_development()));
}

fn check_admin_ttl(environment: &HashMap<String, String>, session_ttl_ns: i64) {
    let response = oracle().request(&method_request(
        environment,
        "admin_ttl",
        &[("session_ttl_ns", json!(session_ttl_ns))],
    ));
    let config = load_rust(environment).expect("admin_ttl base environment must load");
    let config = Config {
        session_ttl: Duration::from_nanos(session_ttl_ns.unsigned_abs()),
        ..config
    };

    assert_eq!(response["ok"], json!(true), "{response}");
    assert_eq!(
        response["admin_session_ttl_ns"].as_i64(),
        Some(duration_ns(config.admin_session_ttl())),
        "session_ttl_ns={session_ttl_ns}"
    );
}

#[test]
fn differential_environment_matrix() {
    let development = development_env();
    let production = production_env();

    // APP_ENV handling.
    check_load(&HashMap::new());
    check_load(&env(&[("APP_ENV", "")]));
    check_load(&env(&[("APP_ENV", "   ")]));
    check_load(&env(&[("APP_ENV", "staging")]));
    check_load(&env(&[("APP_ENV", " Production ")]));
    check_load(&env(&[("APP_ENV", "DEVELOPMENT")]));
    check_load(&env(&[("APP_ENV", "\u{00a0}development\u{00a0}")]));
    check_load(&env(&[("APP_ENV", "productİon")]));
    check_load(&env(&[("APP_ENV", "producſion")]));
    check_load(&development.clone());

    // Development defaults and string normalization.
    check_load(&with(development.clone(), "API_ADDR", ""));
    check_load(&with(development.clone(), "API_ADDR", "   "));
    check_load(&with(development.clone(), "API_ADDR", ":9000"));
    check_load(&with(development.clone(), "APP_ORIGIN", ""));
    check_load(&with(development.clone(), "APP_ORIGIN", "///"));
    check_load(&with(
        development.clone(),
        "APP_ORIGIN",
        "http://localhost:5173///",
    ));
    check_load(&with(
        development.clone(),
        "APP_ORIGIN",
        "HTTP://Example.COM/X",
    ));
    check_load(&with(
        development.clone(),
        "APP_ORIGIN",
        "http://user:pass@example.com",
    ));
    check_load(&with(
        development.clone(),
        "ADMIN_EMAIL",
        " Admin@Example.COM ",
    ));
    check_load(&with(development.clone(), "CUSTOMER_EMAIL", "ADMİN@X"));
    check_load(&with(development.clone(), "EMPLOYEE_EMAIL", " StAff@X "));
    check_load(&with(development.clone(), "ADMIN_EMAIL", "ΣİGMA@X"));
    check_load(&with(development.clone(), "ADMIN_EMAIL", "KK@X"));
    check_load(&with(development.clone(), "ADMIN_EMAIL", "ẞ@X"));
    check_load(&with(development.clone(), "COURIER_KEY", " SandBox "));
    check_load(&with(
        development.clone(),
        "COURIER_API_URL",
        " http://freeship.example/ ",
    ));
    check_load(&with(
        development.clone(),
        "COURIER_API_URL",
        "https://freeship.example///",
    ));
    check_load(&with(
        development.clone(),
        "AI_BASE_URL",
        " https://api.example.test/v1/ ",
    ));
    check_load(&with(
        development.clone(),
        "TURSO_DATABASE_URL",
        "libsql://db.example/",
    ));
    check_load(&with(development.clone(), "TURSO_AUTH_TOKEN", " tok "));
    check_load(&with(
        development.clone(),
        "COURIER_CREDENTIALS",
        "  {\"a\":\"b\"}  ",
    ));
    check_load(&with(
        development.clone(),
        "PLUGIN_SECRET_KEY",
        "0123456789abcdef",
    ));
    check_load(&with(development.clone(), "PLUGIN_SECRET_KEY", "short"));
    check_load(&with(development.clone(), "PLUGIN_SECRET_KEY", ""));
    check_load(&with(development.clone(), "SEED_DEMO", "false"));
    check_load(&with(development.clone(), "SEED_DEMO", "true"));
    check_load(&with(development.clone(), "COOKIE_SECURE", "true"));
    check_load(&with(development.clone(), "TRUST_PROXY", "TRUE"));

    // Invalid URL shapes.
    for value in [
        "localhost:5173",
        "ftp://shop.example",
        "http://",
        "https:///path",
        "http://exa mple.com",
        "http://example.com/%zz",
        "http://example.com:bad",
        "http://[::1",
        "example.com",
        "://x",
        "http:opaque",
        "http://example.com/a b",
        "http://example.com/é",
        "http://example.com/%FF",
        "http://[127.0.0.1]",
        "http://[fe80::1%25eth0]:80",
        "http://[fe80::1%25]:80",
    ] {
        check_load(&with(development.clone(), "APP_ORIGIN", value));
        check_load(&with(development.clone(), "COURIER_API_URL", value));
        check_load(&with(development.clone(), "AI_BASE_URL", value));
    }
    check_load(&with(
        development.clone(),
        "AI_BASE_URL",
        "https://user:secret@ai.example.test/v1",
    ));
    check_load(&with(
        development.clone(),
        "AI_BASE_URL",
        "http://user:secret@ai.example.test/v1",
    ));

    // Integer parsing and bounds.
    for value in ["1", "58", "0", "59", "-1", "16.5", "+16", " 16 "] {
        check_load(&with(development.clone(), "COURIER_FROM_WILAYA", value));
    }
    for value in ["1", "30", "0", "31", "-5", "abc", "+12", "012", " 12 "] {
        check_load(&with(development.clone(), "COURIER_TIMEOUT_SECONDS", value));
    }
    for value in ["5", "60", "4", "61", "-1", "abc", "+25"] {
        check_load(&with(development.clone(), "AI_TIMEOUT_SECONDS", value));
    }
    for value in [
        "-9223372036854775808",
        "9223372036854775807",
        "9223372036854775808",
    ] {
        check_load(&with(development.clone(), "COURIER_TIMEOUT_SECONDS", value));
        check_load(&with(development.clone(), "SESSION_TTL_HOURS", value));
        check_load(&with(development.clone(), "AI_TIMEOUT_SECONDS", value));
        check_load(&with(development.clone(), "AUDIT_BATCH_INTERVAL_MS", value));
        check_load(&with(development.clone(), "COURIER_FROM_WILAYA", value));
    }
    for value in [
        "",
        "0",
        "-1",
        "1",
        "168",
        "720",
        "721",
        "1e3",
        "99999999999999999999",
        "9223372036854775807",
    ] {
        check_load(&with(development.clone(), "SESSION_TTL_HOURS", value));
    }
    for value in [
        "",
        "100",
        "499",
        "500",
        "2500",
        "59999",
        "60000",
        "60001",
        "90000",
        "-100",
        "not-a-number",
        "9223372036854775807",
    ] {
        check_load(&with(development.clone(), "AUDIT_BATCH_INTERVAL_MS", value));
    }
    check_load(&with(development.clone(), "COURIER_KEY", &"x".repeat(64)));
    check_load(&with(development.clone(), "COURIER_KEY", &"x".repeat(65)));

    // Boolean spellings.
    for value in [
        "1", "t", "T", "TRUE", "true", "True", "0", "f", "F", "FALSE", "false", "False", "", "  ",
        "yes", "tRuE", "2", " true ",
    ] {
        let base = with(development.clone(), "SEED_DEMO", value);
        check_load(&with(base.clone(), "COOKIE_SECURE", value));
        check_load(&with(base, "TRUST_PROXY", value));
    }

    // Production requirements, one at a time.
    check_load(&production.clone());
    for key in [
        "TURSO_DATABASE_URL",
        "TURSO_AUTH_TOKEN",
        "APP_ORIGIN",
        "ADMIN_EMAIL",
        "ADMIN_PASSWORD",
        "TURNSTILE_SECRET_KEY",
        "PLUGIN_SECRET_KEY",
    ] {
        check_load(&without(production.clone(), key));
        check_load(&with(production.clone(), key, ""));
    }
    check_load(&with(production.clone(), "ADMIN_PASSWORD", "short"));
    check_load(&with(production.clone(), "ADMIN_PASSWORD", "exactly13char"));
    check_load(&with(
        production.clone(),
        "ADMIN_PASSWORD",
        "exactly14chars",
    ));
    check_load(&with(
        production.clone(),
        "ADMIN_PASSWORD",
        "azahar-dev-admin-2026",
    ));
    check_load(&with(
        production.clone(),
        "TURNSTILE_SECRET_KEY",
        "1x0000000000000000000000000000000AA",
    ));
    check_load(&with(
        production.clone(),
        "TURNSTILE_SECRET_KEY",
        "2x0000000000000000000000000000000AA",
    ));
    check_load(&with(
        production.clone(),
        "TURNSTILE_SECRET_KEY",
        "3x0000000000000000000000000000000AA",
    ));
    check_load(&with(
        production.clone(),
        "TURNSTILE_SECRET_KEY",
        "0x0000000000000000000000000000000AA",
    ));
    check_load(&with(
        production.clone(),
        "TURNSTILE_SECRET_KEY",
        " 1x0000000000000000000000000000000AA ",
    ));
    check_load(&with(
        production.clone(),
        "PLUGIN_SECRET_KEY",
        "azahar-dev-plugin-key-2026",
    ));
    check_load(&with(
        production.clone(),
        "PLUGIN_SECRET_KEY",
        &" ".repeat(16),
    ));
    check_load(&with(production.clone(), "PLUGIN_SECRET_KEY", "short"));
    check_load(&with(
        production.clone(),
        "COURIER_API_URL",
        "http://freeship.example",
    ));
    check_load(&with(
        production.clone(),
        "COURIER_API_URL",
        "ftp://freeship.example",
    ));
    check_load(&with(production.clone(), "COURIER_API_URL", ""));
    check_load(&with(
        production.clone(),
        "AI_BASE_URL",
        "http://ai.example/v1",
    ));
    check_load(&with(
        production.clone(),
        "AI_BASE_URL",
        "https://user:secret@ai.example/v1",
    ));
    check_load(&with(
        production.clone(),
        "AI_BASE_URL",
        "http://user:secret@ai.example/v1",
    ));
    check_load(&with(production.clone(), "AI_TIMEOUT_SECONDS", "4"));
    check_load(&with(production.clone(), "AI_TIMEOUT_SECONDS", "61"));
    check_load(&with(production.clone(), "COURIER_TIMEOUT_SECONDS", "0"));
    check_load(&with(production.clone(), "COURIER_TIMEOUT_SECONDS", "31"));
    check_load(&with(production.clone(), "COURIER_FROM_WILAYA", "0"));
    check_load(&with(production.clone(), "COURIER_FROM_WILAYA", "59"));
    check_load(&with(production.clone(), "COURIER_KEY", ""));
    check_load(&with(production.clone(), "SESSION_TTL_HOURS", "721"));
    check_load(&with(production.clone(), "SESSION_TTL_HOURS", "0"));
    check_load(&with(
        production.clone(),
        "TURSO_AUTH_TOKEN",
        "  spaced-token  ",
    ));
    check_load(&with(
        production.clone(),
        "APP_ORIGIN",
        "https://shop.example/",
    ));
    check_load(&env(&[
        ("APP_ENV", " Production "),
        ("TURSO_DATABASE_URL", "http://127.0.0.1:8880"),
        ("TURSO_AUTH_TOKEN", "test-token"),
        ("APP_ORIGIN", "https://shop.example"),
        ("ADMIN_EMAIL", "admin@shop.example"),
        ("ADMIN_PASSWORD", "a-unique-production-password"),
        ("TURNSTILE_SECRET_KEY", "test-turnstile-secret"),
        ("PLUGIN_SECRET_KEY", "a-long-production-plugin-key"),
        ("AI_BASE_URL", "https://api.openai.com/v1"),
    ]));

    // SEED_DEMO production branches.
    for seed in ["true", "1", "TRUE"] {
        let seeded = with(production.clone(), "SEED_DEMO", seed);
        check_load(&seeded);
        check_load(&with(
            seeded.clone(),
            "CUSTOMER_PASSWORD",
            "azahar-dev-client-2026",
        ));
        check_load(&with(
            seeded.clone(),
            "EMPLOYEE_PASSWORD",
            "azahar-dev-staff-2026",
        ));
        check_load(&with(seeded.clone(), "EMPLOYEE_PASSWORD", "123456789"));
        check_load(&with(seeded, "EMPLOYEE_PASSWORD", "0123456789"));
    }
}

#[test]
fn differential_dsn() {
    let base = development_env();
    let urls = [
        "http://127.0.0.1:8880",
        "http://example.com",
        "http://example.com/",
        "https://shop.example:443",
        "https://shop.example:443/a",
        "HTTP://Example.COM/X",
        "http://example.com/path?x=1&y=two%20words",
        "http://example.com/?authToken=existing",
        "http://example.com/?authToken=",
        "http://example.com/?authToken=existing&authToken=second",
        "http://example.com/?AuthToken=x",
        "http://example.com/?authToken=x+y",
        "http://example.com/?authToken=%zz",
        "http://example.com/?a=%zz&b=2",
        "http://example.com/?a=%FF",
        "http://example.com/%FF",
        "http://example.com/%c3%a9",
        "http://example.com/a%2fb",
        "http://example.com/a b",
        "http://example.com/é",
        "http://example.com/foo#frag",
        "http://example.com/foo?#frag",
        "http://example.com/#",
        "http://example.com/#\u{0}",
        "http://example.com/\u{0}",
        "http://example.com/\u{7f}",
        "http://example.com/%",
        "http://example.com:80/",
        "http://example.com:000/",
        "http://example.com:/",
        "http://example.com:bad",
        "http://example.com: 80/",
        "http://user:pass@example.com/x",
        "http://user@example.com/x",
        "http://[::1]",
        "http://[::1]:8080",
        "http://[::1",
        "http://[fe80::1%25eth0]:80/x",
        "http://[fe80::1%25]:80",
        "http://[::ffff:127.0.0.1]",
        "http://[127.0.0.1]",
        "http://[example.com]",
        "http://a:b:c",
        "http:opaque",
        "mailto:user@example.com",
        "localhost:8080",
        "127.0.0.1:8880",
        "//example.com/x",
        "/x",
        "*",
        "",
        "http:",
        "http://",
        "http:///path",
        "http:/path",
        "http://user@/x",
        "http://example.com/?a=1;b=2",
        "http://example.com/?a;b=1",
        "http://example.com/?=x",
        "http://example.com/?a",
        "http://example.com/?a=1&a=2&b=%20",
        "http://example.com/?q=hello+world",
        "http://example.com/?q=hello%20world",
        "http://example.com/a?x=%41&y=a+b",
        "http://exa mple.com/a b",
        "http://ex%61mple.com/x",
        "http://example.com/a??b",
        "http://example.com/a#b#c",
        "http://example.com/?%3Fb",
    ];
    let tokens = ["", "tok", "a b", "t*~ +/%&=?#", "é", "line\nbreak"];

    for url in urls {
        for token in tokens {
            check_dsn(&base, url, token);
        }
    }

    // Token supplied through the environment, including leading/trailing space.
    check_dsn(
        &with(base.clone(), "TURSO_AUTH_TOKEN", " env-token "),
        "http://example.com",
        " env-token ",
    );
}

#[test]
fn differential_credentials() {
    let base = development_env();
    let cases = [
        "",
        "   ",
        "\u{00a0}",
        "{}",
        r#"{"apiId":"id","apiToken":"token"}"#,
        " null ",
        "null",
        "{not json",
        r#"["nope"]"#,
        r#""nope""#,
        "123",
        "true",
        r#"{"a":null}"#,
        r#"{"a":1}"#,
        r#"{"a":true}"#,
        r#"{"a":{}}"#,
        r#"{"a":[]}"#,
        r#"{"a":"b","a":"y"}"#,
        r#"{"a":"x","a":null}"#,
        r#"{"a":"\ud800"}"#,
        r#"{"a":"\ud800\ud800"}"#,
        r#"{"a":"\ud83d\ude00"}"#,
        r#"{"a":"\ud800\uZZZZ"}"#,
        r#"{"\u0061":"x"}"#,
        r#"{"a":"x\ny\t\"\\\/"}"#,
        r#"{"a":"x\u0000y"}"#,
        r#"{"a":"x"}"#,
        r#"{"a":"x"} x"#,
        r#"{ "a" : "b" }"#,
        r#"{"a":"b",}"#,
        r#"{"a":"b""#,
        r#"{"a":"é","b":"İ"}"#,
        "{\"a\":\"raw\ncontrol\"}",
        "{\"a\":\"\u{7f}\"}",
    ];

    for raw in cases {
        check_credentials(&base, raw);
    }
}

#[test]
fn differential_flags_and_admin_ttl() {
    let base = development_env();
    for app_env in [
        "production",
        "PRODUCTION",
        "Production",
        " production",
        "production ",
        "staging",
        "development",
        "DEVELOPMENT",
        "DevElopment",
        "",
        "productİon",
        "producſion",
        "producTion",
    ] {
        check_flags(&base, app_env);
    }

    for nanos in [
        0,
        1,
        12 * 3600 * 1_000_000_000 - 1,
        12 * 3600 * 1_000_000_000,
        12 * 3600 * 1_000_000_000 + 1,
        13 * 3600 * 1_000_000_000,
        100 * 3600 * 1_000_000_000,
        168 * 3600 * 1_000_000_000,
        720 * 3600 * 1_000_000_000,
    ] {
        check_admin_ttl(&base, nanos);
    }
}

fn key_strategy() -> impl Strategy<Value = String> {
    proptest::sample::select(CONFIG_KEYS.to_vec()).prop_map(str::to_string)
}

const INTERESTING_CHARS: &[char] = &[
    'a', 'Z', '0', ' ', '\t', '\n', ':', '/', '?', '#', '@', '%', '[', ']', '+', '=', '&', ';',
    ',', '.', '-', '_', '~', '!', '$', '\'', '(', ')', '*', '"', '\\', '{', '}', 'é', 'İ',
    '\u{00a0}', '\u{212a}', '\u{7f}', 'x',
];

fn value_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        2 => prop::collection::vec(any::<char>().prop_filter("no NUL", |c| *c != '\0'), 0..20)
            .prop_map(|chars| chars.into_iter().collect()),
        3 => prop::collection::vec(
            proptest::sample::select(INTERESTING_CHARS.to_vec()),
            0..24,
        )
        .prop_map(|chars| chars.into_iter().collect()),
        1 => Just(String::new()),
    ]
}

fn random_env() -> impl Strategy<Value = HashMap<String, String>> {
    prop::collection::hash_map(key_strategy(), value_strategy(), 0..=12)
}

fn mutated_env() -> impl Strategy<Value = HashMap<String, String>> {
    (
        0usize..2,
        prop::collection::vec((key_strategy(), value_strategy()), 0..=4),
    )
        .prop_map(|(base, mutations)| {
            let mut environment = if base == 0 {
                development_env()
            } else {
                production_env()
            };
            for (key, value) in mutations {
                environment.insert(key, value);
            }
            environment
        })
}

const URL_CHARS: &[char] = &[
    'a', 'Z', '0', '1', '9', ':', '/', '?', '#', '@', '%', '[', ']', '+', '=', '&', ';', ',', '.',
    '-', '_', '~', '!', '$', '\'', '(', ')', '*', '"', '\\', ' ', '\t', '\n', '\u{0}', 'é', 'İ',
    '\u{7f}', 'x', 'F', 'f', 'e', 'h', 't', 'p', 's', 'u', 'r', 'l', 'o', 'c', 'd', 'b',
];

fn opaque_string() -> impl Strategy<Value = String> {
    prop::collection::vec(proptest::sample::select(URL_CHARS.to_vec()), 0..32)
        .prop_map(|chars| chars.into_iter().collect())
}

fn url_string() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => opaque_string(),
        1 => prop_oneof![
            Just("http://example.com".to_string()),
            Just("https://shop.example/a?b=c#d".to_string()),
            Just("http://[::1]:8080/x".to_string()),
            Just("http://user:pass@example.com/".to_string()),
            Just("http://example.com/?authToken=x".to_string()),
            Just("http://example.com/%FF".to_string()),
            Just("http:opaque".to_string()),
            Just(String::new()),
        ],
    ]
}

#[test]
fn differential_dsn_fixed_extra() {
    let base = development_env();
    for url in [
        "http://ex%61mple.com/x",
        "http://ex%2fmple.com/x",
        "http://[fe80::1%25a%20b]:80",
        "http://[fe80::1%25eth%25x]:80",
        "http://example.com/?%",
        "http://example.com/?a=%",
        "http://example.com/?a+=b",
        "http://example.com/?a%20=b",
        "http://example.com/?a=%26%3D",
        "http://example.com/#%zz",
        "http://example.com/#%00",
        "http://example.com/a%2F%2f",
        "https://user:p%40ss@example.com/x?z=1",
        "//",
        "///",
        "a/b",
        "a:b",
        "1a:b",
    ] {
        check_dsn(&base, url, "tok");
        check_dsn(&base, url, "");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn generated_environments_match(environment in prop_oneof![random_env(), mutated_env()]) {
        check_load(&environment);
    }

    #[test]
    fn generated_dsn_matches(url in url_string(), token in value_strategy()) {
        check_dsn(&development_env(), &url, &token);
    }

    #[test]
    fn generated_credentials_match(raw in value_strategy()) {
        check_credentials(&development_env(), &raw);
    }
}
