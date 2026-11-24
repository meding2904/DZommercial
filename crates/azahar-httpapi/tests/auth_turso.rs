//! End-to-end session test against the real local Turso/libSQL server.
//!
//! The router is driven in-process with `tower::ServiceExt::oneshot`; only the
//! database is remote. Set `AZAHAR_TEST_DATABASE_URL` to point at a different
//! libSQL server. The test fails, rather than skipping, when the server is
//! unreachable.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{Body, to_bytes};
use axum::extract::State;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use azahar_auth::{hash_password, verify_password};
use azahar_httpapi::session_layer::{
    SESSION_COOKIE_NAME, USER_ID_KEY, current_user_id, login_with_ttl, logout, session_layer,
};
use azahar_httpapi::session_store::LibsqlSessionStore;
use azahar_store::{Store, User};
use chrono::{Duration, Utc};
use libsql::{Connection, params};
use serde::Deserialize;
use serde_json::{Value, json};
use time::OffsetDateTime;
use tower::ServiceExt;
use tower_sessions::Session;
use tower_sessions::session::{Id, Record};
use tower_sessions::session_store::SessionStore;

const PASSWORD: &str = "auth-turso-password-123";
const TTL_SECONDS: i64 = 60 * 60;

const DEFAULT_DSN: &str = "http://127.0.0.1:8880";

struct AppState {
    store: Store,
    ttl: Duration,
}

#[derive(Deserialize)]
struct LoginBody {
    email: String,
    password: String,
}

async fn login_handler(
    State(state): State<Arc<AppState>>,
    session: Session,
    Json(body): Json<LoginBody>,
) -> Response {
    let user = match state.store.user_by_email(&body.email).await {
        Ok(user) => user,
        Err(_) => return unauthorized(),
    };
    if !verify_password(&body.password, &user.password_hash) {
        return unauthorized();
    }
    match login_with_ttl(&session, &user.id, state.ttl).await {
        Ok(()) => (StatusCode::OK, Json(json!({ "user_id": user.id }))).into_response(),
        Err(_) => internal_error(),
    }
}

async fn me_handler(session: Session) -> Response {
    match current_user_id(&session).await {
        Some(user_id) => (StatusCode::OK, Json(json!({ "user_id": user_id }))).into_response(),
        None => unauthorized(),
    }
}

async fn logout_handler(session: Session) -> Response {
    match logout(&session).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => internal_error(),
    }
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "unauthorized" })),
    )
        .into_response()
}

fn internal_error() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "internal_error" })),
    )
        .into_response()
}

fn build_app(state: Arc<AppState>, secure: bool) -> Router {
    let ttl = state.ttl;
    let session_store = LibsqlSessionStore::new(state.store.conn().clone());
    Router::new()
        .route("/login", post(login_handler))
        .route("/me", get(me_handler))
        .route("/logout", post(logout_handler))
        .layer(session_layer(session_store, secure, ttl))
        .with_state(state)
}

fn request(method: Method, uri: &str, cookie: Option<&str>, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, format!("{SESSION_COOKIE_NAME}={cookie}"));
    }
    match body {
        Some(body) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .expect("build request"),
        None => builder.body(Body::empty()).expect("build request"),
    }
}

fn set_cookie_headers(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok().map(str::to_owned))
        .collect()
}

fn session_cookie(headers: &HeaderMap) -> String {
    for raw in set_cookie_headers(headers) {
        if let Some(rest) = raw.strip_prefix(&format!("{SESSION_COOKIE_NAME}=")) {
            let value = rest.split(';').next().unwrap_or_default().trim();
            return value.to_string();
        }
    }
    panic!(
        "response has no {SESSION_COOKIE_NAME} cookie: {:?}",
        set_cookie_headers(headers)
    );
}

async fn json_body(response: Response) -> Value {
    let bytes = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("json response")
}

async fn session_row(conn: &Connection, id: &str) -> Option<(i64, Vec<u8>)> {
    let mut rows = conn
        .query(
            "SELECT expiry, data FROM web_sessions WHERE id = ?",
            params![id],
        )
        .await
        .expect("query session row");
    let row = rows.next().await.expect("session row")?;
    Some((
        row.get(0).expect("expiry column"),
        row.get(1).expect("data column"),
    ))
}

/// Deletes every row this test could have created, even when an assertion
/// panics. The user id is embedded in the MessagePack session blob, so
/// `instr` finds those rows without a separate index column.
struct Cleanup {
    dsn: String,
    user_id: String,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        let dsn = self.dsn.clone();
        let user_id = self.user_id.clone();
        let handle = std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    eprintln!("auth_turso cleanup runtime: {err}");
                    return;
                }
            };
            let result = runtime.block_on(async move {
                let database = azahar_db::open(&dsn).await.map_err(|err| err.to_string())?;
                let conn = database.connect().map_err(|err| err.to_string())?;
                conn.execute(
                    "DELETE FROM web_sessions WHERE instr(data, ?) > 0",
                    params![user_id.clone()],
                )
                .await
                .map_err(|err| err.to_string())?;
                conn.execute("DELETE FROM users WHERE id = ?", params![user_id])
                    .await
                    .map_err(|err| err.to_string())?;
                Ok::<(), String>(())
            });
            if let Err(err) = result {
                eprintln!("auth_turso cleanup: {err}");
            }
        });
        let _ = handle.join();
    }
}

#[tokio::test]
async fn turso_backed_login_me_logout_expiry_and_rotation() {
    let dsn = std::env::var("AZAHAR_TEST_DATABASE_URL").unwrap_or_else(|_| DEFAULT_DSN.to_string());
    let database = azahar_db::open(&dsn).await.unwrap_or_else(|err| {
        panic!(
            "cannot open Turso at AZAHAR_TEST_DATABASE_URL={dsn}: {err}. \
             Start it with `make dev-db` or point the variable at a reachable libSQL server."
        )
    });
    let conn = database.connect().expect("connect");
    azahar_db::migrate(&conn).await.expect("run migrations");

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let email = format!("auth_turso_{nanos}@example.test");
    let user_id = format!("usr_auth_turso_{nanos}");

    let store = Store::new(database).expect("store");
    let now = Utc::now();
    let user = User {
        id: user_id.clone(),
        name: "Auth Turso".to_string(),
        email: email.clone(),
        password_hash: hash_password(PASSWORD).expect("hash password"),
        role: "customer".to_string(),
        theme_preset: "cellar".to_string(),
        created_at: Some(now),
        updated_at: Some(now),
    };
    store
        .create_user(store.conn(), &user)
        .await
        .expect("create fixture user");
    let _cleanup = Cleanup {
        dsn: dsn.clone(),
        user_id: user_id.clone(),
    };

    let state = Arc::new(AppState {
        store,
        ttl: Duration::seconds(TTL_SECONDS),
    });
    let app = build_app(state.clone(), false);
    let conn = state.store.conn().clone();

    // Login: cookie attributes and a stored row with an absolute deadline.
    let login = app
        .clone()
        .oneshot(request(
            Method::POST,
            "/login",
            None,
            Some(json!({"email": email, "password": PASSWORD})),
        ))
        .await
        .expect("login response");
    assert_eq!(login.status(), StatusCode::OK);
    let first_cookie = session_cookie(login.headers());
    assert!(!first_cookie.is_empty(), "login must issue a session id");

    let raw_cookie = set_cookie_headers(login.headers())
        .into_iter()
        .find(|raw| raw.starts_with(&format!("{SESSION_COOKIE_NAME}=")))
        .expect("session Set-Cookie header");
    assert!(raw_cookie.contains("HttpOnly"), "cookie: {raw_cookie}");
    assert!(raw_cookie.contains("SameSite=Lax"), "cookie: {raw_cookie}");
    assert!(raw_cookie.contains("Path=/"), "cookie: {raw_cookie}");
    assert!(!raw_cookie.contains("Secure"), "cookie: {raw_cookie}");

    let issued_at = Utc::now().timestamp();
    let (expiry, blob) = session_row(&conn, &first_cookie)
        .await
        .expect("session row exists after login");
    let expected_expiry = issued_at + TTL_SECONDS;
    assert!(
        (expiry - expected_expiry).abs() <= 5,
        "expiry {expiry} should be about {expected_expiry}"
    );

    // The row is MessagePack, not JSON, and round-trips to the same user id.
    // MessagePack stores string bytes verbatim, so the useful check is the
    // encoding shape: JSON cannot parse it and JSON key syntax is absent.
    // Decoding it back proves the user id lives in the MessagePack record.
    assert!(
        !blob.starts_with(b"{"),
        "session blob must not be a JSON object"
    );
    assert!(
        serde_json::from_slice::<Value>(&blob).is_err(),
        "session blob must not parse as JSON"
    );
    assert!(
        !blob.windows(9).any(|window| window == b"\"user_id\""),
        "session blob contains JSON key syntax"
    );
    let decoded: Record = rmp_serde::from_slice(&blob).expect("decode MessagePack record");
    assert_eq!(
        decoded.data.get(USER_ID_KEY).and_then(Value::as_str),
        Some(user_id.as_str())
    );

    // /me reads the same user id back through the session store.
    let me = app
        .clone()
        .oneshot(request(Method::GET, "/me", Some(&first_cookie), None))
        .await
        .expect("me response");
    assert_eq!(me.status(), StatusCode::OK);
    assert_eq!(json_body(me).await["user_id"], user_id);

    // Logout deletes the row and expires the cookie.
    let logout_response = app
        .clone()
        .oneshot(request(Method::POST, "/logout", Some(&first_cookie), None))
        .await
        .expect("logout response");
    assert_eq!(logout_response.status(), StatusCode::NO_CONTENT);
    let removal = set_cookie_headers(logout_response.headers())
        .into_iter()
        .find(|raw| raw.starts_with(&format!("{SESSION_COOKIE_NAME}=")))
        .expect("logout Set-Cookie header");
    assert!(
        removal.contains("Max-Age=0"),
        "logout must expire the cookie: {removal}"
    );
    assert!(
        session_row(&conn, &first_cookie).await.is_none(),
        "logout must delete the session row"
    );

    let unauthenticated = app
        .clone()
        .oneshot(request(Method::GET, "/me", Some(&first_cookie), None))
        .await
        .expect("me after logout");
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    // A second login rotates the id and invalidates the first one.
    let second_login = app
        .clone()
        .oneshot(request(
            Method::POST,
            "/login",
            Some(&first_cookie),
            Some(json!({"email": email, "password": PASSWORD})),
        ))
        .await
        .expect("second login response");
    assert_eq!(second_login.status(), StatusCode::OK);
    let second_cookie = session_cookie(second_login.headers());
    assert_ne!(
        first_cookie, second_cookie,
        "login must cycle the session id"
    );
    assert!(
        session_row(&conn, &second_cookie).await.is_some(),
        "rotated session is stored"
    );

    let sessions = LibsqlSessionStore::new(conn.clone());
    let old_id: Id = first_cookie.parse().expect("first cookie is a session id");
    assert!(
        sessions.load(&old_id).await.expect("load old id").is_none(),
        "the old session id must no longer load"
    );
    let old_cookie_me = app
        .clone()
        .oneshot(request(Method::GET, "/me", Some(&first_cookie), None))
        .await
        .expect("old cookie response");
    assert_eq!(old_cookie_me.status(), StatusCode::UNAUTHORIZED);

    // A row whose expiry is in the past is never accepted, even if it exists.
    let expired_id = Id::default();
    let mut expired_data = HashMap::new();
    expired_data.insert(USER_ID_KEY.to_string(), Value::String(user_id.clone()));
    let expired_record = Record {
        id: expired_id,
        data: expired_data,
        expiry_date: OffsetDateTime::now_utc() - time::Duration::hours(1),
    };
    let expired_blob = rmp_serde::to_vec(&expired_record).expect("encode expired record");
    conn.execute(
        "INSERT INTO web_sessions (id, data, expiry) VALUES (?, ?, ?)",
        params![
            expired_id.to_string(),
            expired_blob,
            expired_record.expiry_date.unix_timestamp()
        ],
    )
    .await
    .expect("insert expired row");
    assert!(
        sessions
            .load(&expired_id)
            .await
            .expect("load expired")
            .is_none(),
        "expired rows must not load"
    );
    let expired_me = app
        .clone()
        .oneshot(request(
            Method::GET,
            "/me",
            Some(&expired_id.to_string()),
            None,
        ))
        .await
        .expect("expired cookie response");
    assert_eq!(expired_me.status(), StatusCode::UNAUTHORIZED);

    let purged = sessions
        .delete_expired(Utc::now())
        .await
        .expect("delete expired rows");
    assert!(purged >= 1, "the expired row should be purged");
    assert!(
        session_row(&conn, &expired_id.to_string()).await.is_none(),
        "expired row is gone after cleanup"
    );

    // The secure flag is part of the factory contract, not the cookie name.
    let secure_app = build_app(state.clone(), true);
    let secure_login = secure_app
        .clone()
        .oneshot(request(
            Method::POST,
            "/login",
            None,
            Some(json!({"email": email, "password": PASSWORD})),
        ))
        .await
        .expect("secure login response");
    assert_eq!(secure_login.status(), StatusCode::OK);
    let secure_cookie = set_cookie_headers(secure_login.headers())
        .into_iter()
        .find(|raw| raw.starts_with(&format!("{SESSION_COOKIE_NAME}=")))
        .expect("secure Set-Cookie header");
    assert!(secure_cookie.contains("Secure"), "cookie: {secure_cookie}");
}
