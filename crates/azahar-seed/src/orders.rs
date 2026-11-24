//! Demo orders, ported from `backend/internal/seed/seed.go`.
//!
//! Go uses `math/rand` with the fixed seed `20261111`. The plan, day offsets,
//! ranges and branch logic are reproduced here with a seeded `StdRng`; the
//! concrete permutation and draw values differ from Go's generator. Guests are
//! still the first six shuffled slots, line counts stay 1..4, quantities 1..3,
//! and the demo keeps exactly 6/6/5/8/20/3 orders by status.

use azahar_auth::{hash_token, new_token};
use azahar_ids::new as new_id;
use azahar_store::products::normalize_plugin_id;
use azahar_store::{Order, OrderItem, Store};
use chrono::{Datelike, NaiveTime, TimeDelta, Utc};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};

use crate::{StageError, StageResult};

const RNG_SEED: u64 = 20_261_111;

const CITIES: [&str; 6] = [
    "Algiers",
    "Oran",
    "Constantine",
    "Tlemcen",
    "Annaba",
    "Blida",
];

#[derive(Clone, Copy)]
struct Destination {
    code: i32,
    wilaya: &'static str,
    commune: &'static str,
}

const DESTINATIONS: [(&str, Destination); 6] = [
    (
        "Algiers",
        Destination {
            code: 16,
            wilaya: "Alger",
            commune: "Alger Centre",
        },
    ),
    (
        "Oran",
        Destination {
            code: 31,
            wilaya: "Oran",
            commune: "Oran",
        },
    ),
    (
        "Constantine",
        Destination {
            code: 25,
            wilaya: "Constantine",
            commune: "Constantine",
        },
    ),
    (
        "Tlemcen",
        Destination {
            code: 13,
            wilaya: "Tlemcen",
            commune: "Tlemcen",
        },
    ),
    (
        "Annaba",
        Destination {
            code: 23,
            wilaya: "Annaba",
            commune: "Annaba",
        },
    ),
    (
        "Blida",
        Destination {
            code: 9,
            wilaya: "Blida",
            commune: "Blida",
        },
    ),
];

struct StatusPlan {
    status: &'static str,
    count: usize,
    min_day: i64,
    max_day: i64,
}

const STATUS_PLANS: &[StatusPlan] = &[
    StatusPlan {
        status: "pending",
        count: 6,
        min_day: 0,
        max_day: 4,
    },
    StatusPlan {
        status: "confirmed",
        count: 6,
        min_day: 1,
        max_day: 7,
    },
    StatusPlan {
        status: "packed",
        count: 5,
        min_day: 2,
        max_day: 10,
    },
    StatusPlan {
        status: "shipped",
        count: 8,
        min_day: 4,
        max_day: 20,
    },
    StatusPlan {
        status: "delivered",
        count: 20,
        min_day: 8,
        max_day: 44,
    },
    StatusPlan {
        status: "cancelled",
        count: 3,
        min_day: 5,
        max_day: 30,
    },
];

struct SeedOrderProduct {
    id: String,
    slug: String,
    name: String,
    image: String,
    plugin_id: String,
    price_cents: i64,
}

struct SeedOrderUser {
    id: String,
    name: String,
    email: String,
}

fn destination(city: &str) -> Option<Destination> {
    DESTINATIONS
        .iter()
        .find(|(name, _)| *name == city)
        .map(|(_, destination)| *destination)
}

pub(crate) async fn seed_orders(store: &Store) -> StageResult<usize> {
    if store.count_orders().await? > 0 {
        return Ok(0);
    }

    let mut rows = store
        .conn()
        .query(
            "SELECT id, slug, name, image, plugin_id, price_cents FROM products
             WHERE active = 1 AND plugin_id = 'oils' ORDER BY id",
            (),
        )
        .await?;
    let mut catalog = Vec::new();
    while let Some(row) = rows.next().await? {
        catalog.push(SeedOrderProduct {
            id: row.get(0)?,
            slug: row.get(1)?,
            name: row.get(2)?,
            image: row.get(3)?,
            plugin_id: row.get(4)?,
            price_cents: row.get(5)?,
        });
    }
    if catalog.is_empty() {
        return Err(StageError::message(
            "no active products to seed orders with",
        ));
    }

    let mut rows = store
        .conn()
        .query(
            "SELECT id, name, email FROM users WHERE role = 'customer' ORDER BY email",
            (),
        )
        .await?;
    let mut customers = Vec::new();
    while let Some(row) = rows.next().await? {
        customers.push(SeedOrderUser {
            id: row.get(0)?,
            name: row.get(1)?,
            email: row.get(2)?,
        });
    }
    if customers.is_empty() {
        return Err(StageError::message("no customers to seed orders with"));
    }

    let mut rng = StdRng::seed_from_u64(RNG_SEED);
    let mut queue: Vec<(&'static str, i64)> =
        Vec::with_capacity(STATUS_PLANS.iter().map(|plan| plan.count).sum());
    for plan in STATUS_PLANS {
        for index in 0..plan.count {
            let mut offset = plan.min_day;
            if plan.max_day > plan.min_day {
                offset += index as i64 * (plan.max_day - plan.min_day) / (plan.count as i64 - 1);
            }
            queue.push((plan.status, offset));
        }
    }
    queue.shuffle(&mut rng);

    let now = Utc::now();
    let today = now.date_naive().and_time(NaiveTime::MIN).and_utc();
    let mut guest_count = 0usize;
    let mut created = 0usize;

    let tx = store.begin_write().await?;
    for (index, (status, offset)) in queue.iter().enumerate() {
        let created_at = today - TimeDelta::days(*offset)
            + TimeDelta::hours(9 + i64::from(rng.random_range(0..10)))
            + TimeDelta::minutes(i64::from(rng.random_range(0..60)));

        let (user_id, name, email) = if index < 6 {
            guest_count += 1;
            (
                None,
                format!("Sample guest {guest_count:02}"),
                format!("guest{guest_count:02}@azahar.example"),
            )
        } else {
            let customer = &customers[rng.random_range(0..customers.len())];
            (
                Some(customer.id.clone()),
                customer.name.clone(),
                customer.email.clone(),
            )
        };

        // Go indexes a full permutation with a 1..4 line count and relies on
        // the demo shelf having at least four active oils products. Clamp so a
        // shrunken catalogue cannot panic; the normal eight-product shelf is
        // unchanged.
        let line_count = (1 + rng.random_range(0..4usize)).min(catalog.len());
        let mut permutation: Vec<usize> = (0..catalog.len()).collect();
        permutation.shuffle(&mut rng);
        let mut items = Vec::with_capacity(line_count);
        let mut subtotal = 0i64;
        for line in 0..line_count {
            let product = &catalog[permutation[line]];
            let quantity = 1 + rng.random_range(0..3i32);
            let line_total = product.price_cents * i64::from(quantity);
            subtotal += line_total;
            items.push(OrderItem {
                id: new_id("itm"),
                product_id: product.id.clone(),
                slug: product.slug.clone(),
                name: product.name.clone(),
                image: product.image.clone(),
                unit_price_cents: product.price_cents,
                quantity,
                line_total_cents: line_total,
                plugin_id: normalize_plugin_id(&product.plugin_id),
                ..OrderItem::default()
            });
        }

        let shipping = if subtotal > 800_000 { 0 } else { 60_000 };
        let token = new_token()?;
        let order_id = new_id("ord");
        let number = store
            .next_order_number(tx.conn(), created_at.year())
            .await?;
        let notes = if rng.random_range(0..4) == 0 {
            "Leave with the concierge"
        } else {
            ""
        };
        let city = CITIES[rng.random_range(0..CITIES.len())];
        let Some(destination) = destination(city) else {
            return Err(StageError::message(format!("unknown demo city {city}")));
        };
        let phone = format!(
            "0550 {:02} {:02} {:02}",
            10 + rng.random_range(0..80),
            10 + rng.random_range(0..80),
            10 + rng.random_range(0..80)
        );
        let address = format!("{} rue des Oliviers", 4 + rng.random_range(0..80));

        let order = Order {
            id: order_id,
            number,
            user_id,
            name,
            email,
            phone,
            address,
            city: destination.commune.to_string(),
            notes: notes.to_string(),
            subtotal_cents: subtotal,
            shipping_cents: shipping,
            total_cents: subtotal + shipping,
            currency: "DZD".to_string(),
            status: (*status).to_string(),
            token_hash: hash_token(&token),
            created_at: Some(created_at),
            updated_at: Some(created_at),
            wilaya_code: destination.code,
            wilaya_name: destination.wilaya.to_string(),
            commune_name: destination.commune.to_string(),
            delivery_type: "home".to_string(),
            ..Order::default()
        };
        store.create_order(tx.conn(), &order, &items).await?;
        created += 1;
    }
    tx.commit().await?;

    Ok(created)
}
