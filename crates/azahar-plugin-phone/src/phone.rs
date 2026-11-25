//! The phone plugin: manifest, schema, startup reconciliation and the service
//! handle. Port of `backend/internal/plugins/phone/phone.go`.

use std::collections::{BTreeMap, HashMap};

use azahar_plugins::{
    Category, Manifest, Migration, OptionField, Plugin as PluginTrait, ProductType, Route,
};
use azahar_store::{StoreError, format_time};
use chrono::{DateTime, Utc};
use libsql::{Connection, params};

pub const PLUGIN_ID: &str = "phone";

const DEVICES_MIGRATION_SQL: &str = "CREATE TABLE IF NOT EXISTS phone_devices (
  id TEXT PRIMARY KEY,
  product_id TEXT NOT NULL REFERENCES products(id),
  variant_key TEXT NOT NULL,
  storage TEXT NOT NULL DEFAULT '',
  ram TEXT NOT NULL DEFAULT '',
  color TEXT NOT NULL DEFAULT '',
  imei TEXT NOT NULL UNIQUE,
  status TEXT NOT NULL DEFAULT 'in_stock',
  order_id TEXT NOT NULL DEFAULT '',
  order_item_id TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_phone_devices_variant ON phone_devices(product_id, variant_key, status);
CREATE INDEX IF NOT EXISTS idx_phone_devices_order ON phone_devices(order_id);";

const CONFIG_MIGRATION_SQL: &str = "ALTER TABLE phone_devices ADD COLUMN config TEXT NOT NULL DEFAULT '';
UPDATE phone_devices SET config = ram || ' RAM + ' || storage WHERE config = '';
CREATE INDEX IF NOT EXISTS idx_phone_devices_config ON phone_devices(product_id, config, color, status);";

/// The device schema, manifest and migrations the registry registers.
#[derive(Debug, Default, Clone, Copy)]
pub struct Plugin;

#[must_use]
pub fn new() -> Plugin {
    Plugin
}

/// The async half of the Go plugin. The HTTP layer mounts the routes and
/// supplies the connection or transaction for each call.
#[derive(Debug, Default, Clone, Copy)]
pub struct PhoneService;

impl PhoneService {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Startup work, mirroring `Plugin.Init`: reconcile the counter and repair
    /// legacy attributes. Failures are warnings, exactly as in Go.
    pub async fn init(&self, conn: &Connection, now: DateTime<Utc>) {
        if let Err(error) = reconcile_stock(conn, now).await {
            tracing::warn!(error = %error, "phone stock reconciliation failed");
        }
        repair_legacy_attributes(conn).await;
    }

    pub async fn reconcile_stock(
        &self,
        conn: &Connection,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        reconcile_stock(conn, now).await
    }

    pub async fn repair_legacy_attributes(&self, conn: &Connection) {
        repair_legacy_attributes(conn).await;
    }
}

/// IMEI rows are the real stock for a handset. Reconcile the product counter at
/// startup so an import, a manual edit or an older deployment cannot leave the
/// storefront showing a different quantity than the shelf.
pub async fn reconcile_stock(conn: &Connection, now: DateTime<Utc>) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE products SET stock = (
		   SELECT COUNT(*) FROM phone_devices d
		   WHERE d.product_id = products.id AND d.status = 'in_stock'
		 ), updated_at = ? WHERE plugin_id = ?",
        params![format_time(&now), PLUGIN_ID],
    )
    .await?;
    Ok(())
}

/// Older demo seeds stored separate ram and storage lists. The product type now
/// sells one paired configuration, so a legacy row could not be edited or sold.
/// Pair the old lists once and keep the row usable.
///
/// The Go method swallows its failures after logging; this port does the same.
pub async fn repair_legacy_attributes(conn: &Connection) {
    let mut rows = match conn
        .query(
            "SELECT id, attributes FROM products WHERE plugin_id = ?",
            params![PLUGIN_ID],
        )
        .await
    {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(error = %error, "phone attribute repair failed");
            return;
        }
    };

    let mut pending: Vec<(String, String)> = Vec::new();
    loop {
        let row = match rows.next().await {
            Ok(Some(row)) => row,
            Ok(None) => break,
            Err(_) => return,
        };
        let id: String = match row.get(0) {
            Ok(id) => id,
            Err(_) => return,
        };
        let raw: String = match row.get(1) {
            Ok(raw) => raw,
            Err(_) => return,
        };
        let Some(mut attributes) = parse_legacy_attributes(&raw) else {
            continue;
        };
        if attributes
            .get("config")
            .and_then(Option::as_ref)
            .is_some_and(|configs| !configs.is_empty())
        {
            continue;
        }
        let ram = attributes
            .get("ram")
            .and_then(Option::as_ref)
            .map_or(&[][..], Vec::as_slice);
        let storage = attributes
            .get("storage")
            .and_then(Option::as_ref)
            .map_or(&[][..], Vec::as_slice);
        let configs = legacy_configs(ram, storage);
        if configs.is_empty() {
            continue;
        }
        attributes.insert("config".to_string(), Some(configs));
        attributes.remove("ram");
        attributes.remove("storage");
        let Some(encoded) = encode_legacy_attributes(&attributes) else {
            continue;
        };
        pending.push((id, encoded));
    }
    drop(rows);

    if pending.is_empty() {
        return;
    }

    let transaction = match conn.transaction().await {
        Ok(transaction) => transaction,
        Err(error) => {
            tracing::warn!(error = %error, "phone attribute repair write failed");
            return;
        }
    };
    for (id, attributes) in pending {
        if let Err(error) = transaction
            .execute(
                "UPDATE products SET attributes = ?, updated_at = ? WHERE id = ?",
                params![attributes, format_time(&Utc::now()), id],
            )
            .await
        {
            let _ = transaction.rollback().await;
            tracing::warn!(error = %error, "phone attribute repair write failed");
            return;
        }
    }
    if let Err(error) = transaction.commit().await {
        tracing::warn!(error = %error, "phone attribute repair write failed");
    }
}

type LegacyAttributes = HashMap<String, Option<Vec<String>>>;

/// Go decodes into `map[string][]string`: `null` values keep an empty slice,
/// and any non-string element fails the whole row.
fn parse_legacy_attributes(raw: &str) -> Option<LegacyAttributes> {
    let document: serde_json::Value = serde_json::from_str(raw).ok()?;
    match document {
        serde_json::Value::Null => Some(LegacyAttributes::new()),
        serde_json::Value::Object(object) => {
            let mut attributes = LegacyAttributes::new();
            for (key, value) in object {
                let items = match value {
                    serde_json::Value::Null => None,
                    serde_json::Value::Array(values) => {
                        let mut items = Vec::with_capacity(values.len());
                        for value in values {
                            match value {
                                serde_json::Value::String(text) => items.push(text),
                                serde_json::Value::Null => items.push(String::new()),
                                _ => return None,
                            }
                        }
                        Some(items)
                    }
                    _ => return None,
                };
                attributes.insert(key, items);
            }
            Some(attributes)
        }
        _ => None,
    }
}

fn encode_legacy_attributes(attributes: &LegacyAttributes) -> Option<String> {
    let sorted: BTreeMap<&String, &Option<Vec<String>>> = attributes.iter().collect();
    let encoded = serde_json::to_string(&sorted).ok()?;
    Some(escape_html(encoded))
}

fn escape_html(encoded: String) -> String {
    if !encoded.contains(['<', '>', '&', '\u{2028}', '\u{2029}']) {
        return encoded;
    }
    encoded
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn legacy_configs(ram: &[String], storage: &[String]) -> Vec<String> {
    let limit = ram.len().min(storage.len());
    let mut configs = Vec::with_capacity(limit);
    for index in 0..limit {
        let memory = ram[index].trim();
        let disk = storage[index].trim();
        if memory.is_empty() || disk.is_empty() {
            continue;
        }
        configs.push(format!("{memory} RAM + {disk}"));
    }
    configs
}

impl PluginTrait for Plugin {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PLUGIN_ID.to_string(),
            name: "Phone catalog".to_string(),
            version: "1.0.0".to_string(),
            description: "Sell handsets while tracking every unit by IMEI.".to_string(),
            author: "Azahar".to_string(),
            admin_path: "/admin/plugins/phone".to_string(),
            categories: vec![Category {
                slug: "phones".to_string(),
                name: "Phones".to_string(),
                description: "Handsets with registered IMEI stock.".to_string(),
                sort_order: 1,
            }],
            product_type: Some(ProductType {
                label: "Phone".to_string(),
                summary: "Every handset is tracked by IMEI and sold as one memory and storage configuration.".to_string(),
                fields: vec![
                    OptionField {
                        key: "config".to_string(),
                        label: "Memory and storage".to_string(),
                        field_type: "select".to_string(),
                        required: true,
                        help: "One paired memory and storage configuration, for example 12 GB RAM + 256 GB.".to_string(),
                        suggestions: vec![
                            "12 GB RAM + 256 GB".to_string(),
                            "16 GB RAM + 512 GB".to_string(),
                            "16 GB RAM + 1 TB".to_string(),
                        ],
                    },
                    OptionField {
                        key: "color".to_string(),
                        label: "Color".to_string(),
                        field_type: "select".to_string(),
                        required: true,
                        help: "The finish shown on the product and order.".to_string(),
                        suggestions: vec![
                            "Midnight".to_string(),
                            "Graphite".to_string(),
                            "Silver".to_string(),
                            "Blue".to_string(),
                        ],
                    },
                ],
            }),
            ..Manifest::default()
        }
    }

    fn routes(&self) -> Vec<Route> {
        vec![
            Route {
                method: "GET".to_string(),
                pattern: "/devices".to_string(),
                admin: true,
            },
            Route {
                method: "POST".to_string(),
                pattern: "/devices".to_string(),
                admin: true,
            },
            Route {
                method: "DELETE".to_string(),
                pattern: "/devices/{id}".to_string(),
                admin: true,
            },
            Route {
                method: "GET".to_string(),
                pattern: "/orders/{orderID}/devices".to_string(),
                admin: true,
            },
        ]
    }

    fn migrations(&self) -> Vec<Migration> {
        vec![
            Migration {
                version: "001_devices.sql".to_string(),
                sql: DEVICES_MIGRATION_SQL.to_string(),
            },
            Migration {
                version: "002_config.sql".to_string(),
                sql: CONFIG_MIGRATION_SQL.to_string(),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_matches_the_go_values() {
        let manifest = new().manifest();
        assert_eq!(manifest.id, "phone");
        assert_eq!(manifest.name, "Phone catalog");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.author, "Azahar");
        assert_eq!(
            manifest.description,
            "Sell handsets while tracking every unit by IMEI."
        );
        assert_eq!(manifest.admin_path, "/admin/plugins/phone");
        assert_eq!(manifest.categories.len(), 1);
        assert_eq!(manifest.categories[0].slug, "phones");

        let product_type = manifest.product_type.expect("product type");
        assert_eq!(product_type.label, "Phone");
        assert_eq!(
            product_type.summary,
            "Every handset is tracked by IMEI and sold as one memory and storage configuration."
        );
        let keys: Vec<&str> = product_type
            .fields
            .iter()
            .map(|field| field.key.as_str())
            .collect();
        assert_eq!(keys, ["config", "color"]);
        assert!(product_type.fields.iter().all(|field| field.required));
        assert_eq!(
            product_type.fields[0].suggestions,
            [
                "12 GB RAM + 256 GB",
                "16 GB RAM + 512 GB",
                "16 GB RAM + 1 TB"
            ]
        );
        assert_eq!(
            product_type.fields[1].suggestions,
            ["Midnight", "Graphite", "Silver", "Blue"]
        );
    }

    #[test]
    fn routes_match_the_go_registration() {
        let routes = new().routes();
        let listed: Vec<(String, String, bool)> = routes
            .into_iter()
            .map(|route| (route.method, route.pattern, route.admin))
            .collect();
        assert_eq!(
            listed,
            [
                ("GET".to_string(), "/devices".to_string(), true),
                ("POST".to_string(), "/devices".to_string(), true),
                ("DELETE".to_string(), "/devices/{id}".to_string(), true),
                (
                    "GET".to_string(),
                    "/orders/{orderID}/devices".to_string(),
                    true
                ),
            ]
        );
    }

    #[test]
    fn migrations_match_the_go_sql() {
        let migrations = new().migrations();
        assert_eq!(migrations.len(), 2);
        assert_eq!(migrations[0].version, "001_devices.sql");
        assert_eq!(migrations[0].sql, DEVICES_MIGRATION_SQL);
        assert!(migrations[0].sql.contains("imei TEXT NOT NULL UNIQUE"));
        assert_eq!(migrations[1].version, "002_config.sql");
        assert_eq!(migrations[1].sql, CONFIG_MIGRATION_SQL);
        assert!(migrations[1].sql.contains("ADD COLUMN config"));
    }

    #[test]
    fn legacy_configs_pair_the_old_lists() {
        let configs = legacy_configs(
            &["12 GB".to_string(), " 16 GB ".to_string()],
            &["256 GB".to_string(), "512 GB".to_string()],
        );
        assert_eq!(configs, ["12 GB RAM + 256 GB", "16 GB RAM + 512 GB"]);
        assert!(legacy_configs(&["12 GB".to_string()], &[]).is_empty());
    }

    #[test]
    fn legacy_attributes_keep_null_values_and_escape_html() {
        let parsed =
            parse_legacy_attributes(r#"{"ram":["12 GB",null],"storage":["256 GB"],"note":null}"#)
                .expect("parse");
        assert_eq!(
            parsed.get("ram").cloned().flatten(),
            Some(vec!["12 GB".to_string(), String::new()])
        );
        assert_eq!(parsed.get("note").cloned().flatten(), None);
        assert!(parse_legacy_attributes("[]").is_none());
        assert!(parse_legacy_attributes(r#"{"ram":[1]}"#).is_none());
        assert!(parse_legacy_attributes("null").is_some());

        let encoded = encode_legacy_attributes(&parsed).expect("encode");
        assert!(encoded.contains("\"note\":null"));

        let mut escaped = LegacyAttributes::new();
        escaped.insert(
            "color".to_string(),
            Some(vec!["<Black> & \"Gold\"".to_string()]),
        );
        assert_eq!(
            encode_legacy_attributes(&escaped).expect("encode"),
            "{\"color\":[\"\\u003cBlack\\u003e \\u0026 \\\"Gold\\\"\"]}"
        );
    }
}
