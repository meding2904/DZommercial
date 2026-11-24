//! JSON shapes shared by the product integration and differential tests.
//!
//! The Go oracle prints these same shapes, so a comparison is a plain
//! `serde_json::Value` equality.

#![allow(dead_code)]

use azahar_store::{Category, Product, Timestamp, format_time};
use serde_json::{Value, json};

pub fn time_json(value: Timestamp) -> Value {
    match value {
        Some(time) => Value::String(format_time(&time)),
        None => Value::Null,
    }
}

pub fn category_json(category: &Category) -> Value {
    json!({
        "id": &category.id,
        "slug": &category.slug,
        "name": &category.name,
        "description": &category.description,
        "sort_order": category.sort_order,
        "plugin_id": &category.plugin_id,
    })
}

pub fn product_json(product: &Product) -> Value {
    json!({
        "id": &product.id,
        "slug": &product.slug,
        "name": &product.name,
        "subtitle": &product.subtitle,
        "description": &product.description,
        "price_cents": product.price_cents,
        "compare_at_cents": product.compare_at_cents,
        "sale_price_cents": product.sale_price_cents,
        "sale_starts_at": time_json(product.sale_starts_at),
        "sale_ends_at": time_json(product.sale_ends_at),
        "currency": &product.currency,
        "size": &product.size,
        "category": category_json(&product.category),
        "concerns": &product.concerns,
        "ingredients": &product.ingredients,
        "how_to": &product.how_to,
        "image": &product.image,
        "gallery": &product.gallery,
        "stock": product.stock,
        "featured": product.featured,
        "active": product.active,
        "plugin_id": &product.plugin_id,
        "attributes": &product.attributes,
        "created_at": time_json(product.created_at),
        "updated_at": time_json(product.updated_at),
    })
}

pub fn products_json(products: &[Product]) -> Value {
    Value::Array(products.iter().map(product_json).collect())
}

pub fn categories_json(categories: &[Category]) -> Value {
    Value::Array(categories.iter().map(category_json).collect())
}
