//! Settings merge, mask and encryption behaviour, including the SQL-backed
//! storage path.

use std::collections::HashMap;
use std::sync::Arc;

use azahar_db::migrate;
use azahar_plugins::{
    Env, Manifest, SettingField, SettingOption, UNREADABLE_MESSAGE, mask_settings, merge_settings,
    settings_complete,
};
use azahar_secrets::SecretBox;
use azahar_store::Store;
use chrono::{TimeZone, Utc};
use libsql::Builder;

fn settings_manifest() -> Manifest {
    Manifest {
        id: "sample".to_string(),
        name: "Sample".to_string(),
        version: "1.0.0".to_string(),
        settings: vec![
            SettingField {
                key: "api_key".to_string(),
                label: "API key".to_string(),
                field_type: "secret".to_string(),
                required: true,
                max_length: 512,
                ..SettingField::default()
            },
            SettingField {
                key: "model".to_string(),
                label: "Model".to_string(),
                field_type: "text".to_string(),
                required: true,
                default: "gpt-4o-mini".to_string(),
                ..SettingField::default()
            },
            SettingField {
                key: "enabled".to_string(),
                label: "Enabled".to_string(),
                field_type: "boolean".to_string(),
                default: "true".to_string(),
                ..SettingField::default()
            },
        ],
        ..Manifest::default()
    }
}

fn map(entries: &[(&str, &str)]) -> HashMap<String, String> {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

fn text_field(key: &str, field_type: &str) -> SettingField {
    SettingField {
        key: key.to_string(),
        label: key.to_string(),
        field_type: field_type.to_string(),
        ..SettingField::default()
    }
}

#[test]
fn merge_keeps_blank_secret() {
    let existing = map(&[
        ("api_key", "sk-old"),
        ("model", "gpt-4o-mini"),
        ("enabled", "true"),
    ]);
    let (merged, fields) = merge_settings(
        &settings_manifest(),
        &existing,
        &map(&[
            ("api_key", ""),
            ("model", "gpt-4.1-mini"),
            ("enabled", "false"),
        ]),
    );
    assert!(fields.is_empty());
    assert_eq!(merged["api_key"], "sk-old");
    assert_eq!(merged["model"], "gpt-4.1-mini");
    assert_eq!(merged["enabled"], "false");
}

#[test]
fn merge_rejects_unknown_and_missing() {
    let (_, fields) = merge_settings(
        &settings_manifest(),
        &HashMap::new(),
        &map(&[("nonsense", "x")]),
    );
    assert_eq!(fields["nonsense"], "This plugin has no such setting.");
    assert_eq!(fields["api_key"], "This setting is required.");
    assert_eq!(fields["model"], "This setting is required.");
    assert!(!fields.contains_key("enabled"));
}

#[test]
fn merge_trims_submitted_values() {
    let (merged, fields) = merge_settings(
        &settings_manifest(),
        &map(&[("api_key", "sk-old")]),
        &map(&[("model", "  slim  "), ("enabled", " true ")]),
    );
    assert!(fields.is_empty());
    assert_eq!(merged["model"], "slim");
    assert_eq!(merged["enabled"], "true");
}

#[test]
fn merge_required_secret_can_come_from_storage() {
    let (merged, fields) = merge_settings(
        &settings_manifest(),
        &map(&[("api_key", "sk-old"), ("model", "m")]),
        &map(&[("api_key", "")]),
    );
    assert!(fields.is_empty());
    assert_eq!(merged["api_key"], "sk-old");
}

#[test]
fn merge_required_text_accepts_an_empty_submission() {
    // Go only checks `required` when the key is absent, so an explicitly empty
    // non-secret value is stored as empty. SettingsComplete catches it later.
    let (merged, fields) = merge_settings(
        &settings_manifest(),
        &map(&[("api_key", "sk-old")]),
        &map(&[("model", "")]),
    );
    assert!(fields.is_empty());
    assert_eq!(merged["model"], "");
    assert!(!settings_complete(&settings_manifest(), &merged));
}

#[test]
fn merge_text_length_uses_defaults_and_reports_max_plus_one() {
    let manifest = Manifest {
        settings: vec![text_field("model", "text")],
        ..Manifest::default()
    };
    let (_, fields) = merge_settings(
        &manifest,
        &HashMap::new(),
        &map(&[("model", &"a".repeat(300))]),
    );
    assert!(fields.is_empty());
    let (_, fields) = merge_settings(
        &manifest,
        &HashMap::new(),
        &map(&[("model", &"a".repeat(301))]),
    );
    assert_eq!(fields["model"], "Keep this under 301 characters.");

    let manifest = Manifest {
        settings: vec![text_field("api_key", "secret")],
        ..Manifest::default()
    };
    let (_, fields) = merge_settings(
        &manifest,
        &HashMap::new(),
        &map(&[("api_key", &"a".repeat(513))]),
    );
    assert_eq!(fields["api_key"], "Keep this under 513 characters.");

    let mut short = text_field("model", "text");
    short.max_length = 10;
    let manifest = Manifest {
        settings: vec![short],
        ..Manifest::default()
    };
    let (_, fields) = merge_settings(
        &manifest,
        &HashMap::new(),
        &map(&[("model", &"a".repeat(11))]),
    );
    assert_eq!(fields["model"], "Keep this under 11 characters.");
}

#[test]
fn merge_number_accepts_go_float_syntax() {
    let manifest = Manifest {
        settings: vec![text_field("count", "number")],
        ..Manifest::default()
    };
    for value in [
        "1", "1.5", "+1.5", "-1.5", ".5", "1.", "1e3", "1E3", "1e+3", "1e-3", "0x1p-2", "0x1.8p1",
        "1_000", "1_0.5", "NaN", "nan", "Inf", "+Inf", "-Inf", "Infinity", "infinity", "1e308",
        "1e-400",
    ] {
        let (merged, fields) =
            merge_settings(&manifest, &HashMap::new(), &map(&[("count", value)]));
        assert!(fields.is_empty(), "{value} was rejected: {fields:?}");
        assert_eq!(merged["count"], value);
    }
    for value in [
        "_1", "1_", "0x1", "1e", "e3", ".", "+", "1,5", "1e400", "0x1p9999", "",
    ] {
        let (_, fields) = merge_settings(&manifest, &HashMap::new(), &map(&[("count", value)]));
        assert_eq!(
            fields.get("count").map(String::as_str),
            Some("Enter a number."),
            "{value} was accepted"
        );
    }
}

#[test]
fn merge_boolean_and_select() {
    let mut select = text_field("mode", "select");
    select.options = vec![
        SettingOption {
            value: "fast".to_string(),
            label: "Fast".to_string(),
        },
        SettingOption {
            value: "slow".to_string(),
            label: "Slow".to_string(),
        },
    ];
    let manifest = Manifest {
        settings: vec![text_field("enabled", "boolean"), select],
        ..Manifest::default()
    };

    let (merged, fields) = merge_settings(
        &manifest,
        &HashMap::new(),
        &map(&[("enabled", "true"), ("mode", "fast")]),
    );
    assert!(fields.is_empty());
    assert_eq!(merged["enabled"], "true");
    assert_eq!(merged["mode"], "fast");

    let (_, fields) = merge_settings(
        &manifest,
        &HashMap::new(),
        &map(&[("enabled", "TRUE"), ("mode", "medium")]),
    );
    assert_eq!(fields["enabled"], "Choose true or false.");
    assert_eq!(fields["mode"], "Choose one of the listed options.");
}

#[test]
fn mask_hides_secrets_and_fills_defaults() {
    let (masked, set) = mask_settings(
        &settings_manifest(),
        &map(&[
            ("api_key", "sk-secret"),
            ("model", "gpt-4o-mini"),
            ("enabled", "true"),
        ]),
    );
    assert_eq!(masked["api_key"], "");
    assert_eq!(set, vec!["api_key"]);
    assert_eq!(masked["model"], "gpt-4o-mini");

    let (masked, set) = mask_settings(&settings_manifest(), &HashMap::new());
    assert_eq!(masked["model"], "gpt-4o-mini");
    assert_eq!(masked["enabled"], "true");
    assert_eq!(masked["api_key"], "");
    assert!(set.is_empty());
}

#[test]
fn settings_complete_checks_required_values() {
    let manifest = settings_manifest();
    assert!(settings_complete(
        &manifest,
        &map(&[("api_key", "x"), ("model", "m")])
    ));
    assert!(!settings_complete(&manifest, &map(&[("model", "m")])));
    assert!(!settings_complete(
        &manifest,
        &map(&[("api_key", "  "), ("model", "m")])
    ));
    // Defaults are display values, not stored ones.
    assert!(!settings_complete(&manifest, &HashMap::new()));
}

async fn memory_env(secret_key: &str) -> Env {
    let database = Builder::new_local(":memory:")
        .build()
        .await
        .expect("open in-memory database");
    let store = Store::new(database).expect("store");
    migrate(store.conn()).await.expect("migrate");
    Env::new(Arc::new(store), secret_key)
}

async fn save(env: &Env, plugin_id: &str, values: &HashMap<String, String>) {
    let payload = env.encode_settings(values).expect("encrypt settings");
    let now = Utc.with_ymd_and_hms(2026, 3, 4, 5, 6, 7).unwrap();
    env.store
        .save_plugin_settings(env.store.conn(), plugin_id, &payload, now)
        .await
        .expect("save settings");
}

#[test]
fn number_acceptance_matches_go_parse_float_for_review_cases() {
    let manifest = Manifest {
        settings: vec![text_field("count", "number")],
        ..Manifest::default()
    };
    let accepted = |value: &str| {
        let (_, fields) = merge_settings(&manifest, &HashMap::new(), &map(&[("count", value)]));
        fields.is_empty()
    };

    for value in [
        "0x_1p0",
        "0x_1.8p0",
        "0x1p-1024",
        "0x1p-1074",
        "0x1p-2000",
        "0x0.0000000000001p1024",
        "0x1.fffffffffffffp1023",
        "0x0p99999",
        "0x0p-99999",
        "0x.8p1",
        "0x8.p1",
        "1e-400",
        "1.7976931348623157e308",
        "NaN",
        "nan",
        "Inf",
        "+Inf",
        "-Inf",
        "Infinity",
        "+Infinity",
        "-infinity",
    ] {
        assert!(accepted(value), "{value} must be accepted like Go");
    }

    for value in [
        "+NaN",
        "-NaN",
        "0x1_p0",
        "0x1_p",
        "0x1p_0",
        "0x1p+_0",
        "0x1p-_0",
        "0x1.fffffffffffff8p1023",
        "0x1p1024",
        "0x2p1023",
        "0x1p99999",
        "1e400",
        "1e309",
        "1__0",
        "1_._5",
        "1._5",
        "1_.5",
        "_1",
        "1_",
        "1e_10",
        "1e10_",
        ".",
        "+",
        "-",
        "0x",
        "0x.p1",
        "0x1",
    ] {
        assert!(!accepted(value), "{value} must be rejected like Go");
    }
}

#[tokio::test]
async fn settings_round_trip_through_the_database() {
    let env = memory_env("a long development key").await;
    let values = map(&[
        ("api_key", "sk-test"),
        ("model", "gpt-4.1-mini"),
        ("enabled", "false"),
        ("unicode", "café ☕"),
    ]);

    assert_eq!(env.settings("sample").await.expect("load"), HashMap::new());
    let (missing, updated_at) = env
        .settings_with_time("sample")
        .await
        .expect("load with time");
    assert!(missing.is_empty());
    assert!(updated_at.is_none());

    save(&env, "sample", &values).await;
    let (loaded, updated_at) = env
        .settings_with_time("sample")
        .await
        .expect("load saved settings");
    assert_eq!(loaded, values);
    assert_eq!(
        updated_at,
        Some(Utc.with_ymd_and_hms(2026, 3, 4, 5, 6, 7).unwrap())
    );
    assert_eq!(env.settings("sample").await.expect("load"), values);
}

#[tokio::test]
async fn settings_are_encrypted_with_the_configured_key() {
    let env = memory_env("key one").await;
    let values = map(&[("api_key", "sk-test")]);
    save(&env, "sample", &values).await;

    let (raw, _) = env
        .store
        .plugin_settings("sample")
        .await
        .expect("read stored payload");
    assert!(raw.starts_with(azahar_secrets::PREFIX));
    assert!(!raw.contains("sk-test"));
    let plain = SecretBox::new("key one")
        .decrypt(&raw)
        .expect("decrypt stored payload");
    let decoded: HashMap<String, String> = serde_json::from_slice(&plain).expect("json");
    assert_eq!(decoded, values);

    let other = Env::new(Arc::clone(&env.store), "key two");
    let error = other.settings("sample").await.expect_err("wrong key");
    assert_eq!(error.to_string(), UNREADABLE_MESSAGE);
}

#[tokio::test]
async fn unreadable_payloads_use_the_go_message() {
    let env = memory_env("key").await;
    let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();

    env.store
        .save_plugin_settings(env.store.conn(), "plain", "not encrypted", now)
        .await
        .expect("save plain row");
    let error = env.settings("plain").await.expect_err("plain row");
    assert_eq!(error.to_string(), UNREADABLE_MESSAGE);

    let mut json_array = SecretBox::new("key").encrypt(b"[]").expect("encrypt");
    env.store
        .save_plugin_settings(env.store.conn(), "array", &json_array, now)
        .await
        .expect("save array row");
    assert_eq!(
        env.settings("array")
            .await
            .expect_err("array row")
            .to_string(),
        UNREADABLE_MESSAGE
    );

    json_array = SecretBox::new("key").encrypt(b"null").expect("encrypt");
    env.store
        .save_plugin_settings(env.store.conn(), "null", &json_array, now)
        .await
        .expect("save null row");
    assert!(
        env.settings("null")
            .await
            .expect("null is an empty map")
            .is_empty()
    );
}

#[tokio::test]
async fn merge_and_mask_round_trip_through_the_database() {
    let env = memory_env("round-trip-key").await;
    let stored = map(&[
        ("api_key", "sk-old"),
        ("model", "gpt-4o-mini"),
        ("enabled", "true"),
    ]);
    save(&env, "sample", &stored).await;

    let existing = env.settings("sample").await.expect("load existing");
    let (merged, fields) = merge_settings(
        &settings_manifest(),
        &existing,
        &map(&[("api_key", ""), ("model", "gpt-4.1-mini")]),
    );
    assert!(fields.is_empty());
    assert_eq!(merged["api_key"], "sk-old");
    assert!(settings_complete(&settings_manifest(), &merged));

    let payload = env.encode_settings(&merged).expect("encrypt merged");
    let now = Utc.with_ymd_and_hms(2026, 4, 5, 6, 7, 8).unwrap();
    env.store
        .save_plugin_settings(env.store.conn(), "sample", &payload, now)
        .await
        .expect("save merged");

    let (loaded, _) = env.settings_with_time("sample").await.expect("reload");
    let (masked, secrets_set) = mask_settings(&settings_manifest(), &loaded);
    assert_eq!(masked["api_key"], "");
    assert_eq!(secrets_set, vec!["api_key"]);
    assert_eq!(masked["model"], "gpt-4.1-mini");
    assert_eq!(masked["enabled"], "true");
}

#[tokio::test]
async fn null_setting_values_become_empty_strings() {
    let env = memory_env("key").await;
    let now = Utc.with_ymd_and_hms(2026, 2, 3, 4, 5, 6).unwrap();

    let payload = SecretBox::new("key")
        .encrypt(br#"{"a":null,"b":"x"}"#)
        .expect("encrypt null value");
    env.store
        .save_plugin_settings(env.store.conn(), "nulls", &payload, now)
        .await
        .expect("save null row");
    let values = env.settings("nulls").await.expect("null value is empty");
    assert_eq!(values, map(&[("a", ""), ("b", "x")]));

    let payload = SecretBox::new("key")
        .encrypt(br#"{"a":"x","a":null}"#)
        .expect("encrypt duplicate null");
    env.store
        .save_plugin_settings(env.store.conn(), "duplicate-null", &payload, now)
        .await
        .expect("save duplicate null row");
    assert_eq!(
        env.settings("duplicate-null")
            .await
            .expect("last null wins"),
        map(&[("a", "")])
    );

    // Go rejects every non-string, non-null value with the shared message,
    // even when a later duplicate key would overwrite it.
    for (id, plain) in [
        ("number", br#"{"a":1}"#.as_slice()),
        ("array", br#"{"a":[]}"#.as_slice()),
        ("object", br#"{"a":{}}"#.as_slice()),
        ("boolean", br#"{"a":true}"#.as_slice()),
        ("duplicate-before", br#"{"a":1,"a":"x"}"#.as_slice()),
        ("duplicate-after", br#"{"a":"x","a":1}"#.as_slice()),
    ] {
        let payload = SecretBox::new("key").encrypt(plain).expect("encrypt");
        env.store
            .save_plugin_settings(env.store.conn(), id, &payload, now)
            .await
            .expect("save typed row");
        assert_eq!(
            env.settings(id)
                .await
                .expect_err("non-string value")
                .to_string(),
            UNREADABLE_MESSAGE,
            "{id}"
        );
    }
}

#[tokio::test]
async fn encode_settings_accepts_an_empty_map() {
    let env = memory_env("key").await;
    let payload = env.encode_settings(&HashMap::new()).expect("encrypt");
    let plain = SecretBox::new("key").decrypt(&payload).expect("decrypt");
    assert_eq!(plain, b"{}");
    assert_eq!(env.settings("missing").await.expect("load"), HashMap::new());
}
