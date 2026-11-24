//! Differential test against the Go oracle (`desktop/cmd/oracle-api`).
//!
//! The oracle calls the real Go package for pure decisions, request headers,
//! settings/DTO marshalling, cookies, error envelopes and a scripted local
//! login/logout/me flow. The keyring is excluded: it needs a live Secret
//! Service session on the developer machine, so the Go side is driven through
//! an in-memory `secretStore` and only the file store is ported for comparison
//! through the session tests.

#[allow(dead_code)]
mod oracle;
#[allow(dead_code)]
mod support;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use azahar_desktop_api::{
    ApiError, Client, ClientOptions, Environment, HeaderInput, MemorySecretStore, Settings,
    StoredCookie, WindowBounds, build_headers, cookie_from_set_cookie, device_hash, firewall_guide,
    is_csrf_endpoint, is_local_host, is_loopback_host, normalize_api_url, normalize_host,
    normalize_origin, session_scope, target_url_with, validate_api_path, validate_server_address,
};
use oracle::{Checker, epoch, unix};
use serde_json::{Value, json};
use support::{MockResponse, MockServer};

const NOW: i64 = 1_800_000_000;
const USER_AGENT: &str = "AzaharDesktop/oracle (fixeddevice)";

fn validation(value: Option<ApiError>) -> Result<Value, ApiError> {
    match value {
        Some(error) => Err(error),
        None => Ok(Value::Bool(true)),
    }
}

fn headers_json(headers: Vec<(String, String)>) -> Value {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, value) in headers {
        map.entry(name).or_default().push(value);
    }
    Value::Object(
        map.into_iter()
            .map(|(name, values)| (name, json!(values)))
            .collect(),
    )
}

fn url_cases() -> Vec<String> {
    let schemes = ["http", "https", "HTTP", "ftp", ""];
    let hosts = [
        "127.0.0.1",
        "localhost",
        "LOCALHOST.",
        "[::1]",
        "[fe80::1%25eth0]",
        "192.168.1.10",
        "10.20.30.40",
        "172.16.0.1",
        "172.32.0.1",
        "169.254.1.1",
        "api.local",
        "api.localhost",
        "api.example.com",
        "127.0.0.1.nip.io",
        "user:pass@api.example.com",
        "",
    ];
    let ports = ["", ":80", ":443", ":8080", ":99999", ":0", ":"];
    let mut cases = Vec::new();
    for scheme in schemes {
        for host in hosts {
            for port in ports {
                cases.push(if scheme.is_empty() {
                    format!("//{host}{port}")
                } else {
                    format!("{scheme}://{host}{port}")
                });
            }
        }
    }
    let suffixes = ["/", "/api", "?x=1", "#x", "#", "/?a=b", "/api/x"];
    for scheme in ["http", "https"] {
        for host in [
            "127.0.0.1",
            "localhost",
            "[::1]",
            "192.168.1.10",
            "api.example.com",
        ] {
            for suffix in suffixes {
                cases.push(format!("{scheme}://{host}{suffix}"));
            }
        }
    }
    cases.extend(
        [
            "http://[fe80::1%25eth0]:8080",
            "http://[::1]",
            "https://admin.example.com:443",
            "http://api.local:8080",
            "http://",
            "https://",
            "http:example.com",
            "http:////api.example.com",
            "http://[::1]:",
            "http://[::1]x",
            "http://[127.0.0.1]",
            "http://a:b:8080",
            "https://api.example.com/%zz",
            "https://api.example.com/%2e",
            "http://%31%32%37.0.0.1:8080",
            "https://exa%20mple.com",
            "https://exa%5cmple.com",
            "https://exa%c3%a9.com",
            " http://api.example.com ",
        ]
        .map(str::to_owned),
    );
    cases
}

fn host_cases() -> Vec<String> {
    let mut cases: Vec<String> = vec![
        "localhost",
        "LOCALHOST",
        "localhost.",
        "localhost..",
        "api.localhost",
        "api.localhost.",
        "api.local",
        "api.local.",
        "api.example.com",
        "example.localhost.evil",
        "127.0.0.1",
        "127.255.255.255",
        "128.0.0.1",
        "10.0.0.1",
        "172.15.255.255",
        "172.16.0.0",
        "172.31.255.255",
        "172.32.0.0",
        "192.168.0.1",
        "192.169.0.1",
        "169.254.1.1",
        "0.0.0.0",
        "224.0.0.1",
        "255.255.255.255",
        "999.1.1.1",
        "127.0.0.01",
        "::1",
        "::",
        "::ffff:127.0.0.1",
        "::ffff:10.0.0.1",
        "::ffff:192.0.2.1",
        "fc00::1",
        "fdff::1",
        "fe80::1",
        "febf::1",
        "fec0::1",
        "ff02::1",
        "2001:db8::1",
        "fe80::1%eth0",
        "fe80::1%25eth0",
        "[::1]",
        "",
        "  LocalHost.  ",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();

    for first in [0, 10, 100, 127, 169, 172, 192, 224, 255] {
        for second in [0, 1, 16, 31, 168, 254, 255] {
            for third in [0, 1, 255] {
                for fourth in [0, 1, 255] {
                    cases.push(format!("{first}.{second}.{third}.{fourth}"));
                }
            }
        }
    }
    cases.extend(
        [
            "::1",
            "::",
            "fc00::1",
            "fdff::1",
            "fe80::1",
            "febf::1",
            "ff02::1",
            "2001:db8::1",
            "::ffff:127.0.0.1",
            "::ffff:192.168.0.1",
        ]
        .into_iter()
        .map(str::to_owned),
    );
    cases
}

fn path_cases() -> Vec<String> {
    let mut cases: Vec<String> = [
        "",
        "/api",
        "/api/",
        "/api/health",
        "/api/../admin",
        "/api/%2e%2e/admin",
        "/api/%2E%2E/admin",
        "/api/..%2f..%2fetc",
        "/api/%2f%2fevil.com",
        "/api/%252e%252e/admin",
        "/api/x%5Cy",
        "/api/x\\y",
        "/api/x\0y",
        "//evil.com/api/x",
        "http://evil.com/api/x",
        "/other/x",
        "/api/ok?q=%2e%2e%2f&page=2",
        "/api/ok?q=%00",
        "/api/ok?q=%zz",
        "/api/ok?",
        "/api/ok?x=1?y=2",
        "/api//csrf",
        "/api/csrf",
        "/api/csrf?x=1",
        "/api/csrf/",
        "/api/csrf;foo",
        "/api/csrf%3bfoo",
        "/api/csrf.",
        "/api/csrf%20",
        "/api/CSRF",
        "/api/%63srf",
        "/api/csrfx",
        "/api/images/a.png",
        "/api/images/%2e%2e/secret",
        "/api/./x",
        "/api/x/../y",
        "/api/%41",
        "/api/%41%42",
        "/api/%e2%82%ac",
        "/api/\u{20ac}",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for segment in [
        "x", "x.", "x ", "csrf", "CsRf;x", "csrf.", "csrf ", "%63srf",
    ] {
        for suffix in ["", "/", "/y", "?a=b", "?", "//z"] {
            cases.push(format!("/api/{segment}{suffix}"));
        }
    }
    cases
}

fn error_cases() -> Vec<(i32, String)> {
    let mut cases = vec![
        (500, String::new()),
        (500, "not json".to_owned()),
        (500, "null".to_owned()),
        (500, "[]".to_owned()),
        (500, "{}".to_owned()),
        (400, r#"{"error":null}"#.to_owned()),
        (401, r#"{"error":{}}"#.to_owned()),
        (
            422,
            r#"{"error":{"code":"validation_error","message":"Check the fields.","fields":{"email":"Required."}}}"#
                .to_owned(),
        ),
        (
            403,
            r#"{"error":{"code":"  csrf_failed  ","message":"  Nope.  ","fields":{"x":"y"}}}"#
                .to_owned(),
        ),
        (
            500,
            r#"{"error":{"code":42,"message":"bad"}}"#.to_owned(),
        ),
        (
            500,
            r#"{"error":{"code":"code","message":42}}"#.to_owned(),
        ),
        (
            500,
            r#"{"error":{"code":"code","message":"message","fields":{"a":1}}}"#
                .to_owned(),
        ),
        (
            500,
            r#"{"error":"string"}"#.to_owned(),
        ),
        (
            500,
            r#"{"error":{"code":"code","message":"message","fields":null}}"#
                .to_owned(),
        ),
        (
            500,
            r#"{"error":{"code":"code","message":"message","fields":{}},"extra":true}"#
                .to_owned(),
        ),
        (
            500,
            r#"{"error":{"code":"code","message":"message","fields":{}}}"#
                .to_owned(),
        ),
        (
            500,
            r#"{"error":{"code":"code","message":"message","fields":{}}} trailing"#
                .to_owned(),
        ),
        (
            500,
            r#"{"error":{"code":null,"message":null}}"#.to_owned(),
        ),
        (
            500,
            r#"{"error":{"code":"code","code":"later"}}"#.to_owned(),
        ),
    ];
    cases.push((
        500,
        format!(
            r#"{{"error":{{"code":"{}","message":"m"}}}}"#,
            "x".repeat(10_000)
        ),
    ));
    cases
}

fn settings_cases() -> Vec<Settings> {
    vec![
        Settings::default(),
        Settings {
            api_url: "http://127.0.0.1:8080".to_owned(),
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
            api_url: "http://127.0.0.1:9090".to_owned(),
            origin: "https://admin.example/app".to_owned(),
            network_allowed: true,
            network_allowed_host: String::new(),
            window: WindowBounds {
                width: i64::MAX,
                height: i64::MIN,
                x: 0,
                y: 0,
            },
        },
    ]
}

fn settings_command(settings: &Settings) -> Value {
    json!({
        "op": "settings_json",
        "settings": serde_json::to_value(settings).expect("settings json"),
    })
}

fn dto_rust(kind: &str) -> Value {
    match kind {
        "info" => serde_json::to_value(azahar_desktop_api::Info {
            version: "1".to_owned(),
            platform: "linux".to_owned(),
            api_url: "http://127.0.0.1:8080".to_owned(),
            origin: String::new(),
            resolved_origin: String::new(),
            network_allowed: false,
        }),
        "health" => serde_json::to_value(azahar_desktop_api::Health {
            ok: true,
            status: 200,
            env: "test".to_owned(),
            origin: "http://localhost:3000".to_owned(),
            server_time: "now".to_owned(),
            message: String::new(),
        }),
        "error" => serde_json::to_value(ApiError {
            status: 422,
            code: "validation_error".to_owned(),
            message: "Check the fields.".to_owned(),
            fields: [("email".to_owned(), "Required.".to_owned())].into(),
        }),
        "response_success" => serde_json::to_value(azahar_desktop_api::Response::success(Some(
            json!({"user": "Amina"}),
        ))),
        "response_empty" => serde_json::to_value(azahar_desktop_api::Response::success(None)),
        "response_failure" => serde_json::to_value(azahar_desktop_api::Response::failure(
            ApiError::new(403, "csrf_failed", "No."),
        )),
        "settings_result" => serde_json::to_value(azahar_desktop_api::SettingsResult {
            api_url: "http://127.0.0.1:8080".to_owned(),
            origin: String::new(),
            resolved_origin: "http://localhost:3000".to_owned(),
            network_allowed: false,
            warning: String::new(),
        }),
        "settings_result_warning" => serde_json::to_value(azahar_desktop_api::SettingsResult {
            api_url: "http://127.0.0.1:8080".to_owned(),
            origin: "http://admin.example".to_owned(),
            resolved_origin: "http://admin.example".to_owned(),
            network_allowed: true,
            warning: "warning".to_owned(),
        }),
        "settings" => serde_json::to_value(Settings {
            api_url: "http://127.0.0.1:8080".to_owned(),
            origin: String::new(),
            network_allowed: false,
            network_allowed_host: String::new(),
            window: WindowBounds {
                width: 1,
                height: 2,
                x: 3,
                y: 4,
            },
        }),
        "window" => serde_json::to_value(WindowBounds {
            width: 1,
            height: 2,
            x: 3,
            y: 4,
        }),
        "firewall" => serde_json::to_value(azahar_desktop_api::FirewallGuide {
            platform: "linux".to_owned(),
            title: "t".to_owned(),
            intro: "i".to_owned(),
            steps: vec!["s".to_owned()],
            note: "n".to_owned(),
        }),
        other => panic!("unknown dto {other}"),
    }
    .expect("dto json")
}

fn rust_session_script() -> Value {
    let observations = Arc::new(Mutex::new(Vec::<Value>::new()));
    let recorded = Arc::clone(&observations);
    let server = MockServer::start(move |request| {
        recorded.lock().unwrap().push(json!({
            "method": request.method,
            "path": request.path,
            "raw_query": request.raw_query,
            "cookie": request.header("cookie"),
            "csrf": request.header("x-csrf-token"),
            "origin": request.header("origin"),
            "requested_with": request.header("x-requested-with"),
            "accept": request.header("accept"),
            "content_type": request.header("content-type"),
            "body": request.body_text(),
        }));
        match request.path.as_str() {
            "/api/health" => MockResponse::json(
                200,
                r#"{"origin":"http://localhost:3000","env":"test","server_time":"2026-11-21T10:46:28Z"}"#,
            ),
            "/api/session" => MockResponse::json(200, r#"{"user":{"name":"Amina"}}"#)
                .with_set_cookie("azahar_session=session-one; Path=/")
                .with_set_cookie("azahar_csrf=csrf-one; Path=/"),
            "/api/me" => MockResponse::json(200, r#"{"user":{"name":"Amina"}}"#),
            "/api/logout" => MockResponse::json(200, r#"{"ok":true}"#)
                .with_set_cookie("azahar_session=; Max-Age=-1; Path=/")
                .with_set_cookie("azahar_csrf=; Max-Age=-1; Path=/"),
            _ => MockResponse::json(
                404,
                r#"{"error":{"code":"not_found","message":"Not found.","fields":{}}}"#,
            ),
        }
    });
    let temp = tempfile::tempdir().expect("temp profile");
    let store = Arc::new(MemorySecretStore::new());
    let client = Client::with_options(
        ClientOptions::new("2.0.0-test", temp.path().join("profile"))
            .environment(Environment::default().api_url(server.url()).origin(""))
            .secret_store(store.clone()),
    )
    .expect("client");

    let calls = [
        ("GET", "/api/session", None),
        ("GET", "/api/me", None),
        ("POST", "/api/logout", Some(json!({"reason": "done"}))),
        ("GET", "/api/me", None),
    ];
    let mut responses = Vec::new();
    for (method, path, body) in calls {
        let response = client.request(method, path, body.as_ref());
        responses.push(serde_json::to_value(response).expect("response json"));
    }
    let (session, csrf) = client.session_state();
    let secret = store.stored();
    let observations = observations.lock().unwrap().clone();
    json!({
        "observations": observations,
        "responses": responses,
        "session": session,
        "csrf": csrf,
        "secret": {
            "value": secret.clone().unwrap_or_default(),
            "present": secret.is_some(),
        },
    })
}

fn main_differential() {
    let mut checker = Checker::default();

    // URL canonicalisation.
    let urls = url_cases();
    for raw in &urls {
        let command = json!({"op": "normalize_api_url", "raw": raw});
        checker.check(
            "normalize_api_url",
            command,
            normalize_api_url(raw).map(Value::String),
        );
        for allow in [false, true] {
            let command = json!({
                "op": "validate_server_address",
                "raw": raw,
                "allow_network": allow,
            });
            checker.check(
                "validate_server_address",
                command,
                validation(validate_server_address(raw, allow)),
            );
        }
        let command = json!({"op": "api_host", "raw": raw});
        checker.check(
            "api_host",
            command,
            azahar_desktop_api::api_host(raw).map(Value::String),
        );
    }

    // Origin canonicalisation.
    for raw in &urls {
        let command = json!({"op": "normalize_origin", "raw": raw});
        checker.check(
            "normalize_origin",
            command,
            normalize_origin(raw).map(Value::String),
        );
        let canonical = azahar_desktop_api::parse_url(raw)
            .map(|parsed| azahar_desktop_api::canonical_origin(&parsed))
            .unwrap_or_default();
        checker.check_ok(
            "canonical_origin",
            json!({"op": "canonical_origin", "raw": raw}),
            Value::String(canonical),
        );
    }

    // Host policy.
    for host in host_cases() {
        let loopback = is_loopback_host(&host);
        let local = is_local_host(&host);
        let normalized = normalize_host(&host);
        checker.check_ok(
            "host_rules",
            json!({"op": "host_rules", "host": &host}),
            json!({"loopback": loopback, "local": local}),
        );
        checker.check_ok(
            "normalize_host",
            json!({"op": "normalize_host", "host": &host}),
            Value::String(normalized),
        );
    }

    // API path guard.
    for raw in path_cases() {
        let rust = match validate_api_path(&raw) {
            Ok(path) => Ok(json!({
                "path": path.path,
                "raw_query": path.raw_query,
                "csrf": is_csrf_endpoint(&raw),
            })),
            Err(error) => Err(error),
        };
        checker.check(
            "validate_path",
            json!({"op": "validate_path", "raw": raw}),
            rust,
        );
    }

    // Target URL resolution.
    let api_urls = [
        "http://127.0.0.1:8080",
        "http://localhost:3000",
        "http://192.168.1.10:8080",
        "https://remote.example",
        "http://[::1]:8080",
    ];
    let allowed_hosts = ["", "192.168.1.10", "remote.example"];
    let targets = [
        "/api/health",
        "/api/x?q=1",
        "/api/%63srf",
        "/api//x",
        "/api/x?",
        "/api/images/a.png",
        "/api/%2f",
        "/api/../x",
        "/api/a b",
        "/api/caf\u{e9}",
        "/api/a%2Bb",
        "/api/%41%42",
        "/api/x?q=a b",
    ];
    for api_url in api_urls {
        for allowed in allowed_hosts {
            for raw in targets {
                let command = json!({
                    "op": "target_url",
                    "api_url": api_url,
                    "allowed_host": allowed,
                    "generation": 7,
                    "raw": raw,
                });
                let rust = target_url_with(api_url, allowed, 7, raw)
                    .map(|target| json!({"url": target.url, "generation": target.generation}));
                checker.check("target_url", command, rust);
            }
        }
    }

    // Request headers.
    let now = epoch(NOW);
    let header_cases = [
        HeaderCase::new("", "", "", "", false, false, false),
        HeaderCase::new("a.example", "", "", "s", true, true, true),
        HeaderCase::new("http://a.example", "", "", "", false, true, true),
        HeaderCase::new(
            "",
            "http://resolved.example",
            "sess",
            "csrf",
            true,
            true,
            false,
        ),
        HeaderCase::new(
            "http://a.example",
            "http://b.example",
            "sess",
            "csrf",
            false,
            false,
            true,
        ),
    ];
    for case in header_cases {
        let session = StoredCookie {
            value: case.session.to_owned(),
            expires: None,
        };
        let csrf = StoredCookie {
            value: case.csrf.to_owned(),
            expires: None,
        };
        let command = json!({
            "op": "request_headers",
            "user_agent": USER_AGENT,
            "api_url": "http://127.0.0.1:8080",
            "origin": case.origin,
            "resolved_origin": case.resolved_origin,
            "allowed_host": "",
            "session": case.session,
            "csrf": case.csrf,
            "session_expires_unix": 0,
            "csrf_expires_unix": 0,
            "now_unix": NOW,
            "include_origin": case.include_origin,
            "include_csrf": case.include_csrf,
            "has_body": case.has_body,
        });
        let headers = build_headers(&HeaderInput {
            user_agent: USER_AGENT,
            origin: case.origin,
            resolved_origin: case.resolved_origin,
            session: &session,
            csrf: &csrf,
            now,
            include_origin: case.include_origin,
            include_csrf: case.include_csrf,
            has_body: case.has_body,
        });
        checker.check_ok("request_headers", command, headers_json(headers));
    }
    // Expired cookies are omitted, like the Go implementation.
    let session = StoredCookie {
        value: "sess".to_owned(),
        expires: Some(epoch(NOW - 10)),
    };
    let csrf = StoredCookie {
        value: "csrf".to_owned(),
        expires: Some(epoch(NOW + 10)),
    };
    let headers = build_headers(&HeaderInput {
        user_agent: USER_AGENT,
        origin: "",
        resolved_origin: "",
        session: &session,
        csrf: &csrf,
        now,
        include_origin: true,
        include_csrf: true,
        has_body: false,
    });
    checker.check_ok(
        "request_headers expired",
        json!({
            "op": "request_headers",
            "user_agent": USER_AGENT,
            "api_url": "http://127.0.0.1:8080",
            "origin": "",
            "allowed_host": "",
            "session": "sess",
            "csrf": "csrf",
            "session_expires_unix": NOW - 10,
            "csrf_expires_unix": NOW + 10,
            "now_unix": NOW,
            "include_origin": true,
            "include_csrf": true,
            "has_body": false,
        }),
        headers_json(headers),
    );

    // Cookie parsing and expiry.
    let cookie_values = [
        "abc",
        "",
        "a b",
        "a;b",
        "a\"b",
        "a,b",
        "a\\b",
        "token-123",
        "=x",
    ];
    let attributes = [
        "",
        "; Path=/",
        "; Max-Age=60",
        "; Max-Age=0",
        "; Max-Age=-1",
        "; Max-Age=abc",
        "; Expires=Wed, 21 Oct 2015 07:28:00 GMT",
        "; Expires=Fri, 01 Jan 2100 00:00:00 GMT",
        "; Expires=bad",
        "; Max-Age=9223372036854775807",
        "; Max-Age=-9223372036854775808",
        "; Max-Age=60; Expires=Wed, 21 Oct 2015 07:28:00 GMT",
    ];
    for name in ["azahar_session", "azahar_csrf", "other"] {
        for value in cookie_values {
            for attribute in attributes {
                let header = format!("{name}={value}{attribute}");
                let rust = cookie_from_set_cookie(&header, now);
                let (value, expires, present) = match rust {
                    Some(cookie) => (cookie.value, unix(cookie.expires), true),
                    None => (String::new(), 0, false),
                };
                checker.check_ok(
                    "cookie_from_set_cookie",
                    json!({
                        "op": "cookie_from_set_cookie",
                        "set_cookie": header,
                        "now_unix": NOW,
                    }),
                    json!({"present": present, "value": value, "expires_unix": expires}),
                );
            }
        }
    }
    let mut cookie_values = vec![
        "abc".to_owned(),
        String::new(),
        "a b".to_owned(),
        "a;b".to_owned(),
        "a\"b".to_owned(),
        "a,b".to_owned(),
        "a\\b".to_owned(),
    ];
    cookie_values.push("x".repeat(4097));
    for value in &cookie_values {
        checker.check_ok(
            "valid_cookie_value",
            json!({"op": "valid_cookie_value", "value": value}),
            Value::Bool(azahar_desktop_api::valid_cookie_value(value)),
        );
    }

    // Error envelopes.
    for (status, body) in error_cases() {
        let rust = azahar_desktop_api::harness::error_from_result(
            &azahar_desktop_api::harness::HttpResult {
                status,
                body: body.clone().into_bytes(),
                content_type: String::new(),
            },
        );
        checker.check(
            "error_from_result",
            json!({"op": "error_from_result", "status": status, "body_string": body}),
            Ok(serde_json::to_value(rust).expect("error json")),
        );
        let code = azahar_desktop_api::harness::error_code(body.as_bytes());
        checker.check_ok(
            "error_code",
            json!({"op": "error_code", "body_string": body}),
            Value::String(code),
        );
        let decoded = azahar_desktop_api::harness::decode_json(body.as_bytes());
        checker.check_ok(
            "decode_json",
            json!({"op": "decode_json", "body_string": body}),
            json!({"present": decoded.is_some(), "value": decoded.unwrap_or(Value::Null)}),
        );
    }

    // Request body encoding.
    let bodies = [
        (false, Value::Null),
        (true, Value::Null),
        (
            true,
            json!({"name": "Amina", "html": "<b>&</b>", "lines": "a\u{2028}b"}),
        ),
        (true, json!([])),
        (true, json!([1, 2, 3])),
        (true, json!("string")),
        (true, json!(42)),
        (true, json!(true)),
        (true, json!({"nested": {"a": [1, {"b": null}]}})),
    ];
    for method in ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE"] {
        for (present, payload) in &bodies {
            let command = json!({
                "op": "encode_body",
                "method": method,
                "payload_present": present,
                "payload": if *present { payload.clone() } else { Value::Null },
            });
            let payload_option = if *present && !payload.is_null() {
                Some(payload)
            } else {
                None
            };
            let rust = azahar_desktop_api::harness::encode_request_body(method, payload_option)
                .map(|payload| {
                    Value::String(
                        payload
                            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                            .unwrap_or_default(),
                    )
                });
            checker.check("encode_body", command, rust);
        }
    }

    // Scope and device hash.
    for profile in [
        "/home/a/profile",
        "/tmp/x/",
        "profile-a",
        "/home/a/./b/../profile",
    ] {
        for api_url in [
            "http://127.0.0.1:8080",
            "https://A.EXAMPLE.COM:443",
            "http://127.0.0.1:8080/",
            "bad url",
        ] {
            checker.check_ok(
                "session_scope",
                json!({"op": "session_scope", "profile": profile, "api_url": api_url}),
                Value::String(session_scope(Path::new(profile), api_url)),
            );
            checker.check_ok(
                "device_hash",
                json!({"op": "device_hash", "profile": profile}),
                Value::String(device_hash(Path::new(profile))),
            );
        }
    }

    // Firewall guides.
    for platform in [
        "windows", "Windows", "WINDOWS", "darwin", "Darwin", "linux", "Linux", "freebsd",
        "android", "ios", "js", "", " ",
    ] {
        checker.check_ok(
            "firewall",
            json!({"op": "firewall", "platform": platform}),
            serde_json::to_value(firewall_guide(platform)).expect("firewall json"),
        );
    }

    // DTO marshalling.
    for kind in [
        "info",
        "health",
        "error",
        "response_success",
        "response_empty",
        "response_failure",
        "settings_result",
        "settings_result_warning",
        "settings",
        "window",
        "firewall",
    ] {
        checker.check_ok("dto", json!({"op": "dto", "kind": kind}), dto_rust(kind));
    }
    for settings in settings_cases() {
        checker.check_ok(
            "settings_json",
            settings_command(&settings),
            serde_json::to_value(&settings).expect("settings json"),
        );
    }

    // Login / cookie / CSRF state transition against a local fake server.
    let oracle_script = checker.oracle().command(&json!({"op": "session_script"}));
    assert!(
        oracle_script
            .get("ok")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "session script failed: {oracle_script}"
    );
    let rust_script = rust_session_script();
    assert_eq!(
        rust_script,
        oracle_script.get("value").cloned().unwrap_or(Value::Null),
        "session script mismatch"
    );
    checker.comparisons += 1;

    println!(
        "desktop API differential comparisons: {}",
        checker.comparisons
    );
}

#[test]
fn differential_against_go_oracle() {
    main_differential();
}

struct HeaderCase {
    origin: &'static str,
    resolved_origin: &'static str,
    session: &'static str,
    csrf: &'static str,
    include_origin: bool,
    include_csrf: bool,
    has_body: bool,
}

impl HeaderCase {
    const fn new(
        origin: &'static str,
        resolved_origin: &'static str,
        session: &'static str,
        csrf: &'static str,
        include_origin: bool,
        include_csrf: bool,
        has_body: bool,
    ) -> Self {
        Self {
            origin,
            resolved_origin,
            session,
            csrf,
            include_origin,
            include_csrf,
            has_body,
        }
    }
}
