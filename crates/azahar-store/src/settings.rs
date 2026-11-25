//! Storefront and plugin settings blobs.

use chrono::{DateTime, Utc};
use libsql::Connection;

use crate::store::{Store, StoreError};
use crate::types::{Timestamp, format_time, parse_time};

impl Store {
    pub async fn plugin_settings(
        &self,
        plugin_id: &str,
    ) -> Result<(String, Timestamp), StoreError> {
        let mut rows = self
            .conn()
            .query(
                "SELECT settings, updated_at FROM plugin_settings WHERE plugin_id = ?",
                libsql::params![plugin_id],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(StoreError::NotFound);
        };
        let updated_at: String = row.get(1)?;
        Ok((row.get(0)?, parse_time(&updated_at)))
    }

    pub async fn save_plugin_settings(
        &self,
        conn: &Connection,
        plugin_id: &str,
        payload: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        conn.execute(
            "INSERT INTO plugin_settings (plugin_id, settings, updated_at) VALUES (?, ?, ?)
             ON CONFLICT(plugin_id) DO UPDATE SET settings = excluded.settings, updated_at = excluded.updated_at",
            libsql::params![plugin_id, payload, format_time(&now)],
        )
        .await
        .map_err(|err| StoreError::Database(format!("save plugin settings: {err}")))?;
        Ok(())
    }

    pub async fn storefront_settings(&self) -> Result<(String, Timestamp), StoreError> {
        self.storefront_settings_on(self.conn()).await
    }

    /// Reads the theme inside a transaction so a writer can compare it with
    /// the preview baseline before saving.
    pub async fn storefront_settings_tx(
        &self,
        conn: &Connection,
    ) -> Result<(String, Timestamp), StoreError> {
        self.storefront_settings_on(conn).await
    }

    async fn storefront_settings_on(
        &self,
        conn: &Connection,
    ) -> Result<(String, Timestamp), StoreError> {
        let mut rows = conn
            .query(
                "SELECT data, updated_at FROM storefront_settings WHERE id = 'default'",
                (),
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(StoreError::NotFound);
        };
        let updated_at: String = row.get(1)?;
        Ok((row.get(0)?, parse_time(&updated_at)))
    }

    pub async fn save_storefront_settings(
        &self,
        conn: &Connection,
        payload: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        conn.execute(
            "INSERT INTO storefront_settings (id, data, updated_at) VALUES ('default', ?, ?)
             ON CONFLICT(id) DO UPDATE SET data = excluded.data, updated_at = excluded.updated_at",
            libsql::params![payload, format_time(&now)],
        )
        .await
        .map_err(|err| StoreError::Database(format!("save storefront settings: {err}")))?;
        Ok(())
    }
}
