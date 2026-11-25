//! Order writes, order reads and the admin order list.
//!
//! Ported from `backend/internal/store/orders.go`. Reads that Go does through
//! `s.db` use [`Store::conn`]; functions whose Go signature takes a `querier`
//! take a [`Connection`] so callers can read inside a write transaction.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use libsql::{Connection, Row, Value, params_from_iter};

use crate::products::{default_currency, normalize_plugin_id};
use crate::store::{Store, StoreError};
use crate::types::{
    CourierEvent, Order, OrderItem, decode_courier_events, decode_string_map,
    encode_courier_events, encode_string_map, format_optional_time, format_time,
    format_time_or_zero, parse_time,
};

const ORDER_COLUMNS: &str = "o.id, o.number, o.user_id, o.name, o.email, o.phone,
    o.address, o.city, o.notes, o.subtotal_cents, o.shipping_cents,
    o.total_cents, o.currency, o.status, o.token_hash, o.created_at, o.updated_at,
    o.wilaya_code, o.wilaya_name, o.commune_name, o.delivery_type,
    o.stop_desk_id, o.stop_desk_name, o.courier_key, o.tracking_number,
    o.courier_status, o.courier_events, o.courier_synced_at";

const ORDER_ITEM_COLUMNS: &str = "id, order_id, product_id, slug, name, image, unit_price_cents,
    quantity, line_total_cents, options, options_label, plugin_id";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdminOrderFilter {
    pub status: String,
    pub query: String,
    pub limit: i64,
}

impl Store {
    pub async fn count_orders(&self) -> Result<i64, StoreError> {
        let mut rows = self.conn().query("SELECT COUNT(*) FROM orders", ()).await?;
        let row = rows.next().await?.ok_or(StoreError::NotFound)?;
        Ok(row.get(0)?)
    }

    pub async fn next_order_number(
        &self,
        conn: &Connection,
        year: i32,
    ) -> Result<String, StoreError> {
        conn.execute(
            "INSERT INTO order_sequences (year, last_value) VALUES (?, 1)
             ON CONFLICT(year) DO UPDATE SET last_value = last_value + 1",
            libsql::params![year],
        )
        .await
        .map_err(|err| StoreError::Database(format!("advance order sequence: {err}")))?;

        let mut rows = conn
            .query(
                "SELECT last_value FROM order_sequences WHERE year = ?",
                libsql::params![year],
            )
            .await
            .map_err(|err| StoreError::Database(format!("read order sequence: {err}")))?;
        let row = rows.next().await?.ok_or(StoreError::NotFound)?;
        let last: i64 = row
            .get(0)
            .map_err(|err| StoreError::Database(format!("read order sequence: {err}")))?;
        Ok(format!("AZ-{year}-{last:04}"))
    }

    pub async fn create_order(
        &self,
        conn: &Connection,
        order: &Order,
        items: &[OrderItem],
    ) -> Result<(), StoreError> {
        let delivery_type = if order.delivery_type.is_empty() {
            "home"
        } else {
            order.delivery_type.as_str()
        };
        conn.execute(
            "INSERT INTO orders (id, number, user_id, name, email, phone, address, city,
             notes, subtotal_cents, shipping_cents, total_cents, currency, status,
             token_hash, created_at, updated_at, wilaya_code, wilaya_name, commune_name,
             delivery_type, stop_desk_id, stop_desk_name, courier_key, tracking_number,
             courier_status, courier_events, courier_synced_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            libsql::params![
                order.id.clone(),
                order.number.clone(),
                order.user_id.clone(),
                order.name.clone(),
                order.email.clone(),
                order.phone.clone(),
                order.address.clone(),
                order.city.clone(),
                order.notes.clone(),
                order.subtotal_cents,
                order.shipping_cents,
                order.total_cents,
                default_currency(&order.currency),
                order.status.clone(),
                order.token_hash.clone(),
                format_time_or_zero(order.created_at.as_ref()),
                format_time_or_zero(order.updated_at.as_ref()),
                order.wilaya_code,
                order.wilaya_name.clone(),
                order.commune_name.clone(),
                delivery_type,
                order.stop_desk_id.clone(),
                order.stop_desk_name.clone(),
                order.courier_key.clone(),
                order.tracking_number.clone(),
                order.courier_status.clone(),
                encode_courier_events(&order.courier_events),
                format_optional_time(order.courier_synced_at.as_ref()),
            ],
        )
        .await
        .map_err(|err| StoreError::Database(format!("create order: {err}")))?;

        for item in items {
            conn.execute(
                "INSERT INTO order_items (id, order_id, product_id, slug, name, image,
                 unit_price_cents, quantity, line_total_cents, options, options_label, plugin_id)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                libsql::params![
                    item.id.clone(),
                    order.id.clone(),
                    item.product_id.clone(),
                    item.slug.clone(),
                    item.name.clone(),
                    item.image.clone(),
                    item.unit_price_cents,
                    item.quantity,
                    item.line_total_cents,
                    encode_string_map(&item.options),
                    item.options_label.clone(),
                    normalize_plugin_id(&item.plugin_id),
                ],
            )
            .await
            .map_err(|err| StoreError::Database(format!("create order item: {err}")))?;
        }
        Ok(())
    }

    pub async fn order_by_token_hash(&self, token_hash: &str) -> Result<Order, StoreError> {
        load_order(
            self.conn(),
            &format!("SELECT {ORDER_COLUMNS} FROM orders o WHERE o.token_hash = ?"),
            vec![Value::from(token_hash)],
        )
        .await
    }

    pub async fn order_by_id(&self, id: &str) -> Result<Order, StoreError> {
        self.order_by_id_on(self.conn(), id).await
    }

    /// Reads an order inside a transaction, so callers that change stock or
    /// status do not act on a row read outside the write boundary.
    pub async fn order_by_id_tx(&self, conn: &Connection, id: &str) -> Result<Order, StoreError> {
        self.order_by_id_on(conn, id).await
    }

    async fn order_by_id_on(&self, conn: &Connection, id: &str) -> Result<Order, StoreError> {
        load_order(
            conn,
            &format!("SELECT {ORDER_COLUMNS} FROM orders o WHERE o.id = ?"),
            vec![Value::from(id)],
        )
        .await
    }

    pub async fn order_items(&self, order_id: &str) -> Result<Vec<OrderItem>, StoreError> {
        self.order_items_on(self.conn(), order_id).await
    }

    pub async fn order_items_tx(
        &self,
        conn: &Connection,
        order_id: &str,
    ) -> Result<Vec<OrderItem>, StoreError> {
        self.order_items_on(conn, order_id).await
    }

    async fn order_items_on(
        &self,
        conn: &Connection,
        order_id: &str,
    ) -> Result<Vec<OrderItem>, StoreError> {
        let mut rows = conn
            .query(
                &format!(
                    "SELECT {ORDER_ITEM_COLUMNS}
                     FROM order_items WHERE order_id = ? ORDER BY rowid"
                ),
                libsql::params![order_id],
            )
            .await
            .map_err(|err| StoreError::Database(format!("list order items: {err}")))?;
        let mut items = Vec::new();
        while let Some(row) = rows.next().await? {
            items.push(scan_order_item(&row)?);
        }
        Ok(items)
    }

    /// Reads the lines for a page of orders in one query. Orders without any
    /// lines get no entry in the map, like the Go implementation.
    pub async fn order_items_by_order_ids(
        &self,
        ids: &[String],
    ) -> Result<HashMap<String, Vec<OrderItem>>, StoreError> {
        let mut result = HashMap::with_capacity(ids.len());
        if ids.is_empty() {
            return Ok(result);
        }
        let placeholders = vec!["?"; ids.len()].join(",");
        let query = format!(
            "SELECT {ORDER_ITEM_COLUMNS}
             FROM order_items WHERE order_id IN ({placeholders}) ORDER BY rowid"
        );
        let args: Vec<Value> = ids.iter().cloned().map(Value::from).collect();
        let mut rows = self
            .conn()
            .query(&query, params_from_iter(args))
            .await
            .map_err(|err| StoreError::Database(format!("list order items: {err}")))?;
        while let Some(row) = rows.next().await? {
            let item = scan_order_item(&row)
                .map_err(|err| StoreError::Database(format!("scan order item: {err}")))?;
            result.entry(item.order_id.clone()).or_default().push(item);
        }
        Ok(result)
    }

    pub async fn orders_by_user(&self, user_id: &str) -> Result<Vec<Order>, StoreError> {
        let mut rows = self
            .conn()
            .query(
                &format!(
                    "SELECT {ORDER_COLUMNS} FROM orders o
                     WHERE o.user_id = ? ORDER BY o.created_at DESC"
                ),
                libsql::params![user_id],
            )
            .await
            .map_err(|err| StoreError::Database(format!("list customer orders: {err}")))?;
        let mut orders = Vec::new();
        while let Some(row) = rows.next().await? {
            orders.push(
                scan_order(&row)
                    .map_err(|err| StoreError::Database(format!("scan customer order: {err}")))?,
            );
        }
        Ok(orders)
    }

    pub async fn admin_orders(&self, filter: &AdminOrderFilter) -> Result<Vec<Order>, StoreError> {
        let mut where_clauses: Vec<&str> = Vec::new();
        let mut args: Vec<Value> = Vec::new();
        if !filter.status.is_empty() {
            where_clauses.push("o.status = ?");
            args.push(Value::from(filter.status.clone()));
        }
        if !filter.query.is_empty() {
            where_clauses.push(
                "(o.number LIKE ? OR o.name LIKE ? OR o.email LIKE ? OR o.phone LIKE ? OR o.tracking_number LIKE ?)",
            );
            let like = format!("%{}%", filter.query);
            for _ in 0..5 {
                args.push(Value::from(like.clone()));
            }
        }
        let mut query = format!("SELECT {ORDER_COLUMNS} FROM orders o");
        if !where_clauses.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&where_clauses.join(" AND "));
        }
        query.push_str(" ORDER BY o.created_at DESC");
        let limit = if filter.limit <= 0 || filter.limit > 500 {
            200
        } else {
            filter.limit
        };
        query.push_str(" LIMIT ?");
        args.push(Value::from(limit));

        let mut rows = self
            .conn()
            .query(&query, params_from_iter(args))
            .await
            .map_err(|err| StoreError::Database(format!("list admin orders: {err}")))?;
        let mut orders = Vec::new();
        while let Some(row) = rows.next().await? {
            orders.push(
                scan_order(&row)
                    .map_err(|err| StoreError::Database(format!("scan admin order: {err}")))?,
            );
        }
        Ok(orders)
    }

    pub async fn update_order_status(
        &self,
        conn: &Connection,
        id: &str,
        from: &str,
        to: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let affected = conn
            .execute(
                "UPDATE orders SET status = ?, updated_at = ? WHERE id = ? AND status = ?",
                libsql::params![to, format_time(&now), id, from],
            )
            .await
            .map_err(|err| StoreError::Database(format!("update order status: {err}")))?;
        if affected == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_shipment(
        &self,
        conn: &Connection,
        id: &str,
        courier_key: &str,
        tracking_number: &str,
        status: &str,
        events: &[CourierEvent],
        synced_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let affected = conn
            .execute(
                "UPDATE orders SET courier_key = ?, tracking_number = ?, courier_status = ?,
                 courier_events = ?, courier_synced_at = ?, updated_at = ? WHERE id = ?",
                libsql::params![
                    courier_key,
                    tracking_number,
                    status,
                    encode_courier_events(events),
                    format_optional_time(synced_at.as_ref()),
                    format_time(&now),
                    id,
                ],
            )
            .await
            .map_err(|err| StoreError::Database(format!("update shipment: {err}")))?;
        if affected == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    /// Attaches the first shipment to an order. The empty-tracking guard makes
    /// two concurrent create-shipment requests race safely: only the first write
    /// matches, so the loser gets `NotFound` instead of overwriting the parcel.
    #[allow(clippy::too_many_arguments)]
    pub async fn attach_shipment(
        &self,
        conn: &Connection,
        id: &str,
        courier_key: &str,
        tracking_number: &str,
        status: &str,
        events: &[CourierEvent],
        synced_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let affected = conn
            .execute(
                "UPDATE orders SET courier_key = ?, tracking_number = ?, courier_status = ?,
                 courier_events = ?, courier_synced_at = ?, updated_at = ?
                 WHERE id = ? AND tracking_number = ''",
                libsql::params![
                    courier_key,
                    tracking_number,
                    status,
                    encode_courier_events(events),
                    format_optional_time(synced_at.as_ref()),
                    format_time(&now),
                    id,
                ],
            )
            .await
            .map_err(|err| StoreError::Database(format!("attach shipment: {err}")))?;
        if affected == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    pub async fn update_tracking(
        &self,
        conn: &Connection,
        id: &str,
        status: &str,
        events: &[CourierEvent],
        synced_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let affected = conn
            .execute(
                "UPDATE orders SET courier_status = ?, courier_events = ?,
                 courier_synced_at = ?, updated_at = ? WHERE id = ?",
                libsql::params![
                    status,
                    encode_courier_events(events),
                    format_optional_time(synced_at.as_ref()),
                    format_time(&now),
                    id,
                ],
            )
            .await
            .map_err(|err| StoreError::Database(format!("update tracking: {err}")))?;
        if affected == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }
}

async fn load_order(conn: &Connection, query: &str, args: Vec<Value>) -> Result<Order, StoreError> {
    let mut rows = conn.query(query, params_from_iter(args)).await?;
    match rows.next().await? {
        Some(row) => scan_order(&row),
        None => Err(StoreError::NotFound),
    }
}

fn scan_order(row: &Row) -> Result<Order, StoreError> {
    let user_id: Option<String> = row.get(2)?;
    let created_at: String = row.get(15)?;
    let updated_at: String = row.get(16)?;
    let wilaya_code: Option<i64> = row.get(17)?;
    let courier_events: String = row.get(26)?;
    let courier_synced_at: String = row.get(27)?;
    Ok(Order {
        id: row.get(0)?,
        number: row.get(1)?,
        user_id,
        name: row.get(3)?,
        email: row.get(4)?,
        phone: row.get(5)?,
        address: row.get(6)?,
        city: row.get(7)?,
        notes: row.get(8)?,
        subtotal_cents: row.get(9)?,
        shipping_cents: row.get(10)?,
        total_cents: row.get(11)?,
        currency: row.get(12)?,
        status: row.get(13)?,
        token_hash: row.get(14)?,
        created_at: parse_time(&created_at),
        updated_at: parse_time(&updated_at),
        wilaya_code: wilaya_code
            .and_then(|code| i32::try_from(code).ok())
            .unwrap_or_default(),
        wilaya_name: row.get(18)?,
        commune_name: row.get(19)?,
        delivery_type: row.get(20)?,
        stop_desk_id: row.get(21)?,
        stop_desk_name: row.get(22)?,
        courier_key: row.get(23)?,
        tracking_number: row.get(24)?,
        courier_status: row.get(25)?,
        courier_events: decode_courier_events(&courier_events),
        courier_synced_at: parse_time(&courier_synced_at),
    })
}

fn scan_order_item(row: &Row) -> Result<OrderItem, StoreError> {
    let options: String = row.get(9).map_err(scan_item_error)?;
    Ok(OrderItem {
        id: row.get(0).map_err(scan_item_error)?,
        order_id: row.get(1).map_err(scan_item_error)?,
        product_id: row.get(2).map_err(scan_item_error)?,
        slug: row.get(3).map_err(scan_item_error)?,
        name: row.get(4).map_err(scan_item_error)?,
        image: row.get(5).map_err(scan_item_error)?,
        unit_price_cents: row.get(6).map_err(scan_item_error)?,
        quantity: row.get(7).map_err(scan_item_error)?,
        line_total_cents: row.get(8).map_err(scan_item_error)?,
        options: decode_string_map(&options),
        options_label: row.get(10).map_err(scan_item_error)?,
        plugin_id: row.get(11).map_err(scan_item_error)?,
    })
}

fn scan_item_error(err: libsql::Error) -> StoreError {
    StoreError::Database(format!("scan order item: {err}"))
}
