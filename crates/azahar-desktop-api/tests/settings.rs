#[allow(dead_code)]
mod common;
#[allow(dead_code)]
mod support;

use std::sync::Arc;

use azahar_desktop_api::{
    Client, ClientError, Environment, FALLBACK_ORIGIN, MemorySecretStore, Settings, WindowBounds,
    go_platform,
};
use support::{MockResponse, MockServer};

fn build(profile: &std::path::Path, environment: Environment) -> Client {
    Client::with_environment_and_secret_store(
        "2.0.0-test",
        profile,
        environment,
        Arc::new(MemorySecretStore::new()),
    )
    .expect("client")
}

#[test]
fn settings_defaults() {
    let test = common::test_client("");
    let info = test.client.info();
    assert_eq!(info.version, "2.0.0-test");
    assert_eq!(info.platform, go_platform());
    assert_eq!(info.api_url, azahar_desktop_api::DEFAULT_API_URL);
    assert_eq!(info.origin, "");
    assert_eq!(info.resolved_origin, "");
    assert!(info.network_allowed);
    let bounds = test.client.window_bounds();
    assert_eq!(bounds.width, azahar_desktop_api::DEFAULT_WINDOW_WIDTH);
    assert_eq!(bounds.height, azahar_desktop_api::DEFAULT_WINDOW_HEIGHT);
}

#[test]
fn settings_env_overrides() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let saved = Settings {
        api_url: "https://saved.example:8443".to_owned(),
        origin: "http://saved-origin.example".to_owned(),
        network_allowed: true,
        network_allowed_host: "saved.example".to_owned(),
        ..Settings::default()
    };
    common::write_settings(&profile, &saved);

    let client = build(
        &profile,
        Environment::default()
            .api_url("http://127.0.0.1:9090/")
            .origin("https://override.example/"),
    );
    let info = client.info();
    assert_eq!(info.api_url, "http://127.0.0.1:9090");
    assert_eq!(info.origin, "https://override.example");
    assert!(info.network_allowed, "saved network consent was lost");

    let client = build(
        &profile,
        Environment::default()
            .api_url("http://public.example")
            .origin("ftp://bad.example"),
    );
    let info = client.info();
    assert_eq!(info.api_url, saved.api_url);
    assert_eq!(info.origin, saved.origin);
    assert!(info.network_allowed);
}

#[test]
fn settings_persistence_and_modes() {
    let server = MockServer::start(|_| {
        MockResponse::json(200, r#"{"env":"test","origin":"http://localhost:3000"}"#)
    });
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    std::fs::create_dir_all(&profile).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let client = build(&profile, Environment::default());

    let result = client
        .update_settings(&format!("{}/", server.url()), "", false)
        .expect("update settings");
    assert_eq!(result.api_url, server.url());
    assert_eq!(result.origin, "");
    assert_eq!(result.resolved_origin, "http://localhost:3000");
    assert!(result.network_allowed);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&profile).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(profile.join("settings.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let stored = common::read_settings(&profile);
    assert_eq!(stored.api_url, server.url());
    assert_eq!(stored.origin, "");
    assert!(!stored.network_allowed);
    assert_eq!(stored.network_allowed_host, "");

    let reloaded = build(&profile, Environment::default());
    assert_eq!(reloaded.info().api_url, server.url());

    let bounds = WindowBounds {
        width: 1200,
        height: 800,
        x: 40,
        y: 30,
    };
    reloaded.save_window_bounds(bounds);
    assert_eq!(reloaded.window_bounds(), bounds);
    assert_eq!(common::read_settings(&profile).window, bounds);

    let again = build(&profile, Environment::default());
    assert_eq!(again.window_bounds(), bounds);
    again.save_window_bounds(WindowBounds {
        width: -1,
        height: 0,
        ..WindowBounds::default()
    });
    assert_eq!(
        again.window_bounds(),
        WindowBounds {
            width: azahar_desktop_api::DEFAULT_WINDOW_WIDTH,
            height: azahar_desktop_api::DEFAULT_WINDOW_HEIGHT,
            x: 0,
            y: 0,
        }
    );
}

#[test]
fn settings_decoding_is_lenient() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    std::fs::create_dir_all(&profile).unwrap();
    let settings_path = profile.join("settings.json");

    std::fs::write(
        &settings_path,
        r#"{"window":"broken","apiUrl":"https://saved.example","origin":"http://saved-origin.example","networkAllowedHost":"saved.example"}"#,
    )
    .unwrap();
    let client = build(&profile, Environment::default());
    let info = client.info();
    assert_eq!(info.api_url, "https://saved.example");
    assert_eq!(info.origin, "http://saved-origin.example");
    assert!(info.network_allowed);

    std::fs::write(
        &settings_path,
        r#"{"apiUrl":123,"origin":"http://ok.example","window":{"width":"bad"}}"#,
    )
    .unwrap();
    let client = build(&profile, Environment::default());
    let info = client.info();
    assert_eq!(info.api_url, azahar_desktop_api::DEFAULT_API_URL);
    assert_eq!(info.origin, "http://ok.example");
    assert!(info.network_allowed);

    std::fs::write(&settings_path, r#"{"apiUrl":"#).unwrap();
    let client = build(&profile, Environment::default());
    let info = client.info();
    assert_eq!(info.api_url, azahar_desktop_api::DEFAULT_API_URL);
    assert_eq!(info.origin, "");
    assert!(info.network_allowed);
}

#[test]
fn update_settings_save_failure_is_friendly() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    std::fs::create_dir_all(profile.join("settings.json")).unwrap();
    let client = build(&profile, Environment::default());
    let error = client
        .update_settings("http://127.0.0.1:9090", "", false)
        .expect_err("save failure");
    assert!(
        matches!(error, ClientError::SettingsNotSaved),
        "unexpected error: {error}"
    );
    assert!(error.to_string().contains("profile folder is writable"));
}

#[test]
fn update_settings_save_failure_keeps_session() {
    let test = common::test_client("");
    test.client.seed_session("session-token");
    test.client.seed_csrf("csrf-token");
    let before = test.client.info();

    let blocker = test.profile.join("settings.json");
    std::fs::create_dir_all(&blocker).unwrap();
    let error = test
        .client
        .update_settings("http://127.0.0.1:9090", "http://admin.example", true)
        .expect_err("save failure");
    assert_eq!(
        error.to_string(),
        "The settings could not be saved. Check that the profile folder is writable."
    );
    assert_eq!(test.client.info(), before);
    assert_eq!(
        test.client.session_state(),
        ("session-token".to_owned(), "csrf-token".to_owned())
    );
}

#[test]
fn update_settings_skips_identical_values() {
    let server =
        MockServer::start(|_| MockResponse::json(200, r#"{"origin":"http://localhost:3000"}"#));
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let settings = Settings {
        api_url: server.url().to_owned(),
        ..Settings::default()
    };
    common::write_settings(&profile, &settings);
    let client = build(&profile, Environment::default());
    std::fs::remove_file(profile.join("settings.json")).unwrap();

    let result = client
        .update_settings(server.url(), "", true)
        .expect("unchanged update");
    assert_eq!(result.api_url, server.url());
    assert!(result.network_allowed);
    assert!(
        server.requests().is_empty(),
        "unchanged update probed health"
    );
    assert!(
        !profile.join("settings.json").exists(),
        "unchanged update wrote settings"
    );
}

#[test]
fn consent_is_per_host() {
    let transport = common::FailingTransport::new();
    let test = common::client_with(
        "",
        Environment::default().origin("http://admin.example"),
        Some(transport),
    );
    let origin = "http://admin.example";

    let result = test
        .client
        .update_settings("https://a.example.com", origin, true)
        .expect("consent for A");
    assert_eq!(result.api_url, "https://a.example.com");
    assert!(result.network_allowed);

    let error = test
        .client
        .update_settings("https://b.example.com", origin, false)
        .expect_err("B without consent");
    assert_eq!(
        error.api().expect("api error").code,
        "network_consent_required"
    );
    let info = test.client.info();
    assert_eq!(info.api_url, "https://a.example.com");
    assert!(info.network_allowed);

    let result = test
        .client
        .update_settings("https://b.example.com", origin, true)
        .expect("consent for B");
    assert_eq!(result.api_url, "https://b.example.com");
    assert!(result.network_allowed);

    let error = test
        .client
        .update_settings("https://a.example.com", origin, false)
        .expect_err("A after B without consent");
    assert_eq!(
        error.api().expect("api error").code,
        "network_consent_required"
    );

    let result = test
        .client
        .update_settings("http://127.0.0.1:9090", origin, false)
        .expect("loopback update");
    assert!(result.network_allowed);
    assert_eq!(
        common::read_settings(&test.profile).network_allowed_host,
        ""
    );
}

#[test]
fn settings_consent_requires_matching_host() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");

    common::write_settings(
        &profile,
        &Settings {
            api_url: "https://saved.example".to_owned(),
            network_allowed: true,
            ..Settings::default()
        },
    );
    let client = build(&profile, Environment::default());
    assert!(
        !client.info().network_allowed,
        "legacy global consent granted access"
    );

    common::write_settings(
        &profile,
        &Settings {
            api_url: "https://saved.example".to_owned(),
            network_allowed_host: "other.example".to_owned(),
            ..Settings::default()
        },
    );
    let client = build(&profile, Environment::default());
    assert!(!client.info().network_allowed);
    assert_eq!(client.settings_snapshot().network_allowed_host, "");

    common::write_settings(
        &profile,
        &Settings {
            api_url: "https://saved.example".to_owned(),
            network_allowed_host: "saved.example".to_owned(),
            ..Settings::default()
        },
    );
    let client = build(&profile, Environment::default());
    assert!(
        client.info().network_allowed,
        "matching host consent was not loaded"
    );
}

#[test]
fn legacy_electron_settings_import() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("config");
    let legacy_dir = config.join("Azahar Admin");
    std::fs::create_dir_all(&legacy_dir).unwrap();
    std::fs::write(
        legacy_dir.join("settings.json"),
        r#"{"apiUrl":"https://legacy.example.com","origin":"https://legacy.example.com","window":{"width":1200,"height":800,"x":40,"y":50}}"#,
    )
    .unwrap();
    let profile = temp.path().join("profile");
    let client = build(&profile, Environment::default().config_home(&config));
    let info = client.info();
    assert_eq!(info.api_url, "https://legacy.example.com");
    assert_eq!(info.origin, "https://legacy.example.com");
    assert_eq!(
        client.window_bounds(),
        WindowBounds {
            width: 1200,
            height: 800,
            x: 40,
            y: 50,
        }
    );
    assert!(
        profile.join("settings.json").exists(),
        "imported settings were not persisted"
    );
}

#[test]
fn consent_can_be_revoked_for_current_host() {
    let transport = common::FailingTransport::new();
    let test = common::client_with("", Environment::default(), Some(transport));
    test.client
        .update_settings("https://a.example.com", "https://a.example.com", true)
        .expect("grant consent");
    assert!(test.client.info().network_allowed);
    test.client
        .update_settings("https://a.example.com", "https://a.example.com", false)
        .expect("revoke consent");
    assert!(!test.client.info().network_allowed);
    assert_eq!(
        common::read_settings(&test.profile).network_allowed_host,
        ""
    );
}

#[test]
fn update_settings_reports_old_session_delete_failure() {
    let store = common::DeleteFailingStore::new();
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let options = azahar_desktop_api::ClientOptions::new("2.0.0-test", &profile)
        .environment(Environment::default())
        .secret_store(store)
        .transport(common::FailingTransport::new());
    let client = Client::with_options(options).unwrap();
    let result = client
        .update_settings("http://127.0.0.1:9090", "", false)
        .expect("update settings");
    assert_eq!(result.api_url, "http://127.0.0.1:9090");
    assert!(
        !result.warning.is_empty(),
        "failed old-session delete was not reported"
    );
}

#[test]
fn update_settings_falls_back_when_health_fails() {
    let server = MockServer::start(|_| {
        MockResponse::json(
            503,
            r#"{"error":{"code":"unavailable","message":"Try later.","fields":{}}}"#,
        )
    });
    let test = common::test_client("");
    let result = test
        .client
        .update_settings(server.url(), "", false)
        .expect("update settings");
    assert_eq!(result.api_url, server.url());
    assert_eq!(result.resolved_origin, FALLBACK_ORIGIN);
}

#[test]
fn settings_origin_override() {
    let server =
        MockServer::start(|_| MockResponse::json(200, r#"{"origin":"http://server.example"}"#));
    let test = common::test_client("");
    let result = test
        .client
        .update_settings(server.url(), "http://admin.example", true)
        .expect("update settings");
    assert_eq!(result.origin, "http://admin.example");
    assert_eq!(result.resolved_origin, "http://admin.example");
    assert!(result.network_allowed);
}

#[test]
fn client_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Client>();
    let _ = Arc::new(0);
}
