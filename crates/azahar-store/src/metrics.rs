//! The admin dashboard aggregates.
//!
//! Ported from `backend/internal/store/metrics.go`. Date ranges are inclusive
//! of both midnight endpoints; the SQL upper bound is the day after `to`.

use std::collections::HashMap;

use chrono::{DateTime, TimeDelta, Utc};
use libsql::{Connection, Value, params_from_iter};

use crate::store::{Store, StoreError};
use crate::types::{Timestamp, format_time, parse_time};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevenuePoint {
    pub date: String,
    pub revenue_cents: i64,
    pub orders: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusCount {
    pub status: String,
    pub count: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TopProduct {
    pub product_id: String,
    pub name: String,
    pub units: i64,
    pub revenue_cents: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecentOrder {
    pub id: String,
    pub number: String,
    pub name: String,
    pub total_cents: i64,
    pub status: String,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LowStockItem {
    pub id: String,
    pub name: String,
    pub stock: i32,
    pub image: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metrics {
    pub demo: bool,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub revenue_cents: i64,
    pub previous_revenue_cents: i64,
    pub orders_count: i64,
    pub previous_orders_count: i64,
    pub units_sold: i64,
    pub new_customers: i64,
    pub low_stock: Vec<LowStockItem>,
    pub revenue_series: Vec<RevenuePoint>,
    pub status_split: HashMap<String, i64>,
    pub top_products: Vec<TopProduct>,
    pub recent_orders: Vec<RecentOrder>,
}

impl Store {
    pub async fn metrics(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        previous_from: DateTime<Utc>,
        previous_to: DateTime<Utc>,
    ) -> Result<Metrics, StoreError> {
        let mut metrics = Metrics {
            demo: false,
            from,
            to,
            revenue_cents: 0,
            previous_revenue_cents: 0,
            orders_count: 0,
            previous_orders_count: 0,
            units_sold: 0,
            new_customers: 0,
            low_stock: Vec::new(),
            revenue_series: Vec::new(),
            status_split: HashMap::new(),
            top_products: Vec::new(),
            recent_orders: Vec::new(),
        };

        let start = format_time(&from);
        let end = format_time(&day_after(to));
        let previous_start = format_time(&previous_from);
        let previous_end = format_time(&day_after(previous_to));

        let (revenue, orders) = metric_pair(
            self.conn(),
            "SELECT COALESCE(SUM(total_cents), 0), COUNT(*) FROM orders
             WHERE created_at >= ? AND created_at < ? AND status <> 'cancelled'",
            vec![Value::from(start.clone()), Value::from(end.clone())],
            "range revenue",
        )
        .await?;
        metrics.revenue_cents = revenue;
        metrics.orders_count = orders;

        let (previous_revenue, previous_orders) = metric_pair(
            self.conn(),
            "SELECT COALESCE(SUM(total_cents), 0), COUNT(*) FROM orders
             WHERE created_at >= ? AND created_at < ? AND status <> 'cancelled'",
            vec![Value::from(previous_start), Value::from(previous_end)],
            "previous revenue",
        )
        .await?;
        metrics.previous_revenue_cents = previous_revenue;
        metrics.previous_orders_count = previous_orders;

        metrics.units_sold = metric_value(
            self.conn(),
            "SELECT COALESCE(SUM(oi.quantity), 0) FROM order_items oi
             JOIN orders o ON o.id = oi.order_id
             WHERE o.created_at >= ? AND o.created_at < ? AND o.status <> 'cancelled'",
            vec![Value::from(start.clone()), Value::from(end.clone())],
            "units sold",
        )
        .await?;

        metrics.new_customers = metric_value(
            self.conn(),
            "SELECT COUNT(*) FROM users
             WHERE role = 'customer' AND created_at >= ? AND created_at < ?",
            vec![Value::from(start.clone()), Value::from(end.clone())],
            "new customers",
        )
        .await?;

        let mut series: HashMap<String, RevenuePoint> = HashMap::new();
        let mut series_rows = self
            .conn()
            .query(
                "SELECT substr(created_at, 1, 10) AS day,
                        COALESCE(SUM(total_cents), 0), COUNT(*)
                 FROM orders
                 WHERE created_at >= ? AND created_at < ? AND status <> 'cancelled'
                 GROUP BY day ORDER BY day",
                vec![Value::from(start.clone()), Value::from(end.clone())],
            )
            .await
            .map_err(|err| StoreError::Database(format!("revenue series: {err}")))?;
        while let Some(row) = series_rows.next().await? {
            let date: String = row
                .get(0)
                .map_err(|err| StoreError::Database(format!("scan revenue point: {err}")))?;
            let revenue_cents: i64 = row
                .get(1)
                .map_err(|err| StoreError::Database(format!("scan revenue point: {err}")))?;
            let orders: i64 = row
                .get(2)
                .map_err(|err| StoreError::Database(format!("scan revenue point: {err}")))?;
            series.insert(
                date.clone(),
                RevenuePoint {
                    date,
                    revenue_cents,
                    orders,
                },
            );
        }
        let mut day = from;
        while day <= to {
            let key = day.format("%Y-%m-%d").to_string();
            metrics
                .revenue_series
                .push(series.remove(&key).unwrap_or(RevenuePoint {
                    date: key,
                    ..RevenuePoint::default()
                }));
            let Some(next) = day.checked_add_signed(TimeDelta::days(1)) else {
                break;
            };
            day = next;
        }

        let mut status_rows = self
            .conn()
            .query(
                "SELECT status, COUNT(*) FROM orders
                 WHERE created_at >= ? AND created_at < ? GROUP BY status",
                vec![Value::from(start.clone()), Value::from(end.clone())],
            )
            .await
            .map_err(|err| StoreError::Database(format!("status split: {err}")))?;
        while let Some(row) = status_rows.next().await? {
            let status: String = row
                .get(0)
                .map_err(|err| StoreError::Database(format!("scan status split: {err}")))?;
            let count: i64 = row
                .get(1)
                .map_err(|err| StoreError::Database(format!("scan status split: {err}")))?;
            metrics.status_split.insert(status, count);
        }

        let mut top_rows = self
            .conn()
            .query(
                "SELECT oi.product_id, MAX(oi.name), SUM(oi.quantity), SUM(oi.line_total_cents)
                 FROM order_items oi JOIN orders o ON o.id = oi.order_id
                 WHERE o.created_at >= ? AND o.created_at < ? AND o.status <> 'cancelled'
                 GROUP BY oi.product_id
                 ORDER BY SUM(oi.quantity) DESC, SUM(oi.line_total_cents) DESC
                 LIMIT 5",
                vec![Value::from(start.clone()), Value::from(end.clone())],
            )
            .await
            .map_err(|err| StoreError::Database(format!("top products: {err}")))?;
        while let Some(row) = top_rows.next().await? {
            metrics.top_products.push(TopProduct {
                product_id: row
                    .get(0)
                    .map_err(|err| StoreError::Database(format!("scan top product: {err}")))?,
                name: row
                    .get(1)
                    .map_err(|err| StoreError::Database(format!("scan top product: {err}")))?,
                units: row
                    .get(2)
                    .map_err(|err| StoreError::Database(format!("scan top product: {err}")))?,
                revenue_cents: row
                    .get(3)
                    .map_err(|err| StoreError::Database(format!("scan top product: {err}")))?,
            });
        }

        let mut low_rows = self
            .conn()
            .query(
                "SELECT id, name, stock, image FROM products
                 WHERE active = 1 AND stock <= 5 ORDER BY stock ASC, name ASC LIMIT 10",
                (),
            )
            .await
            .map_err(|err| StoreError::Database(format!("low stock: {err}")))?;
        while let Some(row) = low_rows.next().await? {
            metrics.low_stock.push(LowStockItem {
                id: row
                    .get(0)
                    .map_err(|err| StoreError::Database(format!("scan low stock: {err}")))?,
                name: row
                    .get(1)
                    .map_err(|err| StoreError::Database(format!("scan low stock: {err}")))?,
                stock: row
                    .get(2)
                    .map_err(|err| StoreError::Database(format!("scan low stock: {err}")))?,
                image: row
                    .get(3)
                    .map_err(|err| StoreError::Database(format!("scan low stock: {err}")))?,
            });
        }

        let mut recent_rows = self
            .conn()
            .query(
                "SELECT id, number, name, total_cents, status, created_at FROM orders
                 WHERE created_at >= ? AND created_at < ?
                 ORDER BY created_at DESC LIMIT 8",
                vec![Value::from(start), Value::from(end)],
            )
            .await
            .map_err(|err| StoreError::Database(format!("recent orders: {err}")))?;
        while let Some(row) = recent_rows.next().await? {
            let created_at: String = row
                .get(5)
                .map_err(|err| StoreError::Database(format!("scan recent order: {err}")))?;
            metrics.recent_orders.push(RecentOrder {
                id: row
                    .get(0)
                    .map_err(|err| StoreError::Database(format!("scan recent order: {err}")))?,
                number: row
                    .get(1)
                    .map_err(|err| StoreError::Database(format!("scan recent order: {err}")))?,
                name: row
                    .get(2)
                    .map_err(|err| StoreError::Database(format!("scan recent order: {err}")))?,
                total_cents: row
                    .get(3)
                    .map_err(|err| StoreError::Database(format!("scan recent order: {err}")))?,
                status: row
                    .get(4)
                    .map_err(|err| StoreError::Database(format!("scan recent order: {err}")))?,
                created_at: parse_time(&created_at),
            });
        }

        let demo = metric_value(
            self.conn(),
            "SELECT CASE WHEN
               EXISTS (SELECT 1 FROM users WHERE email = 'sample01@azahar.example')
               OR EXISTS (SELECT 1 FROM orders WHERE email = 'sample01@azahar.example')
             THEN 1 ELSE 0 END",
            Vec::new(),
            "demo flag",
        )
        .await?;
        metrics.demo = demo != 0;

        Ok(metrics)
    }
}

fn day_after(value: DateTime<Utc>) -> DateTime<Utc> {
    value
        .checked_add_signed(TimeDelta::days(1))
        .unwrap_or(value)
}

async fn metric_pair(
    conn: &Connection,
    query: &str,
    args: Vec<Value>,
    label: &str,
) -> Result<(i64, i64), StoreError> {
    let mut rows = conn
        .query(query, params_from_iter(args))
        .await
        .map_err(|err| StoreError::Database(format!("{label}: {err}")))?;
    let row = rows
        .next()
        .await
        .map_err(|err| StoreError::Database(format!("{label}: {err}")))?
        .ok_or_else(|| StoreError::Database(format!("{label}: no rows")))?;
    let first = row
        .get(0)
        .map_err(|err| StoreError::Database(format!("{label}: {err}")))?;
    let second = row
        .get(1)
        .map_err(|err| StoreError::Database(format!("{label}: {err}")))?;
    Ok((first, second))
}

async fn metric_value(
    conn: &Connection,
    query: &str,
    args: Vec<Value>,
    label: &str,
) -> Result<i64, StoreError> {
    let mut rows = conn
        .query(query, params_from_iter(args))
        .await
        .map_err(|err| StoreError::Database(format!("{label}: {err}")))?;
    let row = rows
        .next()
        .await
        .map_err(|err| StoreError::Database(format!("{label}: {err}")))?
        .ok_or_else(|| StoreError::Database(format!("{label}: no rows")))?;
    row.get(0)
        .map_err(|err| StoreError::Database(format!("{label}: {err}")))
}
