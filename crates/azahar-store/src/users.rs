//! Users and the admin customer list.

use chrono::{DateTime, Utc};
use libsql::{Connection, Row};

use crate::store::{Store, StoreError};
use crate::types::{CustomerRow, User, format_time, format_time_or_zero, null_time, parse_time};

const USER_COLUMNS: &str =
    "id, name, email, password_hash, role, theme_preset, created_at, updated_at";

impl Store {
    pub async fn count_users(&self) -> Result<i64, StoreError> {
        let mut rows = self.conn().query("SELECT COUNT(*) FROM users", ()).await?;
        let row = rows.next().await?.ok_or(StoreError::NotFound)?;
        Ok(row.get(0)?)
    }

    pub async fn create_user(&self, conn: &Connection, user: &User) -> Result<(), StoreError> {
        conn.execute(
            "INSERT INTO users (id, name, email, password_hash, role, theme_preset, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            libsql::params![
                user.id.clone(),
                user.name.clone(),
                user.email.clone(),
                user.password_hash.clone(),
                user.role.clone(),
                default_preset(&user.theme_preset),
                format_time_or_zero(user.created_at.as_ref()),
                format_time_or_zero(user.updated_at.as_ref())
            ],
        )
        .await
        .map_err(|err| StoreError::Database(format!("create user: {err}")))?;
        Ok(())
    }

    pub async fn user_by_email(&self, email: &str) -> Result<User, StoreError> {
        let mut rows = self
            .conn()
            .query(
                &format!("SELECT {USER_COLUMNS} FROM users WHERE email = ?"),
                libsql::params![email],
            )
            .await?;
        match rows.next().await? {
            Some(row) => scan_user(&row),
            None => Err(StoreError::NotFound),
        }
    }

    pub async fn user_by_id(&self, id: &str) -> Result<User, StoreError> {
        let mut rows = self
            .conn()
            .query(
                &format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?"),
                libsql::params![id],
            )
            .await?;
        match rows.next().await? {
            Some(row) => scan_user(&row),
            None => Err(StoreError::NotFound),
        }
    }

    /// Every account with staff first, then the newest accounts. The admin
    /// Team screen uses it to assign roles.
    pub async fn list_users(&self) -> Result<Vec<User>, StoreError> {
        let mut rows = self
            .conn()
            .query(
                &format!(
                    "SELECT {USER_COLUMNS}
                     FROM users
                     ORDER BY CASE role WHEN 'admin' THEN 0 WHEN 'employee' THEN 1 ELSE 2 END,
                              created_at DESC, email ASC"
                ),
                (),
            )
            .await
            .map_err(|err| StoreError::Database(format!("list users: {err}")))?;
        let mut users = Vec::new();
        while let Some(row) = rows.next().await? {
            users.push(
                scan_user(&row).map_err(|err| StoreError::Database(format!("scan user: {err}")))?,
            );
        }
        Ok(users)
    }

    pub async fn update_user_role(
        &self,
        conn: &Connection,
        user_id: &str,
        role: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let affected = conn
            .execute(
                "UPDATE users SET role = ?, updated_at = ? WHERE id = ?",
                libsql::params![role, format_time(&now), user_id],
            )
            .await
            .map_err(|err| StoreError::Database(format!("update user role: {err}")))?;
        if affected == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    pub async fn count_admins(&self, conn: &Connection) -> Result<i64, StoreError> {
        let mut rows = conn
            .query("SELECT COUNT(*) FROM users WHERE role = 'admin'", ())
            .await
            .map_err(|err| StoreError::Database(format!("count admins: {err}")))?;
        let row = rows.next().await?.ok_or(StoreError::NotFound)?;
        row.get(0)
            .map_err(|err| StoreError::Database(format!("count admins: {err}")))
    }

    pub async fn update_theme(
        &self,
        conn: &Connection,
        user_id: &str,
        preset: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let affected = conn
            .execute(
                "UPDATE users SET theme_preset = ?, updated_at = ? WHERE id = ?",
                libsql::params![preset, format_time(&now), user_id],
            )
            .await
            .map_err(|err| StoreError::Database(format!("update theme: {err}")))?;
        if affected == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    pub async fn list_customers(&self) -> Result<Vec<CustomerRow>, StoreError> {
        let mut rows = self
            .conn()
            .query(
                "SELECT u.id, u.name, u.email, u.created_at,
                        COUNT(CASE WHEN o.id IS NOT NULL AND o.status <> 'cancelled' THEN 1 END),
                        COALESCE(SUM(CASE WHEN o.status <> 'cancelled' THEN o.total_cents ELSE 0 END), 0),
                        COALESCE(MAX(CASE WHEN o.status <> 'cancelled' THEN o.created_at END), '')
                 FROM users u
                 LEFT JOIN orders o ON o.user_id = u.id
                 WHERE u.role = 'customer'
                 GROUP BY u.id, u.name, u.email, u.created_at
                 ORDER BY u.created_at DESC, u.email ASC",
                (),
            )
            .await
            .map_err(|err| StoreError::Database(format!("list customers: {err}")))?;
        let mut customers = Vec::new();
        while let Some(row) = rows.next().await? {
            let created_at: String = row
                .get(3)
                .map_err(|err| StoreError::Database(format!("scan customer: {err}")))?;
            let last_order_at: String = row
                .get(6)
                .map_err(|err| StoreError::Database(format!("scan customer: {err}")))?;
            customers.push(CustomerRow {
                id: row.get(0)?,
                name: row.get(1)?,
                email: row.get(2)?,
                orders_count: row.get(4)?,
                total_spent_cents: row.get(5)?,
                last_order_at: null_time(&last_order_at),
                created_at: parse_time(&created_at),
            });
        }
        Ok(customers)
    }
}

fn scan_user(row: &Row) -> Result<User, StoreError> {
    let created_at: String = row.get(6)?;
    let updated_at: String = row.get(7)?;
    Ok(User {
        id: row.get(0)?,
        name: row.get(1)?,
        email: row.get(2)?,
        password_hash: row.get(3)?,
        role: row.get(4)?,
        theme_preset: row.get(5)?,
        created_at: parse_time(&created_at),
        updated_at: parse_time(&updated_at),
    })
}

pub fn default_preset(preset: &str) -> String {
    if preset.is_empty() {
        "cellar".to_string()
    } else {
        preset.to_string()
    }
}
