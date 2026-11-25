//! Differential tests for the ported store basics against the Go oracle in
//! `backend/cmd/oracle-store-basics`.
//!
//! The oracle and this test share one libSQL database. Fixtures are seeded
//! through whichever side is not under test, read through the other, and then
//! removed best-effort even when an assertion panics. The pricing section needs
//! no fixtures but does use the same oracle process.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use azahar_store::pricing::SaleWindow;
use azahar_store::{AuditEntry, CustomerRow, Order, Product, Session, Store, Timestamp, User};
use chrono::{DateTime, Duration, SecondsFormat, TimeZone, Utc};
use proptest::prelude::{Just, Strategy, any, prop_oneof};
use proptest::strategy::ValueTree;
use proptest::test_runner::{Config, RngSeed, TestRunner};
use serde_json::{Value, json};

const KNOWN_HASH: &str =
    "$argon2id$v=19$m=65536,t=3,p=4$c29tZXNhbHRmb3J0ZXN0$aGFzaGhhc2hoYXNoaGFzaA";
const PASSWORD_HASH: &str = "a-hash-known-to-the-fixture";

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn dsn() -> String {
    std::env::var("AZAHAR_TEST_DATABASE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8880".to_string())
}

fn at(year: i32, month: u32, day: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, 10, 0, 0).unwrap()
}

fn time_text(value: Timestamp) -> String {
    value.map_or_else(
        || "0001-01-01T00:00:00Z".to_string(),
        |value| value.to_rfc3339_opts(SecondsFormat::Secs, true),
    )
}

fn optional_time_value(value: Timestamp) -> Value {
    value.map_or(Value::Null, |value| Value::String(time_text(Some(value))))
}

struct Oracle {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Oracle {
    fn spawn(dsn: &str) -> Self {
        let backend = repository_root().join("backend");
        let mut child = Command::new("go")
            .args(["run", "./cmd/oracle-store-basics"])
            .current_dir(&backend)
            .env("AZAHAR_TEST_DATABASE_URL", dsn)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap_or_else(|error| {
                panic!(
                    "start Go oracle in {}: {error}\nrun `make dev-db` first",
                    backend.display()
                )
            });
        let stdin = child.stdin.take().expect("oracle stdin");
        let stdout = child.stdout.take().expect("oracle stdout");
        Self {
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
        }
    }

    fn request(&mut self, request: &Value) -> Value {
        let mut responses = self.batch(std::slice::from_ref(request));
        responses.remove(0)
    }

    fn batch(&mut self, requests: &[Value]) -> Vec<Value> {
        {
            let stdin = self.stdin.as_mut().expect("oracle stdin is open");
            for request in requests {
                serde_json::to_writer(&mut *stdin, request).expect("encode oracle request");
                stdin.write_all(b"\n").expect("write oracle request");
            }
            stdin.flush().expect("flush oracle requests");
        }

        let mut responses = Vec::with_capacity(requests.len());
        for _ in requests {
            let mut line = String::new();
            let read = self
                .stdout
                .read_line(&mut line)
                .expect("read oracle response");
            assert!(
                read > 0,
                "Go oracle exited before answering; run `make dev-db` and check that \
                 `cd backend && go build ./cmd/oracle-store-basics` succeeds"
            );
            responses.push(serde_json::from_str(&line).expect("decode oracle response"));
        }
        responses
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

fn expect_ok(response: &Value, label: &str) -> Value {
    if let Some(error) = response.get("error").and_then(Value::as_str) {
        panic!("Go oracle {label} failed: {error}");
    }
    response.clone()
}

#[derive(Default)]
struct Checker {
    comparisons: usize,
}

impl Checker {
    fn equal(&mut self, label: &str, rust: &Value, go: &Value) {
        self.comparisons += 1;
        assert_eq!(
            rust, go,
            "differential mismatch for {label}\nRust: {rust}\nGo:   {go}"
        );
    }

    fn is_true(&mut self, label: &str, value: bool) {
        self.comparisons += 1;
        assert!(value, "differential mismatch for {label}");
    }
}

fn user_value(user: &User) -> Value {
    json!({
        "id": user.id,
        "name": user.name,
        "email": user.email,
        "password_hash": user.password_hash,
        "role": user.role,
        "theme_preset": user.theme_preset,
        "created_at": time_text(user.created_at),
        "updated_at": time_text(user.updated_at),
    })
}

fn session_value(session: &Session) -> Value {
    json!({
        "id": session.id,
        "user_id": session.user_id,
        "token_hash": session.token_hash,
        "user_agent": session.user_agent,
        "ip": session.ip,
        "created_at": time_text(session.created_at),
        "expires_at": time_text(session.expires_at),
    })
}

fn customer_value(row: &CustomerRow) -> Value {
    json!({
        "id": row.id,
        "name": row.name,
        "email": row.email,
        "orders_count": row.orders_count,
        "total_spent_cents": row.total_spent_cents,
        "last_order_at": optional_time_value(row.last_order_at),
        "created_at": time_text(row.created_at),
    })
}

fn settings_value(raw: &str, updated_at: Timestamp) -> Value {
    json!({
        "found": true,
        "settings": raw,
        "updated_at": time_text(updated_at),
    })
}

fn user_create_request(user: &User) -> Value {
    json!({
        "op": "create_user",
        "at": time_text(user.created_at),
        "user": {
            "id": user.id,
            "name": user.name,
            "email": user.email,
            "password_hash": user.password_hash,
            "role": user.role,
            "theme_preset": user.theme_preset,
            "created_at": time_text(user.created_at),
            "updated_at": time_text(user.updated_at),
        },
    })
}

fn session_create_request(session: &Session) -> Value {
    json!({
        "op": "create_session",
        "session": {
            "id": session.id,
            "user_id": session.user_id,
            "token_hash": session.token_hash,
            "user_agent": session.user_agent,
            "ip": session.ip,
            "created_at": time_text(session.created_at),
            "expires_at": time_text(session.expires_at),
        },
    })
}

fn audit_create_request(entry: &AuditEntry) -> Value {
    json!({
        "op": "insert_audit",
        "entry": {
            "id": entry.id,
            "actor_user_id": entry.actor_user_id,
            "action": entry.action,
            "entity_type": entry.entity_type,
            "entity_id": entry.entity_id,
            "detail": entry.detail,
            "created_at": time_text(entry.created_at),
        },
    })
}

async fn open_store(dsn: &str) -> Store {
    let database = match azahar_db::open(dsn).await {
        Ok(database) => database,
        Err(error) => panic!(
            "cannot reach the test database at {dsn}: {error}\n\
             run `make dev-db` first; this differential test does not skip when the \
             database is down"
        ),
    };
    let conn = database.connect().expect("connect to the test database");
    azahar_db::migrate(&conn)
        .await
        .expect("apply store migrations");
    Store::new(database).expect("build store")
}

/// Fixture ids registered as they are created. The explicit `cleanup` uses the
/// test's store; `Drop` repeats the deletes from a fresh thread when the test
/// panics before cleanup runs.
#[derive(Clone, Default)]
struct Fixtures {
    dsn: String,
    user_ids: Vec<String>,
    session_hashes: Vec<String>,
    order_ids: Vec<String>,
    plugin_settings_ids: Vec<String>,
    plugin_state_ids: Vec<String>,
    audit_ids: Vec<String>,
    storefront_backup: Option<(String, String)>,
    storefront_touched: bool,
    cleaned: bool,
}

impl Fixtures {
    fn new(dsn: &str) -> Self {
        Self {
            dsn: dsn.to_string(),
            user_ids: Vec::new(),
            session_hashes: Vec::new(),
            order_ids: Vec::new(),
            plugin_settings_ids: Vec::new(),
            plugin_state_ids: Vec::new(),
            audit_ids: Vec::new(),
            storefront_backup: None,
            storefront_touched: false,
            cleaned: false,
        }
    }

    async fn cleanup(&mut self, store: &Store) -> Result<(), String> {
        if self.cleaned {
            return Ok(());
        }
        let failures = cleanup_plan(store.conn(), self).await;
        if !failures.is_empty() {
            return Err(failures.join("; "));
        }
        self.cleaned = true;
        Ok(())
    }
}

impl Drop for Fixtures {
    fn drop(&mut self) {
        if self.cleaned {
            return;
        }
        let mut plan = self.clone();
        // The clone is only a work list for the cleanup thread. Mark it
        // cleaned so dropping it after the thread finishes cannot recurse
        // into another cleanup pass.
        plan.cleaned = true;
        let handle = std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    eprintln!("store fixture cleanup runtime: {error}");
                    return;
                }
            };
            runtime.block_on(async move {
                let database = match azahar_db::open(&plan.dsn).await {
                    Ok(database) => database,
                    Err(error) => {
                        eprintln!("store fixture cleanup could not open {}: {error}", plan.dsn);
                        return;
                    }
                };
                let conn = match database.connect() {
                    Ok(conn) => conn,
                    Err(error) => {
                        eprintln!("store fixture cleanup connect: {error}");
                        return;
                    }
                };
                let failures = cleanup_plan(&conn, &plan).await;
                if !failures.is_empty() {
                    eprintln!("store fixture cleanup failures: {}", failures.join("; "));
                }
            });
        });
        let _ = handle.join();
    }
}

async fn delete_where(
    conn: &libsql::Connection,
    table: &str,
    column: &str,
    value: &str,
) -> Result<(), libsql::Error> {
    conn.execute(&format!("DELETE FROM {table} WHERE {column} = ?"), [value])
        .await
        .map(|_| ())
}

async fn cleanup_plan(conn: &libsql::Connection, plan: &Fixtures) -> Vec<String> {
    let mut failures = Vec::new();

    macro_rules! attempt {
        ($future:expr, $label:expr) => {
            if let Err(error) = $future.await {
                failures.push(format!("{}: {error}", $label));
            }
        };
    }

    for hash in &plan.session_hashes {
        attempt!(
            delete_where(conn, "sessions", "token_hash", hash),
            "delete session"
        );
    }
    for user_id in &plan.user_ids {
        attempt!(
            delete_where(conn, "sessions", "user_id", user_id),
            "delete sessions for user"
        );
    }
    for id in &plan.order_ids {
        attempt!(delete_where(conn, "orders", "id", id), "delete order");
    }
    for id in &plan.user_ids {
        attempt!(delete_where(conn, "users", "id", id), "delete user");
    }
    for id in &plan.plugin_settings_ids {
        attempt!(
            delete_where(conn, "plugin_settings", "plugin_id", id),
            "delete plugin settings"
        );
    }
    for id in &plan.plugin_state_ids {
        attempt!(
            delete_where(conn, "plugin_states", "plugin_id", id),
            "delete plugin state"
        );
    }
    for id in &plan.audit_ids {
        attempt!(
            delete_where(conn, "audit_outbox", "id", id),
            "delete audit entry"
        );
    }
    if plan.storefront_touched {
        match &plan.storefront_backup {
            Some((data, updated_at)) => {
                attempt!(
                    conn.execute(
                        "INSERT INTO storefront_settings (id, data, updated_at)
                         VALUES ('default', ?, ?)
                         ON CONFLICT(id) DO UPDATE SET
                           data = excluded.data,
                           updated_at = excluded.updated_at",
                        [data.as_str(), updated_at.as_str()],
                    ),
                    "restore storefront settings"
                );
            }
            None => {
                attempt!(
                    conn.execute("DELETE FROM storefront_settings WHERE id = 'default'", ()),
                    "delete storefront settings"
                );
            }
        }
    }

    failures
}

async fn insert_order(
    store: &Store,
    id: &str,
    number: &str,
    user: &User,
    total_cents: i64,
    status: &str,
    created_at: DateTime<Utc>,
) {
    let created = time_text(Some(created_at));
    let token_hash = format!("itb-order-token-{id}");
    store
        .conn()
        .execute(
            "INSERT INTO orders (
                 id, number, user_id, name, email, phone, address, city, notes,
                 subtotal_cents, shipping_cents, total_cents, currency, status,
                 token_hash, created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, '', ?, 0, ?, 'DZD', ?, ?, ?, ?)",
            (
                id,
                number,
                user.id.as_str(),
                user.name.as_str(),
                user.email.as_str(),
                "0550000000",
                "1 Test Street",
                "Alger",
                total_cents,
                total_cents,
                status,
                token_hash,
                created.as_str(),
                created.as_str(),
            ),
        )
        .await
        .expect("insert order fixture");
}

async fn audit_row(store: &Store, id: &str) -> Vec<String> {
    let mut rows = store
        .conn()
        .query(
            "SELECT id, actor_user_id, action, entity_type, entity_id, detail, created_at
             FROM audit_outbox WHERE id = ?",
            [id],
        )
        .await
        .expect("query audit row");
    let row = rows
        .next()
        .await
        .expect("audit cursor")
        .expect("audit row for fixture");
    (0..7)
        .map(|index| {
            row.get::<Option<String>>(index)
                .expect("audit column")
                .unwrap_or_default()
        })
        .collect()
}

#[tokio::test]
async fn differential_store_basics_match_go() {
    let dsn = dsn();
    let mut fixtures = Fixtures::new(&dsn);
    let store = open_store(&dsn).await;
    let mut oracle = Oracle::spawn(&dsn);
    let mut checker = Checker::default();

    // Other suites may write to the shared dev database while this test runs.
    // Reads that scan a whole table are retried a couple of times so a row
    // inserted between the Go read and the Rust read cannot flake the run; a
    // real ordering or value difference persists through every attempt.
    macro_rules! compare_read {
        ($label:expr, $request:expr, $rust:expr) => {{
            let mut last_go = Value::Null;
            let mut last_rust = Value::Null;
            for attempt in 0..3 {
                last_go = expect_ok(&oracle.request(&$request), $label);
                last_rust = $rust;
                if last_rust == last_go {
                    break;
                }
                if attempt < 2 {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
            checker.equal($label, &last_rust, &last_go);
        }};
    }

    let suffix = format!(
        "{}_{}",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );

    let admin = User {
        id: format!("usr_itb_admin_{suffix}"),
        name: "IT Basics Admin".to_string(),
        email: format!("itb.admin.{suffix}@basics.test"),
        password_hash: KNOWN_HASH.to_string(),
        role: "admin".to_string(),
        theme_preset: String::new(),
        created_at: Some(at(2026, 1, 1)),
        updated_at: Some(at(2026, 1, 1)),
    };
    let employee = User {
        id: format!("usr_itb_employee_{suffix}"),
        name: "IT Basics Employee".to_string(),
        email: format!("itb.employee.{suffix}@basics.test"),
        password_hash: PASSWORD_HASH.to_string(),
        role: "employee".to_string(),
        theme_preset: "midnight".to_string(),
        created_at: Some(at(2026, 1, 2)),
        updated_at: Some(at(2026, 1, 2)),
    };
    let customer = User {
        id: format!("usr_itb_customer_{suffix}"),
        name: "IT Basics Customer".to_string(),
        email: format!("itb.customer.{suffix}@basics.test"),
        password_hash: PASSWORD_HASH.to_string(),
        role: "customer".to_string(),
        theme_preset: "cellar".to_string(),
        created_at: Some(at(2026, 1, 3)),
        updated_at: Some(at(2026, 1, 3)),
    };
    let customer2 = User {
        id: format!("usr_itb_customer2_{suffix}"),
        name: "IT Basics Customer Two".to_string(),
        email: format!("itb.customer2.{suffix}@basics.test"),
        password_hash: PASSWORD_HASH.to_string(),
        role: "customer".to_string(),
        theme_preset: "citrus".to_string(),
        created_at: Some(at(2026, 1, 4)),
        updated_at: Some(at(2026, 1, 4)),
    };

    let session_admin_alpha = Session {
        id: format!("ses_itb_a1_{suffix}"),
        user_id: admin.id.clone(),
        token_hash: format!("itb-token-a1-{suffix}"),
        user_agent: "agent alpha".to_string(),
        ip: "10.0.0.1".to_string(),
        created_at: Some(at(2026, 2, 1)),
        expires_at: Some(at(2027, 1, 1)),
    };
    let session_admin_alpha_expired = Session {
        id: format!("ses_itb_a2_{suffix}"),
        user_id: admin.id.clone(),
        token_hash: format!("itb-token-a2-{suffix}"),
        user_agent: "agent alpha".to_string(),
        ip: "10.0.0.2".to_string(),
        created_at: Some(at(2020, 1, 1)),
        expires_at: Some(at(2020, 1, 2)),
    };
    let session_admin_beta = Session {
        id: format!("ses_itb_a3_{suffix}"),
        user_id: admin.id.clone(),
        token_hash: format!("itb-token-a3-{suffix}"),
        user_agent: "agent beta".to_string(),
        ip: "10.0.0.3".to_string(),
        created_at: Some(at(2026, 2, 3)),
        expires_at: Some(at(2027, 2, 3)),
    };
    let session_admin_gamma = Session {
        id: format!("ses_itb_a4_{suffix}"),
        user_id: admin.id.clone(),
        token_hash: format!("itb-token-a4-{suffix}"),
        user_agent: "agent gamma".to_string(),
        ip: "10.0.0.4".to_string(),
        created_at: Some(at(2026, 2, 4)),
        expires_at: Some(at(2026, 6, 1)),
    };
    let session_employee_alpha = Session {
        id: format!("ses_itb_e1_{suffix}"),
        user_id: employee.id.clone(),
        token_hash: format!("itb-token-e1-{suffix}"),
        user_agent: "agent alpha".to_string(),
        ip: "10.0.0.5".to_string(),
        created_at: Some(at(2026, 2, 5)),
        expires_at: Some(at(2027, 2, 5)),
    };

    let plugin_id = format!("itb_plugin_{suffix}");
    let plugin_insert_id = format!("itb_plugin_insert_{suffix}");
    let plugin_oracle_id = format!("itb_plugin_oracle_{suffix}");
    let plugin_state_a = format!("itb_state_a_{suffix}");
    let plugin_state_b = format!("itb_state_b_{suffix}");
    let plugin_state_oracle = format!("itb_state_oracle_{suffix}");
    let audit_oracle_id = format!("itb_audit_oracle_{suffix}");
    let audit_rust_id = format!("itb_audit_rust_{suffix}");
    let audit_detail_id = format!("itb_audit_detail_{suffix}");

    let audit_oracle = AuditEntry {
        id: audit_oracle_id.clone(),
        actor_user_id: admin.id.clone(),
        action: "oracle.create".to_string(),
        entity_type: "product".to_string(),
        entity_id: "prd_oracle".to_string(),
        detail: String::new(),
        created_at: Some(at(2026, 3, 1)),
    };
    let audit_rust = AuditEntry {
        id: audit_rust_id.clone(),
        actor_user_id: admin.id.clone(),
        action: "rust.create".to_string(),
        entity_type: "plugin".to_string(),
        entity_id: "plugin_rust".to_string(),
        detail: String::new(),
        created_at: Some(at(2026, 3, 2)),
    };
    let audit_detail = AuditEntry {
        id: audit_detail_id.clone(),
        actor_user_id: admin.id.clone(),
        action: "rust.detail".to_string(),
        entity_type: "order".to_string(),
        entity_id: "ord_rust".to_string(),
        detail: "{\"k\":1}".to_string(),
        created_at: Some(at(2026, 3, 3)),
    };

    for user in [&admin, &employee, &customer, &customer2] {
        fixtures.user_ids.push(user.id.clone());
    }
    for session in [
        &session_admin_alpha,
        &session_admin_alpha_expired,
        &session_admin_beta,
        &session_admin_gamma,
        &session_employee_alpha,
    ] {
        fixtures.session_hashes.push(session.token_hash.clone());
    }
    fixtures.plugin_settings_ids.extend([
        plugin_id.clone(),
        plugin_insert_id.clone(),
        plugin_oracle_id.clone(),
    ]);
    fixtures.plugin_state_ids.extend([
        plugin_state_a.clone(),
        plugin_state_b.clone(),
        plugin_state_oracle.clone(),
    ]);
    fixtures.audit_ids.extend([
        audit_oracle_id.clone(),
        audit_rust_id.clone(),
        audit_detail_id.clone(),
    ]);

    // Seed the users, sessions, plugin settings, plugin state and audit row
    // through the Go oracle, then read them through Rust.
    let seed_requests = vec![
        user_create_request(&admin),
        user_create_request(&employee),
        user_create_request(&customer),
        user_create_request(&customer2),
        session_create_request(&session_admin_alpha),
        session_create_request(&session_admin_alpha_expired),
        session_create_request(&session_admin_beta),
        session_create_request(&session_admin_gamma),
        session_create_request(&session_employee_alpha),
        json!({
            "op": "save_plugin_settings",
            "plugin_id": plugin_id,
            "payload": "{\"seeded\":true}",
            "now": time_text(Some(at(2026, 2, 10))),
        }),
        json!({
            "op": "set_plugin_state",
            "plugin_id": plugin_state_a,
            "enabled": false,
            "now": time_text(Some(at(2026, 2, 10))),
        }),
        audit_create_request(&audit_oracle),
    ];
    for (index, response) in oracle.batch(&seed_requests).iter().enumerate() {
        expect_ok(response, &format!("seed request {index}"));
    }

    // Orders are not part of the ported surface yet, but `ListCustomers`
    // aggregates them. Seed two paid orders plus a cancelled one for the first
    // customer and only a cancelled order for the second, so COUNT, SUM,
    // MAX and the cancelled exclusion all run through both implementations.
    let order_ids = [
        format!("ord_itb_a_{suffix}"),
        format!("ord_itb_b_{suffix}"),
        format!("ord_itb_c_{suffix}"),
        format!("ord_itb_d_{suffix}"),
    ];
    fixtures.order_ids.extend(order_ids.iter().cloned());
    insert_order(
        &store,
        &order_ids[0],
        &format!("ITB-A-{suffix}"),
        &customer,
        5_000,
        "delivered",
        at(2026, 1, 10),
    )
    .await;
    insert_order(
        &store,
        &order_ids[1],
        &format!("ITB-B-{suffix}"),
        &customer,
        2_500,
        "pending",
        at(2026, 1, 12),
    )
    .await;
    insert_order(
        &store,
        &order_ids[2],
        &format!("ITB-C-{suffix}"),
        &customer,
        9_000,
        "cancelled",
        at(2026, 1, 15),
    )
    .await;
    insert_order(
        &store,
        &order_ids[3],
        &format!("ITB-D-{suffix}"),
        &customer2,
        7_000,
        "cancelled",
        at(2026, 1, 16),
    )
    .await;

    // The storefront row is a singleton shared with `make dev-api`. Snapshot
    // it, remove it so the insert path is exercised, and put it back on
    // cleanup.
    let storefront_before = oracle.request(&json!({"op": "storefront_settings"}));
    if storefront_before["found"] == json!(true) {
        fixtures.storefront_backup = Some((
            storefront_before["settings"]
                .as_str()
                .expect("storefront settings")
                .to_string(),
            storefront_before["updated_at"]
                .as_str()
                .expect("storefront updated_at")
                .to_string(),
        ));
    }
    store
        .conn()
        .execute("DELETE FROM storefront_settings WHERE id = 'default'", ())
        .await
        .expect("clear storefront fixture");
    fixtures.storefront_touched = true;

    // ---- reads: seeded by Go, read by Rust ------------------------------

    let go = expect_ok(
        &oracle.request(&json!({"op": "user_by_email", "email": admin.email})),
        "user_by_email admin",
    );
    let rust = store
        .user_by_email(&admin.email)
        .await
        .expect("rust user_by_email");
    checker.equal("user_by_email admin", &user_value(&rust), &go);

    let go = expect_ok(
        &oracle.request(&json!({"op": "user_by_id", "id": employee.id})),
        "user_by_id employee",
    );
    let rust = store
        .user_by_id(&employee.id)
        .await
        .expect("rust user_by_id");
    checker.equal("user_by_id employee", &user_value(&rust), &go);

    let go = expect_ok(
        &oracle.request(&json!({"op": "user_by_id", "id": customer.id})),
        "user_by_id customer",
    );
    let rust = store
        .user_by_id(&customer.id)
        .await
        .expect("rust user_by_id customer");
    checker.equal("user_by_id customer", &user_value(&rust), &go);

    let missing_email = format!("itb.missing.{suffix}@basics.test");
    let go = oracle.request(&json!({"op": "user_by_email", "email": missing_email}));
    checker.equal(
        "user_by_email missing",
        &json!(
            store
                .user_by_email(&missing_email)
                .await
                .is_err_and(|error| error.is_not_found())
        ),
        &json!(go["not_found"] == json!(true)),
    );

    let missing_id = format!("usr_itb_missing_{suffix}");
    let go = oracle.request(&json!({"op": "user_by_id", "id": missing_id}));
    checker.equal(
        "user_by_id missing",
        &json!(
            store
                .user_by_id(&missing_id)
                .await
                .is_err_and(|error| error.is_not_found())
        ),
        &json!(go["not_found"] == json!(true)),
    );

    compare_read!("list_users", json!({"op": "list_users"}), {
        let users = store.list_users().await.expect("rust list_users");
        json!({"users": users.iter().map(user_value).collect::<Vec<_>>()})
    });

    compare_read!("count_users", json!({"op": "count_users"}), {
        json!({
            "count": store.count_users().await.expect("rust count_users")
        })
    });

    compare_read!("count_admins", json!({"op": "count_admins"}), {
        json!({
            "count": store
                .count_admins(store.conn())
                .await
                .expect("rust count_admins")
        })
    });

    compare_read!("list_customers", json!({"op": "list_customers"}), {
        let customers = store.list_customers().await.expect("rust list_customers");
        json!({"customers": customers.iter().map(customer_value).collect::<Vec<_>>()})
    });

    let go = expect_ok(
        &oracle.request(&json!({
            "op": "session_user_by_hash",
            "hash": session_admin_alpha.token_hash,
        })),
        "session_user_by_hash admin",
    );
    let (rust_user, rust_session, found) = store
        .session_user_by_hash(&session_admin_alpha.token_hash)
        .await
        .expect("rust session_user_by_hash");
    checker.is_true("session_user_by_hash found", found);
    checker.equal(
        "session_user_by_hash user",
        &user_value(&rust_user),
        &go["user"],
    );
    checker.equal(
        "session_user_by_hash session",
        &session_value(&rust_session),
        &go["session"],
    );

    let go = oracle.request(&json!({
        "op": "session_user_by_hash",
        "hash": format!("itb-token-missing-{suffix}"),
    }));
    let (_, _, found) = store
        .session_user_by_hash(&format!("itb-token-missing-{suffix}"))
        .await
        .expect("rust session_user_by_hash missing");
    checker.equal(
        "session_user_by_hash missing",
        &json!(found),
        &json!(go["found"]),
    );

    let go = expect_ok(
        &oracle.request(&json!({"op": "plugin_settings", "plugin_id": plugin_id})),
        "plugin_settings seeded",
    );
    let (raw, updated_at) = store
        .plugin_settings(&plugin_id)
        .await
        .expect("rust plugin_settings seeded");
    checker.equal(
        "plugin_settings seeded",
        &settings_value(&raw, updated_at),
        &go,
    );

    let missing_plugin = format!("itb_plugin_missing_{suffix}");
    let go = oracle.request(&json!({"op": "plugin_settings", "plugin_id": missing_plugin}));
    checker.equal(
        "plugin_settings missing",
        &json!(
            store
                .plugin_settings(&missing_plugin)
                .await
                .is_err_and(|error| error.is_not_found())
        ),
        &json!(go["found"] == json!(false)),
    );

    let go = expect_ok(
        &oracle.request(&json!({"op": "storefront_settings"})),
        "storefront_settings missing",
    );
    checker.equal(
        "storefront_settings missing",
        &json!(
            store
                .storefront_settings()
                .await
                .is_err_and(|error| error.is_not_found())
        ),
        &json!(go["found"] == json!(false)),
    );

    compare_read!("plugin_states seeded", json!({"op": "plugin_states"}), {
        json!({
            "states": store
                .plugin_states()
                .await
                .expect("rust plugin_states seeded")
        })
    });

    let row = audit_row(&store, &audit_oracle_id).await;
    checker.equal(
        "audit oracle insert",
        &json!({
            "id": row[0],
            "actor_user_id": row[1],
            "action": row[2],
            "entity_type": row[3],
            "entity_id": row[4],
            "detail": row[5],
            "created_at": row[6],
        }),
        &json!({
            "id": audit_oracle.id,
            "actor_user_id": audit_oracle.actor_user_id,
            "action": audit_oracle.action,
            "entity_type": audit_oracle.entity_type,
            "entity_id": audit_oracle.entity_id,
            "detail": "{}",
            "created_at": time_text(audit_oracle.created_at),
        }),
    );

    // ---- writes through Rust, reads through Go --------------------------

    let inert = User {
        id: format!("usr_itb_inert_{suffix}"),
        name: "IT Basics Inert".to_string(),
        email: format!("itb.inert.{suffix}@basics.test"),
        password_hash: PASSWORD_HASH.to_string(),
        role: "customer".to_string(),
        theme_preset: String::new(),
        created_at: Some(at(2026, 1, 5)),
        updated_at: Some(at(2026, 1, 5)),
    };
    fixtures.user_ids.push(inert.id.clone());
    store
        .create_user(store.conn(), &inert)
        .await
        .expect("rust create_user");
    let go = expect_ok(
        &oracle.request(&json!({"op": "user_by_email", "email": inert.email})),
        "user_by_email rust insert",
    );
    checker.equal(
        "create_user default preset",
        &json!("cellar"),
        &go["theme_preset"],
    );
    let rust = store
        .user_by_email(&inert.email)
        .await
        .expect("rust read own insert");
    checker.equal("create_user rust read", &user_value(&rust), &go);

    let duplicate = store.create_user(store.conn(), &inert).await;
    checker.is_true(
        "create_user duplicate is unique violation",
        duplicate.is_err_and(|error| error.is_unique_violation()),
    );
    let go_duplicate = oracle.request(&user_create_request(&inert));
    checker.is_true(
        "create_user duplicate errors in Go",
        go_duplicate.get("error").is_some(),
    );

    // Store methods take a `&Connection`, so the same call must work on the
    // transaction connection the API uses for grouped writes.
    let tx_user = User {
        id: format!("usr_itb_tx_{suffix}"),
        name: "IT Basics Transaction".to_string(),
        email: format!("itb.tx.{suffix}@basics.test"),
        password_hash: PASSWORD_HASH.to_string(),
        role: "employee".to_string(),
        theme_preset: String::new(),
        created_at: Some(at(2026, 1, 6)),
        updated_at: Some(at(2026, 1, 6)),
    };
    fixtures.user_ids.push(tx_user.id.clone());
    let tx = store
        .begin_write()
        .await
        .expect("begin write for create_user");
    store
        .create_user(tx.conn(), &tx_user)
        .await
        .expect("rust create_user in transaction");
    tx.commit().await.expect("commit create_user transaction");
    let go = expect_ok(
        &oracle.request(&json!({"op": "user_by_email", "email": tx_user.email})),
        "user_by_email transaction insert",
    );
    let rust = store
        .user_by_email(&tx_user.email)
        .await
        .expect("rust read transaction insert");
    checker.equal("create_user in transaction", &user_value(&rust), &go);

    let role_now = at(2026, 4, 1);
    store
        .update_user_role(store.conn(), &employee.id, "admin", role_now)
        .await
        .expect("rust update_user_role");
    let go = expect_ok(
        &oracle.request(&json!({"op": "user_by_id", "id": employee.id})),
        "user_by_id after role",
    );
    checker.equal("update_user_role role", &json!("admin"), &go["role"]);
    checker.equal(
        "update_user_role updated_at",
        &json!(time_text(Some(role_now))),
        &go["updated_at"],
    );

    let missing_role_id = format!("usr_itb_missing_role_{suffix}");
    let rust = store
        .update_user_role(store.conn(), &missing_role_id, "admin", role_now)
        .await;
    checker.is_true(
        "update_user_role missing is NotFound",
        rust.is_err_and(|error| error.is_not_found()),
    );
    let go = oracle.request(&json!({
        "op": "update_user_role",
        "id": missing_role_id,
        "role": "admin",
        "now": time_text(Some(role_now)),
    }));
    checker.equal(
        "update_user_role missing Go error",
        &json!(go["error"]),
        &json!("sql: no rows in result set"),
    );

    let theme_now = at(2026, 4, 2);
    store
        .update_theme(store.conn(), &admin.id, "midnight", theme_now)
        .await
        .expect("rust update_theme");
    let go = expect_ok(
        &oracle.request(&json!({"op": "user_by_id", "id": admin.id})),
        "user_by_id after theme",
    );
    checker.equal(
        "update_theme preset",
        &json!("midnight"),
        &go["theme_preset"],
    );
    checker.equal(
        "update_theme updated_at",
        &json!(time_text(Some(theme_now))),
        &go["updated_at"],
    );

    let rust = store
        .update_theme(store.conn(), &missing_role_id, "midnight", theme_now)
        .await;
    checker.is_true(
        "update_theme missing is NotFound",
        rust.is_err_and(|error| error.is_not_found()),
    );
    let go = oracle.request(&json!({
        "op": "update_theme",
        "id": missing_role_id,
        "preset": "midnight",
        "now": time_text(Some(theme_now)),
    }));
    checker.equal(
        "update_theme missing Go error",
        &json!(go["error"]),
        &json!("sql: no rows in result set"),
    );

    compare_read!("count_admins after role", json!({"op": "count_admins"}), {
        json!({
            "count": store
                .count_admins(store.conn())
                .await
                .expect("rust count_admins after role")
        })
    });

    let plugin_update_now = at(2026, 4, 3);
    store
        .save_plugin_settings(
            store.conn(),
            &plugin_id,
            "{\"seeded\":false,\"updated\":true}",
            plugin_update_now,
        )
        .await
        .expect("rust save_plugin_settings update");
    let go = expect_ok(
        &oracle.request(&json!({"op": "plugin_settings", "plugin_id": plugin_id})),
        "plugin_settings after update",
    );
    checker.equal(
        "save_plugin_settings update",
        &settings_value(
            "{\"seeded\":false,\"updated\":true}",
            Some(plugin_update_now),
        ),
        &go,
    );

    let plugin_insert_now = at(2026, 4, 4);
    store
        .save_plugin_settings(
            store.conn(),
            &plugin_insert_id,
            "{\"inserted\":true}",
            plugin_insert_now,
        )
        .await
        .expect("rust save_plugin_settings insert");
    let go = expect_ok(
        &oracle.request(&json!({"op": "plugin_settings", "plugin_id": plugin_insert_id})),
        "plugin_settings after insert",
    );
    checker.equal(
        "save_plugin_settings insert",
        &settings_value("{\"inserted\":true}", Some(plugin_insert_now)),
        &go,
    );

    let storefront_insert_now = at(2026, 4, 5);
    store
        .save_storefront_settings(store.conn(), "{\"theme\":\"first\"}", storefront_insert_now)
        .await
        .expect("rust save_storefront_settings insert");
    let go = expect_ok(
        &oracle.request(&json!({"op": "storefront_settings"})),
        "storefront_settings after insert",
    );
    checker.equal(
        "save_storefront_settings insert",
        &settings_value("{\"theme\":\"first\"}", Some(storefront_insert_now)),
        &go,
    );

    let storefront_update_now = at(2026, 4, 6);
    store
        .save_storefront_settings(
            store.conn(),
            "{\"theme\":\"second\"}",
            storefront_update_now,
        )
        .await
        .expect("rust save_storefront_settings update");
    let go = expect_ok(
        &oracle.request(&json!({"op": "storefront_settings"})),
        "storefront_settings after update",
    );
    checker.equal(
        "save_storefront_settings update",
        &settings_value("{\"theme\":\"second\"}", Some(storefront_update_now)),
        &go,
    );

    let tx = store
        .begin_write()
        .await
        .expect("begin write for storefront read");
    let (raw, updated_at) = store
        .storefront_settings_tx(tx.conn())
        .await
        .expect("rust storefront_settings_tx");
    tx.rollback().await.expect("rollback storefront read");
    checker.equal(
        "storefront_settings_tx",
        &settings_value(&raw, updated_at),
        &settings_value("{\"theme\":\"second\"}", Some(storefront_update_now)),
    );

    let state_now = at(2026, 4, 7);
    store
        .set_plugin_state(store.conn(), &plugin_state_a, true, state_now)
        .await
        .expect("rust set_plugin_state update");
    compare_read!(
        "plugin_states after update",
        json!({"op": "plugin_states"}),
        {
            json!({
                "states": store
                    .plugin_states()
                    .await
                    .expect("rust plugin_states after update")
            })
        }
    );

    let state_b_now = at(2026, 4, 8);
    store
        .set_plugin_state(store.conn(), &plugin_state_b, false, state_b_now)
        .await
        .expect("rust set_plugin_state insert");
    compare_read!(
        "plugin_states after insert",
        json!({"op": "plugin_states"}),
        {
            json!({
                "states": store
                    .plugin_states()
                    .await
                    .expect("rust plugin_states after insert")
            })
        }
    );

    store
        .insert_audit(store.conn(), &audit_rust)
        .await
        .expect("rust insert_audit default detail");
    let row = audit_row(&store, &audit_rust_id).await;
    checker.equal(
        "insert_audit default detail",
        &json!({
            "id": row[0],
            "actor_user_id": row[1],
            "action": row[2],
            "entity_type": row[3],
            "entity_id": row[4],
            "detail": row[5],
            "created_at": row[6],
        }),
        &json!({
            "id": audit_rust.id,
            "actor_user_id": audit_rust.actor_user_id,
            "action": audit_rust.action,
            "entity_type": audit_rust.entity_type,
            "entity_id": audit_rust.entity_id,
            "detail": "{}",
            "created_at": time_text(audit_rust.created_at),
        }),
    );

    store
        .insert_audit(store.conn(), &audit_detail)
        .await
        .expect("rust insert_audit detail");
    let row = audit_row(&store, &audit_detail_id).await;
    checker.equal(
        "insert_audit keeps detail",
        &json!(row[5]),
        &json!(audit_detail.detail),
    );

    let rust_session = Session {
        id: format!("ses_itb_rust_{suffix}"),
        user_id: customer.id.clone(),
        token_hash: format!("itb-token-rust-{suffix}"),
        user_agent: "rust agent".to_string(),
        ip: "10.0.0.9".to_string(),
        created_at: Some(at(2026, 4, 9)),
        expires_at: Some(at(2025, 1, 1)),
    };
    fixtures
        .session_hashes
        .push(rust_session.token_hash.clone());
    store
        .create_session(store.conn(), &rust_session)
        .await
        .expect("rust create_session");
    let go = expect_ok(
        &oracle.request(&json!({
            "op": "session_user_by_hash",
            "hash": rust_session.token_hash,
        })),
        "session_user_by_hash rust insert",
    );
    checker.equal(
        "create_session session",
        &session_value(&rust_session),
        &go["session"],
    );
    checker.equal("create_session user", &user_value(&customer), &go["user"]);

    store
        .delete_session_by_hash(store.conn(), &rust_session.token_hash)
        .await
        .expect("rust delete_session_by_hash");
    let go = expect_ok(
        &oracle.request(&json!({
            "op": "session_user_by_hash",
            "hash": rust_session.token_hash,
        })),
        "session_user_by_hash after delete",
    );
    checker.equal("delete_session_by_hash", &json!(false), &go["found"]);

    let delta_session = Session {
        id: format!("ses_itb_delta_{suffix}"),
        user_id: customer2.id.clone(),
        token_hash: format!("itb-token-delta-{suffix}"),
        user_agent: "agent delta".to_string(),
        ip: "10.0.0.10".to_string(),
        created_at: Some(at(2026, 4, 10)),
        expires_at: Some(at(2027, 4, 10)),
    };
    fixtures
        .session_hashes
        .push(delta_session.token_hash.clone());
    store
        .create_session(store.conn(), &delta_session)
        .await
        .expect("rust create delta session");

    store
        .delete_sessions_for_user_agent(store.conn(), &customer2.id, "")
        .await
        .expect("rust delete sessions with empty agent");
    let go = expect_ok(
        &oracle.request(&json!({
            "op": "session_user_by_hash",
            "hash": delta_session.token_hash,
        })),
        "session after empty agent delete",
    );
    checker.equal(
        "delete_sessions_for_user_agent empty is no-op",
        &json!(true),
        &go["found"],
    );

    store
        .delete_sessions_for_user_agent(store.conn(), &admin.id, "agent alpha")
        .await
        .expect("rust delete sessions for agent");
    for (label, hash, expected) in [
        (
            "delete_sessions_for_user_agent removes alpha",
            &session_admin_alpha.token_hash,
            false,
        ),
        (
            "delete_sessions_for_user_agent removes expired alpha",
            &session_admin_alpha_expired.token_hash,
            false,
        ),
        (
            "delete_sessions_for_user_agent keeps beta",
            &session_admin_beta.token_hash,
            true,
        ),
        (
            "delete_sessions_for_user_agent keeps gamma",
            &session_admin_gamma.token_hash,
            true,
        ),
        (
            "delete_sessions_for_user_agent keeps employee",
            &session_employee_alpha.token_hash,
            true,
        ),
    ] {
        let go = expect_ok(
            &oracle.request(&json!({"op": "session_user_by_hash", "hash": hash})),
            label,
        );
        checker.equal(label, &json!(expected), &go["found"]);
    }

    store
        .delete_expired_sessions(store.conn(), at(2026, 6, 1))
        .await
        .expect("rust delete_expired_sessions");
    let go = expect_ok(
        &oracle.request(&json!({
            "op": "session_user_by_hash",
            "hash": session_admin_gamma.token_hash,
        })),
        "session at boundary",
    );
    checker.equal(
        "delete_expired_sessions keeps expiry equal to now",
        &json!(true),
        &go["found"],
    );

    // ---- writes through Go, reads through Rust --------------------------

    let oracle_plugin_now = at(2026, 5, 1);
    expect_ok(
        &oracle.request(&json!({
            "op": "save_plugin_settings",
            "plugin_id": plugin_oracle_id,
            "payload": "{\"from\":\"go\"}",
            "now": time_text(Some(oracle_plugin_now)),
        })),
        "save_plugin_settings go",
    );
    let (raw, updated_at) = store
        .plugin_settings(&plugin_oracle_id)
        .await
        .expect("rust read Go plugin settings");
    checker.equal(
        "save_plugin_settings Go to Rust",
        &settings_value("{\"from\":\"go\"}", updated_at),
        &settings_value("{\"from\":\"go\"}", Some(oracle_plugin_now)),
    );
    checker.equal(
        "save_plugin_settings Go payload",
        &json!(raw),
        &json!("{\"from\":\"go\"}"),
    );

    let oracle_storefront_now = at(2026, 5, 2);
    expect_ok(
        &oracle.request(&json!({
            "op": "save_storefront_settings",
            "payload": "{\"theme\":\"from-go\"}",
            "now": time_text(Some(oracle_storefront_now)),
        })),
        "save_storefront_settings go",
    );
    let (raw, updated_at) = store
        .storefront_settings()
        .await
        .expect("rust read Go storefront settings");
    checker.equal(
        "save_storefront_settings Go to Rust",
        &settings_value("{\"theme\":\"from-go\"}", updated_at),
        &settings_value("{\"theme\":\"from-go\"}", Some(oracle_storefront_now)),
    );
    checker.equal(
        "save_storefront_settings Go payload",
        &json!(raw),
        &json!("{\"theme\":\"from-go\"}"),
    );

    expect_ok(
        &oracle.request(&json!({
            "op": "set_plugin_state",
            "plugin_id": plugin_state_oracle,
            "enabled": true,
            "now": time_text(Some(at(2026, 5, 3))),
        })),
        "set_plugin_state go",
    );
    compare_read!(
        "set_plugin_state Go to Rust",
        json!({"op": "plugin_states"}),
        {
            json!({
                "states": store
                    .plugin_states()
                    .await
                    .expect("rust plugin_states final")
            })
        }
    );

    let oracle_session = Session {
        id: format!("ses_itb_oracle_{suffix}"),
        user_id: employee.id.clone(),
        token_hash: format!("itb-token-oracle-{suffix}"),
        user_agent: "agent omega".to_string(),
        ip: "10.0.0.11".to_string(),
        created_at: Some(at(2026, 5, 4)),
        expires_at: Some(at(2027, 5, 4)),
    };
    fixtures
        .session_hashes
        .push(oracle_session.token_hash.clone());
    expect_ok(
        &oracle.request(&session_create_request(&oracle_session)),
        "create_session go",
    );
    let (rust_user, rust_session, found) = store
        .session_user_by_hash(&oracle_session.token_hash)
        .await
        .expect("rust read Go session");
    checker.is_true("create_session Go found", found);
    checker.equal(
        "create_session Go session",
        &session_value(&rust_session),
        &session_value(&oracle_session),
    );
    let go = expect_ok(
        &oracle.request(&json!({"op": "user_by_id", "id": employee.id})),
        "user_by_id after Go session",
    );
    checker.equal("create_session Go user", &user_value(&rust_user), &go);

    expect_ok(
        &oracle.request(&json!({
            "op": "update_theme",
            "id": customer.id,
            "preset": "citrus",
            "now": time_text(Some(at(2026, 5, 5))),
        })),
        "update_theme go",
    );
    let rust = store
        .user_by_id(&customer.id)
        .await
        .expect("rust read Go theme");
    checker.equal(
        "update_theme Go to Rust",
        &json!("citrus"),
        &json!(rust.theme_preset),
    );

    expect_ok(
        &oracle.request(&json!({
            "op": "delete_expired_sessions",
            "now": time_text(Some(at(2026, 6, 2))),
        })),
        "delete_expired_sessions go",
    );
    let (_, _, found) = store
        .session_user_by_hash(&session_admin_gamma.token_hash)
        .await
        .expect("rust read deleted boundary session");
    checker.equal(
        "delete_expired_sessions Go removes boundary",
        &json!(false),
        &json!(found),
    );

    // ---- unset timestamps store Go's zero time, not "now" --------------
    //
    // Go formats a zero `time.Time` as `0001-01-01T00:00:00Z` for a required
    // column. Every `Timestamp` here is None, so the stored text must be that
    // exact value and the Go oracle must read it back unchanged.
    let zero_user = User {
        id: format!("usr_itb_zero_{suffix}"),
        name: "IT Basics Zero".to_string(),
        email: format!("itb.zero.{suffix}@basics.test"),
        role: "customer".to_string(),
        ..User::default()
    };
    let zero_session = Session {
        id: format!("ses_itb_zero_{suffix}"),
        user_id: zero_user.id.clone(),
        token_hash: format!("itb-zero-token-{suffix}"),
        ..Session::default()
    };
    let zero_order = Order {
        id: format!("ord_itb_zero_{suffix}"),
        number: format!("ITB-ZERO-{suffix}"),
        user_id: Some(zero_user.id.clone()),
        token_hash: format!("itb-zero-order-token-{suffix}"),
        ..Order::default()
    };
    let zero_audit = AuditEntry {
        id: format!("itb_audit_zero_{suffix}"),
        ..AuditEntry::default()
    };
    fixtures.user_ids.push(zero_user.id.clone());
    fixtures
        .session_hashes
        .push(zero_session.token_hash.clone());
    fixtures.order_ids.push(zero_order.id.clone());
    fixtures.audit_ids.push(zero_audit.id.clone());

    store
        .create_user(store.conn(), &zero_user)
        .await
        .expect("insert default-time user");
    store
        .create_session(store.conn(), &zero_session)
        .await
        .expect("insert default-time session");
    store
        .create_order(store.conn(), &zero_order, &[])
        .await
        .expect("insert default-time order");
    store
        .insert_audit(store.conn(), &zero_audit)
        .await
        .expect("insert default-time audit entry");

    let zero_time_columns = [
        (
            "user",
            "SELECT created_at, updated_at FROM users WHERE id = ?",
            zero_user.id.as_str(),
            2,
        ),
        (
            "session",
            "SELECT created_at, expires_at FROM sessions WHERE id = ?",
            zero_session.id.as_str(),
            2,
        ),
        (
            "order",
            "SELECT created_at, updated_at FROM orders WHERE id = ?",
            zero_order.id.as_str(),
            2,
        ),
        (
            "audit",
            "SELECT created_at FROM audit_outbox WHERE id = ?",
            zero_audit.id.as_str(),
            1,
        ),
    ];
    for (label, query, id, column_count) in zero_time_columns {
        let mut rows = store.conn().query(query, [id]).await.expect("zero query");
        let row = rows.next().await.expect("zero cursor").expect("zero row");
        for index in 0..column_count {
            let value: String = row.get(index).expect("zero timestamp column");
            checker.equal(
                &format!("zero time {label} column {index}"),
                &json!(value),
                &json!("0001-01-01T00:00:00Z"),
            );
        }
    }

    let go = expect_ok(
        &oracle.request(&json!({"op": "user_by_id", "id": zero_user.id})),
        "user_by_id zero-time user",
    );
    checker.equal(
        "zero time user created_at read by Go",
        &go["created_at"],
        &json!("0001-01-01T00:00:00Z"),
    );
    checker.equal(
        "zero time user updated_at read by Go",
        &go["updated_at"],
        &json!("0001-01-01T00:00:00Z"),
    );

    // `NullTime` must treat the zero aggregate as unset: Go returns nil for
    // `last_order_at` when the newest order was created at year 1.
    let go = expect_ok(
        &oracle.request(&json!({"op": "list_customers"})),
        "list_customers zero-time customer",
    );
    let go_row = go["customers"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["id"] == json!(zero_user.id)))
        .unwrap_or_else(|| panic!("Go zero-time customer missing from {go}"))
        .clone();
    let rust_customers = store
        .list_customers()
        .await
        .expect("rust list_customers zero");
    let rust_row = rust_customers
        .iter()
        .find(|row| row.id == zero_user.id)
        .expect("Rust zero-time customer");
    let rust_value = customer_value(rust_row);
    checker.equal(
        "zero time customer created_at",
        &rust_value["created_at"],
        &go_row["created_at"],
    );
    checker.equal(
        "zero time customer last_order_at",
        &rust_value["last_order_at"],
        &go_row["last_order_at"],
    );
    checker.equal(
        "zero time customer last_order_at is null",
        &go_row["last_order_at"],
        &Value::Null,
    );

    // ---- cleanup -------------------------------------------------------

    if let Err(error) = fixtures.cleanup(&store).await {
        panic!("fixture cleanup failed: {error}");
    }

    println!(
        "store basics differential: {} comparisons against the Go oracle",
        checker.comparisons
    );
}

#[derive(Clone, Debug)]
struct PricingCase {
    price_cents: Option<i64>,
    starts: Option<DateTime<Utc>>,
    ends: Option<DateTime<Utc>>,
    regular_price_cents: i64,
    now: DateTime<Utc>,
}

fn pricing_time(value: DateTime<Utc>) -> String {
    // Do not use the store's second-precision formatter here: the fractional
    // boundary cases must reach Go with their sub-second part intact.
    value.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

fn pricing_request(case: &PricingCase) -> Value {
    json!({
        "price_cents": case.price_cents,
        "starts": case.starts.map(pricing_time),
        "ends": case.ends.map(pricing_time),
        "regular_price_cents": case.regular_price_cents,
        "now": pricing_time(case.now),
    })
}

fn exhaustive_pricing_cases() -> Vec<PricingCase> {
    let start = at(2026, 3, 1);
    let end = at(2026, 3, 8);
    let regular = 10_000_i64;
    let prices = [
        None,
        Some(-7),
        Some(0),
        Some(1),
        Some(9_999),
        Some(regular),
        Some(regular + 1),
        Some(i64::MAX),
    ];
    let starts = [None, Some(start)];
    let ends = [
        None,
        Some(end),
        Some(start),
        Some(start - Duration::hours(1)),
    ];
    let nows = [
        start - Duration::hours(1),
        start,
        start + Duration::hours(1),
        end - Duration::seconds(1),
        end,
        end + Duration::hours(1),
    ];

    let mut cases = Vec::with_capacity(8 * 2 * 4 * 6 + 28);
    for price_cents in prices {
        for starts_at in starts {
            for ends_at in ends {
                for now in nows {
                    cases.push(PricingCase {
                        price_cents,
                        starts: starts_at,
                        ends: ends_at,
                        regular_price_cents: regular,
                        now,
                    });
                }
            }
        }
    }

    // Regular prices at and below zero exercise the price comparison without
    // touching the time rules.
    for regular_price_cents in [0, 1, -100, i64::MAX] {
        for price_cents in [None, Some(-1), Some(0), Some(1), Some(i64::MAX)] {
            cases.push(PricingCase {
                price_cents,
                starts: Some(start),
                ends: Some(end),
                regular_price_cents,
                now: start,
            });
        }
    }

    // Sub-second endpoints: the Go oracle parses fractional RFC3339 and the
    // Rust parser keeps the fraction, so the inclusive/exclusive boundaries
    // must agree.
    let fractional_start = start + Duration::milliseconds(500);
    let fractional_end = start + Duration::seconds(1);
    for now in [
        fractional_start - Duration::milliseconds(1),
        fractional_start,
        fractional_start + Duration::milliseconds(1),
        fractional_end - Duration::milliseconds(1),
        fractional_end,
    ] {
        for (starts_at, ends_at) in [
            (Some(fractional_start), Some(fractional_end)),
            (Some(fractional_start), None),
            (None, Some(fractional_end)),
        ] {
            cases.push(PricingCase {
                price_cents: Some(9_000),
                starts: starts_at,
                ends: ends_at,
                regular_price_cents: regular,
                now,
            });
        }
    }

    cases
}

const PRICING_SEED: u64 = 0x5eed_0bad_cafe_1234;

/// Deterministic proptest batch. A fixed harness seed and disabled failure
/// persistence make the run reproducible without writing a regressions file.
fn generated_pricing_cases() -> Vec<PricingCase> {
    let base = at(2026, 3, 1);
    let price = prop_oneof![Just(None::<i64>), any::<i64>().prop_map(Some)];
    let regular = prop_oneof![Just(0_i64), Just(1_i64), Just(10_000_i64), any::<i64>(),];
    let starts = prop_oneof![
        Just(None::<DateTime<Utc>>),
        (-86_400_i64..=86_400_i64).prop_map(move |offset| Some(base + Duration::seconds(offset))),
    ];
    let ends = prop_oneof![
        Just(None::<DateTime<Utc>>),
        (-86_400_i64..=86_400_i64).prop_map(move |offset| Some(base + Duration::seconds(offset))),
    ];
    let case = (price, regular, starts, ends, -86_400_i64..=86_400_i64).prop_map(
        |(price_cents, regular_price_cents, starts, ends, now_offset)| PricingCase {
            price_cents,
            regular_price_cents,
            starts,
            ends,
            now: base + Duration::seconds(now_offset),
        },
    );
    let strategy = proptest::collection::vec(case, 256..=256);
    let config = Config {
        cases: 1,
        rng_seed: RngSeed::Fixed(PRICING_SEED),
        failure_persistence: None,
        source_file: None,
        ..Config::default()
    };
    let mut runner = TestRunner::new(config);
    strategy
        .new_tree(&mut runner)
        .expect("generate pricing cases")
        .current()
}

fn compare_pricing(checker: &mut Checker, cases: &[PricingCase], response: &Value) {
    let results = response
        .as_array()
        .unwrap_or_else(|| panic!("pricing response is not a JSON array: {response}"));
    assert_eq!(
        results.len(),
        cases.len(),
        "Go returned {} pricing results for {} cases",
        results.len(),
        cases.len()
    );

    for (index, (case, go)) in cases.iter().zip(results).enumerate() {
        let window = SaleWindow {
            price_cents: case.price_cents,
            starts_at: case.starts,
            ends_at: case.ends,
        };
        let product = Product {
            price_cents: case.regular_price_cents,
            sale_price_cents: case.price_cents,
            sale_starts_at: case.starts,
            sale_ends_at: case.ends,
            ..Product::default()
        };

        checker.equal(
            &format!("pricing {index} problems"),
            &json!(window.problems(case.regular_price_cents)),
            &go["problems"],
        );
        checker.equal(
            &format!("pricing {index} valid"),
            &json!(window.valid(case.regular_price_cents)),
            &go["valid"],
        );
        checker.equal(
            &format!("pricing {index} active"),
            &json!(window.active(case.now, case.regular_price_cents)),
            &go["active"],
        );
        checker.equal(
            &format!("pricing {index} effective"),
            &json!(product.effective_price_cents(case.now)),
            &go["effective"],
        );
        checker.equal(
            &format!("pricing {index} sale_active"),
            &json!(product.sale_active(case.now)),
            &go["active"],
        );
    }
}

#[test]
fn differential_pricing_rules_match_go() {
    let dsn = dsn();
    let mut oracle = Oracle::spawn(&dsn);
    let mut checker = Checker::default();

    let exhaustive = exhaustive_pricing_cases();
    let response = oracle.request(&json!({
        "op": "pricing",
        "cases": exhaustive.iter().map(pricing_request).collect::<Vec<_>>(),
    }));
    compare_pricing(&mut checker, &exhaustive, &response);

    let generated = generated_pricing_cases();
    let response = oracle.request(&json!({
        "op": "pricing",
        "cases": generated.iter().map(pricing_request).collect::<Vec<_>>(),
    }));
    compare_pricing(&mut checker, &generated, &response);

    println!(
        "pricing differential: {} exhaustive + {} seeded proptest cases, {} comparisons \
         (proptest seed 0x{:x})",
        exhaustive.len(),
        generated.len(),
        checker.comparisons,
        PRICING_SEED
    );
}
