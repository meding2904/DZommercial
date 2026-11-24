//! Audit outbox writes.

use libsql::Connection;

use crate::store::{Store, StoreError};
use crate::types::{AuditEntry, format_time_or_zero};

impl Store {
    pub async fn insert_audit(
        &self,
        conn: &Connection,
        entry: &AuditEntry,
    ) -> Result<(), StoreError> {
        let detail = if entry.detail.is_empty() {
            "{}"
        } else {
            entry.detail.as_str()
        };
        conn.execute(
            "INSERT INTO audit_outbox (id, actor_user_id, action, entity_type, entity_id, detail, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            libsql::params![
                entry.id.clone(),
                entry.actor_user_id.clone(),
                entry.action.clone(),
                entry.entity_type.clone(),
                entry.entity_id.clone(),
                detail,
                format_time_or_zero(entry.created_at.as_ref())
            ],
        )
        .await
        .map_err(|err| StoreError::Database(format!("write audit log: {err}")))?;
        Ok(())
    }
}
