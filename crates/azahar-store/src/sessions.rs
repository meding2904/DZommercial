//! Session rows used by the Go oracle while both services run.

use chrono::{DateTime, Utc};

use crate::store::{Store, StoreError};
use crate::types::{Session, User, format_time, format_time_or_zero, parse_time};

impl Store {
    pub async fn create_session(
        &self,
        conn: &libsql::Connection,
        session: &Session,
    ) -> Result<(), StoreError> {
        conn.execute(
            "INSERT INTO sessions (id, user_id, token_hash, user_agent, ip, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            libsql::params![
                session.id.clone(),
                session.user_id.clone(),
                session.token_hash.clone(),
                session.user_agent.clone(),
                session.ip.clone(),
                format_time_or_zero(session.created_at.as_ref()),
                format_time_or_zero(session.expires_at.as_ref())
            ],
        )
        .await
        .map_err(|err| StoreError::Database(format!("create session: {err}")))?;
        Ok(())
    }

    pub async fn session_user_by_hash(
        &self,
        token_hash: &str,
    ) -> Result<(User, Session, bool), StoreError> {
        let mut rows = self
            .conn()
            .query(
                "SELECT s.id, s.user_id, s.token_hash, s.user_agent, s.ip, s.created_at, s.expires_at,
                        u.id, u.name, u.email, u.password_hash, u.role, u.theme_preset, u.created_at, u.updated_at
                 FROM sessions s JOIN users u ON u.id = s.user_id
                 WHERE s.token_hash = ?",
                libsql::params![token_hash],
            )
            .await
            .map_err(|err| StoreError::Database(format!("load session: {err}")))?;
        let Some(row) = rows.next().await? else {
            return Ok((User::default(), Session::default(), false));
        };

        let session_created: String = row.get(5)?;
        let session_expires: String = row.get(6)?;
        let user_created: String = row.get(13)?;
        let user_updated: String = row.get(14)?;
        let session = Session {
            id: row.get(0)?,
            user_id: row.get(1)?,
            token_hash: row.get(2)?,
            user_agent: row.get(3)?,
            ip: row.get(4)?,
            created_at: parse_time(&session_created),
            expires_at: parse_time(&session_expires),
        };
        let user = User {
            id: row.get(7)?,
            name: row.get(8)?,
            email: row.get(9)?,
            password_hash: row.get(10)?,
            role: row.get(11)?,
            theme_preset: row.get(12)?,
            created_at: parse_time(&user_created),
            updated_at: parse_time(&user_updated),
        };
        Ok((user, session, true))
    }

    pub async fn delete_session_by_hash(
        &self,
        conn: &libsql::Connection,
        token_hash: &str,
    ) -> Result<(), StoreError> {
        conn.execute(
            "DELETE FROM sessions WHERE token_hash = ?",
            libsql::params![token_hash],
        )
        .await?;
        Ok(())
    }

    pub async fn delete_sessions_for_user_agent(
        &self,
        conn: &libsql::Connection,
        user_id: &str,
        user_agent: &str,
    ) -> Result<(), StoreError> {
        if user_agent.is_empty() {
            return Ok(());
        }
        conn.execute(
            "DELETE FROM sessions WHERE user_id = ? AND user_agent = ?",
            libsql::params![user_id, user_agent],
        )
        .await?;
        Ok(())
    }

    pub async fn delete_expired_sessions(
        &self,
        conn: &libsql::Connection,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        conn.execute(
            "DELETE FROM sessions WHERE expires_at < ?",
            libsql::params![format_time(&now)],
        )
        .await?;
        Ok(())
    }
}
