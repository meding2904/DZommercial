#[allow(dead_code)]
mod common;
#[allow(dead_code)]
mod support;

use azahar_desktop_api::{
    ClientError, DEFAULT_API_URL, normalize_api_url, normalize_origin, validate_server_address,
};
use support::{MockResponse, MockServer};

#[test]
fn normalize_api_url_and_network_policy() {
    struct Case {
        name: &'static str,
        raw: &'static str,
        allow: bool,
        want_url: &'static str,
        want_code: &'static str,
    }
    let cases = [
        Case {
            name: "loopback default",
            raw: "http://127.0.0.1:8080",
            allow: false,
            want_url: "http://127.0.0.1:8080",
            want_code: "",
        },
        Case {
            name: "loopback trailing slash",
            raw: "http://127.0.0.1:8080/",
            allow: false,
            want_url: "http://127.0.0.1:8080",
            want_code: "",
        },
        Case {
            name: "loopback ipv6",
            raw: "http://[::1]:8080",
            allow: false,
            want_url: "http://[::1]:8080",
            want_code: "",
        },
        Case {
            name: "localhost",
            raw: "http://localhost:8080",
            allow: false,
            want_url: "http://localhost:8080",
            want_code: "",
        },
        Case {
            name: "private http allowed",
            raw: "http://192.168.1.10:8080",
            allow: true,
            want_url: "http://192.168.1.10:8080",
            want_code: "",
        },
        Case {
            name: "private ten network",
            raw: "http://10.20.30.40:8080",
            allow: true,
            want_url: "http://10.20.30.40:8080",
            want_code: "",
        },
        Case {
            name: "private http needs consent",
            raw: "http://192.168.1.10:8080",
            allow: false,
            want_url: "",
            want_code: "network_consent_required",
        },
        Case {
            name: "local name http allowed",
            raw: "http://api.local:8080",
            allow: true,
            want_url: "http://api.local:8080",
            want_code: "",
        },
        Case {
            name: "link local ipv6 http allowed",
            raw: "http://[fe80::1%25eth0]:8080",
            allow: true,
            want_url: "http://[fe80::1%25eth0]:8080",
            want_code: "",
        },
        Case {
            name: "public https allowed",
            raw: "https://api.example.com",
            allow: true,
            want_url: "https://api.example.com",
            want_code: "",
        },
        Case {
            name: "public https needs consent",
            raw: "https://api.example.com",
            allow: false,
            want_url: "",
            want_code: "network_consent_required",
        },
        Case {
            name: "public http rejected",
            raw: "http://api.example.com",
            allow: true,
            want_url: "",
            want_code: "invalid_api_url",
        },
        Case {
            name: "public http rejected without consent",
            raw: "http://api.example.com",
            allow: false,
            want_url: "",
            want_code: "invalid_api_url",
        },
        Case {
            name: "credentials rejected",
            raw: "https://user:pass@api.example.com",
            allow: true,
            want_url: "",
            want_code: "invalid_api_url",
        },
        Case {
            name: "path rejected",
            raw: "https://api.example.com/api",
            allow: true,
            want_url: "",
            want_code: "invalid_api_url",
        },
        Case {
            name: "query rejected",
            raw: "https://api.example.com?x=1",
            allow: true,
            want_url: "",
            want_code: "invalid_api_url",
        },
        Case {
            name: "fragment rejected",
            raw: "https://api.example.com#x",
            allow: true,
            want_url: "",
            want_code: "invalid_api_url",
        },
        Case {
            name: "empty fragment rejected",
            raw: "https://api.example.com#",
            allow: true,
            want_url: "",
            want_code: "invalid_api_url",
        },
        Case {
            name: "protocol relative rejected",
            raw: "//api.example.com",
            allow: true,
            want_url: "",
            want_code: "invalid_api_url",
        },
        Case {
            name: "scheme trick rejected",
            raw: "http:////api.example.com",
            allow: true,
            want_url: "",
            want_code: "invalid_api_url",
        },
        Case {
            name: "opaque scheme rejected",
            raw: "http:\\\\api.example.com",
            allow: true,
            want_url: "",
            want_code: "invalid_api_url",
        },
        Case {
            name: "ftp rejected",
            raw: "ftp://api.example.com",
            allow: true,
            want_url: "",
            want_code: "invalid_api_url",
        },
        Case {
            name: "empty rejected",
            raw: "  ",
            allow: false,
            want_url: "",
            want_code: "invalid_api_url",
        },
        Case {
            name: "backslash rejected",
            raw: "https://api.example.com\\@evil.com",
            allow: true,
            want_url: "",
            want_code: "invalid_api_url",
        },
        Case {
            name: "public loopback name rejected",
            raw: "http://127.0.0.1.nip.io:8080",
            allow: true,
            want_url: "",
            want_code: "invalid_api_url",
        },
    ];

    for case in cases {
        let normalized = normalize_api_url(case.raw);
        let error = match &normalized {
            Ok(url) => validate_server_address(url, case.allow),
            Err(error) => Some(error.clone()),
        };
        if case.want_code.is_empty() {
            assert!(error.is_none(), "{}: unexpected error {error:?}", case.name);
            assert_eq!(
                normalized.as_deref().ok(),
                Some(case.want_url),
                "{}",
                case.name
            );
        } else {
            let error =
                error.unwrap_or_else(|| panic!("{}: expected {}", case.name, case.want_code));
            assert_eq!(
                error.code, case.want_code,
                "{}: {}",
                case.name, error.message
            );
            assert!(!error.message.is_empty(), "{}: empty message", case.name);
        }
    }
}

#[test]
fn update_settings_requires_network_consent() {
    let test = common::test_client("");
    let error = test
        .client
        .update_settings("https://api.example.com", "", false)
        .expect_err("consent error");
    let api = error.api().expect("api error");
    assert_eq!(api.code, "network_consent_required");
    assert_eq!(test.client.info().api_url, DEFAULT_API_URL);

    let error = test
        .client
        .update_settings("http://api.example.com", "", true)
        .expect_err("public http error");
    assert_eq!(error.api().expect("api error").code, "invalid_api_url");
    assert!(matches!(error, ClientError::Api(_)));
}

#[test]
fn normalize_origin_table() {
    let cases = [
        ("", ""),
        ("http://localhost:3000", "http://localhost:3000"),
        ("http://localhost:3000/", "http://localhost:3000"),
        ("https://admin.example.com/app", "https://admin.example.com"),
        ("https://admin.example.com:443", "https://admin.example.com"),
    ];
    for (input, want) in cases {
        assert_eq!(
            normalize_origin(input).as_deref().ok(),
            Some(want),
            "{input}"
        );
    }
    for input in [
        "//admin.example.com",
        "ftp://admin.example.com",
        "https://user@admin.example.com",
    ] {
        assert!(normalize_origin(input).is_err(), "{input}");
    }
}

#[test]
fn request_rejects_unsafe_paths() {
    let server = MockServer::start(|_| MockResponse::json(200, r#"{"ok":true}"#));
    let test = common::test_client(server.url());
    let mut paths = vec![
        String::new(),
        "/api".to_owned(),
        "/api/../admin".to_owned(),
        "/api/%2e%2e/admin".to_owned(),
        "/api/%2E%2E/admin".to_owned(),
        "/api/..%2f..%2fetc".to_owned(),
        "/api/%2f%2fevil.com".to_owned(),
        "/api/%252e%252e/admin".to_owned(),
        "/api/x%5Cy".to_owned(),
        "/api/x\\y".to_owned(),
        "/api/x\0y".to_owned(),
        "//evil.com/api/x".to_owned(),
        "http://evil.com/api/x".to_owned(),
        "/other/x".to_owned(),
    ];
    paths.push(format!(
        "/api/{}",
        "a".repeat(azahar_desktop_api::MAX_API_PATH_LENGTH)
    ));

    for path in paths {
        let response = test.client.request("GET", &path, None);
        assert!(!response.ok, "path {path:?} was accepted");
        assert_eq!(
            response.error.as_ref().map(|error| error.code.as_str()),
            Some("invalid_path"),
            "path {path:?}"
        );
    }
    assert!(
        server.requests().is_empty(),
        "unsafe paths reached the server"
    );
}

#[test]
fn request_allows_query_strings() {
    let server = MockServer::start(|_| MockResponse::json(200, r#"{"ok":true}"#));
    let test = common::test_client(server.url());
    let response = test
        .client
        .request("GET", "/api/ok?q=%2e%2e%2f&page=2", None);
    assert!(response.ok, "{:?}", response.error);
    let requests = server.requests();
    assert_eq!(requests[0].path, "/api/ok");
    assert_eq!(requests[0].raw_query, "q=%2e%2e%2f&page=2");
}

#[test]
fn request_rejects_unknown_method() {
    let server = MockServer::start(|_| MockResponse::json(200, "{}"));
    let test = common::test_client(server.url());
    let response = test.client.request("TRACE", "/api/ok", None);
    assert!(!response.ok);
    assert_eq!(response.error.expect("error").code, "invalid_method");
    assert!(
        server.requests().is_empty(),
        "unknown method reached the server"
    );
}
