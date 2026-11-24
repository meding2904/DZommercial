#[allow(dead_code)]
mod common;
#[allow(dead_code)]
mod support;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use azahar_desktop_api::{
    Client, ClientOptions, Environment, FallbackSecretStore, FileSecretStore, MemorySecretStore,
    SecretStore, TimeoutSecretStore, default_secret_store, env_enabled, session_scope,
};
use support::{MockResponse, MockServer};

fn session_cookie(value: &str) -> String {
    format!("azahar_session={value}; Path=/")
}

#[test]
fn cookie_scoping_and_deletion() {
    let calls = Arc::new(AtomicUsize::new(0));
    let cookies = Arc::new(Mutex::new(HashMap::<usize, String>::new()));
    let state = Arc::clone(&calls);
    let recorded = Arc::clone(&cookies);
    let server = MockServer::start(move |request| {
        let call = state.fetch_add(1, Ordering::SeqCst) + 1;
        recorded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(call, request.header("cookie"));
        match call {
            1 => MockResponse::json(200, "{}")
                .with_set_cookie("azahar_session=session-one; Path=/; HttpOnly")
                .with_set_cookie("azahar_csrf=csrf-one; Path=/")
                .with_set_cookie("other=not-ours; Path=/")
                .with_set_cookie("azahar_extra=not-ours; Path=/"),
            3 => MockResponse::json(200, "{}")
                .with_set_cookie("azahar_session=; Max-Age=-1; Path=/")
                .with_set_cookie("azahar_csrf=; Max-Age=-1; Path=/"),
            _ => MockResponse::json(200, "{}"),
        }
    });
    let test = common::test_client(server.url());
    for index in 0..4 {
        let response = test.client.request("GET", "/api/cookies", None);
        assert!(response.ok, "request {}: {:?}", index + 1, response.error);
    }

    let cookies = cookies.lock().unwrap();
    assert_eq!(cookies[&1], "");
    assert_eq!(
        cookies[&2],
        "azahar_session=session-one; azahar_csrf=csrf-one"
    );
    assert_eq!(cookies[&4], "", "cookie was sent after deletion");
    assert_eq!(
        test.secrets.stored(),
        None,
        "session was persisted after deletion"
    );
}

#[test]
fn only_session_is_persisted() {
    let cookies = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorded = Arc::clone(&cookies);
    let server = MockServer::start(move |request| {
        recorded.lock().unwrap().push(request.header("cookie"));
        MockResponse::json(200, "{}")
            .with_set_cookie("azahar_session=session-one; Path=/")
            .with_set_cookie("azahar_csrf=csrf-one; Path=/")
    });
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let secrets = Arc::new(MemorySecretStore::new());

    let first = Client::with_environment_and_secret_store(
        "2.0.0-test",
        &profile,
        Environment::default().api_url(server.url()).origin(""),
        secrets.clone(),
    )
    .unwrap();
    let response = first.request("GET", "/api/cookies", None);
    assert!(response.ok, "{:?}", response.error);
    assert_eq!(secrets.stored().as_deref(), Some("session-one"));

    let second = Client::with_environment_and_secret_store(
        "2.0.0-test",
        &profile,
        Environment::default().api_url(server.url()).origin(""),
        secrets.clone(),
    )
    .unwrap();
    let response = second.request("GET", "/api/cookies", None);
    assert!(response.ok, "{:?}", response.error);

    let cookies = cookies.lock().unwrap();
    assert_eq!(cookies.len(), 2);
    assert_eq!(cookies[1], "azahar_session=session-one");
}

#[test]
fn expired_cookie_is_not_stored() {
    let cookies = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorded = Arc::clone(&cookies);
    let server = MockServer::start(move |request| {
        recorded.lock().unwrap().push(request.header("cookie"));
        MockResponse::json(200, "{}").with_set_cookie(
            "azahar_session=stale-session; Path=/; Expires=Wed, 21 Oct 2015 07:28:00 GMT",
        )
    });
    let test = common::test_client(server.url());
    assert!(test.client.request("GET", "/api/cookies", None).ok);
    assert!(test.client.request("GET", "/api/cookies", None).ok);
    let cookies = cookies.lock().unwrap();
    assert_eq!(cookies[1], "", "expired cookie was sent");
    assert_eq!(test.secrets.stored(), None, "expired cookie was persisted");
}

#[test]
fn session_file_mode_and_contents() {
    let server = MockServer::start(|_| {
        MockResponse::json(200, "{}")
            .with_set_cookie("azahar_session=session-file; Path=/")
            .with_set_cookie("azahar_csrf=csrf-memory-only; Path=/")
    });
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let session_path = profile.join("session.json");
    let store = Arc::new(FileSecretStore::new(&session_path));
    let client = Client::with_environment_and_secret_store(
        "2.0.0-test",
        &profile,
        Environment::default().api_url(server.url()).origin(""),
        store,
    )
    .unwrap();
    assert!(client.request("GET", "/api/cookies", None).ok);

    let metadata = std::fs::metadata(&session_path).expect("session file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    }
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&session_path).unwrap()).unwrap();
    assert_eq!(record.as_object().unwrap().len(), 1);
    assert_eq!(record["azahar_session"], "session-file");
}

#[test]
fn keyring_failure_falls_back_to_file() {
    let server = MockServer::start(|_| {
        MockResponse::json(200, "{}").with_set_cookie("azahar_session=fallback-session; Path=/")
    });
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let session_path = profile.join("session.json");
    let store: Arc<dyn SecretStore> = Arc::new(FallbackSecretStore::new(
        Some(Arc::new(MemorySecretStore::failing())),
        Arc::new(FileSecretStore::new(&session_path)),
    ));
    let client = Client::with_environment_and_secret_store(
        "2.0.0-test",
        &profile,
        Environment::default().api_url(server.url()).origin(""),
        store.clone(),
    )
    .unwrap();
    assert!(client.request("GET", "/api/cookies", None).ok);

    let metadata = std::fs::metadata(&session_path).expect("fallback session file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    }

    let second = Client::with_environment_and_secret_store(
        "2.0.0-test",
        &profile,
        Environment::default().api_url(server.url()).origin(""),
        store.clone(),
    )
    .unwrap();
    assert_eq!(store.load().unwrap().as_deref(), Some("fallback-session"));
    assert!(
        second.clear_session().is_err(),
        "keyring delete failure was hidden"
    );
    assert!(
        !session_path.exists(),
        "fallback file still exists after clear"
    );
}

#[test]
fn clear_session_removes_secret() {
    let server = MockServer::start(|_| {
        MockResponse::json(200, "{}").with_set_cookie(session_cookie("session-one"))
    });
    let test = common::test_client(server.url());
    assert!(test.client.request("GET", "/api/session", None).ok);
    assert_eq!(test.secrets.stored().as_deref(), Some("session-one"));
    test.client.clear_session().expect("clear session");
    assert_eq!(test.secrets.stored(), None);
}

#[test]
fn update_settings_clears_session() {
    let first = MockServer::start(|request| {
        if request.path == "/api/health" {
            return MockResponse::json(200, r#"{"origin":"http://localhost:3000"}"#);
        }
        MockResponse::json(200, "{}").with_set_cookie(session_cookie("old-session"))
    });
    let second = MockServer::start(|request| {
        if request.path == "/api/health" {
            return MockResponse::json(200, r#"{"origin":"http://localhost:3000"}"#);
        }
        MockResponse::json(200, "{}")
    });
    let test = common::test_client(first.url());
    assert!(test.client.request("GET", "/api/session", None).ok);
    assert!(test.secrets.stored().is_some());

    let result = test
        .client
        .update_settings(second.url(), "", false)
        .expect("update settings");
    assert_eq!(result.api_url, second.url());
    assert_eq!(
        test.secrets.stored(),
        None,
        "session survived server change"
    );
}

#[test]
fn timed_secret_store_times_out() {
    let store = common::BlockingStore::new();
    let timed = TimeoutSecretStore::new(store.clone(), common::timeout());
    let started = SystemTime::now();
    assert_eq!(
        timed.load().expect_err("load timeout").message,
        "secret store timed out"
    );
    assert!(timed.load().expect_err("load timeout").timeout);
    assert!(timed.save("value").is_err());
    assert!(timed.delete().is_err());
    store.release();
    assert!(started.elapsed().unwrap_or(Duration::ZERO) < Duration::from_secs(1));
}

#[test]
fn keyring_timeout_falls_back_to_file() {
    let store = common::BlockingStore::new();
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    std::fs::create_dir_all(&profile).unwrap();
    let session_path = profile.join("session-test.json");
    let secrets: Arc<dyn SecretStore> = Arc::new(FallbackSecretStore::new(
        Some(Arc::new(TimeoutSecretStore::new(
            store.clone(),
            common::timeout(),
        ))),
        Arc::new(FileSecretStore::new(&session_path)),
    ));

    let started = SystemTime::now();
    let client = Client::with_environment_and_secret_store(
        "2.0.0-test",
        &profile,
        Environment::default(),
        secrets.clone(),
    )
    .expect("client");
    assert!(started.elapsed().unwrap_or_default() < Duration::from_secs(1));

    secrets.save("timeout-session").expect("fallback save");
    let metadata = std::fs::metadata(&session_path).expect("session file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    }

    let reloaded = Client::with_environment_and_secret_store(
        "2.0.0-test",
        &profile,
        Environment::default(),
        secrets.clone(),
    )
    .expect("reload");
    assert_eq!(reloaded.session_state().0, "timeout-session");
    let error = reloaded.clear_session().expect_err("clear session");
    assert!(error.to_string().contains("timed out"), "{error}");
    assert!(
        !session_path.exists(),
        "session file still exists after clear"
    );
    store.release();
    let _ = client;
}

#[test]
fn clear_session_returns_delete_error() {
    let store = common::DeleteFailingStore::new();
    let temp = tempfile::tempdir().unwrap();
    let client =
        Client::with_secret_store("2.0.0-test", temp.path().join("profile"), store).unwrap();
    client.seed_session("session-one");
    client.seed_csrf("csrf-one");
    let error = client.clear_session().expect_err("delete error");
    assert!(error.to_string().contains("delete failed"));
    assert_eq!(client.session_state(), (String::new(), String::new()));
}

#[test]
fn session_scope_is_profile_and_server_specific() {
    let root = tempfile::tempdir().unwrap();
    let profile_a = root.path().join("profile-a");
    let profile_b = root.path().join("profile-b");
    let url_a = "https://a.example.com";
    let url_b = "https://b.example.com";

    let scope_a = session_scope(&profile_a, url_a);
    let scope_b = session_scope(&profile_a, url_b);
    let scope_profile_b = session_scope(&profile_b, url_a);
    assert_ne!(scope_a, scope_b);
    assert_ne!(scope_a, scope_profile_b);
    assert_ne!(scope_b, scope_profile_b);
    assert!(scope_a.starts_with("session-"));
    assert_eq!(scope_a.len(), "session-".len() + 16);
    assert_eq!(session_scope(&profile_a, &format!("{url_a}/")), scope_a);
    assert_eq!(
        session_scope(&profile_a, "https://A.EXAMPLE.COM:443"),
        scope_a
    );

    let environment = Environment::default().session_backend("file");
    let store = default_secret_store(&profile_a, url_a, &environment);
    store.save("file-session").expect("save file store");
    assert_eq!(store.load().unwrap().as_deref(), Some("file-session"));
    assert!(profile_a.join(format!("{scope_a}.json")).exists());

    let other = default_secret_store(&profile_a, url_b, &environment);
    assert_eq!(other.load().unwrap(), None);
    let other_profile = default_secret_store(&profile_b, url_a, &environment);
    assert_eq!(other_profile.load().unwrap(), None);

    assert!(!env_enabled(None));
    assert!(!env_enabled(Some("")));
    assert!(!env_enabled(Some("0")));
    assert!(!env_enabled(Some("false")));
    assert!(!env_enabled(Some("no")));
    assert!(!env_enabled(Some("off")));
    assert!(env_enabled(Some("1")));
    assert!(env_enabled(Some("yes")));
}

#[test]
fn session_stores_are_scoped_by_server() {
    let state = Arc::new(Mutex::new(HashMap::<String, Arc<MemorySecretStore>>::new()));
    let captured = Arc::new(Mutex::new(HashMap::<String, Vec<String>>::new()));

    let make_server = |name: &'static str, session: &'static str| {
        let captured = Arc::clone(&captured);
        MockServer::start(move |request| {
            if request.path == "/api/health" {
                return MockResponse::json(200, r#"{"origin":"http://localhost:3000"}"#);
            }
            captured
                .lock()
                .unwrap()
                .entry(name.to_owned())
                .or_default()
                .push(request.header("cookie"));
            MockResponse::json(200, "{}").with_set_cookie(session_cookie(session))
        })
    };
    let server_a = make_server("a", "session-a");
    let server_b = make_server("b", "session-b");

    let temp = tempfile::tempdir().unwrap();
    let profile = PathBuf::from(temp.path()).join("profile");
    let factory_state = Arc::clone(&state);
    let factory = move |api_url: &str| {
        let mut stores = factory_state.lock().unwrap();
        let store: Arc<dyn SecretStore> = stores
            .entry(api_url.to_owned())
            .or_insert_with(|| Arc::new(MemorySecretStore::new()))
            .clone();
        Some(store)
    };

    let client_a = Client::with_options(
        ClientOptions::new("2.0.0-test", &profile)
            .environment(Environment::default().api_url(server_a.url()).origin(""))
            .secret_store_factory(factory.clone()),
    )
    .unwrap();
    assert!(client_a.request("GET", "/api/session", None).ok);

    let client_b = Client::with_options(
        ClientOptions::new("2.0.0-test", &profile)
            .environment(Environment::default().api_url(server_b.url()).origin(""))
            .secret_store_factory(factory),
    )
    .unwrap();
    assert!(client_b.request("GET", "/api/session", None).ok);

    let store_a = {
        let stores = state.lock().unwrap();
        stores[server_a.url()].clone()
    };
    let store_b = {
        let stores = state.lock().unwrap();
        stores[server_b.url()].clone()
    };
    assert_eq!(store_a.stored().as_deref(), Some("session-a"));
    assert_eq!(store_b.stored().as_deref(), Some("session-b"));
    {
        let captured = captured.lock().unwrap();
        let last = captured["b"].last().cloned().unwrap_or_default();
        assert_eq!(last, "", "client B sent another server's session");
    }

    client_a
        .update_settings(server_b.url(), "", false)
        .expect("switch to B");
    assert_eq!(store_a.stored(), None, "old server session survived switch");
    assert_eq!(store_b.stored().as_deref(), Some("session-b"));

    captured.lock().unwrap().insert("b".to_owned(), Vec::new());
    assert!(client_a.request("GET", "/api/session", None).ok);
    {
        let captured = captured.lock().unwrap();
        let last = captured["b"].last().cloned().unwrap_or_default();
        assert_eq!(last, "", "switch loaded another server's session");
    }

    store_a.save("other-session").unwrap();
    client_a.clear_session().expect("clear session");
    assert_eq!(
        store_b.stored(),
        None,
        "clear session did not delete the current store"
    );
    assert_eq!(store_a.stored().as_deref(), Some("other-session"));
}

#[test]
fn fallback_secret_store_loads_fallback_on_primary_miss() {
    let temp = tempfile::tempdir().unwrap();
    let file = Arc::new(FileSecretStore::new(temp.path().join("session.json")));
    file.save("fallback-session").expect("save fallback");
    let store = FallbackSecretStore::new(Some(Arc::new(MemorySecretStore::new())), file);
    assert_eq!(store.load().unwrap().as_deref(), Some("fallback-session"));
}
