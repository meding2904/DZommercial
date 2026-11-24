//! Moving queued audit rows into the log.
//!
//! Ported from `backend/internal/store/audit_flush.go`. One batch is copied
//! into `audit_log` and deleted from `audit_outbox` inside one write
//! transaction, under the store's single-writer lock, so a crash leaves the
//! rows queued rather than lost.

use crate::store::{Store, StoreError};

pub const DEFAULT_AUDIT_FLUSH_LIMIT: i64 = 200;
pub const MAX_AUDIT_FLUSH_LIMIT: i64 = 1000;

impl Store {
    pub async fn flush_audit_batch(&self, limit: i64) -> Result<i64, StoreError> {
        let limit = if limit <= 0 {
            DEFAULT_AUDIT_FLUSH_LIMIT
        } else if limit > MAX_AUDIT_FLUSH_LIMIT {
            MAX_AUDIT_FLUSH_LIMIT
        } else {
            limit
        };

        let tx = self
            .begin_write()
            .await
            .map_err(|err| StoreError::Database(format!("begin audit flush: {err}")))?;

        // Audit ids are unique. If a row is already in audit_log, the entry is
        // present and the outbox copy is safe to remove.
        tx.conn()
            .execute(
                "INSERT OR IGNORE INTO audit_log (id, actor_user_id, action, entity_type, entity_id, detail, created_at)
                 SELECT id, actor_user_id, action, entity_type, entity_id, detail, created_at
                 FROM audit_outbox
                 ORDER BY created_at, id
                 LIMIT ?",
                libsql::params![limit],
            )
            .await
            .map_err(|err| StoreError::Database(format!("insert audit batch: {err}")))?;

        let deleted = tx
            .conn()
            .execute(
                "DELETE FROM audit_outbox
                 WHERE id IN (SELECT id FROM audit_outbox ORDER BY created_at, id LIMIT ?)",
                libsql::params![limit],
            )
            .await
            .map_err(|err| StoreError::Database(format!("delete audit batch: {err}")))?;

        tx.commit()
            .await
            .map_err(|err| StoreError::Database(format!("commit audit flush: {err}")))?;
        Ok(i64::try_from(deleted).unwrap_or(i64::MAX))
    }
}
