//! The oils demo shelf, ported from `backend/internal/seed/seed.go`.

use std::collections::HashMap;

use azahar_store::{Product, Store};
use chrono::{TimeDelta, Utc};

use crate::{StageError, StageResult};

struct SeedProduct {
    slug: &'static str,
    name: &'static str,
    subtitle: &'static str,
    description: &'static str,
    price_cents: i64,
    sale_price_cents: i64,
    size: &'static str,
    category: &'static str,
    concerns: &'static [&'static str],
    ingredients: &'static [&'static str],
    how_to: &'static str,
    image: &'static str,
    stock: i32,
    featured: bool,
}

static PRODUCTS: &[SeedProduct] = &[
    SeedProduct {
        slug: "orange-blossom-cleansing-oil",
        name: "Orange blossom cleansing oil",
        subtitle: "The day, lifted",
        description: "A light oil that lifts makeup and the day's dust. Massage it over dry skin, add water, and rinse to a clean, soft finish.",
        price_cents: 480_000,
        sale_price_cents: 0,
        size: "100 ml",
        category: "face",
        concerns: &["dry", "sensitive"],
        ingredients: &["Olive squalane", "Orange blossom", "Sweet almond"],
        how_to: "Warm two pumps between dry hands, massage over the face, add water, then rinse.",
        image: "/images/products/cleansing-oil-1200.webp",
        stock: 24,
        featured: true,
    },
    SeedProduct {
        slug: "rosehip-and-olive-face-serum",
        name: "Rosehip and olive face serum",
        subtitle: "A quiet layer of oil",
        description: "Rosehip and olive oils sink in without a heavy film. Use it at night after cleansing, or in the morning under a cream.",
        price_cents: 690_000,
        sale_price_cents: 0,
        size: "30 ml",
        category: "face",
        concerns: &["dry", "dull", "texture"],
        ingredients: &["Rosehip seed oil", "Olive oil", "Jojoba"],
        how_to: "Press three drops into clean skin, morning or night.",
        image: "/images/products/serum-1200.webp",
        stock: 18,
        featured: true,
    },
    SeedProduct {
        slug: "rose-clay-mask",
        name: "Rose clay mask",
        subtitle: "Ten minutes in the grove",
        description: "A soft clay mask with rose and chamomile. It lifts oil from the surface and leaves the skin comfortable, not tight.",
        price_cents: 360_000,
        sale_price_cents: 288_000,
        size: "75 g",
        category: "face",
        concerns: &["oily", "congested"],
        ingredients: &["Rose clay", "Kaolin", "Chamomile"],
        how_to: "Spread a thin layer over clean skin, leave for ten minutes, then rinse with warm water.",
        image: "/images/products/clay-mask-1200.webp",
        stock: 4,
        featured: false,
    },
    SeedProduct {
        slug: "fig-and-sesame-body-balm",
        name: "Fig and sesame body balm",
        subtitle: "For the space after a bath",
        description: "A firm balm that melts with body heat. Fig and sesame soften rough patches on elbows, knees and hands.",
        price_cents: 420_000,
        sale_price_cents: 0,
        size: "120 ml",
        category: "body",
        concerns: &["dry", "rough"],
        ingredients: &["Fig extract", "Sesame oil", "Shea butter"],
        how_to: "Warm a small amount between the palms and press it into damp skin.",
        image: "/images/products/body-balm-1200.webp",
        stock: 12,
        featured: false,
    },
    SeedProduct {
        slug: "prickly-pear-face-oil",
        name: "Prickly pear face oil",
        subtitle: "The desert, pressed",
        description: "Prickly pear and argan oils in a light blend. It leaves a soft sheen and works well on cold mornings.",
        price_cents: 840_000,
        sale_price_cents: 0,
        size: "30 ml",
        category: "face",
        concerns: &["dry", "dull"],
        ingredients: &["Prickly pear seed oil", "Argan oil", "Vitamin E"],
        how_to: "Warm two drops and press over the face and neck.",
        image: "/images/products/prickly-pear-1200.webp",
        stock: 2,
        featured: true,
    },
    SeedProduct {
        slug: "cedar-and-myrrh-hand-cream",
        name: "Cedar and myrrh hand cream",
        subtitle: "Wood, resin, warm hands",
        description: "A rich cream that stays through a day of washing. Cedar and myrrh leave a dry, woody scent.",
        price_cents: 260_000,
        sale_price_cents: 0,
        size: "60 ml",
        category: "body",
        concerns: &["dry", "rough"],
        ingredients: &["Cedarwood", "Myrrh", "Shea butter"],
        how_to: "Massage into hands and cuticles, then work the rest into the wrists.",
        image: "/images/products/hand-cream-1200.webp",
        stock: 30,
        featured: false,
    },
    SeedProduct {
        slug: "green-fig-body-oil",
        name: "Green fig body oil",
        subtitle: "Oil for bare arms",
        description: "Green fig and sesame in a quick body oil. It absorbs after a minute and leaves the skin soft, not wet.",
        price_cents: 540_000,
        sale_price_cents: 0,
        size: "100 ml",
        category: "body",
        concerns: &["dry", "dull"],
        ingredients: &["Green fig", "Sesame oil", "Grape seed oil"],
        how_to: "Smooth over damp skin after a shower and let it settle.",
        image: "/images/products/fig-oil-1200.webp",
        stock: 15,
        featured: true,
    },
    SeedProduct {
        slug: "rose-petal-bath-soak",
        name: "Rose petal bath soak",
        subtitle: "The bath, slowed",
        description: "Rose petals, sea salt and oats for a warm bath. The water turns soft and the room smells of the garden.",
        price_cents: 320_000,
        sale_price_cents: 0,
        size: "200 g",
        category: "ritual",
        concerns: &["dry", "sensitive"],
        ingredients: &["Rose petals", "Sea salt", "Oat"],
        how_to: "Tip a handful into warm running water and steep for ten minutes.",
        image: "/images/products/rose-soak-1200.webp",
        stock: 9,
        featured: true,
    },
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CatalogResult {
    pub categories: usize,
    pub products: usize,
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

pub(crate) async fn seed_catalog(store: &Store) -> StageResult<CatalogResult> {
    let mut result = CatalogResult::default();
    if store.count_products().await? > 0 {
        return Ok(result);
    }

    let now = Utc::now();
    let existing = store.categories().await?;
    let by_slug: HashMap<String, _> = existing
        .into_iter()
        .filter(|category| category.plugin_id == "oils")
        .map(|category| (category.slug.clone(), category))
        .collect();

    for item in PRODUCTS {
        if !by_slug.contains_key(item.category) {
            return Err(StageError::message(format!(
                "category {} is not registered by the oils plugin",
                item.category
            )));
        }
    }
    result.categories = by_slug.len();

    let tx = store.begin_write().await?;
    for (index, item) in PRODUCTS.iter().enumerate() {
        let Some(category) = by_slug.get(item.category).cloned() else {
            return Err(StageError::message(format!(
                "category {} is not registered by the oils plugin",
                item.category
            )));
        };

        let created_at = now - TimeDelta::days((PRODUCTS.len() - index) as i64);
        let (sale_price_cents, sale_starts_at, sale_ends_at) =
            if item.sale_price_cents > 0 && item.sale_price_cents < item.price_cents {
                (
                    Some(item.sale_price_cents),
                    Some(now - TimeDelta::hours(1)),
                    Some(now + TimeDelta::days(7)),
                )
            } else {
                (None, None, None)
            };

        let concerns = strings(item.concerns);
        let ingredients = strings(item.ingredients);
        let image = item.image.to_string();
        let attributes = HashMap::from([
            ("size".to_string(), vec![item.size.to_string()]),
            ("concerns".to_string(), concerns.clone()),
            ("ingredients".to_string(), ingredients.clone()),
            ("how_to".to_string(), vec![item.how_to.to_string()]),
        ]);

        let product = Product {
            id: format!("prd_{:02}", index + 1),
            slug: item.slug.to_string(),
            name: item.name.to_string(),
            subtitle: item.subtitle.to_string(),
            description: item.description.to_string(),
            price_cents: item.price_cents,
            sale_price_cents,
            sale_starts_at,
            sale_ends_at,
            currency: "DZD".to_string(),
            size: item.size.to_string(),
            category,
            concerns,
            ingredients,
            how_to: item.how_to.to_string(),
            image: image.clone(),
            gallery: vec![image],
            stock: item.stock,
            featured: item.featured,
            active: true,
            plugin_id: "oils".to_string(),
            attributes,
            created_at: Some(created_at),
            updated_at: Some(created_at),
            ..Product::default()
        };
        store.create_product(tx.conn(), &product).await?;
        result.products += 1;
    }
    tx.commit().await?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn product_contract_matches_the_go_seed() {
        let expected = [
            (
                "orange-blossom-cleansing-oil",
                "Orange blossom cleansing oil",
                "100 ml",
                480_000,
                "/images/products/cleansing-oil-1200.webp",
            ),
            (
                "rosehip-and-olive-face-serum",
                "Rosehip and olive face serum",
                "30 ml",
                690_000,
                "/images/products/serum-1200.webp",
            ),
            (
                "rose-clay-mask",
                "Rose clay mask",
                "75 g",
                360_000,
                "/images/products/clay-mask-1200.webp",
            ),
            (
                "fig-and-sesame-body-balm",
                "Fig and sesame body balm",
                "120 ml",
                420_000,
                "/images/products/body-balm-1200.webp",
            ),
            (
                "prickly-pear-face-oil",
                "Prickly pear face oil",
                "30 ml",
                840_000,
                "/images/products/prickly-pear-1200.webp",
            ),
            (
                "cedar-and-myrrh-hand-cream",
                "Cedar and myrrh hand cream",
                "60 ml",
                260_000,
                "/images/products/hand-cream-1200.webp",
            ),
            (
                "green-fig-body-oil",
                "Green fig body oil",
                "100 ml",
                540_000,
                "/images/products/fig-oil-1200.webp",
            ),
            (
                "rose-petal-bath-soak",
                "Rose petal bath soak",
                "200 g",
                320_000,
                "/images/products/rose-soak-1200.webp",
            ),
        ];

        assert_eq!(PRODUCTS.len(), expected.len());
        let mut featured = 0;
        for (index, (slug, name, size, price, image)) in expected.iter().enumerate() {
            let product = &PRODUCTS[index];
            assert_eq!(product.slug, *slug);
            assert_eq!(product.name, *name);
            assert_eq!(product.size, *size);
            assert_eq!(product.price_cents, *price);
            assert_eq!(product.image, *image);
            assert!((2..=4).contains(&product.concerns.len()));
            assert!((3..=4).contains(&product.ingredients.len()));
            assert!((0..=30).contains(&product.stock));
            if product.featured {
                featured += 1;
            }
        }
        assert!((4..=6).contains(&featured));
    }

    #[test]
    fn every_product_uses_a_registered_oils_category() {
        let known: HashSet<&str> = ["face", "body", "ritual"].into_iter().collect();
        for product in PRODUCTS {
            assert!(known.contains(product.category), "{}", product.slug);
        }
        assert_eq!(
            PRODUCTS
                .iter()
                .filter(|product| product.sale_price_cents > 0)
                .count(),
            1
        );
        assert_eq!(PRODUCTS[2].sale_price_cents, 288_000);
    }
}
