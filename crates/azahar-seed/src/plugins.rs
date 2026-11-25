//! Plugin demo shelves, ported from `backend/internal/seed/plugins.go`.

use std::collections::HashMap;

use azahar_ids::new as new_id;
use azahar_store::{Product, Store, format_time};
use chrono::{DateTime, Utc};
use libsql::params;

use crate::{StageError, StageResult};

struct DemoPart {
    slug: &'static str,
    name: &'static str,
    category: &'static str,
    price_cents: i64,
    stock: i32,
    attributes: &'static [(&'static str, &'static [&'static str])],
}

static DEMO_PARTS: &[DemoPart] = &[
    DemoPart {
        slug: "demo-ryzen-7",
        name: "Ryzen 7 9700X",
        category: "cpus",
        price_cents: 5_490_000,
        stock: 6,
        attributes: &[("socket", &["AM5"]), ("tdp_watts", &["120"])],
    },
    DemoPart {
        slug: "demo-b650m",
        name: "B650M Pro",
        category: "motherboards",
        price_cents: 2_890_000,
        stock: 5,
        attributes: &[
            ("socket", &["AM5"]),
            ("memory_type", &["DDR5"]),
            ("form_factor", &["micro-ATX"]),
        ],
    },
    DemoPart {
        slug: "demo-ddr5-32",
        name: "DDR5 32 GB kit",
        category: "memory",
        price_cents: 1_590_000,
        stock: 9,
        attributes: &[("memory_type", &["DDR5"])],
    },
    DemoPart {
        slug: "demo-nvme-1tb",
        name: "1 TB NVMe drive",
        category: "storage",
        price_cents: 1_190_000,
        stock: 12,
        attributes: &[("interface", &["NVMe"])],
    },
    DemoPart {
        slug: "demo-psu-750",
        name: "750 W Gold supply",
        category: "psus",
        price_cents: 1_290_000,
        stock: 7,
        attributes: &[("wattage_watts", &["750"])],
    },
    DemoPart {
        slug: "demo-mesh-atx",
        name: "Mesh ATX case",
        category: "cases",
        price_cents: 990_000,
        stock: 4,
        attributes: &[
            ("supported_form_factor", &["ATX, micro-ATX"]),
            ("max_gpu_length_mm", &["360"]),
        ],
    },
    DemoPart {
        slug: "demo-rx-9070",
        name: "RX 9070",
        category: "gpus",
        price_cents: 8_990_000,
        stock: 3,
        attributes: &[("tdp_watts", &["260"]), ("length_mm", &["320"])],
    },
    DemoPart {
        slug: "demo-am5-tower",
        name: "AM5 tower cooler",
        category: "cooling",
        price_cents: 690_000,
        stock: 8,
        attributes: &[("socket", &["AM5, AM4"])],
    },
];

struct DemoDevice {
    imei: &'static str,
    config: &'static str,
    color: &'static str,
}

static DEMO_DEVICES: &[DemoDevice] = &[
    DemoDevice {
        imei: "356938035640003",
        config: "12 GB RAM + 256 GB",
        color: "Silver",
    },
    DemoDevice {
        imei: "356938035640011",
        config: "12 GB RAM + 256 GB",
        color: "Silver",
    },
    DemoDevice {
        imei: "356938035640029",
        config: "16 GB RAM + 512 GB",
        color: "Midnight",
    },
    DemoDevice {
        imei: "356938035640037",
        config: "16 GB RAM + 512 GB",
        color: "Midnight",
    },
    DemoDevice {
        imei: "356938035640045",
        config: "12 GB RAM + 256 GB",
        color: "Midnight",
    },
    DemoDevice {
        imei: "356938035640052",
        config: "16 GB RAM + 512 GB",
        color: "Silver",
    },
];

fn attributes(
    values: &'static [(&'static str, &'static [&'static str])],
) -> HashMap<String, Vec<String>> {
    values
        .iter()
        .map(|(key, list)| {
            (
                (*key).to_string(),
                list.iter().map(|value| (*value).to_string()).collect(),
            )
        })
        .collect()
}

async fn product_count(store: &Store, plugin_id: &str) -> StageResult<i64> {
    let mut rows = store
        .conn()
        .query(
            "SELECT COUNT(*) FROM products WHERE plugin_id = ?",
            params![plugin_id],
        )
        .await?;
    let row = rows
        .next()
        .await?
        .ok_or_else(|| StageError::message("count plugin products: no row"))?;
    Ok(row.get(0)?)
}

pub(crate) async fn seed_plugin_catalog(store: &Store) -> StageResult<usize> {
    let mut created = 0;
    let now = Utc::now();

    if product_count(store, "phone").await? == 0 {
        let category = store
            .category_by_slug("phones")
            .await
            .map_err(|err| StageError::message(format!("phone category: {err}")))?;
        seed_phone_demo(store, category, now)
            .await
            .map_err(|err| StageError::message(format!("seed phone demo: {err}")))?;
        created += 1;
    }

    if product_count(store, "pcparts").await? == 0 {
        seed_pc_parts_demo(store, now)
            .await
            .map_err(|err| StageError::message(format!("seed pc parts demo: {err}")))?;
        created += DEMO_PARTS.len();
    }

    Ok(created)
}

async fn seed_phone_demo(
    store: &Store,
    category: azahar_store::Category,
    now: DateTime<Utc>,
) -> StageResult<()> {
    let product = Product {
        id: "prd_demo_phone".to_string(),
        slug: "aster-5".to_string(),
        name: "Aster 5".to_string(),
        subtitle: "A quiet handset, recorded by IMEI".to_string(),
        description: "A 6.4 inch handset with a clean display and two days of battery. Each unit is recorded with its IMEI before it leaves the shelf.".to_string(),
        price_cents: 6_290_000,
        currency: "DZD".to_string(),
        category,
        image: "/images/demo/phone.svg".to_string(),
        gallery: vec!["/images/demo/phone.svg".to_string()],
        stock: 6,
        featured: true,
        active: true,
        plugin_id: "phone".to_string(),
        attributes: HashMap::from([
            (
                "config".to_string(),
                vec![
                    "12 GB RAM + 256 GB".to_string(),
                    "16 GB RAM + 512 GB".to_string(),
                ],
            ),
            (
                "color".to_string(),
                vec!["Midnight".to_string(), "Silver".to_string()],
            ),
        ]),
        created_at: Some(now),
        updated_at: Some(now),
        ..Product::default()
    };

    let tx = store.begin_write().await?;
    store.create_product(tx.conn(), &product).await?;
    for device in DEMO_DEVICES {
        tx.conn()
            .execute(
                "INSERT INTO phone_devices (id, product_id, variant_key, config, color, imei,
                 status, order_id, order_item_id, created_at, updated_at)
                 VALUES (?, ?, '', ?, ?, ?, 'in_stock', '', '', ?, ?)",
                params![
                    new_id("phn"),
                    product.id.clone(),
                    device.config,
                    device.color,
                    device.imei,
                    format_time(&now),
                    format_time(&now),
                ],
            )
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn seed_pc_parts_demo(store: &Store, now: DateTime<Utc>) -> StageResult<()> {
    let tx = store.begin_write().await?;
    for part in DEMO_PARTS {
        let category = store
            .category_by_slug_tx(tx.conn(), part.category)
            .await
            .map_err(|err| {
                StageError::message(format!("part category {}: {err}", part.category))
            })?;
        let image = format!("/images/demo/{}.svg", part.category);
        let product = Product {
            id: format!("prd_demo_{}", part.category),
            slug: part.slug.to_string(),
            name: part.name.to_string(),
            subtitle: "Builder sample part".to_string(),
            description: "A sample PC part used by the Build your PC demo.".to_string(),
            price_cents: part.price_cents,
            currency: "DZD".to_string(),
            category,
            image: image.clone(),
            gallery: vec![image],
            stock: part.stock,
            featured: false,
            active: true,
            plugin_id: "pcparts".to_string(),
            attributes: attributes(part.attributes),
            created_at: Some(now),
            updated_at: Some(now),
            ..Product::default()
        };
        store.create_product(tx.conn(), &product).await?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_part_contract_matches_the_go_seed() {
        let expected = [
            ("demo-ryzen-7", "cpus", 5_490_000, 6),
            ("demo-b650m", "motherboards", 2_890_000, 5),
            ("demo-ddr5-32", "memory", 1_590_000, 9),
            ("demo-nvme-1tb", "storage", 1_190_000, 12),
            ("demo-psu-750", "psus", 1_290_000, 7),
            ("demo-mesh-atx", "cases", 990_000, 4),
            ("demo-rx-9070", "gpus", 8_990_000, 3),
            ("demo-am5-tower", "cooling", 690_000, 8),
        ];
        assert_eq!(DEMO_PARTS.len(), expected.len());
        for (part, (slug, category, price, stock)) in DEMO_PARTS.iter().zip(expected) {
            assert_eq!(part.slug, slug);
            assert_eq!(part.category, category);
            assert_eq!(part.price_cents, price);
            assert_eq!(part.stock, stock);
            assert!(!part.attributes.is_empty());
        }
    }

    #[test]
    fn phone_demo_has_six_unique_imeis() {
        assert_eq!(DEMO_DEVICES.len(), 6);
        let unique: std::collections::HashSet<&str> =
            DEMO_DEVICES.iter().map(|device| device.imei).collect();
        assert_eq!(unique.len(), 6);
        for device in DEMO_DEVICES {
            assert_eq!(device.imei.len(), 15);
            assert!(device.imei.bytes().all(|byte| byte.is_ascii_digit()));
        }
    }
}
