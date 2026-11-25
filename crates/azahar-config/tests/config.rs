use std::collections::HashMap;
use std::time::Duration;

use azahar_config::{Config, ConfigError};

fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

fn load(env: &HashMap<String, String>) -> Result<Config, ConfigError> {
    Config::load_from(|key| env.get(key).cloned())
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

fn with(mut base: HashMap<String, String>, key: &str, value: &str) -> HashMap<String, String> {
    base.insert(key.to_string(), value.to_string());
    base
}

#[test]
fn production_requires_admin_password() {
    let result = load(&with(production_env(), "ADMIN_PASSWORD", ""));
    assert!(
        result
            .unwrap_err()
            .message()
            .contains("ADMIN_PASSWORD must be set")
    );
}

#[test]
fn production_requires_turnstile_secret() {
    let result = load(&with(production_env(), "TURNSTILE_SECRET_KEY", ""));
    assert!(
        result
            .unwrap_err()
            .message()
            .contains("TURNSTILE_SECRET_KEY must be set")
    );
}

#[test]
fn production_rejects_turnstile_test_secret() {
    let result = load(&with(
        production_env(),
        "TURNSTILE_SECRET_KEY",
        "1x0000000000000000000000000000000AA",
    ));
    assert!(
        result
            .unwrap_err()
            .message()
            .contains("must not be a Turnstile test key")
    );
}

#[test]
fn production_rejects_development_admin_password() {
    let result = load(&with(
        production_env(),
        "ADMIN_PASSWORD",
        "azahar-dev-admin-2026",
    ));
    assert!(
        result
            .unwrap_err()
            .message()
            .contains("development default")
    );
}

#[test]
fn production_requires_app_origin() {
    let result = load(&with(production_env(), "APP_ORIGIN", ""));
    assert!(
        result
            .unwrap_err()
            .message()
            .contains("APP_ORIGIN must be set")
    );
}

#[test]
fn production_rejects_development_customer_password_when_seeding() {
    let base = with(production_env(), "SEED_DEMO", "true");
    let result = load(&with(base, "CUSTOMER_PASSWORD", "azahar-dev-client-2026"));
    assert!(result.unwrap_err().message().contains("CUSTOMER_PASSWORD"));
}

#[test]
fn production_accepts_strong_configuration() {
    let cfg = load(&production_env()).expect("production configuration should load");
    assert!(cfg.cookie_secure, "production must force secure cookies");
    assert_eq!(cfg.admin_password, "a-unique-production-password");
}

#[test]
fn app_env_must_be_explicit() {
    let result = load(&env(&[("APP_ENV", "")]));
    assert!(
        result
            .unwrap_err()
            .message()
            .contains("APP_ENV must be set")
    );
}

#[test]
fn unknown_app_env_fails_closed() {
    let result = load(&env(&[("APP_ENV", "staging")]));
    assert!(result.unwrap_err().message().contains("APP_ENV"));
}

#[test]
fn development_defaults_still_load() {
    load(&env(&[("APP_ENV", "development")])).expect("development configuration should load");
}

#[test]
fn production_requires_https_for_courier() {
    let result = load(&with(
        production_env(),
        "COURIER_API_URL",
        "http://freeship.example",
    ));
    assert!(
        result
            .unwrap_err()
            .message()
            .contains("COURIER_API_URL must use https")
    );
}

#[test]
fn courier_from_wilaya_must_be_valid() {
    let result = load(&with(production_env(), "COURIER_FROM_WILAYA", "0"));
    assert!(
        result
            .unwrap_err()
            .message()
            .contains("COURIER_FROM_WILAYA")
    );
}

#[test]
fn production_requires_plugin_secret() {
    let result = load(&with(production_env(), "PLUGIN_SECRET_KEY", "short"));
    assert!(result.unwrap_err().message().contains("PLUGIN_SECRET_KEY"));
}

#[test]
fn production_requires_https_for_ai() {
    let result = load(&with(
        production_env(),
        "AI_BASE_URL",
        "http://ai.example.test/v1",
    ));
    assert!(
        result
            .unwrap_err()
            .message()
            .contains("AI_BASE_URL must use https")
    );
}

#[test]
fn ai_timeout_bounds() {
    let result = load(&with(production_env(), "AI_TIMEOUT_SECONDS", "3"));
    assert!(result.unwrap_err().message().contains("AI_TIMEOUT_SECONDS"));
}

#[test]
fn ai_url_rejects_credentials() {
    let result = load(&with(
        production_env(),
        "AI_BASE_URL",
        "https://user:secret@ai.example.test/v1",
    ));
    assert!(
        result
            .unwrap_err()
            .message()
            .contains("must not contain credentials")
    );
}

#[test]
fn courier_credentials_parse_leniently() {
    let cases = [
        ("", Some(HashMap::new()), true),
        ("   ", Some(HashMap::new()), true),
        (
            r#"{"apiId":"id","apiToken":"token"}"#,
            Some(HashMap::from([
                ("apiId".to_string(), "id".to_string()),
                ("apiToken".to_string(), "token".to_string()),
            ])),
            true,
        ),
        ("null", None, true),
        ("{not json", Some(HashMap::new()), false),
        (r#"["nope"]"#, Some(HashMap::new()), false),
        (r#""nope""#, Some(HashMap::new()), false),
    ];

    for (raw, want, valid) in cases {
        let cfg = Config {
            courier_credentials: raw.to_string(),
            ..Config::default()
        };
        let (got, got_valid) = cfg.courier_credential_values();
        assert_eq!(got_valid, valid, "{raw:?}");
        assert_eq!(got, want, "{raw:?}");
    }
}

#[test]
fn audit_batch_interval_clamping() {
    let cases = [
        ("", Duration::from_secs(2)),
        ("100", Duration::from_millis(500)),
        ("500", Duration::from_millis(500)),
        ("2500", Duration::from_millis(2500)),
        ("60000", Duration::from_secs(60)),
        ("90000", Duration::from_secs(60)),
        ("not-a-number", Duration::from_secs(2)),
        ("-100", Duration::from_millis(500)),
    ];
    for (value, want) in cases {
        let cfg = load(&env(&[
            ("APP_ENV", "development"),
            ("SEED_DEMO", "false"),
            ("AUDIT_BATCH_INTERVAL_MS", value),
        ]))
        .expect("load configuration");
        assert_eq!(cfg.audit_batch_interval, want, "{value:?}");
    }
}

#[test]
fn session_ttl_boundaries() {
    let cases = [
        ("0", Duration::from_secs(168 * 3600)),
        ("-1", Duration::from_secs(168 * 3600)),
        ("1", Duration::from_secs(3600)),
        ("720", Duration::from_secs(720 * 3600)),
    ];
    for (value, want) in cases {
        let cfg = load(&env(&[
            ("APP_ENV", "development"),
            ("SESSION_TTL_HOURS", value),
        ]))
        .expect("load configuration");
        assert_eq!(cfg.session_ttl, want, "{value:?}");
    }

    let error = load(&env(&[
        ("APP_ENV", "development"),
        ("SESSION_TTL_HOURS", "721"),
    ]))
    .unwrap_err();
    assert_eq!(error.message(), "SESSION_TTL_HOURS must be 720 or less");
}

#[test]
fn boolean_values_follow_parse_bool() {
    for value in ["1", "t", "T", "TRUE", "true", "True"] {
        let cfg = load(&env(&[("APP_ENV", "development"), ("SEED_DEMO", value)]))
            .expect("load configuration");
        assert!(cfg.seed_demo, "{value}");
    }
    for value in ["0", "f", "F", "FALSE", "false", "False"] {
        let cfg = load(&env(&[("APP_ENV", "development"), ("SEED_DEMO", value)]))
            .expect("load configuration");
        assert!(!cfg.seed_demo, "{value}");
    }
    for value in ["", "  ", "yes", "tRuE", "2"] {
        let cfg = load(&env(&[("APP_ENV", "development"), ("SEED_DEMO", value)]))
            .expect("load configuration");
        assert!(
            cfg.seed_demo,
            "{value:?} should fall back to the development default"
        );
    }
}

#[test]
fn admin_session_ttl_caps_at_twelve_hours() {
    let cases = [
        (Duration::ZERO, Duration::ZERO),
        (
            Duration::from_secs(11 * 3600),
            Duration::from_secs(11 * 3600),
        ),
        (
            Duration::from_secs(12 * 3600),
            Duration::from_secs(12 * 3600),
        ),
        (
            Duration::from_secs(13 * 3600),
            Duration::from_secs(12 * 3600),
        ),
        (
            Duration::from_secs(720 * 3600),
            Duration::from_secs(12 * 3600),
        ),
    ];
    for (session_ttl, want) in cases {
        let cfg = Config {
            session_ttl,
            ..Config::default()
        };
        assert_eq!(cfg.admin_session_ttl(), want);
    }
}

#[test]
fn url_validation_matches_go_shapes() {
    let valid = [
        "http://localhost:5173",
        "https://shop.example/",
        "HTTP://Example.COM/X",
        "http://user:pass@example.com",
        "http://[::1]:8080",
    ];
    for value in valid {
        load(&env(&[("APP_ENV", "development"), ("APP_ORIGIN", value)]))
            .unwrap_or_else(|error| panic!("{value:?} should be valid: {error}"));
    }

    let invalid = [
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
    ];
    for value in invalid {
        let error = load(&env(&[("APP_ENV", "development"), ("APP_ORIGIN", value)])).unwrap_err();
        assert_eq!(
            error.message(),
            "APP_ORIGIN must be an absolute http(s) URL"
        );
    }
}

#[test]
fn dsn_matches_go_shapes() {
    let cfg = Config {
        turso_url: "http://127.0.0.1:8880".to_string(),
        turso_token: "tok".to_string(),
        ..Config::default()
    };
    assert_eq!(cfg.dsn(), "http://127.0.0.1:8880?authToken=tok");

    let cfg = Config {
        turso_url: "http://example.com/?authToken=existing&z=1".to_string(),
        turso_token: "tok".to_string(),
        ..Config::default()
    };
    assert_eq!(cfg.dsn(), "http://example.com/?authToken=existing&z=1");

    let cfg = Config {
        turso_url: "http://example.com/#frag".to_string(),
        turso_token: "t*~ +/".to_string(),
        ..Config::default()
    };
    assert_eq!(cfg.dsn(), "http://example.com/?authToken=t%2A~+%2B%2F#frag");

    let cfg = Config {
        turso_url: "http://[::1".to_string(),
        turso_token: "tok".to_string(),
        ..Config::default()
    };
    assert_eq!(cfg.dsn(), "http://[::1");

    let cfg = Config {
        turso_url: "http://example.com".to_string(),
        turso_token: String::new(),
        ..Config::default()
    };
    assert_eq!(cfg.dsn(), "http://example.com");
}
