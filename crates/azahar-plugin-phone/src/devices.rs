//! Device storage and the admin device operations. Port of
//! `backend/internal/plugins/phone/devices.go`.
//!
//! The Go handlers take `*plugins.Context`; this port exposes the data layer as
//! async service methods so `azahar-httpapi` can render the same JSON and
//! `Problem` envelopes.

use std::collections::HashMap;

use azahar_ids::new as new_id;
use azahar_plugins::Problem;
use azahar_store::{StoreError, decode_string_list_map, format_time};
use chrono::Utc;
use libsql::{Connection, Row, Value, params, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::phone::{PLUGIN_ID, PhoneService};
use crate::variant::{OPTION_KEYS, allowed_option, valid_imei, variant_key, variant_label};

pub const DEVICE_SELECT: &str = "SELECT d.id, d.product_id, p.name, d.variant_key, d.config,
		d.color, d.imei, d.status, d.order_id, COALESCE(o.number, ''), d.created_at
	FROM phone_devices d
	JOIN products p ON p.id = d.product_id
	LEFT JOIN orders o ON o.id = d.order_id";

const LIST_SUFFIX: &str = " ORDER BY d.created_at DESC, d.id DESC LIMIT 500";

const ORDER_SUFFIX: &str = " WHERE d.order_id = ? AND d.status IN ('reserved', 'sold')
		 ORDER BY d.config, d.color, d.created_at, d.id";

/// One IMEI-tracked handset, with the Go JSON names.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Device {
    pub id: String,
    pub product_id: String,
    pub product_name: String,
    pub variant_key: String,
    pub variant_label: String,
    pub config: String,
    pub color: String,
    pub imei: String,
    pub status: String,
    pub order_id: String,
    pub order_number: String,
    pub created_at: String,
}

/// Device counts for one product, or for the whole table when the product is
/// empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceCounts {
    pub in_stock: i64,
    pub reserved: i64,
    pub sold: i64,
}

/// The body `POST /devices` accepts. The HTTP layer trims and validates the
/// JSON shape; the service trims again so direct callers get the Go behaviour.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CreateDevicePayload {
    pub product_id: String,
    pub config: String,
    pub color: String,
    pub imei: String,
}

/// `GET /devices` response.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceListResponse {
    pub devices: Vec<Device>,
    pub counts: DeviceCounts,
}

/// `GET /orders/{orderID}/devices` response.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderDevicesResponse {
    pub devices: Vec<Device>,
}

struct PhoneProduct {
    id: String,
    name: String,
    attributes: HashMap<String, Vec<String>>,
}

impl PhoneService {
    /// Devices for one product, or every device when the product id is empty.
    /// A non-empty id must name a phone product.
    pub async fn list_devices(
        &self,
        conn: &Connection,
        product_id: &str,
    ) -> Result<DeviceListResponse, Problem> {
        let product_id = product_id.trim();
        let (suffix, args) = if product_id.is_empty() {
            (LIST_SUFFIX.to_string(), Vec::new())
        } else {
            self.phone_product(conn, product_id).await?;
            (
                format!(" WHERE d.product_id = ?{LIST_SUFFIX}"),
                vec![Value::Text(product_id.to_string())],
            )
        };

        let devices = self
            .query_devices(conn, &suffix, args)
            .await
            .map_err(|_| internal_problem())?;
        let counts = self
            .count_devices(conn, product_id)
            .await
            .map_err(|_| internal_problem())?;
        Ok(DeviceListResponse { devices, counts })
    }

    /// Registers one handset and recomputes the product counter.
    pub async fn create_device(
        &self,
        conn: &Connection,
        payload: &CreateDevicePayload,
    ) -> Result<Device, Problem> {
        let product_id = payload.product_id.trim();
        let config = payload.config.trim();
        let color = payload.color.trim();
        let imei = payload.imei.trim();

        let product = self.phone_product(conn, product_id).await?;

        let options = HashMap::from([
            ("config".to_string(), config.to_string()),
            ("color".to_string(), color.to_string()),
        ]);
        let mut fields = HashMap::new();
        for key in OPTION_KEYS {
            let allowed = product.attributes.get(key).map_or(&[][..], Vec::as_slice);
            if !allowed_option(allowed, &options[key]) {
                fields.insert(
                    key.to_string(),
                    format!(
                        "Choose an available {} option.",
                        crate::variant::option_label(key).to_lowercase()
                    ),
                );
            }
        }
        if !valid_imei(imei) {
            fields.insert(
                "imei".to_string(),
                "Enter a 15-digit IMEI that passes the checksum.".to_string(),
            );
        }
        if !fields.is_empty() {
            return Err(validation_problem(fields));
        }

        let now = format_time(&Utc::now());
        let id = new_id("phn");
        let key = variant_key(&options);
        let transaction = conn.transaction().await.map_err(|_| internal_problem())?;
        if transaction
            .execute(
                "INSERT INTO phone_devices (id, product_id, variant_key, config, color,
			 imei, status, order_id, order_item_id, created_at, updated_at)
			 VALUES (?, ?, ?, ?, ?, ?, 'in_stock', '', '', ?, ?)",
                params![
                    id.clone(),
                    product.id.clone(),
                    key.clone(),
                    config,
                    color,
                    imei,
                    now.clone(),
                    now.clone()
                ],
            )
            .await
            .is_err()
        {
            let _ = transaction.rollback().await;
            // The unique index is the authority. Read once more only to tell a
            // duplicate apart from another database error.
            if self.imei_exists(conn, imei).await {
                let mut fields = HashMap::new();
                fields.insert(
                    "imei".to_string(),
                    "This IMEI is already registered.".to_string(),
                );
                return Err(Problem::new(
                    409,
                    "conflict",
                    "That IMEI is already registered.",
                    Some(fields),
                ));
            }
            return Err(internal_problem());
        }
        if self
            .sync_product_stock(&transaction, &product.id)
            .await
            .is_err()
        {
            let _ = transaction.rollback().await;
            return Err(internal_problem());
        }
        transaction.commit().await.map_err(|_| internal_problem())?;

        Ok(Device {
            id,
            product_id: product.id,
            product_name: product.name,
            variant_key: key,
            variant_label: variant_label(&options),
            config: config.to_string(),
            color: color.to_string(),
            imei: imei.to_string(),
            status: "in_stock".to_string(),
            order_id: String::new(),
            order_number: String::new(),
            created_at: now,
        })
    }

    /// Deletes an in-stock handset and recomputes the product counter.
    pub async fn delete_device(&self, conn: &Connection, id: &str) -> Result<(), Problem> {
        let id = id.trim();
        let (status, product_id) = {
            let mut rows = conn
                .query(
                    "SELECT status, product_id FROM phone_devices WHERE id = ?",
                    params![id],
                )
                .await
                .map_err(|_| internal_problem())?;
            let row = rows.next().await.map_err(|_| internal_problem())?;
            let Some(row) = row else {
                return Err(Problem::new(404, "not_found", "Handset not found.", None));
            };
            let status: String = row.get(0).map_err(|_| internal_problem())?;
            let product_id: String = row.get(1).map_err(|_| internal_problem())?;
            (status, product_id)
        };
        if status != "in_stock" {
            return Err(only_in_stock());
        }

        let transaction = conn.transaction().await.map_err(|_| internal_problem())?;
        let affected = match transaction
            .execute(
                "DELETE FROM phone_devices WHERE id = ? AND status = 'in_stock'",
                params![id],
            )
            .await
        {
            Ok(affected) => affected,
            Err(_) => {
                let _ = transaction.rollback().await;
                return Err(internal_problem());
            }
        };
        if affected != 1 {
            let _ = transaction.rollback().await;
            return Err(only_in_stock());
        }
        if self
            .sync_product_stock(&transaction, &product_id)
            .await
            .is_err()
        {
            let _ = transaction.rollback().await;
            return Err(internal_problem());
        }
        transaction.commit().await.map_err(|_| internal_problem())?;
        Ok(())
    }

    /// Reserved and sold devices attached to an order.
    pub async fn order_devices(
        &self,
        conn: &Connection,
        order_id: &str,
    ) -> Result<OrderDevicesResponse, Problem> {
        let order_id = order_id.trim();
        let devices = self
            .query_devices(conn, ORDER_SUFFIX, vec![Value::Text(order_id.to_string())])
            .await
            .map_err(|_| internal_problem())?;
        Ok(OrderDevicesResponse { devices })
    }

    /// Shared selector plus a caller-supplied suffix, mirroring `queryDevices`.
    pub async fn query_devices(
        &self,
        conn: &Connection,
        suffix: &str,
        args: Vec<Value>,
    ) -> Result<Vec<Device>, StoreError> {
        let sql = format!("{DEVICE_SELECT}{suffix}");
        let mut rows = conn.query(&sql, params_from_iter(args)).await?;
        let mut devices = Vec::new();
        while let Some(row) = rows.next().await? {
            devices.push(scan_device(&row)?);
        }
        Ok(devices)
    }

    pub async fn count_devices(
        &self,
        conn: &Connection,
        product_id: &str,
    ) -> Result<DeviceCounts, StoreError> {
        let query = if product_id.is_empty() {
            "SELECT
		COUNT(CASE WHEN status = 'in_stock' THEN 1 END),
		COUNT(CASE WHEN status = 'reserved' THEN 1 END),
		COUNT(CASE WHEN status = 'sold' THEN 1 END)
		FROM phone_devices"
                .to_string()
        } else {
            "SELECT
		COUNT(CASE WHEN status = 'in_stock' THEN 1 END),
		COUNT(CASE WHEN status = 'reserved' THEN 1 END),
		COUNT(CASE WHEN status = 'sold' THEN 1 END)
		FROM phone_devices WHERE product_id = ?"
                .to_string()
        };
        let args = if product_id.is_empty() {
            Vec::new()
        } else {
            vec![Value::Text(product_id.to_string())]
        };
        let mut rows = conn.query(&query, params_from_iter(args)).await?;
        let row = rows.next().await?.ok_or(StoreError::NotFound)?;
        Ok(DeviceCounts {
            in_stock: row.get(0)?,
            reserved: row.get(1)?,
            sold: row.get(2)?,
        })
    }

    /// Makes the product counter match the shelf count. The IMEI rows are
    /// authoritative for a handset, so add and delete both recompute it instead
    /// of trusting a previous value.
    pub async fn sync_product_stock(
        &self,
        conn: &Connection,
        product_id: &str,
    ) -> Result<(), StoreError> {
        conn.execute(
            "UPDATE products SET stock = (
		   SELECT COUNT(*) FROM phone_devices
		   WHERE product_id = ? AND status = 'in_stock'
		 ), updated_at = ? WHERE id = ?",
            params![product_id, format_time(&Utc::now()), product_id],
        )
        .await?;
        Ok(())
    }

    /// The Go helper swallows read failures and reports "not registered".
    pub async fn imei_exists(&self, conn: &Connection, imei: &str) -> bool {
        let Ok(mut rows) = conn
            .query(
                "SELECT COUNT(*) FROM phone_devices WHERE imei = ?",
                params![imei],
            )
            .await
        else {
            return false;
        };
        let Ok(Some(row)) = rows.next().await else {
            return false;
        };
        let Ok(count) = row.get::<i64>(0) else {
            return false;
        };
        count > 0
    }

    async fn phone_product(
        &self,
        conn: &Connection,
        product_id: &str,
    ) -> Result<PhoneProduct, Problem> {
        let mut rows = conn
            .query(
                "SELECT p.id, p.name, p.plugin_id, p.attributes
				 FROM products p JOIN categories c ON c.id = p.category_id
				 WHERE p.id = ?",
                params![product_id],
            )
            .await
            .map_err(|_| internal_problem())?;
        let row = rows.next().await.map_err(|_| internal_problem())?;
        let Some(row) = row else {
            return Err(phone_product_problem());
        };
        let plugin_id: String = row.get(2).map_err(|_| internal_problem())?;
        if plugin_id != PLUGIN_ID {
            return Err(phone_product_problem());
        }
        let raw_attributes: String = row.get(3).map_err(|_| internal_problem())?;
        Ok(PhoneProduct {
            id: row.get(0).map_err(|_| internal_problem())?,
            name: row.get(1).map_err(|_| internal_problem())?,
            attributes: decode_string_list_map(&raw_attributes),
        })
    }
}

/// One row of [`DEVICE_SELECT`], with the label computed from config and color.
pub fn scan_device(row: &Row) -> Result<Device, StoreError> {
    let config: String = row.get(4)?;
    let color: String = row.get(5)?;
    let options = HashMap::from([
        ("config".to_string(), config.clone()),
        ("color".to_string(), color.clone()),
    ]);
    Ok(Device {
        id: row.get(0)?,
        product_id: row.get(1)?,
        product_name: row.get(2)?,
        variant_key: row.get(3)?,
        variant_label: variant_label(&options),
        config,
        color,
        imei: row.get(6)?,
        status: row.get(7)?,
        order_id: row.get(8)?,
        order_number: row.get(9)?,
        created_at: row.get(10)?,
    })
}

pub(crate) fn internal_problem() -> Problem {
    Problem::new(500, "internal_error", "Something went wrong.", None)
}

pub(crate) fn phone_product_problem() -> Problem {
    let mut fields = HashMap::new();
    fields.insert(
        "product_id".to_string(),
        "Choose a phone product.".to_string(),
    );
    Problem::new(
        422,
        "validation_error",
        "Check the marked fields.",
        Some(fields),
    )
}

pub(crate) fn validation_problem(fields: HashMap<String, String>) -> Problem {
    Problem::new(
        422,
        "validation_error",
        "Check the marked fields.",
        Some(fields),
    )
}

fn only_in_stock() -> Problem {
    Problem::new(
        409,
        "conflict",
        "Only in-stock handsets can be deleted.",
        None,
    )
}
