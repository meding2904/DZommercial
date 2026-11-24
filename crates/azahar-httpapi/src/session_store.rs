//! libSQL-backed persistence for `tower-sessions`.
//!
//! The Rust service owns the `web_sessions` table created by migration
//! `011_web_sessions.sql`. Each row stores the MessagePack encoding of a
//! [`Record`] and its absolute expiry as Unix seconds. The Go `sessions` table
//! is left untouched while the oracle still runs.
//!
//! This adapter uses the connection handed to it by the caller, so it shares
//! the remote libSQL handle opened by [`azahar_db::open`] rather than opening a
//! second database.

use std::fmt;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use libsql::{Connection, Value, params};
use tower_sessions::session::{Id, Record};
use tower_sessions::session_store::{self, Error as StoreError};

/// The table created by `crates/azahar-db/migrations/011_web_sessions.sql`.
pub const SESSION_TABLE: &str = "web_sessions";

/// A [`tower_sessions::SessionStore`] that keeps records in libSQL.
#[derive(Clone)]
pub struct LibsqlSessionStore {
    connection: Connection,
}

impl fmt::Debug for LibsqlSessionStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LibsqlSessionStore")
            .field("connection", &"libsql::Connection")
            .field("table", &SESSION_TABLE)
            .finish()
    }
}

impl LibsqlSessionStore {
    /// Wraps an existing connection. The caller keeps ownership of the
    /// connection it passes, so `Store` and sessions share one remote handle.
    pub fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Removes rows whose expiry is at or before `now`.
    ///
    /// `tower-sessions`' `ExpiredDeletion` trait has no clock parameter, which
    /// makes it awkward to test. This plain method is equivalent and lets
    /// callers pass the same clock the rest of the service uses. Returns the
    /// number of deleted rows.
    pub async fn delete_expired(&self, now: DateTime<Utc>) -> session_store::Result<u64> {
        self.connection
            .execute(
                "DELETE FROM web_sessions WHERE expiry <= ?",
                params![unix_seconds(now)],
            )
            .await
            .map_err(backend_error)
    }

    async fn id_exists(&self, id: &Id) -> session_store::Result<bool> {
        let mut rows = self
            .connection
            .query(
                "SELECT EXISTS(SELECT 1 FROM web_sessions WHERE id = ?)",
                params![id.to_string()],
            )
            .await
            .map_err(backend_error)?;
        let row = rows.next().await.map_err(backend_error)?;
        let Some(row) = row else {
            return Ok(false);
        };
        let value = row
            .get_value(0)
            .map_err(|err| StoreError::Backend(err.to_string()))?;
        Ok(matches!(value, Value::Integer(1)))
    }

    async fn save_record(&self, record: &Record) -> session_store::Result<()> {
        let data = rmp_serde::to_vec(record).map_err(|err| StoreError::Encode(err.to_string()))?;
        self.connection
            .execute(
                "INSERT INTO web_sessions (id, data, expiry) VALUES (?, ?, ?)
                 ON CONFLICT(id) DO UPDATE SET data = excluded.data, expiry = excluded.expiry",
                params![
                    record.id.to_string(),
                    data,
                    record.expiry_date.unix_timestamp()
                ],
            )
            .await
            .map_err(backend_error)?;
        Ok(())
    }
}

#[async_trait]
impl tower_sessions::SessionStore for LibsqlSessionStore {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        // `Id::default()` is a random i128; a collision is vanishingly rare,
        // but regenerating keeps create safe if one ever happens.
        while self.id_exists(&record.id).await? {
            record.id = Id::default();
        }
        self.save_record(record).await
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        self.save_record(record).await
    }

    async fn load(&self, session_id: &Id) -> session_store::Result<Option<Record>> {
        let mut rows = self
            .connection
            .query(
                "SELECT data FROM web_sessions WHERE id = ? AND expiry > ?",
                params![session_id.to_string(), unix_seconds(Utc::now())],
            )
            .await
            .map_err(backend_error)?;
        let row = rows.next().await.map_err(backend_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let data: Vec<u8> = row
            .get(0)
            .map_err(|err| StoreError::Backend(err.to_string()))?;
        let record =
            rmp_serde::from_slice(&data).map_err(|err| StoreError::Decode(err.to_string()))?;
        Ok(Some(record))
    }

    async fn delete(&self, session_id: &Id) -> session_store::Result<()> {
        self.connection
            .execute(
                "DELETE FROM web_sessions WHERE id = ?",
                params![session_id.to_string()],
            )
            .await
            .map_err(backend_error)?;
        Ok(())
    }
}

fn unix_seconds(now: DateTime<Utc>) -> i64 {
    now.timestamp()
}

fn backend_error(error: libsql::Error) -> StoreError {
    StoreError::Backend(error.to_string())
}

#[cfg(test)]
mod tests {
    use libsql::Builder;
    use tower_sessions::SessionStore;

    use super::*;

    async fn memory_store() -> LibsqlSessionStore {
        let database = Builder::new_local(":memory:").build().await.expect("open");
        let connection = database.connect().expect("connect");
        connection
            .execute(
                "CREATE TABLE web_sessions (
                    id TEXT PRIMARY KEY,
                    data BLOB NOT NULL,
                    expiry INTEGER NOT NULL
                )",
                (),
            )
            .await
            .expect("create table");
        LibsqlSessionStore::new(connection)
    }

    #[tokio::test]
    async fn saves_loads_and_deletes_a_record() {
        let store = memory_store().await;
        let mut record = Record {
            id: Id::default(),
            data: std::collections::HashMap::from([(
                "user_id".to_string(),
                serde_json::json!("usr_test"),
            )]),
            expiry_date: time::OffsetDateTime::now_utc() + time::Duration::hours(1),
        };
        store.create(&mut record).await.expect("create");
        let loaded = store
            .load(&record.id)
            .await
            .expect("load")
            .expect("record exists");
        assert_eq!(loaded.id, record.id);
        assert_eq!(loaded.data, record.data);

        store.delete(&record.id).await.expect("delete");
        assert!(store.load(&record.id).await.expect("load").is_none());
    }

    #[tokio::test]
    async fn expired_records_are_not_loaded_and_can_be_purged() {
        let store = memory_store().await;
        let record = Record {
            id: Id::default(),
            data: Default::default(),
            expiry_date: time::OffsetDateTime::now_utc() - time::Duration::minutes(1),
        };
        store.save(&record).await.expect("save expired");
        assert!(store.load(&record.id).await.expect("load").is_none());

        let deleted = store
            .delete_expired(Utc::now() + chrono::Duration::seconds(1))
            .await
            .expect("purge");
        assert_eq!(deleted, 1);
    }
}
