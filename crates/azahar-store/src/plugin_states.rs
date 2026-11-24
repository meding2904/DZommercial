//! Persisted plugin enable/disable flags.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use libsql::Connection;

use crate::store::{Store, StoreError};
use crate::types::format_time;

impl Store {
    /// Plugins without a row are treated as enabled by the registry.
    pub async fn plugin_states(&self) -> Result<HashMap<String, bool>, StoreError> {
        let mut rows = self
            .conn()
            .query("SELECT plugin_id, enabled FROM plugin_states", ())
            .await
            .map_err(|err| StoreError::Database(format!("list plugin states: {err}")))?;
        let mut states = HashMap::new();
        while let Some(row) = rows.next().await? {
            let plugin_id: String = row
                .get(0)
                .map_err(|err| StoreError::Database(format!("scan plugin state: {err}")))?;
            let enabled: i64 = row
                .get(1)
                .map_err(|err| StoreError::Database(format!("scan plugin state: {err}")))?;
            states.insert(plugin_id, enabled != 0);
        }
        Ok(states)
    }

    pub async fn set_plugin_state(
        &self,
        conn: &Connection,
        plugin_id: &str,
        enabled: bool,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let value = i64::from(enabled);
        conn.execute(
            "INSERT INTO plugin_states (plugin_id, enabled, updated_at) VALUES (?, ?, ?)
             ON CONFLICT(plugin_id) DO UPDATE SET
               enabled = excluded.enabled,
               updated_at = excluded.updated_at",
            libsql::params![plugin_id, value, format_time(&now)],
        )
        .await
        .map_err(|err| StoreError::Database(format!("save plugin state: {err}")))?;
        Ok(())
    }
}
