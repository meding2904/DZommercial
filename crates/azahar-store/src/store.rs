//! Store handle, write serialisation and error mapping.

use libsql::{Connection, Database, Transaction};
use tokio::sync::{Mutex, MutexGuard};

pub use crate::types::*;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{0}")]
    Begin(String),
    #[error("commit transaction: {0}")]
    Commit(String),
    #[error("{0}")]
    Database(String),
    #[error("no rows in result set")]
    NotFound,
}

impl From<libsql::Error> for StoreError {
    fn from(value: libsql::Error) -> Self {
        Self::Database(value.to_string())
    }
}

impl StoreError {
    /// The Go layer treats any error containing "unique constraint" as a
    /// duplicate email. Keep that check rather than guessing at SQLite codes:
    /// the remote and local drivers word it the same way.
    pub fn is_unique_violation(&self) -> bool {
        self.to_string()
            .to_lowercase()
            .contains("unique constraint")
    }

    /// Go callers use `errors.Is(err, sql.ErrNoRows)` for missing rows.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound)
    }
}

pub struct Store {
    db: Database,
    conn: Connection,
    write_lock: Mutex<()>,
}

impl Store {
    pub fn new(db: Database) -> Result<Self, StoreError> {
        let conn = db.connect()?;
        Ok(Self {
            db,
            conn,
            write_lock: Mutex::new(()),
        })
    }

    pub fn db(&self) -> &Database {
        &self.db
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Take the single-writer lock and start a transaction.
    ///
    /// The Go store holds a mutex for the whole write. A guard keeps the same
    /// guarantee and lets callers pass `tx.conn()` to the `&Connection`
    /// methods. Call `commit` on success; dropping without committing rolls
    /// back through libsql's close callback.
    pub async fn begin_write(&self) -> Result<WriteTx<'_>, StoreError> {
        let guard = self.write_lock.lock().await;
        let transaction = self
            .conn
            .transaction()
            .await
            .map_err(|err| StoreError::Begin(format!("begin transaction: {err}")))?;
        Ok(WriteTx {
            _guard: guard,
            transaction: Some(transaction),
        })
    }
}

pub struct WriteTx<'a> {
    _guard: MutexGuard<'a, ()>,
    transaction: Option<Transaction>,
}

impl WriteTx<'_> {
    pub fn conn(&self) -> &Connection {
        self.transaction
            .as_ref()
            .expect("write transaction is present until commit")
    }

    pub async fn commit(mut self) -> Result<(), StoreError> {
        let transaction = self
            .transaction
            .take()
            .expect("write transaction is present until commit");
        transaction
            .commit()
            .await
            .map_err(|err| StoreError::Commit(err.to_string()))
    }

    pub async fn rollback(mut self) -> Result<(), StoreError> {
        let transaction = self
            .transaction
            .take()
            .expect("write transaction is present until rollback");
        transaction
            .rollback()
            .await
            .map_err(|err| StoreError::Commit(err.to_string()))
    }
}
