#[allow(dead_code)]
mod common;

use azahar_desktop_api::{
    ApiError, FirewallGuide, Health, Info, Response, Settings, SettingsResult, WindowBounds,
    firewall_guide, go_platform,
};
use serde_json::json;

#[test]
fn firewall_guide_platforms() {
    let cases = [
        (
            "windows",
            vec!["Windows Security", "Allow an app through firewall"],
        ),
        (
            "darwin",
            vec!["System Settings", "Network", "Firewall", "Options"],
        ),
        ("linux", vec!["ufw", "firewalld"]),
    ];
    for (platform, expected) in cases {
        let guide = firewall_guide(platform);
        assert_eq!(guide.platform, platform);
        assert!(!guide.title.is_empty());
        assert!(!guide.intro.is_empty());
        assert!(!guide.note.is_empty());
        assert!(!guide.steps.is_empty());
        assert!(guide.note.contains("never changes firewall rules"));
        let joined = guide.steps.join("\n");
        for want in expected {
            assert!(
                joined.contains(want),
                "{platform} steps lack {want}: {joined}"
            );
        }
    }
    let fallback = firewall_guide("freebsd");
    assert_eq!(fallback.steps.len(), 2);
    let test = common::test_client("");
    assert_eq!(test.client.firewall_guide().platform, go_platform());
}

#[test]
fn json_contract() {
    let cases: Vec<(&str, serde_json::Value, Vec<&str>)> = vec![
        (
            "info",
            serde_json::to_value(Info {
                version: "1".to_owned(),
                platform: "linux".to_owned(),
                api_url: "http://127.0.0.1:8080".to_owned(),
                origin: String::new(),
                resolved_origin: String::new(),
                network_allowed: false,
            })
            .unwrap(),
            vec![
                "version",
                "platform",
                "apiUrl",
                "origin",
                "resolvedOrigin",
                "networkAllowed",
            ],
        ),
        (
            "health",
            serde_json::to_value(Health {
                ok: true,
                status: 200,
                env: "test".to_owned(),
                origin: "http://localhost:3000".to_owned(),
                server_time: "now".to_owned(),
                message: String::new(),
            })
            .unwrap(),
            vec!["ok", "status", "env", "origin", "serverTime", "message"],
        ),
        (
            "error",
            serde_json::to_value(ApiError {
                status: 422,
                code: "validation_error".to_owned(),
                message: "Check the fields.".to_owned(),
                fields: [("email".to_owned(), "Required.".to_owned())].into(),
            })
            .unwrap(),
            vec!["status", "code", "message", "fields"],
        ),
        (
            "response success",
            serde_json::to_value(Response::success(Some(json!({"user": "Amina"})))).unwrap(),
            vec!["ok", "data"],
        ),
        (
            "response failure",
            serde_json::to_value(Response::failure(ApiError::new(403, "csrf_failed", "No.")))
                .unwrap(),
            vec!["ok", "error"],
        ),
        (
            "settings result",
            serde_json::to_value(SettingsResult {
                api_url: "http://127.0.0.1:8080".to_owned(),
                origin: String::new(),
                resolved_origin: "http://localhost:3000".to_owned(),
                network_allowed: false,
                warning: String::new(),
            })
            .unwrap(),
            vec!["apiUrl", "origin", "resolvedOrigin", "networkAllowed"],
        ),
        (
            "settings",
            serde_json::to_value(Settings {
                api_url: "http://127.0.0.1:8080".to_owned(),
                window: WindowBounds {
                    width: 1,
                    height: 2,
                    x: 3,
                    y: 4,
                },
                ..Settings::default()
            })
            .unwrap(),
            vec![
                "apiUrl",
                "origin",
                "networkAllowed",
                "networkAllowedHost",
                "window",
            ],
        ),
        (
            "window",
            serde_json::to_value(WindowBounds {
                width: 1,
                height: 2,
                x: 3,
                y: 4,
            })
            .unwrap(),
            vec!["width", "height", "x", "y"],
        ),
        (
            "firewall guide",
            serde_json::to_value(FirewallGuide {
                platform: "linux".to_owned(),
                title: "t".to_owned(),
                intro: "i".to_owned(),
                steps: vec!["s".to_owned()],
                note: "n".to_owned(),
            })
            .unwrap(),
            vec!["platform", "title", "intro", "steps", "note"],
        ),
    ];
    for (name, value, fields) in cases {
        let object = value
            .as_object()
            .unwrap_or_else(|| panic!("{name} is not an object"));
        for field in fields {
            assert!(
                object.contains_key(field),
                "{field} is missing from {name}: {value}"
            );
        }
    }
}

#[test]
fn optional_fields_are_omitted() {
    assert_eq!(
        serde_json::to_value(Response::success(None)).unwrap(),
        json!({"ok": true})
    );
    let result = SettingsResult {
        api_url: "http://127.0.0.1:8080".to_owned(),
        ..SettingsResult::default()
    };
    assert!(
        !serde_json::to_value(result)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("warning")
    );
    let error = ApiError::new(0, "network_error", "no");
    assert_eq!(
        serde_json::to_value(error).unwrap(),
        json!({"status": 0, "code": "network_error", "message": "no", "fields": {}})
    );
}
