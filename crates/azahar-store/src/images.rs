//! Stored product images.
//!
//! Ported from `backend/internal/store/images.go`. The Go model carries the
//! uploader in `CreatedBy`; the shared Rust model has no such field, so the
//! write takes the actor id separately.

use libsql::Connection;

use crate::store::{Store, StoreError};
use crate::types::{ProductImage, format_time_or_zero, parse_time};

impl Store {
    pub async fn create_product_image(
        &self,
        conn: &Connection,
        image: &ProductImage,
        created_by: &str,
    ) -> Result<(), StoreError> {
        conn.execute(
            "INSERT INTO product_images (id, mime, bytes, created_by, created_at)
             VALUES (?, ?, ?, ?, ?)",
            libsql::params![
                image.id.clone(),
                image.content_type.clone(),
                image.data.clone(),
                created_by,
                format_time_or_zero(image.created_at.as_ref())
            ],
        )
        .await
        .map_err(|err| StoreError::Database(format!("create product image: {err}")))?;
        Ok(())
    }

    pub async fn product_image(&self, id: &str) -> Result<ProductImage, StoreError> {
        let mut rows = self
            .conn()
            .query(
                "SELECT id, mime, bytes, created_by, created_at FROM product_images WHERE id = ?",
                libsql::params![id],
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(StoreError::NotFound);
        };
        let created_at: String = row.get(4)?;
        Ok(ProductImage {
            id: row.get(0)?,
            content_type: row.get(1)?,
            data: row.get(2)?,
            created_at: parse_time(&created_at),
        })
    }
}
