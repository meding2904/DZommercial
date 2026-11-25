//! Products, categories and stock, ported from
//! `backend/internal/store/products.go`.

use chrono::{DateTime, Utc};
use libsql::{Connection, Row, params, params::Params};

use crate::store::{Store, StoreError};
use crate::types::{
    Category, Product, decode_json, decode_string_list_map, encode_json, encode_string_list_map,
    format_optional_time, format_time, format_time_or_zero, parse_time,
};

/// Storefront filter for [`Store::list_products`]. Empty strings and a
/// non-positive limit fall back to the same defaults as the Go store.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProductFilter {
    pub category: String,
    pub plugin_id: String,
    pub concern: String,
    pub query: String,
    /// `price_asc`, `price_desc`, `new`, or empty for featured-first.
    pub sort: String,
    pub include_inactive: bool,
    pub limit: i32,
}

pub(crate) const PRODUCT_COLUMNS: &str = "p.id, p.slug, p.name, p.subtitle, p.description,
	p.price_cents, p.compare_at_cents, p.sale_price_cents, p.sale_starts_at, p.sale_ends_at,
	p.currency, p.size,
	p.concerns, p.ingredients, p.how_to, p.image, p.gallery,
	p.stock, p.featured, p.active, p.plugin_id, p.attributes,
	p.created_at, p.updated_at,
	c.id, c.slug, c.name, c.description, c.plugin_id";

/// Mirrors `Product.EffectivePriceCents` for SQL ordering. The Go store binds
/// the same instant to both placeholders.
pub(crate) const EFFECTIVE_PRICE_ORDER: &str = "(CASE
	WHEN p.sale_price_cents IS NOT NULL
	 AND p.sale_price_cents > 0
	 AND p.sale_price_cents < p.price_cents
	 AND p.sale_starts_at <> ''
	 AND p.sale_ends_at <> ''
	 AND p.sale_starts_at <= ?
	 AND p.sale_ends_at > ?
	THEN p.sale_price_cents ELSE p.price_cents END)";

/// Marks a product that uses no plugin product type.
pub const CORE_PLUGIN_ID: &str = "core";

pub fn normalize_plugin_id(plugin_id: &str) -> String {
    if plugin_id.is_empty() {
        CORE_PLUGIN_ID.to_string()
    } else {
        plugin_id.to_string()
    }
}

pub(crate) fn default_currency(currency: &str) -> String {
    if currency.is_empty() {
        "DZD".to_string()
    } else {
        currency.to_string()
    }
}

fn bool_int(value: bool) -> i64 {
    i64::from(value)
}

impl Store {
    pub async fn categories(&self) -> Result<Vec<Category>, StoreError> {
        let mut rows = self
            .conn()
            .query(
                "SELECT id, slug, name, description, sort_order, plugin_id FROM categories
                 ORDER BY plugin_id, sort_order, name",
                (),
            )
            .await
            .map_err(|err| StoreError::Database(format!("list categories: {err}")))?;
        scan_categories(&mut rows).await
    }

    /// Categories with at least one active product, so a plugin does not leave
    /// an empty shelf in the storefront navigation.
    pub async fn public_categories(&self) -> Result<Vec<Category>, StoreError> {
        let mut rows = self
            .conn()
            .query(
                "SELECT c.id, c.slug, c.name, c.description, c.sort_order, c.plugin_id
                 FROM categories c
                 WHERE EXISTS (SELECT 1 FROM products p WHERE p.category_id = c.id AND p.active = 1)
                 ORDER BY c.plugin_id, c.sort_order, c.name",
                (),
            )
            .await
            .map_err(|err| StoreError::Database(format!("list public categories: {err}")))?;
        scan_categories(&mut rows).await
    }

    pub async fn category_by_slug(&self, slug: &str) -> Result<Category, StoreError> {
        self.category_by_slug_tx(self.conn(), slug).await
    }

    /// A plugin owns the name, description and order of its categories; the
    /// slug is shared so a core product keeps one category foreign key.
    pub async fn upsert_category(
        &self,
        plugin_id: &str,
        category: &Category,
    ) -> Result<(), StoreError> {
        let id = format!("cat_{plugin_id}_{}", category.slug.replace('-', "_"));
        let affected = self
            .conn()
            .execute(
                "INSERT INTO categories (id, slug, name, description, sort_order, plugin_id)
                 VALUES (?, ?, ?, ?, ?, ?)
                 ON CONFLICT(slug) DO UPDATE SET
                   name = excluded.name,
                   description = excluded.description,
                   sort_order = excluded.sort_order
                 WHERE categories.plugin_id = excluded.plugin_id",
                params![
                    id,
                    category.slug.clone(),
                    category.name.clone(),
                    category.description.clone(),
                    category.sort_order,
                    plugin_id
                ],
            )
            .await
            .map_err(|err| StoreError::Database(format!("upsert category: {err}")))?;
        if affected == 0 {
            return Err(StoreError::Database(format!(
                "category {:?} belongs to another plugin",
                category.slug
            )));
        }
        Ok(())
    }

    /// Moves a product to its plugin's first category when an older category
    /// belongs to another plugin.
    pub async fn reconcile_product_categories(&self) -> Result<(), StoreError> {
        self.conn()
            .execute(
                "UPDATE products SET category_id = (
                   SELECT c.id FROM categories c
                   WHERE c.plugin_id = products.plugin_id
                   ORDER BY c.sort_order, c.slug LIMIT 1
                 )
                 WHERE EXISTS (
                   SELECT 1 FROM categories c WHERE c.plugin_id = products.plugin_id
                 )
                 AND category_id NOT IN (
                   SELECT id FROM categories WHERE plugin_id = products.plugin_id
                 )",
                (),
            )
            .await
            .map_err(|err| StoreError::Database(format!("reconcile product categories: {err}")))?;
        Ok(())
    }

    pub async fn category_by_slug_tx(
        &self,
        conn: &Connection,
        slug: &str,
    ) -> Result<Category, StoreError> {
        let mut rows = conn
            .query(
                "SELECT id, slug, name, description, sort_order, plugin_id FROM categories WHERE slug = ?",
                params![slug],
            )
            .await?;
        match rows.next().await? {
            Some(row) => scan_category(&row),
            None => Err(StoreError::NotFound),
        }
    }

    pub async fn count_products(&self) -> Result<i64, StoreError> {
        let mut rows = self
            .conn()
            .query("SELECT COUNT(*) FROM products", ())
            .await?;
        let row = rows.next().await?.ok_or(StoreError::NotFound)?;
        Ok(row.get(0)?)
    }

    pub async fn list_products(&self, filter: &ProductFilter) -> Result<Vec<Product>, StoreError> {
        let mut where_clauses: Vec<&str> = Vec::new();
        let mut values: Vec<libsql::Value> = Vec::new();

        if !filter.include_inactive {
            where_clauses.push("p.active = 1");
        }
        if !filter.category.is_empty() {
            where_clauses.push("c.slug = ?");
            values.push(libsql::Value::Text(filter.category.clone()));
        }
        if !filter.plugin_id.is_empty() {
            where_clauses.push("p.plugin_id = ?");
            values.push(libsql::Value::Text(filter.plugin_id.clone()));
        }
        if !filter.concern.is_empty() {
            where_clauses.push(
                "(EXISTS (SELECT 1 FROM json_each(p.concerns) AS j WHERE j.value = ?)
                  OR EXISTS (SELECT 1 FROM json_each(json_extract(p.attributes, '$.concerns')) AS j WHERE j.value = ?))",
            );
            values.push(libsql::Value::Text(filter.concern.clone()));
            values.push(libsql::Value::Text(filter.concern.clone()));
        }
        if !filter.query.is_empty() {
            where_clauses.push("(p.name LIKE ? OR p.subtitle LIKE ? OR p.description LIKE ?)");
            let like = format!("%{}%", filter.query);
            values.push(libsql::Value::Text(like.clone()));
            values.push(libsql::Value::Text(like.clone()));
            values.push(libsql::Value::Text(like));
        }

        let mut order = "p.featured DESC, p.created_at DESC".to_string();
        let now = format_time(&Utc::now());
        match filter.sort.as_str() {
            "price_asc" => {
                order = format!("{EFFECTIVE_PRICE_ORDER} ASC, p.name ASC");
                values.push(libsql::Value::Text(now.clone()));
                values.push(libsql::Value::Text(now));
            }
            "price_desc" => {
                order = format!("{EFFECTIVE_PRICE_ORDER} DESC, p.name ASC");
                values.push(libsql::Value::Text(now.clone()));
                values.push(libsql::Value::Text(now));
            }
            "new" => order = "p.created_at DESC, p.name ASC".to_string(),
            _ => {}
        }

        let mut query = format!(
            "SELECT {PRODUCT_COLUMNS}
		FROM products p JOIN categories c ON c.id = p.category_id"
        );
        if !where_clauses.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&where_clauses.join(" AND "));
        }
        query.push_str(" ORDER BY ");
        query.push_str(&order);

        let limit = if filter.limit <= 0 || filter.limit > 500 {
            200
        } else {
            filter.limit
        };
        query.push_str(" LIMIT ?");
        values.push(libsql::Value::Integer(i64::from(limit)));

        let mut rows = self
            .conn()
            .query(&query, Params::Positional(values))
            .await
            .map_err(|err| StoreError::Database(format!("list products: {err}")))?;

        let mut products = Vec::new();
        while let Some(row) = rows.next().await? {
            let product = scan_product(&row)
                .map_err(|err| StoreError::Database(format!("scan product: {err}")))?;
            products.push(product);
        }
        Ok(products)
    }

    pub async fn product_by_slug(
        &self,
        slug: &str,
        active_only: bool,
    ) -> Result<Product, StoreError> {
        let mut query = format!(
            "SELECT {PRODUCT_COLUMNS}
		FROM products p JOIN categories c ON c.id = p.category_id WHERE p.slug = ?"
        );
        if active_only {
            query.push_str(" AND p.active = 1");
        }
        let mut rows = self.conn().query(&query, params![slug]).await?;
        match rows.next().await? {
            Some(row) => scan_product(&row),
            None => Err(StoreError::NotFound),
        }
    }

    pub async fn product_by_id(&self, id: &str, active_only: bool) -> Result<Product, StoreError> {
        self.product_by_id_tx(self.conn(), id, active_only).await
    }

    pub async fn product_by_id_tx(
        &self,
        conn: &Connection,
        id: &str,
        active_only: bool,
    ) -> Result<Product, StoreError> {
        let mut query = format!(
            "SELECT {PRODUCT_COLUMNS}
		FROM products p JOIN categories c ON c.id = p.category_id WHERE p.id = ?"
        );
        if active_only {
            query.push_str(" AND p.active = 1");
        }
        let mut rows = conn.query(&query, params![id]).await?;
        match rows.next().await? {
            Some(row) => scan_product(&row),
            None => Err(StoreError::NotFound),
        }
    }

    /// Returns whether the stock was sufficient; a short fall leaves the row
    /// untouched.
    pub async fn decrement_stock(
        &self,
        conn: &Connection,
        product_id: &str,
        quantity: i32,
    ) -> Result<bool, StoreError> {
        let affected = conn
            .execute(
                "UPDATE products SET stock = stock - ?, updated_at = ? WHERE id = ? AND stock >= ?",
                params![quantity, format_time(&Utc::now()), product_id, quantity],
            )
            .await
            .map_err(|err| StoreError::Database(format!("decrement stock: {err}")))?;
        Ok(affected == 1)
    }

    pub async fn increment_stock(
        &self,
        conn: &Connection,
        product_id: &str,
        quantity: i32,
    ) -> Result<(), StoreError> {
        if quantity <= 0 {
            return Ok(());
        }
        conn.execute(
            "UPDATE products SET stock = stock + ?, updated_at = ? WHERE id = ?",
            params![quantity, format_time(&Utc::now()), product_id],
        )
        .await
        .map_err(|err| StoreError::Database(format!("increment stock: {err}")))?;
        Ok(())
    }

    pub async fn related_products(
        &self,
        category_id: &str,
        exclude_id: &str,
        limit: i32,
    ) -> Result<Vec<Product>, StoreError> {
        let limit = if limit <= 0 { 4 } else { limit };
        let mut rows = self
            .conn()
            .query(
                &format!(
                    "SELECT {PRODUCT_COLUMNS}
		 FROM products p JOIN categories c ON c.id = p.category_id
		 WHERE p.active = 1 AND p.category_id = ? AND p.id <> ?
		 ORDER BY p.featured DESC, p.created_at DESC LIMIT ?"
                ),
                params![category_id, exclude_id, i64::from(limit)],
            )
            .await
            .map_err(|err| StoreError::Database(format!("related products: {err}")))?;

        let mut products = Vec::new();
        while let Some(row) = rows.next().await? {
            let product = scan_product(&row)
                .map_err(|err| StoreError::Database(format!("scan related product: {err}")))?;
            products.push(product);
        }
        Ok(products)
    }

    pub async fn create_product(
        &self,
        conn: &Connection,
        product: &Product,
    ) -> Result<(), StoreError> {
        conn.execute(
            "INSERT INTO products (id, slug, name, subtitle, description, price_cents,
             compare_at_cents, sale_price_cents, sale_starts_at, sale_ends_at, currency, size,
             category_id, concerns, ingredients,
             how_to, image, gallery, stock, featured, active, plugin_id, attributes,
             created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                product.id.clone(),
                product.slug.clone(),
                product.name.clone(),
                product.subtitle.clone(),
                product.description.clone(),
                product.price_cents,
                product.compare_at_cents,
                product.sale_price_cents,
                format_optional_time(product.sale_starts_at.as_ref()),
                format_optional_time(product.sale_ends_at.as_ref()),
                default_currency(&product.currency),
                product.size.clone(),
                product.category.id.clone(),
                encode_json(&product.concerns),
                encode_json(&product.ingredients),
                product.how_to.clone(),
                product.image.clone(),
                encode_json(&product.gallery),
                product.stock,
                bool_int(product.featured),
                bool_int(product.active),
                normalize_plugin_id(&product.plugin_id),
                encode_string_list_map(&product.attributes),
                format_time_or_zero(product.created_at.as_ref()),
                format_time_or_zero(product.updated_at.as_ref())
            ],
        )
        .await
        .map_err(|err| StoreError::Database(format!("create product: {err}")))?;
        Ok(())
    }

    pub async fn update_product(
        &self,
        conn: &Connection,
        product: &Product,
    ) -> Result<(), StoreError> {
        let affected = conn
            .execute(
                "UPDATE products SET slug = ?, name = ?, subtitle = ?, description = ?,
                 price_cents = ?, compare_at_cents = ?, sale_price_cents = ?, sale_starts_at = ?,
                 sale_ends_at = ?, currency = ?, size = ?, category_id = ?,
                 concerns = ?, ingredients = ?, how_to = ?, image = ?, gallery = ?, stock = ?,
                 featured = ?, active = ?, plugin_id = ?, attributes = ?, updated_at = ?
                 WHERE id = ?",
                params![
                    product.slug.clone(),
                    product.name.clone(),
                    product.subtitle.clone(),
                    product.description.clone(),
                    product.price_cents,
                    product.compare_at_cents,
                    product.sale_price_cents,
                    format_optional_time(product.sale_starts_at.as_ref()),
                    format_optional_time(product.sale_ends_at.as_ref()),
                    default_currency(&product.currency),
                    product.size.clone(),
                    product.category.id.clone(),
                    encode_json(&product.concerns),
                    encode_json(&product.ingredients),
                    product.how_to.clone(),
                    product.image.clone(),
                    encode_json(&product.gallery),
                    product.stock,
                    bool_int(product.featured),
                    bool_int(product.active),
                    normalize_plugin_id(&product.plugin_id),
                    encode_string_list_map(&product.attributes),
                    format_time_or_zero(product.updated_at.as_ref()),
                    product.id.clone()
                ],
            )
            .await
            .map_err(|err| StoreError::Database(format!("update product: {err}")))?;
        if affected == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    pub async fn set_product_active(
        &self,
        conn: &Connection,
        id: &str,
        active: bool,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let affected = conn
            .execute(
                "UPDATE products SET active = ?, updated_at = ? WHERE id = ?",
                params![bool_int(active), format_time(&now), id],
            )
            .await
            .map_err(|err| StoreError::Database(format!("archive product: {err}")))?;
        if affected == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }
}

async fn scan_categories(rows: &mut libsql::Rows) -> Result<Vec<Category>, StoreError> {
    let mut categories = Vec::new();
    while let Some(row) = rows.next().await? {
        categories.push(
            scan_category(&row)
                .map_err(|err| StoreError::Database(format!("scan category: {err}")))?,
        );
    }
    Ok(categories)
}

fn scan_category(row: &Row) -> Result<Category, StoreError> {
    Ok(Category {
        id: row.get(0)?,
        slug: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        sort_order: row.get(4)?,
        plugin_id: row.get(5)?,
    })
}

fn scan_product(row: &Row) -> Result<Product, StoreError> {
    let compare_at_cents: Option<i64> = row.get(6)?;
    let sale_price_cents: Option<i64> = row.get(7)?;
    let sale_starts_at: String = row.get(8)?;
    let sale_ends_at: String = row.get(9)?;
    let concerns: String = row.get(12)?;
    let ingredients: String = row.get(13)?;
    let gallery: String = row.get(16)?;
    let featured: i64 = row.get(18)?;
    let active: i64 = row.get(19)?;
    let attributes: String = row.get(21)?;
    let created_at: String = row.get(22)?;
    let updated_at: String = row.get(23)?;

    Ok(Product {
        id: row.get(0)?,
        slug: row.get(1)?,
        name: row.get(2)?,
        subtitle: row.get(3)?,
        description: row.get(4)?,
        price_cents: row.get(5)?,
        compare_at_cents,
        sale_price_cents,
        sale_starts_at: parse_time(&sale_starts_at),
        sale_ends_at: parse_time(&sale_ends_at),
        currency: row.get(10)?,
        size: row.get(11)?,
        category: Category {
            id: row.get(24)?,
            slug: row.get(25)?,
            name: row.get(26)?,
            description: row.get(27)?,
            plugin_id: row.get(28)?,
            sort_order: 0,
        },
        concerns: decode_json(&concerns),
        ingredients: decode_json(&ingredients),
        how_to: row.get(14)?,
        image: row.get(15)?,
        gallery: decode_json(&gallery),
        stock: row.get(17)?,
        featured: featured != 0,
        active: active != 0,
        plugin_id: row.get(20)?,
        attributes: decode_string_list_map(&attributes),
        created_at: parse_time(&created_at),
        updated_at: parse_time(&updated_at),
    })
}
