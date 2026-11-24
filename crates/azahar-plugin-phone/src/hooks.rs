//! Checkout and order-status hooks. Port of
//! `backend/internal/plugins/phone/hooks.go`.
//!
//! `ProductEnricher` is synchronous in `azahar-plugins`, so the Go enricher is
//! exposed as [`product_enrichment`] instead. The HTTP layer switches to async
//! enrichment when the phone plugin is wired.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use azahar_orderintake::{
    CheckoutHook, CheckoutLine, HookError, HookProvider, OrderRef, OrderStatusHook,
};
use azahar_plugins::Problem;
use azahar_store::{StoreError, format_time};
use chrono::Utc;
use libsql::{Connection, params};
use serde::Serialize;
use serde_json::Value;

use crate::devices::{internal_problem, validation_problem};
use crate::phone::PLUGIN_ID;
use crate::variant::{OPTION_KEYS, allowed_option, option_label};

/// The phone plugin's order hooks. The service keeps no state, so a unit value
/// is enough.
#[derive(Debug, Default, Clone, Copy)]
pub struct PhoneHooks;

impl PhoneHooks {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CheckoutHook for PhoneHooks {
    async fn validate_checkout_line(
        &self,
        conn: &Connection,
        line: &CheckoutLine,
    ) -> Option<Problem> {
        if line.product.plugin_id != PLUGIN_ID {
            return None;
        }

        let mut options = HashMap::new();
        for key in OPTION_KEYS {
            let value = line.options.get(key).map_or("", String::as_str).trim();
            let allowed = line
                .product
                .attributes
                .get(key)
                .map_or(&[][..], Vec::as_slice);
            if !allowed_option(allowed, value) {
                let mut fields = HashMap::new();
                fields.insert(
                    "items".to_string(),
                    format!(
                        "Choose an available {} option for {}.",
                        option_label(key).to_lowercase(),
                        line.product.name
                    ),
                );
                return Some(validation_problem(fields));
            }
            options.insert(key.to_string(), value.to_string());
        }

        let available = match count_in_stock(
            conn,
            &line.product.id,
            &options["config"],
            &options["color"],
        )
        .await
        {
            Ok(available) => available,
            Err(_) => return Some(internal_problem()),
        };
        if available < line.quantity {
            let mut fields = HashMap::new();
            fields.insert(
                "items".to_string(),
                format!(
                    "Not enough IMEI-registered handsets for {} in that configuration.",
                    line.product.name
                ),
            );
            return Some(Problem::new(
                409,
                "conflict",
                "Some products do not have enough stock.",
                Some(fields),
            ));
        }
        None
    }

    async fn on_order_created(
        &self,
        conn: &Connection,
        order: &OrderRef,
        lines: &[CheckoutLine],
    ) -> Result<(), HookError> {
        let now = format_time(&Utc::now());
        for line in lines {
            if line.product.plugin_id != PLUGIN_ID {
                continue;
            }
            let mut options = HashMap::new();
            for key in OPTION_KEYS {
                options.insert(
                    key.to_string(),
                    line.options
                        .get(key)
                        .map_or("", String::as_str)
                        .trim()
                        .to_string(),
                );
            }
            for _ in 0..line.quantity {
                let device_id = select_oldest_in_stock(
                    conn,
                    &line.product.id,
                    &options["config"],
                    &options["color"],
                )
                .await
                .map_err(|error| {
                    HookError::message(format!(
                        "claim handset for {}: {}",
                        line.product.name,
                        store_error_text(&error)
                    ))
                })?;

                // The status guard on the UPDATE decides the claim, so a racing
                // checkout cannot reserve the same IMEI.
                let affected = conn
                    .execute(
                        "UPDATE phone_devices
				 SET status = 'reserved', order_id = ?, order_item_id = ?, updated_at = ?
				 WHERE id = ? AND status = 'in_stock'",
                        params![
                            order.id.clone(),
                            line.item_id.clone(),
                            now.clone(),
                            device_id.clone()
                        ],
                    )
                    .await
                    .map_err(|error| {
                        HookError::message(format!("claim handset {device_id}: {error}"))
                    })?;
                if affected != 1 {
                    return Err(HookError::message(format!(
                        "claim handset {device_id}: device was already taken"
                    )));
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl OrderStatusHook for PhoneHooks {
    async fn on_order_status_changed(
        &self,
        conn: &Connection,
        order_id: &str,
        _from: &str,
        to: &str,
    ) -> Result<(), HookError> {
        let now = format_time(&Utc::now());
        match to {
            "cancelled" => {
                conn.execute(
                    "UPDATE phone_devices
			 SET status = 'in_stock', order_id = '', order_item_id = '', updated_at = ?
			 WHERE order_id = ? AND status = 'reserved'",
                    params![now, order_id],
                )
                .await
                .map_err(|error| {
                    HookError::message(format!("release handsets for order {order_id}: {error}"))
                })?;
            }
            "delivered" => {
                conn.execute(
                    "UPDATE phone_devices SET status = 'sold', updated_at = ?
			 WHERE order_id = ? AND status = 'reserved'",
                    params![now, order_id],
                )
                .await
                .map_err(|error| {
                    HookError::message(format!("mark handsets sold for order {order_id}: {error}"))
                })?;
            }
            _ => {}
        }
        Ok(())
    }
}

impl HookProvider for PhoneHooks {
    fn checkout_hook(&self, plugin_id: &str) -> Option<Arc<dyn CheckoutHook>> {
        if plugin_id == PLUGIN_ID {
            Some(Arc::new(*self) as Arc<dyn CheckoutHook>)
        } else {
            None
        }
    }

    fn status_hooks(&self) -> Vec<Arc<dyn OrderStatusHook>> {
        vec![Arc::new(*self) as Arc<dyn OrderStatusHook>]
    }
}

/// One in-stock configuration with its shelf count, in the Go enricher's shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VariantCount {
    pub config: String,
    pub color: String,
    pub in_stock: i64,
}

/// The map `Plugin.EnrichProduct` builds: the in-stock device count and one
/// entry per configuration that has stock.
pub async fn product_enrichment(
    conn: &Connection,
    product_id: &str,
) -> Result<HashMap<String, Value>, StoreError> {
    let mut rows = conn
        .query(
            "SELECT
		 COUNT(CASE WHEN status = 'in_stock' THEN 1 END),
		 COUNT(CASE WHEN status = 'reserved' THEN 1 END)
		 FROM phone_devices WHERE product_id = ?",
            params![product_id],
        )
        .await?;
    let row = rows.next().await?.ok_or(StoreError::NotFound)?;
    let in_stock: i64 = row.get(0)?;
    // Go scans the reserved count but never adds it to the result.
    let _reserved: i64 = row.get(1)?;
    drop(rows);

    let mut rows = conn
        .query(
            "SELECT config, color, COUNT(*)
		 FROM phone_devices
		 WHERE product_id = ? AND status = 'in_stock'
		 GROUP BY config, color
		 ORDER BY config, color",
            params![product_id],
        )
        .await?;
    let mut variants = Vec::new();
    while let Some(row) = rows.next().await? {
        variants.push(VariantCount {
            config: row.get(0)?,
            color: row.get(1)?,
            in_stock: row.get(2)?,
        });
    }

    let mut enrichment = HashMap::new();
    enrichment.insert("devices_in_stock".to_string(), Value::from(in_stock));
    enrichment.insert(
        "variants".to_string(),
        serde_json::to_value(variants)
            .map_err(|error| StoreError::Database(format!("encode phone variants: {error}")))?,
    );
    Ok(enrichment)
}

impl crate::phone::PhoneService {
    pub async fn product_enrichment(
        &self,
        conn: &Connection,
        product_id: &str,
    ) -> Result<HashMap<String, Value>, StoreError> {
        product_enrichment(conn, product_id).await
    }
}

async fn count_in_stock(
    conn: &Connection,
    product_id: &str,
    config: &str,
    color: &str,
) -> Result<i64, StoreError> {
    let mut rows = conn
        .query(
            "SELECT COUNT(*) FROM phone_devices
		 WHERE product_id = ? AND config = ? AND color = ? AND status = 'in_stock'",
            params![product_id, config, color],
        )
        .await?;
    let row = rows.next().await?.ok_or(StoreError::NotFound)?;
    Ok(row.get(0)?)
}

async fn select_oldest_in_stock(
    conn: &Connection,
    product_id: &str,
    config: &str,
    color: &str,
) -> Result<String, StoreError> {
    let mut rows = conn
        .query(
            "SELECT id FROM phone_devices
			 WHERE product_id = ? AND config = ? AND color = ? AND status = 'in_stock'
			 ORDER BY created_at, id LIMIT 1",
            params![product_id, config, color],
        )
        .await?;
    let row = rows.next().await?.ok_or(StoreError::NotFound)?;
    Ok(row.get(0)?)
}

/// The Go hook wraps failures with `%w`; `sql.ErrNoRows` prints its standard
/// text, which this keeps for message parity.
fn store_error_text(error: &StoreError) -> String {
    if error.is_not_found() {
        "sql: no rows in result set".to_string()
    } else {
        error.to_string()
    }
}
