//! The public Build your PC plugin: manifest and the compact parts catalog.
//! The HTTP handler itself lives in `azahar-httpapi`, which owns routing.

#![forbid(unsafe_code)]

use std::collections::HashMap;

use azahar_plugins::{Manifest, Migration, Plugin as PluginTrait, Route, StorefrontPage};
use azahar_store::{format_optional_time, Category, Product};
use chrono::Utc;
use serde::Serialize;

pub const PLUGIN_ID: &str = "pcbuilder";
pub const PC_PARTS_PLUGIN_ID: &str = "pcparts";
pub const MAX_PARTS: usize = 500;

pub struct Plugin;

pub fn new() -> Plugin {
    Plugin
}

impl PluginTrait for Plugin {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PLUGIN_ID.to_string(),
            name: "PC builder".to_string(),
            version: "1.0.0".to_string(),
            author: "Azahar".to_string(),
            description: "Lets a customer choose compatible PC parts and add the whole build to the bag.".to_string(),
            dependencies: vec![PC_PARTS_PLUGIN_ID.to_string()],
            storefront_pages: vec![StorefrontPage {
                path: "/build-your-pc".to_string(),
                label: "Build your PC".to_string(),
                sort_order: 10,
            }],
            ..Manifest::default()
        }
    }

    fn routes(&self) -> Vec<Route> {
        vec![Route {
            method: "GET".to_string(),
            pattern: "/parts".to_string(),
            admin: false,
        }]
    }

    fn migrations(&self) -> Vec<Migration> {
        Vec::new()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PartsResponse {
    pub categories: Vec<PartsCategory>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PartsCategory {
    pub slug: String,
    pub name: String,
    pub products: Vec<PartsProduct>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PartsProduct {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub subtitle: String,
    pub description: String,
    pub price_cents: i64,
    pub sale_price_cents: Option<i64>,
    pub sale_starts_at: String,
    pub sale_ends_at: String,
    pub effective_price_cents: i64,
    pub on_sale: bool,
    pub currency: String,
    pub image: String,
    pub stock: i32,
    pub plugin_id: String,
    pub attributes: HashMap<String, Vec<String>>,
    pub category: PartsCategoryRef,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PartsCategoryRef {
    pub slug: String,
    pub name: String,
    pub plugin_id: String,
}

/// Kept free of the database so shape and ordering can be tested directly.
pub fn build_parts_response(categories: &[Category], products: &[Product]) -> PartsResponse {
    let mut owned: Vec<&Category> = Vec::with_capacity(categories.len());
    let mut seen = std::collections::HashSet::new();
    for category in categories {
        if category.plugin_id != PC_PARTS_PLUGIN_ID || !seen.insert(category.slug.clone()) {
            continue;
        }
        owned.push(category);
    }
    owned.sort_by(|left, right| {
        left.sort_order
            .cmp(&right.sort_order)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.slug.cmp(&right.slug))
    });

    let mut grouped: HashMap<&str, Vec<&Product>> = HashMap::new();
    for product in products {
        if !product.active || product.plugin_id != PC_PARTS_PLUGIN_ID {
            continue;
        }
        grouped.entry(&product.category.slug).or_default().push(product);
    }

    let mut response = PartsResponse {
        categories: Vec::with_capacity(owned.len()),
    };
    for category in owned {
        let mut items = grouped.remove(category.slug.as_str()).unwrap_or_default();
        items.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.slug.cmp(&right.slug))
        });
        let listed = items
            .into_iter()
            .take(MAX_PARTS)
            .map(to_parts_product)
            .collect();
        response.categories.push(PartsCategory {
            slug: category.slug.clone(),
            name: category.name.clone(),
            products: listed,
        });
    }
    response
}

pub fn to_parts_product(product: &Product) -> PartsProduct {
    let now = Utc::now();
    PartsProduct {
        id: product.id.clone(),
        slug: product.slug.clone(),
        name: product.name.clone(),
        subtitle: product.subtitle.clone(),
        description: product.description.clone(),
        price_cents: product.price_cents,
        sale_price_cents: product.sale_price_cents,
        sale_starts_at: format_optional_time(product.sale_starts_at.as_ref()),
        sale_ends_at: format_optional_time(product.sale_ends_at.as_ref()),
        effective_price_cents: product.effective_price_cents(now),
        on_sale: product.sale_active(now),
        currency: if product.currency.is_empty() {
            "DZD".to_string()
        } else {
            product.currency.clone()
        },
        image: product.image.clone(),
        stock: product.stock,
        plugin_id: PC_PARTS_PLUGIN_ID.to_string(),
        attributes: product.attributes.clone(),
        category: PartsCategoryRef {
            slug: product.category.slug.clone(),
            name: product.category.name.clone(),
            plugin_id: product.category.plugin_id.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn category(slug: &str, name: &str, sort_order: i32) -> Category {
        Category {
            slug: slug.to_string(),
            name: name.to_string(),
            plugin_id: PC_PARTS_PLUGIN_ID.to_string(),
            sort_order,
            ..Default::default()
        }
    }

    fn product(slug: &str, name: &str, category: &Category) -> Product {
        Product {
            id: format!("prd_{slug}"),
            slug: slug.to_string(),
            name: name.to_string(),
            plugin_id: PC_PARTS_PLUGIN_ID.to_string(),
            active: true,
            category: category.clone(),
            attributes: HashMap::from([("socket".to_string(), vec!["AM5".to_string()])]),
            ..Default::default()
        }
    }

    #[test]
    fn groups_active_parts_in_category_order() {
        let cpu = category("cpu", "CPU", 2);
        let gpu = category("gpu", "GPU", 1);
        let products = vec![
            product("b-cpu", "Beta CPU", &cpu),
            product("a-gpu", "Alpha GPU", &gpu),
            Product {
                active: false,
                ..product("hidden", "Hidden GPU", &gpu)
            },
        ];
        let response = build_parts_response(&[cpu, gpu], &products);
        assert_eq!(response.categories.len(), 2);
        assert_eq!(response.categories[0].slug, "gpu");
        assert_eq!(response.categories[0].products.len(), 1);
        assert_eq!(response.categories[0].products[0].slug, "a-gpu");
        assert_eq!(response.categories[1].slug, "cpu");
    }
}
