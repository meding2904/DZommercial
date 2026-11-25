//! Extended differential coverage for the desktop API.
//!
//! The first differential test compared URL/origin policy, request headers,
//! cookies, error envelopes and one scripted session. This suite drives the
//! remaining surface through the Go oracle: settings files, update policy,
//! health/origin resolution, scripted requests and CSRF retries, images, file
//! and fallback secret stores, and the client factory.
//!
//! The OS keyring is deliberately excluded. Both sides use the same in-memory
//! and file stores; `default_secret_store_kind` compares which backend the
//! default selector would choose without touching a live Secret Service.

#[allow(dead_code)]
mod common;
#[allow(dead_code)]
mod oracle;
#[allow(dead_code)]
mod support;

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use azahar_desktop_api::{
    Client, ClientError, ClientOptions, Environment, FallbackSecretStore, FileSecretStore,
    MemorySecretStore, SecretError, SecretStore, Settings, TimeoutSecretStore, Transport,
    TransportError, TransportRequest, TransportResponse, WindowBounds, consent_host_for_update,
    env_enabled, settings_consent_allowed, write_private_file,
};
use oracle::Checker;
use serde_json::{Value, json};

const MIN_COMPARISONS: usize = 1_500;
const API_URL: &str = "http://127.0.0.1:8080";

fn mode(path: &Path) -> i64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|metadata| (metadata.permissions().mode() & 0o777) as i64)
            .unwrap_or(0)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        0
    }
}

fn file_entry_json(path: &Path, is_dir: bool) -> Value {
    if is_dir {
        return json!({"mode": mode(path), "bytes": "", "dir": true});
    }
    let bytes = fs::read(path).unwrap_or_default();
    json!({"mode": mode(path), "bytes": String::from_utf8_lossy(&bytes)})
}

fn snapshot_path(path: &Path) -> Value {
    match fs::metadata(path) {
        Err(_) => json!({"exists": false, "dir": false, "mode": 0, "bytes": ""}),
        Ok(metadata) if metadata.is_dir() => {
            json!({"exists": true, "dir": true, "mode": mode(path), "bytes": ""})
        }
        Ok(_) => {
            let bytes = fs::read(path).unwrap_or_default();
            json!({
                "exists": true,
                "dir": false,
                "mode": mode(path),
                "bytes": String::from_utf8_lossy(&bytes),
            })
        }
    }
}

fn snapshot_profile(profile: &Path) -> Value {
    let mut settings = Value::Null;
    let mut sessions: Vec<Value> = Vec::new();
    let mut leftovers: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir(profile) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            if name == "settings.json" {
                settings = file_entry_json(&path, is_dir);
                continue;
            }
            if !is_dir && name.starts_with("session-") && name.ends_with(".json") {
                let bytes = fs::read(&path).unwrap_or_default();
                sessions.push(json!({
                    "mode": mode(&path),
                    "bytes": String::from_utf8_lossy(&bytes),
                }));
                continue;
            }
            if name.starts_with(".settings.json.") {
                leftovers.push("temp".to_owned());
            } else {
                leftovers.push("other".to_owned());
            }
        }
    }
    sessions.sort_by(|left, right| {
        let left = (
            left["bytes"].as_str().unwrap_or_default(),
            left["mode"].as_i64(),
        );
        let right = (
            right["bytes"].as_str().unwrap_or_default(),
            right["mode"].as_i64(),
        );
        left.cmp(&right)
    });
    leftovers.sort();
    json!({
        "profile_mode": mode(profile),
        "settings": settings,
        "sessions": sessions,
        "leftovers": leftovers,
    })
}

fn write_leftovers(path: &Path) -> Vec<String> {
    let Some(parent) = path.parent() else {
        return Vec::new();
    };
    let Some(base) = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
    else {
        return Vec::new();
    };
    let prefix = format!(".{base}.");
    let mut leftovers = Vec::new();
    if let Ok(entries) = fs::read_dir(parent) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with(&prefix) {
                leftovers.push("temp".to_owned());
            }
        }
    }
    leftovers.sort();
    leftovers
}

fn legacy_dir(kind: &str, root: &Path) -> Option<PathBuf> {
    let home = root.join("home");
    match kind {
        "" | "xdg" => Some(root.join("config").join("Azahar Admin")),
        "home" => Some(home.join(".config").join("Azahar Admin")),
        "xdg-relative" | "none" => None,
        _ => Some(root.join("config").join("Azahar Admin")),
    }
}

fn legacy_environment(kind: &str, root: &Path) -> Environment {
    let home = root.join("home");
    match kind {
        "" | "xdg" => Environment::default()
            .config_home(root.join("config"))
            .home(home),
        "home" => Environment::default().home(home),
        "xdg-relative" => Environment::default()
            .config_home("relative/config")
            .home(home),
        "none" => Environment::default(),
        _ => Environment::default()
            .config_home(root.join("config"))
            .home(home),
    }
}

fn write_legacy(kind: &str, root: &Path, raw: &str) {
    if let Some(directory) = legacy_dir(kind, root) {
        let _ = fs::create_dir_all(&directory);
        let _ = fs::write(directory.join("settings.json"), raw);
    }
}

fn oracle_environment(values: &Value) -> Value {
    let Some(values) = values.as_object() else {
        return Value::Null;
    };
    let mut environment = serde_json::Map::new();
    for (source, target) in [
        ("api_url", "AZAHAR_API_URL"),
        ("origin", "AZAHAR_ORIGIN"),
        ("session_backend", "AZAHAR_SESSION_BACKEND"),
        ("disable_keyring", "AZAHAR_DISABLE_KEYRING"),
    ] {
        if let Some(value) = values.get(source) {
            environment.insert(target.to_owned(), value.clone());
        }
    }
    Value::Object(environment)
}

fn environment_with(mut environment: Environment, values: &Value) -> Environment {
    if values.is_object() {
        if let Some(value) = values.get("api_url") {
            environment.api_url = value.as_str().map(str::to_owned);
        }
        if let Some(value) = values.get("origin") {
            environment.origin = value.as_str().map(str::to_owned);
        }
        if let Some(value) = values.get("session_backend") {
            environment.session_backend = value.as_str().map(str::to_owned);
        }
        if let Some(value) = values.get("disable_keyring") {
            environment.disable_keyring = value.as_str().map(str::to_owned);
        }
    }
    environment
}

fn settings(api_url: &str, origin: &str, host: &str, bound: bool) -> Settings {
    Settings {
        api_url: api_url.to_owned(),
        origin: origin.to_owned(),
        network_allowed: bound,
        network_allowed_host: host.to_owned(),
        window: WindowBounds::default(),
    }
}

fn expires_from_unix(seconds: i64) -> Option<SystemTime> {
    if seconds == 0 {
        return None;
    }
    if seconds < 0 {
        UNIX_EPOCH.checked_sub(Duration::from_secs(seconds.unsigned_abs()))
    } else {
        Some(UNIX_EPOCH + Duration::from_secs(seconds as u64))
    }
}

// --- Pure helpers -------------------------------------------------------

fn setting_documents() -> Vec<String> {
    let mut documents = vec![
        String::new(),
        "null".to_owned(),
        "[]".to_owned(),
        "not json".to_owned(),
        "{}".to_owned(),
        "{\"apiUrl\":".to_owned(),
        "{\"apiUrl\":null}".to_owned(),
        "{\"apiUrl\":\"https://a.example\",\"origin\":\"http://b.example\",\"networkAllowed\":true,\"networkAllowedHost\":\"a.example\"}".to_owned(),
        "{\"ApiUrl\":\"https://a.example\"}".to_owned(),
        "{\"APIURL\":\"https://a.example\"}".to_owned(),
        "{\"origin\":null,\"networkAllowed\":null,\"networkAllowedHost\":null,\"window\":null}".to_owned(),
        "{\"window\":{}}".to_owned(),
        "{\"window\":[]}".to_owned(),
        "{\"window\":\"broken\"}".to_owned(),
        "{\"window\":true}".to_owned(),
        "{\"window\":{\"width\":10,\"height\":\"bad\",\"x\":5}}".to_owned(),
        "{\"window\":{\"width\":\"bad\",\"height\":20}}".to_owned(),
        "{\"window\":{\"width\":10.5,\"height\":20}}".to_owned(),
        "{\"window\":{\"width\":1e2,\"height\":20}}".to_owned(),
        "{\"window\":{\"width\":-0,\"height\":-0,\"x\":-0,\"y\":-0}}".to_owned(),
        "{\"window\":{\"width\":null,\"height\":null,\"x\":null,\"y\":null}}".to_owned(),
        "{\"window\":{\"width\":-1,\"height\":0,\"x\":-30,\"y\":40}}".to_owned(),
        "{\"window\":{\"width\":9223372036854775807,\"height\":-9223372036854775808}}".to_owned(),
        "{\"window\":{\"width\":9223372036854775808}}".to_owned(),
        "{\"window\":{\"unknown\":\"x\",\"width\":7}}".to_owned(),
        "{\"window\":{\"width\":7,\"width\":8}}".to_owned(),
        "{\"window\":{\"WIDTH\":7}}".to_owned(),
        "{\"window\":{\"Height\":8}}".to_owned(),
        "{\"window\":{\"X\":9,\"Y\":10}}".to_owned(),
        // Duplicate keys that differ only by case are excluded: Go applies
        // them in document order, while serde_json's map has no order left.
        "{\"window\":{\"width\":true,\"height\":2}}".to_owned(),
        "{\"window\":{\"width\":{},\"height\":2}}".to_owned(),
        "{\"window\":{\"width\":[1],\"height\":2}}".to_owned(),
        "{\"window\":{\"width\":\"7\",\"height\":8,\"x\":\"9\"}}".to_owned(),
        "{\"apiUrl\":123,\"origin\":\"http://ok.example\",\"window\":{\"width\":\"bad\"}}".to_owned(),
        "{\"apiUrl\":\"\",\"origin\":\"\"}".to_owned(),
        "{\"apiUrl\":\"  https://a.example  \",\"origin\":\"  http://b.example  \"}".to_owned(),
        "{\"apiUrl\":\"https://a.example\",\"origin\":\"ftp://bad.example\"}".to_owned(),
        "{\"apiUrl\":\"https://a.example/\",\"origin\":\"https://a.example/\"}".to_owned(),
        "{\"apiUrl\":\"https://A.EXAMPLE:443\",\"origin\":\"HTTPS://B.EXAMPLE:443\"}".to_owned(),
        "{\"apiUrl\":\"https://a.example\",\"networkAllowedHost\":\"  A.EXAMPLE.  \"}".to_owned(),
        "{\"apiUrl\":\"http://127.0.0.1:8080\",\"networkAllowedHost\":\"a.example\"}".to_owned(),
        "{\"apiUrl\":\"https://a.example\",\"networkAllowedHost\":\"other.example\"}".to_owned(),
        "{\"apiUrl\":\"https://a.example\",\"networkAllowed\":true}".to_owned(),
        "{\"error\":{}}".to_owned(),
        "{\"window\":{\"width\":1},\"window\":{\"height\":2}}".to_owned(),
        "{\"apiUrl\":\"https://a.example\",\"apiUrl\":\"https://b.example\"}".to_owned(),
    ];
    for key in [
        "apiUrl",
        "origin",
        "networkAllowed",
        "networkAllowedHost",
        "window",
    ] {
        for value in [
            "\"https://a.example\"",
            "\"\"",
            "\"  \"",
            "\"a\\u0000b\"",
            "\"caf\\u00e9\"",
            "123",
            "-123",
            "1.5",
            "1e3",
            "true",
            "false",
            "null",
            "[]",
            "{}",
            "9223372036854775808",
        ] {
            documents.push(format!("{{\"{key}\":{value}}}"));
        }
    }
    documents
}

fn pure_helpers(checker: &mut Checker) {
    for document in setting_documents() {
        for key in [
            "apiUrl",
            "origin",
            "networkAllowed",
            "networkAllowedHost",
            "window",
        ] {
            for kind in ["string", "bool", "window"] {
                let (value, applied) =
                    azahar_desktop_api::harness::decode_setting(document.as_bytes(), key, kind);
                checker.check_ok(
                    "decode_setting",
                    json!({"op": "decode_setting", "doc": document, "key": key, "kind": kind}),
                    json!({"value": value, "applied": applied}),
                );
            }
        }
    }

    let previous = [
        Settings::default(),
        settings("https://saved.example", "", "saved.example", true),
        settings("https://saved.example", "", "other.example", false),
        settings("https://saved.example", "", "SAVED.EXAMPLE.", false),
        settings("http://127.0.0.1:8080", "", "", true),
        settings(
            "https://a.example",
            "http://admin.example",
            "a.example",
            false,
        ),
    ];
    let urls = [
        "http://127.0.0.1:8080",
        "http://localhost:3000",
        "http://[::1]:8080",
        "https://a.example.com",
        "http://a.example.com",
        "https://saved.example",
        "https://SAVED.EXAMPLE.",
        "https://saved.example:443",
        "https://b.example.com",
        "http://192.168.1.10:8080",
        "https://192.168.1.10",
        "https://api.local",
        "https://api.localhost",
        "bad",
        "",
        "ftp://x",
        "https://user:pass@x",
        "https://x:99999",
        "https://a.example.com/",
        "https://a.example.com/path",
    ];
    for previous in &previous {
        for url in urls {
            for allow in [false, true] {
                let rust = consent_host_for_update(previous, url, allow).map(Value::String);
                checker.check(
                    "consent_host_for_update",
                    json!({
                        "op": "consent_host_for_update",
                        "previous": previous,
                        "raw": url,
                        "allow_network": allow,
                    }),
                    rust,
                );
            }
            for host in [
                "",
                "saved.example",
                "SAVED.EXAMPLE.",
                "a.example.com",
                "other.example",
            ] {
                let candidate = Settings {
                    api_url: url.to_owned(),
                    network_allowed_host: host.to_owned(),
                    ..Settings::default()
                };
                checker.check_ok(
                    "settings_consent_allowed",
                    json!({"op": "settings_consent_allowed", "settings": candidate}),
                    Value::Bool(settings_consent_allowed(&candidate)),
                );
            }
        }
    }

    let mut error_documents = vec![
        String::new(),
        "not json".to_owned(),
        "null".to_owned(),
        "[]".to_owned(),
        "{}".to_owned(),
        "{\"error\":null}".to_owned(),
        "{\"error\":{}}".to_owned(),
        "{\"error\":\"string\"}".to_owned(),
        "{\"error\":[]}".to_owned(),
        "{\"error\":{},\"extra\":true}".to_owned(),
        "{\"error\":{}} trailing".to_owned(),
        "{\"Error\":{\"Code\":\"x\"}}".to_owned(),
        "{\"ERROR\":{\"CODE\":\"x\",\"MESSAGE\":\"y\",\"FIELDS\":{\"a\":\"b\"}}}".to_owned(),
        "{\"error\":{\"Code\":\"x\"}}".to_owned(),
        "{\"error\":{\"CODE\":\"x\"}}".to_owned(),
        "{\"error\":{\"code\":\"  x  \",\"message\":\"  y  \"}}".to_owned(),
        "{\"error\":{\"code\":\"   \",\"message\":\"   \"}}".to_owned(),
        "{\"error\":{\"code\":null,\"message\":null,\"fields\":null}}".to_owned(),
        "{\"error\":{\"code\":42,\"message\":\"bad\"}}".to_owned(),
        "{\"error\":{\"code\":\"code\",\"message\":42}}".to_owned(),
        "{\"error\":{\"code\":\"code\",\"message\":\"message\",\"fields\":{\"a\":1}}}".to_owned(),
        "{\"error\":{\"code\":\"code\",\"message\":\"message\",\"fields\":{\"a\":null}}}"
            .to_owned(),
        "{\"error\":{\"code\":\"code\",\"message\":\"message\",\"fields\":{\"a\":\"\"}}}"
            .to_owned(),
        "{\"error\":{\"code\":\"code\",\"message\":\"message\",\"fields\":{}}}".to_owned(),
        "{\"error\":{\"code\":\"code\",\"code\":\"later\"}}".to_owned(),
        "{\"error\":{\"code\":\"code\",\"Code\":\"later\"}}".to_owned(),
        "{\"error\":{\"Code\":\"later\",\"code\":\"code\"}}".to_owned(),
        "{\"error\":{\"message\":\"first\"},\"error\":{\"message\":\"second\"}}".to_owned(),
        "{\"error\":{\"message\":\"first\"},\"Error\":{\"message\":\"second\"}}".to_owned(),
        "{\"error\":{\"code\":\"caf\\u00e9\",\"message\":\"\\u00e9\"}}".to_owned(),
    ];
    for code in ["x", "", "  ", "csrf_failed", "validation_error"] {
        for message in ["m", "", "  ", "Check the fields."] {
            error_documents.push(format!(
                "{{\"error\":{{\"code\":\"{code}\",\"message\":\"{message}\",\"fields\":{{}}}}}}"
            ));
            error_documents.push(format!(
                "{{\"error\":{{\"Code\":\"{code}\",\"Message\":\"{message}\",\"Fields\":{{}}}}}}"
            ));
        }
    }
    for (status, document) in error_documents.iter().enumerate() {
        let status = status as i32;
        let rust = azahar_desktop_api::harness::error_from_result(
            &azahar_desktop_api::harness::HttpResult {
                status,
                body: document.clone().into_bytes(),
                content_type: String::new(),
            },
        );
        checker.check(
            "error_from_result extended",
            json!({"op": "error_from_result", "status": status, "body_string": document}),
            Ok(serde_json::to_value(rust).expect("error json")),
        );
        checker.check_ok(
            "error_code extended",
            json!({"op": "error_code", "body_string": document}),
            Value::String(azahar_desktop_api::harness::error_code(document.as_bytes())),
        );
        let decoded = azahar_desktop_api::harness::decode_json(document.as_bytes());
        checker.check_ok(
            "decode_json extended",
            json!({"op": "decode_json", "body_string": document}),
            json!({"present": decoded.is_some(), "value": decoded.unwrap_or(Value::Null)}),
        );
    }

    for value in [
        None,
        Some(""),
        Some(" "),
        Some("0"),
        Some("false"),
        Some("FALSE"),
        Some("no"),
        Some("NO"),
        Some("off"),
        Some(" Off "),
        Some("1"),
        Some("true"),
        Some("yes"),
        Some("random"),
    ] {
        checker.check_ok(
            "env_enabled",
            json!({"op": "env_enabled", "value": value, "value_present": value.is_some()}),
            Value::Bool(env_enabled(value)),
        );
    }
}

// --- Settings files -----------------------------------------------------

fn run_settings_load(input: &Value) -> Value {
    let root = tempfile::tempdir().expect("settings temp");
    let profile = root.path().join("profile");
    if let Some(raw) = input.get("settings_raw").and_then(Value::as_str) {
        fs::create_dir_all(&profile).expect("profile");
        fs::write(profile.join("settings.json"), raw).expect("settings file");
    }
    let mut environment = Environment::default();
    if let Some(raw) = input.get("legacy_raw").and_then(Value::as_str) {
        let kind = input
            .get("legacy_kind")
            .and_then(Value::as_str)
            .unwrap_or("xdg");
        environment = legacy_environment(kind, root.path());
        write_legacy(kind, root.path(), raw);
    }
    environment = environment_with(environment, input.get("env").unwrap_or(&Value::Null));
    let client = Client::with_options(
        ClientOptions::new("2.0.0-test", &profile)
            .environment(environment)
            .secret_store(Arc::new(MemorySecretStore::new())),
    )
    .expect("client");
    json!({
        "settings": client.settings_snapshot(),
        "info": client.info(),
        "window": client.window_bounds(),
        "profile": snapshot_profile(&profile),
    })
}

fn settings_files(checker: &mut Checker) {
    let cases = [
        json!({}),
        json!({"settings_raw": "{\"apiUrl\":\"https://saved.example\",\"origin\":\"http://b.example\"}"}),
        json!({"settings_raw": "{\"apiUrl\":123,\"origin\":\"http://ok.example\",\"window\":{\"width\":\"bad\"}}"}),
        json!({"settings_raw": "{\"window\":\"broken\",\"apiUrl\":\"https://saved.example\",\"networkAllowedHost\":\"saved.example\"}"}),
        json!({"settings_raw": "{\"apiUrl\":"}),
        json!({"settings_raw": "null"}),
        json!({"settings_raw": "[]"}),
        json!({"settings_raw": "{\"window\":{\"WIDTH\":7,\"HEIGHT\":8}}"}),
        json!({"settings_raw": "{\"apiUrl\":\"  https://A.EXAMPLE  \",\"origin\":\"  HTTPS://B.EXAMPLE  \"}"}),
        json!({"settings_raw": "{\"apiUrl\":\"https://saved.example\",\"networkAllowedHost\":\"SAVED.EXAMPLE.\"}"}),
        json!({"settings_raw": "{\"apiUrl\":\"https://saved.example\",\"networkAllowedHost\":\"other.example\"}"}),
        json!({"settings_raw": "{\"apiUrl\":\"http://127.0.0.1:8080\",\"networkAllowedHost\":\"saved.example\"}"}),
        json!({"settings_raw": "{\"apiUrl\":\"https://saved.example\",\"window\":{\"width\":-1,\"height\":0,\"x\":-30,\"y\":40}}"}),
        json!({
            "settings_raw": "{\"apiUrl\":\"https://saved.example\"}",
            "env": {"api_url": "http://127.0.0.1:9090/", "origin": "https://override.example/"},
        }),
        json!({
            "settings_raw": "{\"apiUrl\":\"https://saved.example\"}",
            "env": {"api_url": "http://public.example", "origin": "ftp://bad.example"},
        }),
        json!({
            "settings_raw": "{\"apiUrl\":\"https://saved.example\"}",
            "env": {"api_url": "", "origin": ""},
        }),
        json!({"legacy_raw": "{\"apiUrl\":\"https://legacy.example\",\"origin\":\"https://legacy.example\",\"window\":{\"width\":1200,\"height\":800,\"x\":40,\"y\":50}}", "legacy_kind": "xdg"}),
        json!({"legacy_raw": "{\"apiUrl\":\"https://legacy.example\"}", "legacy_kind": "home"}),
        json!({"legacy_raw": "{\"apiUrl\":\"https://legacy.example\"}", "legacy_kind": "xdg-relative"}),
        json!({"legacy_raw": "{\"apiUrl\":\"https://legacy.example\"}", "legacy_kind": "none"}),
        json!({"legacy_raw": "{\"apiUrl\":\"\",\"origin\":\"https://legacy.example\"}", "legacy_kind": "xdg"}),
        json!({"legacy_raw": "{\"APIURL\":\"https://legacy.example\",\"ORIGIN\":\"http://o.example\"}", "legacy_kind": "xdg"}),
        json!({"legacy_raw": "{\"ApiUrl\":\"https://legacy.example\"}", "legacy_kind": "xdg"}),
        json!({"legacy_raw": "{\"apiUrl\":123}", "legacy_kind": "xdg"}),
        json!({"legacy_raw": "not json", "legacy_kind": "xdg"}),
        json!({"legacy_raw": "null", "legacy_kind": "xdg"}),
        json!({"legacy_raw": "{\"apiUrl\":\"https://legacy.example\",\"window\":{\"WIDTH\":10,\"Height\":20,\"x\":\"bad\"}}", "legacy_kind": "xdg"}),
        json!({"legacy_raw": "{\"apiUrl\":\"https://legacy.example\",\"window\":null}", "legacy_kind": "xdg"}),
        json!({"legacy_raw": "{\"apiUrl\":\"https://legacy.example\",\"window\":[]}", "legacy_kind": "xdg"}),
        json!({
            "settings_raw": "{\"apiUrl\":\"https://saved.example\"}",
            "legacy_raw": "{\"apiUrl\":\"https://legacy.example\"}",
            "legacy_kind": "xdg",
        }),
    ];
    for input in cases {
        checker.check_ok(
            "settings_load",
            json!({"op": "settings_load", "settings_raw": input.get("settings_raw").cloned().unwrap_or(Value::Null), "legacy_raw": input.get("legacy_raw").cloned().unwrap_or(Value::Null), "legacy_kind": input.get("legacy_kind").cloned().unwrap_or(Value::Null), "env": oracle_environment(input.get("env").unwrap_or(&Value::Null))}),
            run_settings_load(&input),
        );
    }

    let mut documents = setting_documents();
    documents.truncate(30);
    for document in documents {
        for env in [Value::Null, json!({"api_url": "http://127.0.0.1:9090"})] {
            let input = json!({"settings_raw": document, "env": env});
            checker.check_ok(
                "settings_load generated",
                json!({"op": "settings_load", "settings_raw": document, "env": oracle_environment(&env)}),
                run_settings_load(&input),
            );
        }
    }

    for settings in settings_cases() {
        let root = tempfile::tempdir().expect("save temp");
        let path = root.path().join("settings.json");
        let failed = azahar_desktop_api::harness::save_settings(&path, &settings).is_err();
        let rust = json!({
            "failed": failed,
            "path": snapshot_path(&path),
            "profile": snapshot_profile(root.path()),
        });
        checker.check_ok(
            "save_settings",
            json!({"op": "save_settings", "settings": settings}),
            rust,
        );
    }
}

fn settings_cases() -> Vec<Settings> {
    vec![
        Settings::default(),
        Settings {
            api_url: API_URL.to_owned(),
            origin: String::new(),
            network_allowed: false,
            network_allowed_host: String::new(),
            window: WindowBounds {
                width: 1,
                height: 2,
                x: 3,
                y: 4,
            },
        },
        Settings {
            api_url: "https://saved.example:8443".to_owned(),
            origin: "http://admin.example".to_owned(),
            network_allowed: true,
            network_allowed_host: "saved.example".to_owned(),
            window: WindowBounds {
                width: -1,
                height: 0,
                x: -30,
                y: 40,
            },
        },
        Settings {
            api_url: "https://caf\u{e9}.example".to_owned(),
            origin: "https://caf\u{e9}.example".to_owned(),
            network_allowed: false,
            network_allowed_host: "caf\u{e9}.example".to_owned(),
            window: WindowBounds {
                width: i64::MAX,
                height: i64::MIN,
                x: 0,
                y: 0,
            },
        },
        Settings {
            api_url: String::new(),
            origin: String::new(),
            network_allowed: false,
            network_allowed_host: String::new(),
            window: WindowBounds::default(),
        },
    ]
}

fn run_write_private_file(input: &Value) -> Value {
    let root = tempfile::tempdir().expect("private temp");
    let path = root.path().join(
        input
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    if input
        .get("block_parent")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let _ = fs::create_dir_all(path.parent().unwrap_or(root.path()));
        let _ = fs::write(path.parent().unwrap_or(root.path()), b"blocked");
    }
    if let Some(precreate) = input.get("precreate").and_then(Value::as_str) {
        let _ = fs::create_dir_all(path.parent().unwrap_or(root.path()));
        let _ = fs::write(&path, precreate);
    }
    let failed =
        write_private_file(&path, input["data"].as_str().unwrap_or_default().as_bytes()).is_err();
    json!({
        "failed": failed,
        "path": snapshot_path(&path),
        "parent": snapshot_path(path.parent().unwrap_or(root.path())),
        "leftovers": write_leftovers(&path),
    })
}

fn write_private_files(checker: &mut Checker) {
    let mut cases = Vec::new();
    for path in [
        "settings.json",
        "a/b/settings.json",
        "a/b/c.json",
        "deep/nested/dir/file.json",
        ".hidden",
    ] {
        for data in [
            "",
            "hello",
            "{\"apiUrl\":\"http://127.0.0.1:8080\"}",
            "caf\u{e9} \u{1f600}",
            &"x".repeat(4096),
        ] {
            cases.push(json!({"path": path, "data": data}));
        }
    }
    cases.push(json!({"path": "existing.json", "data": "new", "precreate": "old"}));
    cases.push(json!({"path": "blocked/child.json", "data": "new", "block_parent": true}));
    cases.push(json!({"path": "settings.json", "data": "value", "precreate": "old"}));
    for input in cases {
        checker.check_ok(
            "write_private_file",
            json!({"op": "write_private_file", "path": input["path"], "data": input["data"], "precreate": input.get("precreate").cloned().unwrap_or(Value::Null), "block_parent": input.get("block_parent").cloned().unwrap_or(Value::Bool(false))}),
            run_write_private_file(&input),
        );
    }

    let legacy_cases = [
        (
            "xdg",
            "{\"apiUrl\":\"https://legacy.example\",\"origin\":\"https://legacy.example\",\"window\":{\"width\":1200,\"height\":800,\"x\":40,\"y\":50}}",
        ),
        ("xdg", "{\"apiUrl\":\"https://legacy.example\"}"),
        ("xdg", "{\"apiUrl\":\"\"}"),
        ("xdg", "{\"origin\":\"https://legacy.example\"}"),
        ("xdg", "{\"apiUrl\":123}"),
        ("xdg", "not json"),
        ("xdg", "null"),
        (
            "xdg",
            "{\"apiUrl\":\"https://legacy.example\",\"window\":{\"WIDTH\":10,\"Height\":20}}",
        ),
        (
            "xdg",
            "{\"APIURL\":\"https://legacy.example\",\"ORIGIN\":\"http://o.example\",\"WINDOW\":{\"Width\":10,\"HEIGHT\":20}}",
        ),
        (
            "xdg",
            "{\"apiUrl\":\"https://legacy.example\",\"origin\":123}",
        ),
        (
            "xdg",
            "{\"apiUrl\":\"https://legacy.example\",\"origin\":null,\"window\":{\"width\":null,\"height\":3}}",
        ),
        ("xdg", "{\"apiUrl\":null,\"origin\":\"http://o.example\"}"),
        (
            "xdg",
            "{\"apiUrl\":\"https://legacy.example\",\"window\":{\"width\":\"bad\"}}",
        ),
        (
            "xdg",
            "{\"apiUrl\":\"https://legacy.example\",\"window\":[]}",
        ),
        ("home", "{\"apiUrl\":\"https://legacy.example\"}"),
        ("xdg-relative", "{\"apiUrl\":\"https://legacy.example\"}"),
        ("none", "{\"apiUrl\":\"https://legacy.example\"}"),
    ];
    for (kind, raw) in legacy_cases {
        let root = tempfile::tempdir().expect("legacy temp");
        let environment = legacy_environment(kind, root.path());
        write_legacy(kind, root.path(), raw);
        let loaded = azahar_desktop_api::harness::load_legacy_settings(&environment);
        let rust = json!({
            "present": loaded.is_some(),
            "settings": loaded.unwrap_or_default(),
        });
        checker.check_ok(
            "load_legacy",
            json!({"op": "load_legacy", "legacy_raw": raw, "legacy_kind": kind}),
            rust,
        );
    }
}

// --- Secret stores ------------------------------------------------------

struct ScriptStore {
    state: Mutex<(Option<String>, bool, bool)>,
}

impl ScriptStore {
    fn new(value: Option<String>, fail: bool, delete_fail: bool) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new((value, fail, delete_fail)),
        })
    }

    fn stored(&self) -> Option<String> {
        self.state.lock().unwrap().0.clone()
    }
}

impl SecretStore for ScriptStore {
    fn load(&self) -> Result<Option<String>, SecretError> {
        let state = self.state.lock().unwrap();
        if state.1 {
            return Err(SecretError::new("keyring unavailable"));
        }
        Ok(state.0.clone())
    }

    fn save(&self, value: &str) -> Result<(), SecretError> {
        let mut state = self.state.lock().unwrap();
        if state.1 {
            return Err(SecretError::new("keyring unavailable"));
        }
        state.0 = Some(value.to_owned());
        Ok(())
    }

    fn delete(&self) -> Result<(), SecretError> {
        let mut state = self.state.lock().unwrap();
        if state.1 {
            return Err(SecretError::new("keyring unavailable"));
        }
        if state.2 {
            return Err(SecretError::new("delete failed"));
        }
        state.0 = None;
        Ok(())
    }
}

struct SlowStore {
    delay: Duration,
}

impl SecretStore for SlowStore {
    fn load(&self) -> Result<Option<String>, SecretError> {
        std::thread::sleep(self.delay);
        Ok(None)
    }

    fn save(&self, _value: &str) -> Result<(), SecretError> {
        std::thread::sleep(self.delay);
        Ok(())
    }

    fn delete(&self) -> Result<(), SecretError> {
        std::thread::sleep(self.delay);
        Ok(())
    }
}

struct BuiltStore {
    store: Option<Arc<dyn SecretStore>>,
    memory: Option<Arc<ScriptStore>>,
    path: Option<PathBuf>,
}

fn build_store(spec: &Value, root: &Path) -> BuiltStore {
    match spec.get("kind").and_then(Value::as_str).unwrap_or_default() {
        "file" => {
            let relative = spec
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("session.json");
            let path = root.join(relative);
            if let Some(prefill) = spec.get("prefill").and_then(Value::as_str) {
                let _ = fs::create_dir_all(path.parent().unwrap_or(root));
                let _ = fs::write(&path, prefill);
            }
            let store = Arc::new(FileSecretStore::new(&path));
            BuiltStore {
                store: Some(store),
                memory: None,
                path: Some(path),
            }
        }
        "memory" => {
            let value = spec.get("value").and_then(Value::as_str).map(str::to_owned);
            let fail = spec.get("fail").and_then(Value::as_bool).unwrap_or(false);
            let delete_fail = spec
                .get("delete_fail")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let memory = ScriptStore::new(value, fail, delete_fail);
            BuiltStore {
                store: Some(memory.clone()),
                memory: Some(memory),
                path: None,
            }
        }
        "slow" => {
            let delay =
                Duration::from_millis(spec.get("sleep_ms").and_then(Value::as_u64).unwrap_or(50));
            BuiltStore {
                store: Some(Arc::new(SlowStore { delay })),
                memory: None,
                path: None,
            }
        }
        "fallback" => {
            let primary = build_store(spec.get("primary").unwrap_or(&Value::Null), root);
            let fallback = build_store(spec.get("fallback").unwrap_or(&Value::Null), root);
            let (Some(primary_store), Some(fallback_store)) = (primary.store, fallback.store)
            else {
                return BuiltStore {
                    store: None,
                    memory: primary.memory,
                    path: fallback.path,
                };
            };
            let store = Arc::new(FallbackSecretStore::new(
                Some(primary_store),
                fallback_store,
            ));
            BuiltStore {
                store: Some(store),
                memory: primary.memory,
                path: fallback.path,
            }
        }
        "timeout" => {
            let inner = build_store(spec.get("inner").unwrap_or(&Value::Null), root);
            let timeout =
                Duration::from_millis(spec.get("timeout_ms").and_then(Value::as_u64).unwrap_or(20));
            let Some(inner_store) = inner.store else {
                return BuiltStore {
                    store: None,
                    memory: inner.memory,
                    path: inner.path,
                };
            };
            let store = Arc::new(TimeoutSecretStore::new(inner_store, timeout));
            BuiltStore {
                store: Some(store),
                memory: inner.memory,
                path: inner.path,
            }
        }
        _ => BuiltStore {
            store: None,
            memory: None,
            path: None,
        },
    }
}

fn secret_snapshot(store: &dyn SecretStore) -> Value {
    match store.load() {
        Ok(Some(value)) => json!({"value": value, "present": true}),
        Ok(None) => json!({"value": "", "present": false}),
        Err(error) => json!({"value": "", "present": false, "error": error.message}),
    }
}

fn run_secret_ops(input: &Value) -> Value {
    let root = tempfile::tempdir().expect("secret temp");
    let BuiltStore {
        store: maybe_store,
        memory,
        path,
    } = build_store(input.get("store").unwrap_or(&Value::Null), root.path());
    let Some(store) = maybe_store else {
        return json!({"error": "unknown store"});
    };
    let mut results = Vec::new();
    for op in input
        .get("ops")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let kind = op.get("kind").and_then(Value::as_str).unwrap_or_default();
        let value = op.get("value").and_then(Value::as_str).unwrap_or_default();
        match kind {
            "load" => match store.load() {
                Ok(value) => {
                    let present = value.is_some();
                    results.push(json!({
                        "ok": true,
                        "value": value.unwrap_or_default(),
                        "present": present,
                    }));
                }
                Err(error) => results.push(json!({
                    "ok": false,
                    "value": null,
                    "present": false,
                    "plain": error.message,
                })),
            },
            "save" => match store.save(value) {
                Ok(()) => results.push(json!({"ok": true, "value": null, "present": false})),
                Err(error) => results.push(json!({
                    "ok": false,
                    "value": null,
                    "present": false,
                    "plain": error.message,
                })),
            },
            "delete" => match store.delete() {
                Ok(()) => results.push(json!({"ok": true, "value": null, "present": false})),
                Err(error) => results.push(json!({
                    "ok": false,
                    "value": null,
                    "present": false,
                    "plain": error.message,
                })),
            },
            "state" => match &memory {
                Some(memory) => {
                    let stored = memory.stored();
                    results.push(json!({
                        "ok": true,
                        "value": stored.clone().unwrap_or_default(),
                        "present": stored.is_some(),
                    }));
                }
                None => results.push(json!({
                    "ok": false,
                    "value": null,
                    "present": false,
                    "plain": "no memory state",
                })),
            },
            _ => results.push(json!({
                "ok": false,
                "value": null,
                "present": false,
                "plain": "unknown op",
            })),
        }
    }
    json!({
        "ops": results,
        "path": path.as_deref().map(snapshot_path).unwrap_or(Value::Null),
    })
}

fn secret_stores(checker: &mut Checker) {
    let cases = [
        json!({"store": {"kind": "file"}, "ops": [{"kind": "load"}, {"kind": "save", "value": "session-one"}, {"kind": "load"}, {"kind": "delete"}, {"kind": "load"}]}),
        json!({"store": {"kind": "file", "prefill": "{\"azahar_session\":\"file-session\"}"}, "ops": [{"kind": "load"}, {"kind": "delete"}, {"kind": "load"}]}),
        json!({"store": {"kind": "file", "prefill": "{\"azahar_session\":\"\"}"}, "ops": [{"kind": "load"}]}),
        json!({"store": {"kind": "file", "prefill": "{}"}, "ops": [{"kind": "load"}]}),
        json!({"store": {"kind": "file", "prefill": "not json"}, "ops": [{"kind": "load"}]}),
        json!({"store": {"kind": "file", "prefill": "null"}, "ops": [{"kind": "load"}]}),
        json!({"store": {"kind": "file", "prefill": "{\"azahar_session\":42}"}, "ops": [{"kind": "load"}]}),
        json!({"store": {"kind": "file", "prefill": "{\"AZAHAR_SESSION\":\"upper\"}"}, "ops": [{"kind": "load"}]}),
        json!({"store": {"kind": "file", "prefill": "{\"session\":\"field-name\"}"}, "ops": [{"kind": "load"}]}),
        json!({"store": {"kind": "file", "prefill": "{\"Session\":\"field-case\"}"}, "ops": [{"kind": "load"}]}),
        json!({"store": {"kind": "file", "prefill": "{\"azahar_session\":\"a b\"}"}, "ops": [{"kind": "load"}]}),
        json!({"store": {"kind": "file", "prefill": "{\"azahar_session\":\"ok\",\"extra\":1}"}, "ops": [{"kind": "load"}]}),
        json!({"store": {"kind": "memory"}, "ops": [{"kind": "state"}, {"kind": "save", "value": "memory-one"}, {"kind": "load"}, {"kind": "state"}, {"kind": "delete"}, {"kind": "load"}, {"kind": "state"}]}),
        json!({"store": {"kind": "memory", "value": "prefilled"}, "ops": [{"kind": "load"}, {"kind": "state"}]}),
        json!({"store": {"kind": "memory", "fail": true}, "ops": [{"kind": "load"}, {"kind": "save", "value": "x"}, {"kind": "delete"}]}),
        json!({"store": {"kind": "memory", "delete_fail": true, "value": "kept"}, "ops": [{"kind": "delete"}, {"kind": "load"}, {"kind": "state"}]}),
        json!({"store": {"kind": "fallback", "primary": {"kind": "memory", "fail": true}, "fallback": {"kind": "file"}}, "ops": [{"kind": "load"}, {"kind": "save", "value": "fallback-one"}, {"kind": "load"}]}),
        json!({"store": {"kind": "fallback", "primary": {"kind": "memory", "value": "primary-one"}, "fallback": {"kind": "file", "prefill": "{\"azahar_session\":\"fallback-one\"}"}}, "ops": [{"kind": "load"}]}),
        json!({"store": {"kind": "fallback", "primary": {"kind": "memory"}, "fallback": {"kind": "file", "prefill": "{\"azahar_session\":\"fallback-one\"}"}}, "ops": [{"kind": "load"}, {"kind": "save", "value": "primary-two"}, {"kind": "load"}]}),
        json!({"store": {"kind": "fallback", "primary": {"kind": "memory", "delete_fail": true}, "fallback": {"kind": "file", "prefill": "{\"azahar_session\":\"fallback-one\"}"}}, "ops": [{"kind": "delete"}, {"kind": "load"}]}),
        json!({"store": {"kind": "timeout", "timeout_ms": 10, "inner": {"kind": "slow", "sleep_ms": 100}}, "ops": [{"kind": "load"}, {"kind": "save", "value": "x"}, {"kind": "delete"}]}),
        json!({"store": {"kind": "timeout", "timeout_ms": 500, "inner": {"kind": "slow", "sleep_ms": 1}}, "ops": [{"kind": "load"}, {"kind": "save", "value": "x"}, {"kind": "delete"}]}),
    ];
    for case in cases {
        checker.check_ok(
            "secret_ops",
            json!({"op": "secret_ops", "store": case["store"], "ops": case["ops"]}),
            run_secret_ops(&case),
        );
    }
}

// --- Scripted clients ---------------------------------------------------

#[derive(Default)]
struct TransportState {
    mode: String,
    responses: Vec<Value>,
    index: usize,
    requests: Vec<Value>,
}

#[derive(Clone)]
struct ScriptTransport {
    state: Arc<Mutex<TransportState>>,
}

impl ScriptTransport {
    fn new(spec: &Value) -> Self {
        Self {
            state: Arc::new(Mutex::new(TransportState {
                mode: spec
                    .get("mode")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                responses: spec
                    .get("responses")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
                index: 0,
                requests: Vec::new(),
            })),
        }
    }

    fn requests(&self) -> Vec<Value> {
        self.state.lock().unwrap().requests.clone()
    }
}

fn normalize_user_agent(value: &str) -> String {
    if let Some(start) = value.find(" (")
        && value.ends_with(')')
    {
        return format!("{} (device)", &value[..start]);
    }
    value.to_owned()
}

impl Transport for ScriptTransport {
    fn execute(&self, request: TransportRequest) -> Result<TransportResponse, TransportError> {
        let mut headers: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (name, value) in &request.headers {
            let lower = name.to_ascii_lowercase();
            let value = if lower == "user-agent" {
                normalize_user_agent(value)
            } else {
                value.clone()
            };
            headers.entry(lower).or_default().push(value);
        }
        let body =
            String::from_utf8_lossy(request.body.as_deref().unwrap_or_default()).into_owned();
        let mut state = self.state.lock().unwrap();
        state.requests.push(json!({
            "method": request.method,
            "url": request.url,
            "headers": headers,
            "body": body,
        }));
        if state.mode == "fail" {
            return Err(TransportError::Network {
                message: "scripted network failure".to_owned(),
                timeout: false,
            });
        }
        if state.mode == "timeout" {
            return Err(TransportError::Network {
                message: "scripted timeout".to_owned(),
                timeout: true,
            });
        }
        let response_present = state.index < state.responses.len();
        let spec = if response_present {
            let spec = state.responses[state.index].clone();
            state.index += 1;
            spec
        } else {
            json!({"status": 200, "body": "{}"})
        };
        match spec.get("error").and_then(Value::as_str) {
            Some("fail") => {
                return Err(TransportError::Network {
                    message: "scripted network failure".to_owned(),
                    timeout: false,
                });
            }
            Some("timeout") => {
                return Err(TransportError::Network {
                    message: "scripted timeout".to_owned(),
                    timeout: true,
                });
            }
            _ => {}
        }
        if spec
            .get("oversize")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Err(TransportError::ResponseTooLarge);
        }
        let body = if let Some(encoded) = spec.get("body_b64").and_then(Value::as_str) {
            STANDARD.decode(encoded).unwrap_or_default()
        } else if let Some(body) = spec.get("body").and_then(Value::as_str) {
            body.as_bytes().to_vec()
        } else if response_present {
            Vec::new()
        } else {
            b"{}".to_vec()
        };
        Ok(TransportResponse {
            status: spec.get("status").and_then(Value::as_i64).unwrap_or(200) as i32,
            body,
            content_type: spec
                .get("content_type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            set_cookies: spec
                .get("set_cookies")
                .and_then(Value::as_array)
                .map(|cookies| {
                    cookies
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

type FactoryMap = Arc<Mutex<HashMap<String, Arc<MemorySecretStore>>>>;

fn step_ok(value: Value) -> Value {
    json!({"ok": true, "value": value})
}

fn step_error(error: ClientError) -> Value {
    match error {
        ClientError::Api(error) => json!({"ok": false, "value": null, "error": error}),
        other => json!({"ok": false, "value": null, "plain": other.to_string()}),
    }
}

fn step_plain(message: &str) -> Value {
    json!({"ok": false, "value": null, "plain": message})
}

fn run_step(
    client: &Client,
    store: Option<&Arc<dyn SecretStore>>,
    factories: &FactoryMap,
    step: &Value,
) -> Value {
    let kind = step.get("kind").and_then(Value::as_str).unwrap_or_default();
    let current_generation = || {
        if step
            .get("use_current_generation")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            azahar_desktop_api::harness::generation(client)
        } else {
            step.get("generation").and_then(Value::as_u64).unwrap_or(0)
        }
    };
    match kind {
        "info" => step_ok(serde_json::to_value(client.info()).expect("info")),
        "health" => step_ok(serde_json::to_value(client.health()).expect("health")),
        "ensure_origin" => match azahar_desktop_api::harness::ensure_origin(client) {
            Ok(()) => step_ok(Value::Null),
            Err(error) => json!({"ok": false, "value": null, "error": error}),
        },
        "ensure_csrf" => match azahar_desktop_api::harness::ensure_csrf(client) {
            Ok(()) => step_ok(Value::Null),
            Err(error) => json!({"ok": false, "value": null, "error": error}),
        },
        "resolve_origin" => step_ok(Value::String(azahar_desktop_api::harness::resolve_origin(
            client,
            step.get("force").and_then(Value::as_bool).unwrap_or(false),
        ))),
        "window_bounds" => step_ok(serde_json::to_value(client.window_bounds()).expect("bounds")),
        "save_window_bounds" => {
            let bounds: WindowBounds =
                serde_json::from_value(step.get("bounds").cloned().unwrap_or(Value::Null))
                    .unwrap_or_default();
            client.save_window_bounds(bounds);
            step_ok(Value::Null)
        }
        "update_settings" => {
            let api_url = step
                .get("api_url")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let origin = step
                .get("origin")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let allow = step
                .get("allow_network")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            match client.update_settings(api_url, origin, allow) {
                Ok(result) => step_ok(serde_json::to_value(result).expect("settings result")),
                Err(error) => step_error(error),
            }
        }
        "settings_snapshot" => {
            step_ok(serde_json::to_value(client.settings_snapshot()).expect("settings"))
        }
        "settings_consent_allowed" => step_ok(Value::Bool(settings_consent_allowed(
            &client.settings_snapshot(),
        ))),
        "seed_session" => {
            let expires = expires_from_unix(
                step.get("expires_unix")
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
            );
            azahar_desktop_api::harness::seed_session(
                client,
                step.get("value")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                expires,
            );
            step_ok(Value::Null)
        }
        "seed_csrf" => {
            let expires = expires_from_unix(
                step.get("expires_unix")
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
            );
            azahar_desktop_api::harness::seed_csrf(
                client,
                step.get("value")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                expires,
            );
            step_ok(Value::Null)
        }
        "active_csrf" => step_ok(Value::String(azahar_desktop_api::harness::active_csrf(
            client,
        ))),
        "store_csrf" => {
            azahar_desktop_api::harness::store_csrf(
                client,
                step.get("token")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                current_generation(),
            );
            step_ok(Value::Null)
        }
        "clear_csrf" => {
            azahar_desktop_api::harness::clear_csrf(client);
            step_ok(Value::Null)
        }
        "clear_session" => match client.clear_session() {
            Ok(()) => step_ok(Value::Null),
            Err(error) => step_error(error),
        },
        "persist_session" => {
            azahar_desktop_api::harness::persist_session(
                client,
                step.get("value")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            );
            step_ok(Value::Null)
        }
        "delete_stored_session" => {
            match azahar_desktop_api::harness::delete_stored_session(client) {
                Ok(()) => step_ok(Value::Null),
                Err(error) => step_plain(&error),
            }
        }
        "load_session" => {
            azahar_desktop_api::harness::load_session(client);
            step_ok(Value::Null)
        }
        "absorb_cookies" => {
            let cookies: Vec<String> = step
                .get("set_cookies")
                .and_then(Value::as_array)
                .map(|cookies| {
                    cookies
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            azahar_desktop_api::harness::absorb_cookies(client, &cookies, current_generation());
            step_ok(Value::Null)
        }
        "session_state" => {
            let (session, csrf) = client.session_state();
            step_ok(json!({"session": session, "csrf": csrf}))
        }
        "request" => {
            let method = step
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let path = step.get("path").and_then(Value::as_str).unwrap_or_default();
            let present = step
                .get("body_present")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let body = if present {
                let value = step.get("body").cloned().unwrap_or(Value::Null);
                if value.is_null() { None } else { Some(value) }
            } else {
                None
            };
            step_ok(
                serde_json::to_value(client.request(method, path, body.as_ref()))
                    .expect("response"),
            )
        }
        "image" => step_ok(
            serde_json::to_value(
                client.image(step.get("path").and_then(Value::as_str).unwrap_or_default()),
            )
            .expect("image response"),
        ),
        "store_image" => {
            azahar_desktop_api::harness::store_image(
                client,
                step.get("key").and_then(Value::as_str).unwrap_or_default(),
                step.get("value")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                current_generation(),
            );
            step_ok(Value::Null)
        }
        "image_cache" => {
            let (entries, order, bytes) = azahar_desktop_api::harness::image_cache(client);
            step_ok(json!({"entries": entries, "order": order, "bytes": bytes}))
        }
        "clear_image_cache" => {
            azahar_desktop_api::harness::clear_image_cache(client);
            step_ok(Value::Null)
        }
        "secret_state" => match store {
            Some(store) => step_ok(secret_snapshot(store.as_ref())),
            None => step_ok(json!({"value": "", "present": false})),
        },
        "factory_state" => {
            let factories = factories.lock().unwrap();
            let mut state = serde_json::Map::new();
            for (url, memory) in factories.iter() {
                let stored = memory.stored();
                state.insert(
                    url.clone(),
                    json!({
                        "value": stored.clone().unwrap_or_default(),
                        "present": stored.is_some(),
                    }),
                );
            }
            step_ok(Value::Object(state))
        }
        other => step_plain(&format!("unknown step {other}")),
    }
}

fn run_client_script(script: &Value) -> Value {
    let root = tempfile::tempdir().expect("client temp");
    let profile = root.path().join("profile");
    fs::create_dir_all(&profile).expect("profile");
    if let Some(raw) = script.get("settings_raw").and_then(Value::as_str) {
        fs::write(profile.join("settings.json"), raw).expect("settings file");
    }
    if script
        .get("settings_dir")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let path = profile.join("settings.json");
        fs::create_dir_all(&path).expect("settings dir");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o700));
        }
    }
    if let Some(raw) = script.get("secret_raw").and_then(Value::as_str) {
        fs::write(profile.join("session.json"), raw).expect("secret file");
    }
    let mut environment = Environment::default();
    if let Some(raw) = script.get("legacy_raw").and_then(Value::as_str) {
        let kind = script
            .get("legacy_kind")
            .and_then(Value::as_str)
            .unwrap_or("xdg");
        environment = legacy_environment(kind, root.path());
        write_legacy(kind, root.path(), raw);
    }
    environment = environment_with(environment, script.get("env").unwrap_or(&Value::Null));

    let transport = Arc::new(ScriptTransport::new(
        script.get("transport").unwrap_or(&Value::Null),
    ));
    let mut options = ClientOptions::new(
        script
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("2.0.0-test"),
        &profile,
    )
    .environment(environment)
    .transport(transport.clone());

    let secrets = script
        .get("secrets")
        .and_then(Value::as_str)
        .unwrap_or("memory");
    let factories: FactoryMap = Arc::new(Mutex::new(HashMap::new()));
    let store: Option<Arc<dyn SecretStore>> = match secrets {
        "file" => Some(Arc::new(FileSecretStore::new(profile.join("session.json")))),
        "failing" => Some(Arc::new(MemorySecretStore::failing())),
        "delete-failing" => {
            let store: Arc<dyn SecretStore> = DeleteFailingStore::new();
            Some(store)
        }
        "fallback" => Some(Arc::new(FallbackSecretStore::new(
            Some(Arc::new(MemorySecretStore::failing())),
            Arc::new(FileSecretStore::new(profile.join("session.json"))),
        ))),
        "factory" => None,
        "factory-none" => None,
        _ => Some(Arc::new(MemorySecretStore::new())),
    };
    if secrets == "factory" {
        let factory_state = Arc::clone(&factories);
        options = options.secret_store_factory(move |api_url| {
            let mut stores = factory_state.lock().unwrap();
            let store = stores
                .entry(api_url.to_owned())
                .or_insert_with(|| Arc::new(MemorySecretStore::new()))
                .clone();
            Some(store as Arc<dyn SecretStore>)
        });
    } else if secrets == "factory-none" {
        options = options.secret_store_factory(|_| None);
    } else if let Some(store) = &store {
        options = options.secret_store(Arc::clone(store));
    }

    let client = match Client::with_options(options) {
        Ok(client) => client,
        Err(error) => {
            return json!({
                "steps": [],
                "requests": [],
                "state": Value::Null,
                "profile": Value::Null,
                "construction_error": error.to_string(),
            });
        }
    };
    if let Some(limit) = script.get("image_cache_limit").and_then(Value::as_i64) {
        client.set_image_cache_byte_limit(limit);
    }

    let mut results = Vec::new();
    for step in script
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        results.push(run_step(&client, store.as_ref(), &factories, &step));
    }

    let (entries, order, bytes) = azahar_desktop_api::harness::image_cache(&client);
    let factory_state = {
        let stores = factories.lock().unwrap();
        let mut state = serde_json::Map::new();
        for (url, memory) in stores.iter() {
            let stored = memory.stored();
            state.insert(
                url.clone(),
                json!({
                    "value": stored.clone().unwrap_or_default(),
                    "present": stored.is_some(),
                }),
            );
        }
        Value::Object(state)
    };
    json!({
        "steps": results,
        "requests": transport.requests(),
        "state": {
            "client": azahar_desktop_api::harness::client_state(&client),
            "image": {"entries": entries, "order": order, "bytes": bytes},
            "secret": match &store {
                Some(store) => secret_snapshot(store.as_ref()),
                None => json!({"value": "", "present": false}),
            },
            "factory": factory_state,
        },
        "profile": snapshot_profile(&profile),
    })
}

fn check_script(checker: &mut Checker, label: &str, script: Value) {
    checker.check_ok(
        label,
        json!({"op": "client_script", "script": script.clone()}),
        run_client_script(&script),
    );
}

#[derive(Debug)]
struct DeleteFailingStore {
    inner: MemorySecretStore,
}

impl DeleteFailingStore {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: MemorySecretStore::new(),
        })
    }
}

impl SecretStore for DeleteFailingStore {
    fn load(&self) -> Result<Option<String>, SecretError> {
        self.inner.load()
    }

    fn save(&self, value: &str) -> Result<(), SecretError> {
        self.inner.save(value)
    }

    fn delete(&self) -> Result<(), SecretError> {
        Err(SecretError::new("delete failed"))
    }
}

fn script(settings_raw: Value, env: Value, transport: Value, steps: Vec<Value>) -> Value {
    json!({
        "version": "2.0.0-test",
        "settings_raw": settings_raw,
        "env": env,
        "secrets": "memory",
        "transport": transport,
        "steps": steps,
    })
}

fn client_scripts(checker: &mut Checker) {
    // Info, window bounds, health and origin resolution.
    check_script(
        checker,
        "script defaults",
        script(
            Value::Null,
            Value::Null,
            Value::Null,
            vec![
                json!({"kind": "info"}),
                json!({"kind": "window_bounds"}),
                json!({"kind": "settings_snapshot"}),
                json!({"kind": "settings_consent_allowed"}),
            ],
        ),
    );
    for bounds in [
        json!({"width": 0, "height": 0, "x": 0, "y": 0}),
        json!({"width": -1, "height": 0, "x": -30, "y": 40}),
        json!({"width": 1200, "height": 800, "x": 40, "y": 30}),
        json!({"width": 1, "height": 1, "x": 0, "y": 0}),
    ] {
        check_script(
            checker,
            "script window",
            script(
                json!("{\"apiUrl\":\"https://saved.example\",\"origin\":\"http://admin.example\"}"),
                Value::Null,
                Value::Null,
                vec![
                    json!({"kind": "window_bounds"}),
                    json!({"kind": "save_window_bounds", "bounds": bounds}),
                    json!({"kind": "window_bounds"}),
                    json!({"kind": "info"}),
                ],
            ),
        );
    }

    let health_bodies = [
        r#"{"origin":"http://localhost:3000","env":"test","server_time":"2026-11-21T10:46:28Z"}"#,
        r#"{"origin":"http://reported.example","env":"prod","server_time":"now"}"#,
        r#"{"origin":123,"env":"test","server_time":"now"}"#,
        r#"{"ORIGIN":"http://upper.example","ENV":"upper","SERVER_TIME":"upper-now"}"#,
        r#"{"ServerTime":"camel"}"#,
        r#"{"server_time":42,"origin":"http://ok.example"}"#,
        r#"{}"#,
        r#"null"#,
        r#"not json"#,
        r#"{"origin":"ftp://bad.example"}"#,
        r#"{"origin":"  http://padded.example  "}"#,
    ];
    for body in health_bodies {
        check_script(
            checker,
            "script health",
            script(
                Value::Null,
                Value::Null,
                json!({"responses": [{"status": 200, "body": body, "content_type": "application/json"}]}),
                vec![
                    json!({"kind": "health"}),
                    json!({"kind": "info"}),
                    json!({"kind": "health"}),
                ],
            ),
        );
    }
    for transport in [
        json!({"mode": "fail"}),
        json!({"mode": "timeout"}),
        json!({"responses": [{"status": 503, "body": "{}"}]}),
        json!({"responses": [{"status": 302, "body": "{}"}]}),
    ] {
        check_script(
            checker,
            "script health failure",
            script(
                Value::Null,
                Value::Null,
                transport,
                vec![json!({"kind": "health"}), json!({"kind": "info"})],
            ),
        );
    }

    check_script(
        checker,
        "script ensure origin loopback",
        script(
            Value::Null,
            Value::Null,
            json!({"responses": [{"status": 200, "body": "{\"origin\":\"http://reported.example\"}"}]}),
            vec![
                json!({"kind": "ensure_origin"}),
                json!({"kind": "info"}),
                json!({"kind": "ensure_origin"}),
            ],
        ),
    );
    check_script(
        checker,
        "script ensure origin health failure",
        script(
            Value::Null,
            Value::Null,
            json!({"responses": [{"status": 503, "body": "{}"}]}),
            vec![json!({"kind": "ensure_origin"}), json!({"kind": "info"})],
        ),
    );
    check_script(
        checker,
        "script ensure origin remote",
        script(
            json!(
                "{\"apiUrl\":\"https://remote.example\",\"networkAllowedHost\":\"remote.example\"}"
            ),
            Value::Null,
            json!({"responses": [{"status": 200, "body": "{}"}]}),
            vec![json!({"kind": "ensure_origin"}), json!({"kind": "info"})],
        ),
    );
    check_script(
        checker,
        "script ensure origin configured",
        script(
            json!(
                "{\"apiUrl\":\"https://remote.example\",\"origin\":\"http://admin.example\",\"networkAllowedHost\":\"remote.example\"}"
            ),
            Value::Null,
            Value::Null,
            vec![
                json!({"kind": "ensure_origin"}),
                json!({"kind": "resolve_origin", "force": false}),
                json!({"kind": "resolve_origin", "force": true}),
            ],
        ),
    );
    check_script(
        checker,
        "script resolve origin force",
        script(
            Value::Null,
            Value::Null,
            json!({"responses": [{"status": 200, "body": "{\"origin\":\"http://reported.example\"}"}]}),
            vec![
                json!({"kind": "resolve_origin", "force": false}),
                json!({"kind": "resolve_origin", "force": true}),
                json!({"kind": "info"}),
            ],
        ),
    );
    check_script(
        checker,
        "script resolve origin remote",
        script(
            json!(
                "{\"apiUrl\":\"https://remote.example\",\"networkAllowedHost\":\"remote.example\"}"
            ),
            Value::Null,
            json!({"responses": [{"status": 200, "body": "{}"}]}),
            vec![
                json!({"kind": "resolve_origin", "force": true}),
                json!({"kind": "info"}),
            ],
        ),
    );

    // UpdateSettings policy.
    let update_cases = [
        (Value::Null, API_URL, "", false),
        (Value::Null, API_URL, "", true),
        (Value::Null, "http://127.0.0.1:9090/", "", false),
        (
            Value::Null,
            "https://a.example.com",
            "http://admin.example",
            true,
        ),
        (
            Value::Null,
            "https://a.example.com",
            "http://admin.example",
            false,
        ),
        (
            Value::Null,
            "http://a.example.com",
            "http://admin.example",
            true,
        ),
        (
            Value::Null,
            "https://a.example.com",
            "ftp://bad.example",
            true,
        ),
        (Value::Null, "ftp://a.example.com", "", true),
        (Value::Null, "https://user:pass@a.example.com", "", true),
        (Value::Null, "https://a.example.com/path", "", true),
        (
            Value::Null,
            " https://A.EXAMPLE.COM:443 ",
            "HTTPS://ADMIN.EXAMPLE:443",
            true,
        ),
        (
            json!(
                "{\"apiUrl\":\"https://saved.example\",\"networkAllowedHost\":\"saved.example\"}"
            ),
            "https://saved.example",
            "",
            false,
        ),
        (
            json!(
                "{\"apiUrl\":\"https://saved.example\",\"networkAllowedHost\":\"saved.example\"}"
            ),
            "https://saved.example",
            "",
            true,
        ),
        (
            json!(
                "{\"apiUrl\":\"https://saved.example\",\"networkAllowedHost\":\"saved.example\"}"
            ),
            "https://other.example",
            "",
            false,
        ),
        (
            json!(
                "{\"apiUrl\":\"https://saved.example\",\"networkAllowedHost\":\"saved.example\"}"
            ),
            "https://other.example",
            "",
            true,
        ),
        (
            json!(
                "{\"apiUrl\":\"https://saved.example\",\"origin\":\"http://admin.example\",\"networkAllowedHost\":\"saved.example\"}"
            ),
            "https://saved.example",
            "http://admin.example",
            true,
        ),
        (
            json!(
                "{\"apiUrl\":\"https://saved.example\",\"origin\":\"http://admin.example\",\"networkAllowedHost\":\"saved.example\"}"
            ),
            "https://saved.example",
            "http://other.example",
            true,
        ),
        (
            json!(
                "{\"apiUrl\":\"http://127.0.0.1:8080\",\"networkAllowedHost\":\"saved.example\"}"
            ),
            "https://a.example.com",
            "",
            true,
        ),
    ];
    for (settings_raw, api_url, origin, allow) in update_cases {
        check_script(
            checker,
            "script update settings",
            script(
                settings_raw,
                Value::Null,
                json!({"responses": [{"status": 200, "body": "{\"origin\":\"http://health.example\"}"}]}),
                vec![
                    json!({"kind": "seed_session", "value": "old-session"}),
                    json!({"kind": "seed_csrf", "value": "old-csrf"}),
                    json!({"kind": "update_settings", "api_url": api_url, "origin": origin, "allow_network": allow}),
                    json!({"kind": "settings_snapshot"}),
                    json!({"kind": "info"}),
                    json!({"kind": "session_state"}),
                ],
            ),
        );
    }
    check_script(
        checker,
        "script update settings unchanged",
        script(
            json!(
                "{\"apiUrl\":\"https://saved.example\",\"origin\":\"http://admin.example\",\"networkAllowedHost\":\"saved.example\"}"
            ),
            Value::Null,
            json!({"mode": "fail"}),
            vec![
                json!({"kind": "update_settings", "api_url": "https://saved.example", "origin": "http://admin.example", "allow_network": false}),
                json!({"kind": "settings_snapshot"}),
            ],
        ),
    );
    check_script(
        checker,
        "script update settings save failure",
        json!({
            "version": "2.0.0-test",
            "settings_dir": true,
            "env": Value::Null,
            "secrets": "memory",
            "transport": {"responses": [{"status": 200, "body": "{\"origin\":\"http://health.example\"}"}]},
            "steps": [
                {"kind": "seed_session", "value": "old-session"},
                {"kind": "seed_csrf", "value": "old-csrf"},
                {"kind": "update_settings", "api_url": "http://127.0.0.1:9090", "origin": "http://admin.example", "allow_network": true},
                {"kind": "settings_snapshot"},
                {"kind": "session_state"},
            ],
        }),
    );

    // Requests, CSRF retries and the cookie jar.
    let csrf_retry = json!({
        "responses": [
            {"status": 200, "body": "{\"csrf_token\":\"token-one\"}", "set_cookies": ["azahar_csrf=token-one; Path=/"]},
            {"status": 403, "body": "{\"error\":{\"code\":\"csrf_failed\",\"message\":\"No.\",\"fields\":{}}}"},
            {"status": 200, "body": "{\"csrf_token\":\"token-two\"}", "set_cookies": ["azahar_csrf=token-two; Path=/"]},
            {"status": 200, "body": "{\"ok\":true}", "set_cookies": ["azahar_session=session-one; Path=/"]},
        ],
    });
    check_script(
        checker,
        "script csrf retry",
        script(
            Value::Null,
            Value::Null,
            csrf_retry,
            vec![
                json!({"kind": "request", "method": "POST", "path": "/api/login", "body": {"email": "a@example.com"}, "body_present": true}),
                json!({"kind": "session_state"}),
                json!({"kind": "request", "method": "GET", "path": "/api/me"}),
                json!({"kind": "clear_session"}),
                json!({"kind": "session_state"}),
            ],
        ),
    );
    check_script(
        checker,
        "script csrf from cookie",
        script(
            Value::Null,
            Value::Null,
            json!({"responses": [
                {"status": 200, "body": "{}", "set_cookies": ["azahar_csrf=cookie-token; Path=/"]},
                {"status": 200, "body": "{\"ok\":true}"}
            ]}),
            vec![
                json!({"kind": "request", "method": "POST", "path": "/api/action", "body": {"x": 1}, "body_present": true}),
                json!({"kind": "session_state"}),
            ],
        ),
    );
    check_script(
        checker,
        "script csrf from body",
        script(
            Value::Null,
            Value::Null,
            json!({"responses": [
                {"status": 200, "body": "{\"csrf_token\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"}"},
                {"status": 200, "body": "{\"ok\":true}"}
            ]}),
            vec![
                json!({"kind": "request", "method": "POST", "path": "/api/action"}),
                json!({"kind": "session_state"}),
            ],
        ),
    );
    for csrf_response in [
        json!({"status": 200, "body": "{}"}),
        json!({"status": 200, "body": "{\"csrf_token\":\"\"}"}),
        json!({"status": 200, "body": "{\"csrf_token\":\"a b\"}"}),
        json!({"status": 200, "body": "{\"csrf_token\":42}"}),
        json!({"status": 200, "body": "not json"}),
        json!({"status": 500, "body": "{}"}),
        json!({"status": 403, "body": "{}"}),
    ] {
        check_script(
            checker,
            "script csrf unavailable",
            script(
                Value::Null,
                Value::Null,
                json!({"responses": [csrf_response, {"status": 200, "body": "{\"ok\":true}"}]}),
                vec![
                    json!({"kind": "request", "method": "POST", "path": "/api/action"}),
                    json!({"kind": "session_state"}),
                ],
            ),
        );
    }
    for transport in [
        json!({"responses": [{"status": 200, "body": "{\"csrf_token\":\"body-token\"}"}]}),
        json!({"responses": [{"status": 200, "body": "{}", "set_cookies": ["azahar_csrf=cookie-token; Path=/"]}]}),
        json!({"responses": [{"status": 200, "body": "{}"}]}),
        json!({"responses": [{"status": 200, "body": "{\"csrf_token\":\"a b\"}"}]}),
        json!({"responses": [{"status": 500, "body": "{}"}]}),
        json!({"mode": "fail"}),
        json!({"mode": "timeout"}),
    ] {
        check_script(
            checker,
            "script ensure csrf",
            script(
                Value::Null,
                Value::Null,
                transport,
                vec![
                    json!({"kind": "ensure_csrf"}),
                    json!({"kind": "active_csrf"}),
                    json!({"kind": "ensure_csrf"}),
                    json!({"kind": "active_csrf"}),
                ],
            ),
        );
    }
    check_script(
        checker,
        "script ensure csrf already active",
        script(
            Value::Null,
            Value::Null,
            json!({"mode": "fail"}),
            vec![
                json!({"kind": "seed_csrf", "value": "seeded"}),
                json!({"kind": "ensure_csrf"}),
                json!({"kind": "active_csrf"}),
            ],
        ),
    );
    check_script(
        checker,
        "script update settings warning",
        json!({
            "version": "2.0.0-test",
            "env": Value::Null,
            "secrets": "delete-failing",
            "transport": {"responses": [{"status": 200, "body": "{\"origin\":\"http://health.example\"}"}]},
            "steps": [
                {"kind": "update_settings", "api_url": "http://127.0.0.1:9090", "origin": "", "allow_network": false},
                {"kind": "settings_snapshot"},
                {"kind": "delete_stored_session"},
            ],
        }),
    );
    check_script(
        checker,
        "script csrf retry fails twice",
        script(
            Value::Null,
            Value::Null,
            json!({"responses": [
                {"status": 200, "body": "{\"csrf_token\":\"one\"}"},
                {"status": 403, "body": "{\"error\":{\"code\":\"csrf_failed\",\"message\":\"No.\",\"fields\":{}}}"},
                {"status": 200, "body": "{\"csrf_token\":\"two\"}"},
                {"status": 403, "body": "{\"error\":{\"code\":\"csrf_failed\",\"message\":\"Still no.\",\"fields\":{}}}"}
            ]}),
            vec![
                json!({"kind": "request", "method": "POST", "path": "/api/action"}),
                json!({"kind": "session_state"}),
            ],
        ),
    );
    check_script(
        checker,
        "script forbidden is not retried",
        script(
            Value::Null,
            Value::Null,
            json!({"responses": [
                {"status": 200, "body": "{\"csrf_token\":\"one\"}"},
                {"status": 403, "body": "{\"error\":{\"code\":\"forbidden\",\"message\":\"No.\",\"fields\":{}}}"}
            ]}),
            vec![json!({"kind": "request", "method": "POST", "path": "/api/action"})],
        ),
    );
    check_script(
        checker,
        "script cookie jar",
        script(
            Value::Null,
            Value::Null,
            json!({"responses": [
                {"status": 200, "body": "{\"ok\":true}", "set_cookies": [
                    "azahar_session=session-one; Path=/; HttpOnly",
                    "azahar_csrf=csrf-one; Path=/",
                    "other=not-ours; Path=/",
                    "azahar_extra=not-ours; Path=/"
                ]},
                {"status": 200, "body": "{\"ok\":true}"},
                {"status": 200, "body": "{\"ok\":true}", "set_cookies": [
                    "azahar_session=; Max-Age=-1; Path=/",
                    "azahar_csrf=; Max-Age=-1; Path=/"
                ]},
                {"status": 200, "body": "{\"ok\":true}"}
            ]}),
            vec![
                json!({"kind": "request", "method": "GET", "path": "/api/cookies"}),
                json!({"kind": "request", "method": "GET", "path": "/api/cookies"}),
                json!({"kind": "request", "method": "GET", "path": "/api/cookies"}),
                json!({"kind": "request", "method": "GET", "path": "/api/cookies"}),
                json!({"kind": "session_state"}),
            ],
        ),
    );
    check_script(
        checker,
        "script expired cookie",
        script(
            Value::Null,
            Value::Null,
            json!({"responses": [
                {"status": 200, "body": "{}", "set_cookies": ["azahar_session=stale; Path=/; Expires=Wed, 21 Oct 2015 07:28:00 GMT"]},
                {"status": 200, "body": "{}"}
            ]}),
            vec![
                json!({"kind": "request", "method": "GET", "path": "/api/a"}),
                json!({"kind": "request", "method": "GET", "path": "/api/b"}),
                json!({"kind": "session_state"}),
            ],
        ),
    );
    check_script(
        checker,
        "script absorb cookies generation",
        script(
            Value::Null,
            Value::Null,
            Value::Null,
            vec![
                json!({"kind": "seed_session", "value": "kept-session"}),
                json!({"kind": "absorb_cookies", "set_cookies": ["azahar_session=stale; Path=/"], "generation": 99}),
                json!({"kind": "absorb_cookies", "set_cookies": ["azahar_session=fresh; Path=/"], "use_current_generation": true}),
                json!({"kind": "session_state"}),
                json!({"kind": "absorb_cookies", "set_cookies": ["azahar_csrf=csrf-absorbed; Path=/"], "use_current_generation": true}),
                json!({"kind": "active_csrf"}),
            ],
        ),
    );
    check_script(
        checker,
        "script absorb invalid cookies",
        script(
            Value::Null,
            Value::Null,
            Value::Null,
            vec![
                json!({"kind": "seed_session", "value": "kept-session"}),
                json!({"kind": "absorb_cookies", "set_cookies": [
                    "azahar_session=; Path=/",
                    "azahar_session=bad value; Path=/",
                    "azahar_session=bad,value; Path=/",
                    "azahar_session=%zz; Path=/",
                    "=; Path=/",
                    "not a cookie"
                ], "use_current_generation": true}),
                json!({"kind": "session_state"}),
            ],
        ),
    );
    check_script(
        checker,
        "script csrf store rules",
        script(
            Value::Null,
            Value::Null,
            Value::Null,
            vec![
                json!({"kind": "store_csrf", "token": "first-token", "use_current_generation": true}),
                json!({"kind": "active_csrf"}),
                json!({"kind": "store_csrf", "token": "second-token", "use_current_generation": true}),
                json!({"kind": "active_csrf"}),
                json!({"kind": "store_csrf", "token": "stale-token", "generation": 99}),
                json!({"kind": "active_csrf"}),
                json!({"kind": "store_csrf", "token": "bad token", "use_current_generation": true}),
                json!({"kind": "active_csrf"}),
                json!({"kind": "clear_csrf"}),
                json!({"kind": "active_csrf"}),
            ],
        ),
    );
    check_script(
        checker,
        "script csrf expiry",
        script(
            Value::Null,
            Value::Null,
            Value::Null,
            vec![
                json!({"kind": "seed_csrf", "value": "expired", "expires_unix": 1}),
                json!({"kind": "active_csrf"}),
                json!({"kind": "store_csrf", "token": "fresh", "use_current_generation": true}),
                json!({"kind": "active_csrf"}),
                json!({"kind": "seed_csrf", "value": "future", "expires_unix": 4102444800i64}),
                json!({"kind": "active_csrf"}),
            ],
        ),
    );
    check_script(
        checker,
        "script persist session rules",
        script(
            Value::Null,
            Value::Null,
            Value::Null,
            vec![
                json!({"kind": "seed_session", "value": "current"}),
                json!({"kind": "persist_session", "value": "other"}),
                json!({"kind": "secret_state"}),
                json!({"kind": "persist_session", "value": "current"}),
                json!({"kind": "secret_state"}),
                json!({"kind": "persist_session", "value": ""}),
                json!({"kind": "secret_state"}),
            ],
        ),
    );
    check_script(
        checker,
        "script clear session delete failure",
        json!({
            "version": "2.0.0-test",
            "env": Value::Null,
            "secrets": "delete-failing",
            "steps": [
                {"kind": "seed_session", "value": "session-one"},
                {"kind": "seed_csrf", "value": "csrf-one"},
                {"kind": "delete_stored_session"},
                {"kind": "clear_session"},
                {"kind": "session_state"},
            ],
        }),
    );
    check_script(
        checker,
        "script file secret load and save",
        json!({
            "version": "2.0.0-test",
            "secret_raw": "{\"azahar_session\":\"restored-session\"}",
            "env": Value::Null,
            "secrets": "file",
            "transport": {"responses": [
                {"status": 200, "body": "{}", "set_cookies": ["azahar_session=new-session; Path=/"]}
            ]},
            "steps": [
                {"kind": "session_state"},
                {"kind": "request", "method": "GET", "path": "/api/me"},
                {"kind": "session_state"},
                {"kind": "load_session"},
                {"kind": "session_state"},
                {"kind": "clear_session"},
                {"kind": "session_state"},
            ],
        }),
    );
    for secret_raw in [
        "not json",
        "{}",
        "{\"azahar_session\":\"\"}",
        "{\"azahar_session\":\"bad value\"}",
        "{\"AZAHAR_SESSION\":\"upper-session\"}",
        "{\"session\":\"field-session\"}",
    ] {
        check_script(
            checker,
            "script file secret prefill",
            json!({
                "version": "2.0.0-test",
                "secret_raw": secret_raw,
                "env": Value::Null,
                "secrets": "file",
                "steps": [{"kind": "session_state"}],
            }),
        );
    }
    check_script(
        checker,
        "script failing secret store",
        json!({
            "version": "2.0.0-test",
            "env": Value::Null,
            "secrets": "failing",
            "transport": {"responses": [
                {"status": 200, "body": "{}", "set_cookies": ["azahar_session=session-one; Path=/"]}
            ]},
            "steps": [
                {"kind": "request", "method": "GET", "path": "/api/me"},
                {"kind": "session_state"},
                {"kind": "secret_state"},
                {"kind": "clear_session"},
            ],
        }),
    );
    check_script(
        checker,
        "script fallback secret store",
        json!({
            "version": "2.0.0-test",
            "env": Value::Null,
            "secrets": "fallback",
            "transport": {"responses": [
                {"status": 200, "body": "{}", "set_cookies": ["azahar_session=fallback-session; Path=/"]}
            ]},
            "steps": [
                {"kind": "request", "method": "GET", "path": "/api/me"},
                {"kind": "session_state"},
                {"kind": "clear_session"},
            ],
        }),
    );
    check_script(
        checker,
        "script factory without a store",
        json!({
            "version": "2.0.0-test",
            "env": Value::Null,
            "secrets": "factory-none",
            "transport": {"responses": [
                {"status": 200, "body": "{}", "set_cookies": ["azahar_session=file-fallback; Path=/"]}
            ]},
            "steps": [
                {"kind": "request", "method": "GET", "path": "/api/me"},
                {"kind": "session_state"},
                {"kind": "clear_session"},
            ],
        }),
    );
    check_script(
        checker,
        "script factory stores",
        json!({
            "version": "2.0.0-test",
            "env": Value::Null,
            "secrets": "factory",
            "transport": {"responses": [
                {"status": 200, "body": "{\"origin\":\"http://health.example\"}"},
                {"status": 200, "body": "{}", "set_cookies": ["azahar_session=session-a; Path=/"]},
                {"status": 200, "body": "{}", "set_cookies": ["azahar_session=session-b; Path=/"]},
                {"status": 200, "body": "{}"}
            ]},
            "steps": [
                {"kind": "request", "method": "GET", "path": "/api/session"},
                {"kind": "update_settings", "api_url": "https://b.example.com", "origin": "http://admin.example", "allow_network": true},
                {"kind": "factory_state"},
                {"kind": "request", "method": "GET", "path": "/api/session"},
                {"kind": "factory_state"},
                {"kind": "session_state"},
            ],
        }),
    );

    // Images and the cache.
    let image_cases = [
        (
            "/api/images/a.png",
            200,
            "image/png",
            vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
        ),
        (
            "/api/images/a.jpg",
            200,
            "image/jpeg",
            vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, b'J', b'F', b'I', b'F'],
        ),
        (
            "/api/images/a.webp",
            200,
            "image/webp",
            b"RIFF\x24\x00\x00\x00WEBPVP8 ".to_vec(),
        ),
        ("/api/images/a.gif", 200, "image/gif", b"GIF89a".to_vec()),
        (
            "/api/images/a.avif",
            200,
            "image/avif",
            b"\x00\x00\x00\x1cftypavif".to_vec(),
        ),
        (
            "/api/images/upper",
            200,
            "IMAGE/PNG; charset=binary",
            vec![1, 2, 3],
        ),
        (
            "/api/images/quoted",
            200,
            "image/png; note=\"a;b\"",
            vec![1],
        ),
        ("/api/images/html", 200, "text/html", vec![b'<']),
        ("/api/images/svg", 200, "image/svg+xml", vec![b'<']),
        ("/api/images/none", 200, "", vec![1]),
        ("/api/images/bad-param", 200, "image/png; bare", vec![1]),
        (
            "/api/images/missing",
            404,
            "application/json",
            vec![b'{', b'}'],
        ),
        ("/api/images/empty", 200, "image/png", vec![]),
        ("/api/images/space", 200, " image/png ", vec![1]),
        (
            "/api/images/charset",
            200,
            "image/png; charset=UTF-8",
            vec![1],
        ),
        (
            "/api/images/charset-quoted",
            200,
            "image/png;charset=\"utf-8\"",
            vec![1],
        ),
        ("/api/images/empty-param", 200, "image/png; x=", vec![1]),
        (
            "/api/images/empty-quoted",
            200,
            "image/png; x=\"\"",
            vec![1],
        ),
        (
            "/api/images/escaped",
            200,
            "image/png; x=\"a\\\"b\"",
            vec![1],
        ),
        (
            "/api/images/multi-param",
            200,
            "image/png; x=a; y=b",
            vec![1],
        ),
        ("/api/images/trailing-semicolon", 200, "image/png;", vec![1]),
        (
            "/api/images/space-semicolon",
            200,
            "image/png ; charset=binary",
            vec![1],
        ),
        ("/bad/prefix", 200, "image/png", vec![1]),
        ("/api/other/image.png", 200, "image/png", vec![1]),
        ("/api/images/../secret", 200, "image/png", vec![1]),
        ("/api/images/%2e%2e/secret", 200, "image/png", vec![1]),
        ("/api/images/x\\y", 200, "image/png", vec![1]),
    ];
    for (path, status, content_type, body) in image_cases {
        let body_b64 = STANDARD.encode(&body);
        check_script(
            checker,
            "script image types",
            json!({
                "version": "2.0.0-test",
                "env": Value::Null,
                "secrets": "memory",
                "transport": {"responses": [{
                    "status": status,
                    "body_b64": body_b64,
                    "content_type": content_type,
                }]},
                "steps": [
                    {"kind": "image", "path": path},
                    {"kind": "image_cache"},
                ],
            }),
        );
    }
    for transport in [
        json!({"mode": "fail"}),
        json!({"mode": "timeout"}),
        json!({"responses": [{"status": 200, "content_type": "image/png", "oversize": true, "body": "x"}]}),
    ] {
        check_script(
            checker,
            "script image transport failure",
            json!({
                "version": "2.0.0-test",
                "env": Value::Null,
                "secrets": "memory",
                "transport": transport,
                "steps": [{"kind": "image", "path": "/api/images/a.png"}],
            }),
        );
    }
    check_script(
        checker,
        "script image cache reuse",
        json!({
            "version": "2.0.0-test",
            "env": Value::Null,
            "secrets": "memory",
            "transport": {"responses": [
                {"status": 200, "content_type": "image/png", "body": "abc"},
            ]},
            "steps": [
                {"kind": "image", "path": "/api/images/a.png"},
                {"kind": "image", "path": "/api/images/a.png"},
                {"kind": "image_cache"},
            ],
        }),
    );
    check_script(
        checker,
        "script image cache eviction",
        json!({
            "version": "2.0.0-test",
            "env": Value::Null,
            "secrets": "memory",
            "image_cache_limit": 100,
            "steps": [
                {"kind": "store_image", "key": "/api/images/one", "value": "data:image/png;base64,AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "use_current_generation": true},
                {"kind": "store_image", "key": "/api/images/two", "value": "data:image/png;base64,BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB", "use_current_generation": true},
                {"kind": "image_cache"},
                {"kind": "clear_session"},
                {"kind": "store_image", "key": "/api/images/stale", "value": "data:image/png;base64,AAAA", "generation": 0},
                {"kind": "image_cache"},
            ],
        }),
    );
    check_script(
        checker,
        "script image cache default limit",
        json!({
            "version": "2.0.0-test",
            "env": Value::Null,
            "secrets": "memory",
            "image_cache_limit": 0,
            "steps": [
                {"kind": "store_image", "key": "/api/images/one", "value": "data:image/png;base64,AAAA", "use_current_generation": true},
                {"kind": "image_cache"},
                {"kind": "clear_image_cache"},
                {"kind": "image_cache"},
            ],
        }),
    );
    let block = "data:image/png;base64,";
    check_script(
        checker,
        "script image cache replace order",
        json!({
            "version": "2.0.0-test",
            "env": Value::Null,
            "secrets": "memory",
            "image_cache_limit": 120,
            "steps": [
                {"kind": "store_image", "key": "/api/images/one", "value": format!("{block}{}", "A".repeat(32)), "use_current_generation": true},
                {"kind": "store_image", "key": "/api/images/one", "value": format!("{block}{}", "B".repeat(32)), "use_current_generation": true},
                {"kind": "store_image", "key": "/api/images/two", "value": format!("{block}{}", "C".repeat(32)), "use_current_generation": true},
                {"kind": "store_image", "key": "/api/images/three", "value": format!("{block}{}", "D".repeat(32)), "use_current_generation": true},
                {"kind": "image_cache"},
                {"kind": "store_image", "key": "/api/images/huge", "value": format!("{block}{}", "E".repeat(120)), "use_current_generation": true},
                {"kind": "image_cache"},
            ],
        }),
    );

    // Legacy import inside a client.
    check_script(
        checker,
        "script legacy import",
        json!({
            "version": "2.0.0-test",
            "legacy_raw": "{\"apiUrl\":\"https://legacy.example\",\"origin\":\"https://legacy.example\",\"window\":{\"width\":1200,\"height\":800,\"x\":40,\"y\":50}}",
            "legacy_kind": "xdg",
            "env": Value::Null,
            "secrets": "memory",
            "steps": [
                {"kind": "info"},
                {"kind": "window_bounds"},
                {"kind": "settings_snapshot"},
            ],
        }),
    );
    check_script(
        checker,
        "script legacy overridden by env",
        json!({
            "version": "2.0.0-test",
            "legacy_raw": "{\"apiUrl\":\"https://legacy.example\"}",
            "legacy_kind": "xdg",
            "env": {"api_url": "http://127.0.0.1:9090", "origin": "http://admin.example"},
            "secrets": "memory",
            "steps": [{"kind": "info"}, {"kind": "settings_snapshot"}],
        }),
    );
}

fn main_extended() {
    let mut checker = Checker::default();

    let before = checker.comparisons;
    pure_helpers(&mut checker);
    println!(
        "  pure helpers (decode_setting, consent, errors): {}",
        checker.comparisons - before
    );

    let before = checker.comparisons;
    settings_files(&mut checker);
    write_private_files(&mut checker);
    println!(
        "  settings and file I/O (load/save/private/legacy): {}",
        checker.comparisons - before
    );

    let before = checker.comparisons;
    secret_stores(&mut checker);
    println!(
        "  secret stores (file/memory/fallback/timeout): {}",
        checker.comparisons - before
    );

    let before = checker.comparisons;
    client_scripts(&mut checker);
    println!(
        "  scripted clients (settings/health/session/images): {}",
        checker.comparisons - before
    );

    let before = checker.comparisons;

    // The default-store selector, without a live keyring.
    for api_url in ["http://127.0.0.1:8080", "https://a.example.com", "bad"] {
        for session_backend in [
            None,
            Some(""),
            Some("file"),
            Some("FILE"),
            Some(" file "),
            Some("keyring"),
        ] {
            for disable_keyring in [
                None,
                Some(""),
                Some("0"),
                Some("1"),
                Some("false"),
                Some("off"),
            ] {
                let environment = Environment {
                    session_backend: session_backend.map(str::to_owned),
                    disable_keyring: disable_keyring.map(str::to_owned),
                    ..Environment::default()
                };
                checker.check_ok(
                    "default_secret_store_kind",
                    json!({
                        "op": "default_secret_store_kind",
                        "profile": "/home/oracle/profile",
                        "raw": api_url,
                        "env": {
                            "AZAHAR_SESSION_BACKEND": session_backend,
                            "AZAHAR_DISABLE_KEYRING": disable_keyring,
                        },
                    }),
                    Value::String(
                        azahar_desktop_api::harness::default_secret_store_kind(
                            Path::new("/home/oracle/profile"),
                            api_url,
                            &environment,
                        )
                        .to_owned(),
                    ),
                );
            }
        }
    }

    println!(
        "  default secret store selector: {}",
        checker.comparisons - before
    );
    println!(
        "desktop API extended differential comparisons: {}",
        checker.comparisons
    );
    assert!(
        checker.comparisons >= MIN_COMPARISONS,
        "extended differential ran only {} comparisons, expected at least {MIN_COMPARISONS}",
        checker.comparisons
    );
}

#[test]
fn extended_differential_against_go_oracle() {
    main_extended();
}
