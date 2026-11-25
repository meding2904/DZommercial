//! Database opening and migration runner.
//!
//! Mirrors `backend/internal/db`: the same SQL files, the same ordering, the
//! same `schema_migrations` and `plugin_migrations` bookkeeping. The Go oracle
//! and this port can point at the same Turso database because the marker rows
//! are identical.

use std::time::Duration;

use chrono::{SecondsFormat, Utc};
use libsql::{Builder, Connection, Database};

const MIGRATIONS: &[(&str, &str)] = &[
    ("001_init.sql", include_str!("../migrations/001_init.sql")),
    (
        "002_shipping.sql",
        include_str!("../migrations/002_shipping.sql"),
    ),
    (
        "003_framework.sql",
        include_str!("../migrations/003_framework.sql"),
    ),
    (
        "004_order_plugin.sql",
        include_str!("../migrations/004_order_plugin.sql"),
    ),
    (
        "005_oils_plugin.sql",
        include_str!("../migrations/005_oils_plugin.sql"),
    ),
    (
        "006_product_images.sql",
        include_str!("../migrations/006_product_images.sql"),
    ),
    (
        "007_plugin_categories.sql",
        include_str!("../migrations/007_plugin_categories.sql"),
    ),
    (
        "008_plugin_states.sql",
        include_str!("../migrations/008_plugin_states.sql"),
    ),
    (
        "009_audit_outbox.sql",
        include_str!("../migrations/009_audit_outbox.sql"),
    ),
    (
        "010_product_sales.sql",
        include_str!("../migrations/010_product_sales.sql"),
    ),
    (
        "011_web_sessions.sql",
        include_str!("../migrations/011_web_sessions.sql"),
    ),
];

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("open database: {0}")]
    Open(String),
    #[error("ping database: {0}")]
    Ping(String),
    #[error("create schema_migrations: {0}")]
    CreateSchemaMigrations(String),
    #[error("check migration {name}: {reason}")]
    CheckMigration { name: String, reason: String },
    #[error("apply migration {name}: {reason}")]
    ApplyMigration { name: String, reason: String },
    #[error("record migration {name}: {reason}")]
    RecordMigration { name: String, reason: String },
    #[error("commit migration {name}: {reason}")]
    CommitMigration { name: String, reason: String },
    #[error("begin migration {name}: {reason}")]
    BeginMigration { name: String, reason: String },
    #[error("create plugin_migrations: {0}")]
    CreatePluginMigrations(String),
    #[error("plugin migration needs a plugin id and version")]
    PluginMigrationMissingKey,
    #[error("duplicate plugin migration {0}")]
    DuplicatePluginMigration(String),
    #[error("check plugin migration {name}: {reason}")]
    CheckPluginMigration { name: String, reason: String },
    #[error("apply plugin migration {name}: {reason}")]
    ApplyPluginMigration { name: String, reason: String },
    #[error("record plugin migration {name}: {reason}")]
    RecordPluginMigration { name: String, reason: String },
    #[error("commit plugin migration {name}: {reason}")]
    CommitPluginMigration { name: String, reason: String },
    #[error("begin plugin migration {name}: {reason}")]
    BeginPluginMigration { name: String, reason: String },
}

/// One SQL file owned by a plugin. The core records it in the same way as its
/// own migrations, keyed by plugin id and version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginMigration {
    pub plugin_id: String,
    pub version: String,
    pub sql: String,
}

/// A remote DSN after the Go driver's query-parameter handling.
struct RemoteDsn {
    url: String,
    token: String,
}

/// Open a libSQL database from a config DSN.
///
/// Mirrors the Go libsql driver's `Driver.Open`: `auth_token`, `authToken` and
/// `jwt` all carry the token (at most one non-empty, in that precedence), and
/// `tls=0`/`tls=1` selects plaintext or TLS. Any other query parameter is
/// rejected, as it is by the driver. `libsql://` becomes `https://`, or
/// `http://` with `tls=0` and an explicit port. Values without a supported
/// scheme are rejected instead of being treated as local files; tests build
/// local databases with [`Builder`] directly.
pub async fn open(dsn: &str) -> Result<Database, DbError> {
    let remote = remote_dsn(dsn)?;
    let database = Builder::new_remote(remote.url, remote.token)
        .build()
        .await
        .map_err(|err| DbError::Open(err.to_string()))?;

    // Match the Go pool ping with a short deadline.
    let connect = database
        .connect()
        .map_err(|err| DbError::Ping(err.to_string()))?;
    tokio::time::timeout(Duration::from_secs(10), connect.query("SELECT 1", ()))
        .await
        .map_err(|_| DbError::Ping("timeout".to_string()))?
        .map_err(|err| DbError::Ping(err.to_string()))?;
    Ok(database)
}

fn remote_dsn(dsn: &str) -> Result<RemoteDsn, DbError> {
    let mut url =
        url::Url::parse(dsn).map_err(|_| DbError::Open(unsupported_scheme("").to_string()))?;

    // Go's `url.Values.Get` returns the first value for a key, even when it is
    // empty, so duplicates after the first never contribute.
    let mut snake_token: Option<String> = None;
    let mut camel_token: Option<String> = None;
    let mut jwt_token: Option<String> = None;
    let mut tls_value: Option<String> = None;
    let mut unknown: Option<String> = None;
    for (key, value) in url.query_pairs() {
        let slot = match key.as_ref() {
            "auth_token" => &mut snake_token,
            "authToken" => &mut camel_token,
            "jwt" => &mut jwt_token,
            "tls" => &mut tls_value,
            other => {
                if unknown.is_none() {
                    unknown = Some(other.to_string());
                }
                continue;
            }
        };
        if slot.is_none() {
            *slot = Some(value.into_owned());
        }
    }
    let snake_token = snake_token.unwrap_or_default();
    let camel_token = camel_token.unwrap_or_default();
    let jwt_token = jwt_token.unwrap_or_default();

    let non_empty = [&snake_token, &camel_token, &jwt_token]
        .into_iter()
        .filter(|value| !value.is_empty())
        .count();
    if non_empty > 1 {
        return Err(DbError::Open(
            "please use at most one of the following query parameters: \
             'auth_token', 'authToken', 'jwt'"
                .to_string(),
        ));
    }
    let token = if !snake_token.is_empty() {
        snake_token
    } else if !camel_token.is_empty() {
        camel_token
    } else {
        jwt_token
    };

    let scheme = url.scheme().to_string();
    let tls = match tls_value.as_deref() {
        None | Some("") => !matches!(scheme.as_str(), "http" | "ws"),
        Some("0") => false,
        Some("1") => true,
        Some(_) => {
            return Err(DbError::Open(
                "unknown value of tls query parameter. Valid values are 0 and 1".to_string(),
            ));
        }
    };
    if let Some(name) = unknown {
        return Err(DbError::Open(format!("unknown query parameter {name:?}")));
    }

    url.set_query(None);
    let remote_scheme = match scheme.as_str() {
        "libsql" if tls => "https",
        "libsql" => {
            if url.port().is_none() {
                return Err(DbError::Open(
                    "libsql:// URL with ?tls=0 must specify an explicit port".to_string(),
                ));
            }
            "http"
        }
        "https" | "wss" if !tls => {
            return Err(DbError::Open(format!(
                "{scheme}:// URL cannot opt out of TLS using ?tls=0"
            )));
        }
        "http" | "ws" if tls => {
            return Err(DbError::Open(format!(
                "{scheme}:// URL cannot opt in to TLS using ?tls=1"
            )));
        }
        "http" | "https" | "ws" | "wss" => scheme.as_str(),
        other => return Err(DbError::Open(unsupported_scheme(other).to_string())),
    };
    let rest = &url[url::Position::AfterScheme..];
    Ok(RemoteDsn {
        url: format!("{remote_scheme}{rest}"),
        token,
    })
}

fn unsupported_scheme(scheme: &str) -> String {
    format!(
        "unsupported URL scheme: {scheme}\nThis driver supports only URLs that start with \
         libsql://, file://, https://, http://, wss:// and ws://"
    )
}

/// Names of the bundled core migrations, in apply order.
pub fn migration_names() -> Vec<&'static str> {
    MIGRATIONS.iter().map(|(name, _)| *name).collect()
}

/// Apply every bundled core migration that has not been recorded yet.
pub async fn migrate(conn: &Connection) -> Result<(), DbError> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL
        )",
        (),
    )
    .await
    .map_err(|err| DbError::CreateSchemaMigrations(err.to_string()))?;

    for (name, body) in MIGRATIONS {
        if migration_applied(conn, name).await? {
            continue;
        }
        let tx = conn
            .transaction()
            .await
            .map_err(|err| DbError::BeginMigration {
                name: (*name).to_string(),
                reason: err.to_string(),
            })?;
        for statement in split_statements(body) {
            tx.execute(&statement, ())
                .await
                .map_err(|err| DbError::ApplyMigration {
                    name: (*name).to_string(),
                    reason: err.to_string(),
                })?;
        }
        tx.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (?, ?)",
            libsql::params![name.to_string(), now()],
        )
        .await
        .map_err(|err| DbError::RecordMigration {
            name: (*name).to_string(),
            reason: err.to_string(),
        })?;
        tx.commit().await.map_err(|err| DbError::CommitMigration {
            name: (*name).to_string(),
            reason: err.to_string(),
        })?;
    }
    Ok(())
}

pub async fn migration_applied(conn: &Connection, name: &str) -> Result<bool, DbError> {
    let mut rows = conn
        .query(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = ?",
            libsql::params![name.to_string()],
        )
        .await
        .map_err(|err| DbError::CheckMigration {
            name: name.to_string(),
            reason: err.to_string(),
        })?;
    let row = rows
        .next()
        .await
        .map_err(|err| DbError::CheckMigration {
            name: name.to_string(),
            reason: err.to_string(),
        })?
        .ok_or_else(|| DbError::CheckMigration {
            name: name.to_string(),
            reason: "no row".to_string(),
        })?;
    let count: i64 = row.get(0).map_err(|err| DbError::CheckMigration {
        name: name.to_string(),
        reason: err.to_string(),
    })?;
    Ok(count > 0)
}

/// Split a SQL script on top-level `;` characters.
///
/// Semicolons inside single-quoted strings, quoted identifiers, `--` line
/// comments and `/* */` block comments do not split, so a trigger body or a
/// comment stays with its statement. Fragments that are empty or hold only
/// whitespace and comments are dropped.
pub fn split_statements(body: &str) -> Vec<String> {
    let bytes = body.as_bytes();
    let mut statements = Vec::new();
    let mut start = 0;
    let mut has_content = false;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            quote @ (b'\'' | b'"' | b'`') => {
                has_content = true;
                index += 1;
                while index < bytes.len() {
                    if bytes[index] != quote {
                        index += 1;
                        continue;
                    }
                    if bytes.get(index + 1) == Some(&quote) {
                        // A doubled quote escapes the quote character.
                        index += 2;
                        continue;
                    }
                    index += 1;
                    break;
                }
            }
            b'-' if bytes.get(index + 1) == Some(&b'-') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index < bytes.len() {
                    if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                        index += 2;
                        break;
                    }
                    index += 1;
                }
            }
            b';' => {
                push_statement(&mut statements, &body[start..index], has_content);
                has_content = false;
                start = index + 1;
                index += 1;
            }
            byte => {
                if !byte.is_ascii_whitespace() {
                    has_content = true;
                }
                index += 1;
            }
        }
    }
    push_statement(&mut statements, &body[start..], has_content);
    statements
}

fn push_statement(statements: &mut Vec<String>, fragment: &str, has_content: bool) {
    if !has_content {
        return;
    }
    let trimmed = fragment.trim();
    if !trimmed.is_empty() {
        statements.push(trimmed.to_owned());
    }
}

/// Apply plugin migrations in stable (plugin id, version) order.
pub async fn migrate_plugins(conn: &Connection, list: Vec<PluginMigration>) -> Result<(), DbError> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS plugin_migrations (
            plugin_id TEXT NOT NULL,
            version TEXT NOT NULL,
            applied_at TEXT NOT NULL,
            PRIMARY KEY (plugin_id, version)
        )",
        (),
    )
    .await
    .map_err(|err| DbError::CreatePluginMigrations(err.to_string()))?;

    let mut migrations = list;
    migrations.sort_by(|a, b| {
        a.plugin_id
            .cmp(&b.plugin_id)
            .then_with(|| a.version.cmp(&b.version))
    });

    let mut seen = std::collections::HashSet::new();
    for migration in migrations {
        if migration.plugin_id.trim().is_empty() || migration.version.trim().is_empty() {
            return Err(DbError::PluginMigrationMissingKey);
        }
        let key = format!("{}:{}", migration.plugin_id, migration.version);
        if !seen.insert(key.clone()) {
            return Err(DbError::DuplicatePluginMigration(key));
        }

        let mut rows = conn
            .query(
                "SELECT COUNT(*) FROM plugin_migrations WHERE plugin_id = ? AND version = ?",
                libsql::params![migration.plugin_id.clone(), migration.version.clone()],
            )
            .await
            .map_err(|err| DbError::CheckPluginMigration {
                name: key.clone(),
                reason: err.to_string(),
            })?;
        let row = rows
            .next()
            .await
            .map_err(|err| DbError::CheckPluginMigration {
                name: key.clone(),
                reason: err.to_string(),
            })?
            .ok_or_else(|| DbError::CheckPluginMigration {
                name: key.clone(),
                reason: "no row".to_string(),
            })?;
        let applied: i64 = row.get(0).map_err(|err| DbError::CheckPluginMigration {
            name: key.clone(),
            reason: err.to_string(),
        })?;
        if applied > 0 {
            continue;
        }

        let tx = conn
            .transaction()
            .await
            .map_err(|err| DbError::BeginPluginMigration {
                name: key.clone(),
                reason: err.to_string(),
            })?;
        for statement in split_statements(&migration.sql) {
            tx.execute(&statement, ())
                .await
                .map_err(|err| DbError::ApplyPluginMigration {
                    name: key.clone(),
                    reason: err.to_string(),
                })?;
        }
        tx.execute(
            "INSERT INTO plugin_migrations (plugin_id, version, applied_at) VALUES (?, ?, ?)",
            libsql::params![
                migration.plugin_id.clone(),
                migration.version.clone(),
                now()
            ],
        )
        .await
        .map_err(|err| DbError::RecordPluginMigration {
            name: key.clone(),
            reason: err.to_string(),
        })?;
        tx.commit()
            .await
            .map_err(|err| DbError::CommitPluginMigration {
                name: key.clone(),
                reason: err.to_string(),
            })?;
    }
    Ok(())
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_matches_go_shape() {
        assert_eq!(
            split_statements("CREATE TABLE a (id TEXT); ; SELECT 1;"),
            vec!["CREATE TABLE a (id TEXT)", "SELECT 1"]
        );
        assert!(split_statements("  ;  ").is_empty());
    }

    #[test]
    fn split_keeps_semicolons_inside_strings_and_identifiers() {
        let statements = split_statements("SELECT ';' AS x; SELECT 2;");
        assert_eq!(statements.len(), 2);
        assert_eq!(statements[0], "SELECT ';' AS x");
        assert_eq!(statements[1], "SELECT 2");

        assert_eq!(
            split_statements("SELECT 'it''s; fine'; SELECT \"a;b\"; SELECT `c;d`;"),
            vec!["SELECT 'it''s; fine'", "SELECT \"a;b\"", "SELECT `c;d`"]
        );
    }

    #[test]
    fn split_ignores_semicolons_in_line_comments() {
        assert_eq!(
            split_statements("-- keep; this\nSELECT 1;"),
            vec!["-- keep; this\nSELECT 1"]
        );
    }

    #[test]
    fn split_ignores_semicolons_in_block_comments() {
        assert_eq!(
            split_statements("SELECT 1; /* a; b */ SELECT 2;"),
            vec!["SELECT 1", "/* a; b */ SELECT 2"]
        );
    }

    #[test]
    fn split_drops_comment_only_fragments() {
        assert!(split_statements("-- nothing;\n/* here; too */").is_empty());
        assert_eq!(
            split_statements("SELECT 1; -- trailing; note\n;"),
            vec!["SELECT 1"]
        );
    }

    #[tokio::test]
    async fn migrate_is_idempotent_on_memory_database() {
        let database = Builder::new_local(":memory:").build().await.expect("open");
        let conn = database.connect().expect("connect");
        migrate(&conn).await.expect("first migrate");
        migrate(&conn).await.expect("second migrate");

        let mut rows = conn
            .query("SELECT COUNT(*) FROM schema_migrations", ())
            .await
            .expect("count");
        let row = rows.next().await.expect("row").expect("some");
        let count: i64 = row.get(0).expect("count value");
        assert_eq!(count as usize, MIGRATIONS.len());
    }

    fn dsn_error(dsn: &str) -> String {
        remote_dsn(dsn).err().expect("dsn is rejected").to_string()
    }

    #[test]
    fn remote_dsn_matches_the_go_driver_parameters() {
        let remote = remote_dsn("http://127.0.0.1:8880").expect("plain http");
        assert_eq!(remote.url, "http://127.0.0.1:8880/");
        assert_eq!(remote.token, "");

        let remote = remote_dsn("libsql://shop.example/db?authToken=tok").expect("libsql");
        assert_eq!(remote.url, "https://shop.example/db");
        assert_eq!(remote.token, "tok");

        let remote = remote_dsn("libsql://shop.example:8080/db?tls=0&jwt=tok").expect("plaintext");
        assert_eq!(remote.url, "http://shop.example:8080/db");
        assert_eq!(remote.token, "tok");

        let remote = remote_dsn("https://shop.example?auth_token=snake").expect("snake");
        assert_eq!(remote.token, "snake");

        let remote = remote_dsn("https://shop.example?authToken=camel").expect("camel");
        assert_eq!(remote.token, "camel");

        let remote = remote_dsn("wss://shop.example/socket").expect("secure websocket");
        assert_eq!(remote.url, "wss://shop.example/socket");

        // Redundant TLS flags are accepted, like the Go driver.
        let remote = remote_dsn("https://shop.example?tls=1").expect("https with tls=1");
        assert_eq!(remote.url, "https://shop.example/");

        // An empty tls value is the default, and only the first value of a
        // duplicated token key counts (Go's `url.Values.Get`).
        assert_eq!(
            remote_dsn("http://127.0.0.1:8880?tls=")
                .expect("empty tls")
                .url,
            "http://127.0.0.1:8880/"
        );
        assert_eq!(
            remote_dsn("http://127.0.0.1:8880?auth_token=&authToken=camel")
                .expect("camel after empty snake")
                .token,
            "camel"
        );
        assert_eq!(
            remote_dsn("http://127.0.0.1:8880?auth_token=&auth_token=second")
                .expect("first value wins")
                .token,
            ""
        );
    }

    #[test]
    fn remote_dsn_rejects_unknown_go_driver_parameters() {
        assert_eq!(
            dsn_error("http://127.0.0.1:8880?timeout=1"),
            "open database: unknown query parameter \"timeout\""
        );
        assert_eq!(
            dsn_error("http://127.0.0.1:8880?tls=2"),
            "open database: unknown value of tls query parameter. Valid values are 0 and 1"
        );
        assert_eq!(
            dsn_error("http://127.0.0.1:8880?tls=1"),
            "open database: http:// URL cannot opt in to TLS using ?tls=1"
        );
        assert_eq!(
            dsn_error("https://shop.example?tls=0"),
            "open database: https:// URL cannot opt out of TLS using ?tls=0"
        );
        assert_eq!(
            dsn_error("ws://shop.example?tls=1"),
            "open database: ws:// URL cannot opt in to TLS using ?tls=1"
        );
        assert_eq!(
            dsn_error("libsql://shop.example?tls=0"),
            "open database: libsql:// URL with ?tls=0 must specify an explicit port"
        );
        assert_eq!(
            dsn_error("http://shop.example?auth_token=a&authToken=b"),
            "open database: please use at most one of the following query parameters: \
             'auth_token', 'authToken', 'jwt'"
        );
    }

    #[test]
    fn remote_dsn_rejects_scheme_less_values() {
        let message = dsn_error("foo.db");
        assert!(message.contains("unsupported URL scheme:"));
        let message = dsn_error(":memory:");
        assert!(message.contains("unsupported URL scheme:"));
        let message = dsn_error("ftp://shop.example/db");
        assert!(message.contains("unsupported URL scheme: ftp"));
    }

    #[tokio::test]
    async fn plugin_migrations_are_recorded_and_deduplicated() {
        let database = Builder::new_local(":memory:").build().await.expect("open");
        let conn = database.connect().expect("connect");
        let list = vec![PluginMigration {
            plugin_id: "oils".into(),
            version: "001".into(),
            sql: "CREATE TABLE plugin_oils (id TEXT);".into(),
        }];
        migrate_plugins(&conn, list.clone()).await.expect("first");
        migrate_plugins(&conn, list).await.expect("second");

        let mut rows = conn
            .query("SELECT COUNT(*) FROM plugin_migrations", ())
            .await
            .expect("count");
        let row = rows.next().await.expect("row").expect("some");
        let count: i64 = row.get(0).expect("value");
        assert_eq!(count, 1);
    }
}
