//! Prompt construction, ported byte for byte from `prompts.go`.
//!
//! The strings are part of the model input, so ordering, punctuation, the
//! five-item caps and the status sort all match the Go builder exactly.

use std::fmt::Write as _;

use azahar_store::Product;
use azahar_store::metrics::Metrics;

/// Supported answer languages with the English name used in the instruction.
pub const SUPPORTED_LANGUAGES: [(&str, &str); 3] =
    [("en", "English"), ("fr", "French"), ("ar", "Arabic")];

/// English name of a supported language code.
#[must_use]
pub fn language_name(language: &str) -> Option<&'static str> {
    SUPPORTED_LANGUAGES
        .iter()
        .find(|(code, _)| *code == language)
        .map(|(_, name)| *name)
}

#[must_use]
pub fn is_supported_language(language: &str) -> bool {
    language_name(language).is_some()
}

/// The system message: facts only, plain text, in the requested language.
/// Unknown language codes fall back to the default language, like the Go map
/// lookup.
#[must_use]
pub fn analysis_instruction(language: &str) -> String {
    let name = language_name(language).unwrap_or("English");
    format!(
        "Use only the facts below. Do not invent numbers or claims. \
         Plain text, 120 to 180 words, no markdown headings. Write the answer in {name}."
    )
}

/// Store facts and their instruction. Customer details and order identifiers
/// never enter the prompt.
#[must_use]
pub fn build_store_prompt(language: &str, days: i32, metrics: &Metrics) -> (String, String) {
    let mut facts = String::new();

    let _ = writeln!(
        facts,
        "Period: last {days} days ({} to {}).",
        metrics.from.format("%Y-%m-%d"),
        metrics.to.format("%Y-%m-%d")
    );
    let _ = writeln!(facts, "Revenue: {}.", format_dzd(metrics.revenue_cents));
    let _ = writeln!(
        facts,
        "Previous period revenue: {}.",
        format_dzd(metrics.previous_revenue_cents)
    );
    let _ = writeln!(facts, "Orders: {}.", metrics.orders_count);
    let _ = writeln!(facts, "Units sold: {}.", metrics.units_sold);
    let _ = writeln!(facts, "New customers: {}.", metrics.new_customers);

    facts.push_str("Top products by units:\n");
    if metrics.top_products.is_empty() {
        facts.push_str("- None.\n");
    } else {
        for item in metrics.top_products.iter().take(5) {
            let _ = writeln!(
                facts,
                "- {}: {} units, {} revenue.",
                item.name,
                item.units,
                format_dzd(item.revenue_cents)
            );
        }
    }

    facts.push_str("Low stock:\n");
    if metrics.low_stock.is_empty() {
        facts.push_str("- None.\n");
    } else {
        for item in metrics.low_stock.iter().take(5) {
            let _ = writeln!(facts, "- {}: {} in stock.", item.name, item.stock);
        }
    }

    facts.push_str("Order status split:\n");
    if metrics.status_split.is_empty() {
        facts.push_str("- None.\n");
    } else {
        let mut statuses: Vec<&str> = metrics.status_split.keys().map(String::as_str).collect();
        statuses.sort_unstable();
        for status in statuses {
            if let Some(count) = metrics.status_split.get(status) {
                let _ = writeln!(facts, "- {status}: {count} orders.");
            }
        }
    }

    (analysis_instruction(language), facts)
}

/// Sales in the product analysis window. The days are fixed at 30 in the
/// handler, exactly like the Go `productSales{Days: 30}`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProductSales {
    pub days: i32,
    pub units: i64,
    pub orders: i64,
    pub revenue_cents: i64,
}

/// Product facts and their instruction. Only stored product data and aggregate
/// sales are used.
#[must_use]
pub fn build_product_prompt(
    language: &str,
    product: &Product,
    sales: &ProductSales,
) -> (String, String) {
    let mut facts = String::new();

    let _ = writeln!(facts, "Product name: {}", product.name);
    let subtitle = product.subtitle.trim();
    if !subtitle.is_empty() {
        let _ = writeln!(facts, "Subtitle: {subtitle}");
    }
    let description = product.description.trim();
    if !description.is_empty() {
        let _ = writeln!(facts, "Description: {description}");
    }
    let _ = writeln!(facts, "Category: {}", product.category.name);
    let _ = writeln!(facts, "Price: {}", format_dzd(product.price_cents));
    let size = product.size.trim();
    if !size.is_empty() {
        let _ = writeln!(facts, "Size: {size}");
    }
    let _ = writeln!(facts, "Stock: {}", product.stock);

    if sales.units == 0 && sales.orders == 0 {
        let _ = writeln!(facts, "Sales, last {} days: none recorded.", sales.days);
    } else {
        let _ = writeln!(
            facts,
            "Sales, last {} days: {} units, {} revenue, {} orders.",
            sales.days,
            sales.units,
            format_dzd(sales.revenue_cents),
            sales.orders
        );
    }

    if !product.concerns.is_empty() {
        let _ = writeln!(facts, "Concerns: {}", product.concerns.join(", "));
    }
    if !product.ingredients.is_empty() {
        let _ = writeln!(facts, "Ingredients: {}", product.ingredients.join(", "));
    }
    if !product.attributes.is_empty() {
        facts.push_str("Options:\n");
        let mut keys: Vec<&str> = product.attributes.keys().map(String::as_str).collect();
        keys.sort_unstable();
        for key in keys {
            let Some(values) = product.attributes.get(key) else {
                continue;
            };
            if values.is_empty() {
                continue;
            }
            let _ = writeln!(facts, "- {key}: {}", values.join(", "));
        }
    }

    (analysis_instruction(language), facts)
}

/// `DZD <sign><units>.<hundredths>`, matching Go's `%d.%02d`. The negation
/// wraps for `i64::MIN`, like Go's two's-complement `-cents`.
#[must_use]
pub fn format_dzd(cents: i64) -> String {
    let (sign, cents) = if cents < 0 {
        ("-", cents.wrapping_neg())
    } else {
        ("", cents)
    };
    format!("DZD {sign}{}.{:02}", cents / 100, cents % 100)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{TimeZone, Utc};

    use super::*;
    use azahar_store::Category;
    use azahar_store::metrics::{LowStockItem, TopProduct};

    fn utc(year: i32, month: u32, day: u32) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, 0, 0, 0)
            .single()
            .expect("valid date")
    }

    fn metrics() -> Metrics {
        Metrics {
            demo: false,
            from: utc(2026, 11, 1),
            to: utc(2026, 11, 30),
            revenue_cents: 123_456,
            previous_revenue_cents: 100_000,
            orders_count: 9,
            previous_orders_count: 0,
            units_sold: 14,
            new_customers: 3,
            low_stock: vec![
                LowStockItem {
                    id: "prd_cream".to_string(),
                    name: "Cream".to_string(),
                    stock: 2,
                    image: String::new(),
                },
                LowStockItem {
                    id: "prd_extra".to_string(),
                    name: "Extra".to_string(),
                    stock: 4,
                    image: String::new(),
                },
                LowStockItem {
                    id: "prd_third".to_string(),
                    name: "Third".to_string(),
                    stock: 5,
                    image: String::new(),
                },
                LowStockItem {
                    id: "prd_fourth".to_string(),
                    name: "Fourth".to_string(),
                    stock: 5,
                    image: String::new(),
                },
                LowStockItem {
                    id: "prd_fifth".to_string(),
                    name: "Fifth".to_string(),
                    stock: 5,
                    image: String::new(),
                },
                LowStockItem {
                    id: "prd_sixth".to_string(),
                    name: "Sixth".to_string(),
                    stock: 5,
                    image: String::new(),
                },
            ],
            revenue_series: Vec::new(),
            status_split: HashMap::from([("pending".to_string(), 2), ("paid".to_string(), 7)]),
            top_products: vec![
                TopProduct {
                    product_id: "prd_serum".to_string(),
                    name: "Serum".to_string(),
                    units: 6,
                    revenue_cents: 90_000,
                },
                TopProduct {
                    product_id: "prd_second".to_string(),
                    name: "Second".to_string(),
                    units: 5,
                    revenue_cents: 1,
                },
                TopProduct {
                    product_id: "prd_third".to_string(),
                    name: "Third".to_string(),
                    units: 4,
                    revenue_cents: 1,
                },
                TopProduct {
                    product_id: "prd_fourth".to_string(),
                    name: "Fourth".to_string(),
                    units: 3,
                    revenue_cents: 1,
                },
                TopProduct {
                    product_id: "prd_fifth".to_string(),
                    name: "Fifth".to_string(),
                    units: 2,
                    revenue_cents: 1,
                },
                TopProduct {
                    product_id: "prd_sixth".to_string(),
                    name: "Sixth".to_string(),
                    units: 1,
                    revenue_cents: 1,
                },
            ],
            recent_orders: Vec::new(),
        }
    }

    fn product() -> Product {
        Product {
            name: "Hydrating serum".to_string(),
            subtitle: "  Light daily serum  ".to_string(),
            description: "A fragrance-free serum with hyaluronic acid.".to_string(),
            price_cents: 450_000,
            size: " 30 ml ".to_string(),
            stock: 12,
            category: Category {
                name: "Skincare".to_string(),
                ..Category::default()
            },
            concerns: vec!["dryness".to_string()],
            ingredients: vec!["hyaluronic acid".to_string()],
            attributes: HashMap::from([
                (
                    "scent".to_string(),
                    vec!["rose".to_string(), "unscented".to_string()],
                ),
                ("empty".to_string(), Vec::new()),
            ]),
            ..Product::default()
        }
    }

    #[test]
    fn instruction_names_the_answer_language() {
        assert_eq!(
            analysis_instruction("en"),
            "Use only the facts below. Do not invent numbers or claims. Plain text, 120 to 180 words, no markdown headings. Write the answer in English."
        );
        assert!(analysis_instruction("fr").ends_with("Write the answer in French."));
        assert!(analysis_instruction("ar").ends_with("Write the answer in Arabic."));
        assert!(analysis_instruction("de").ends_with("Write the answer in English."));
    }

    #[test]
    fn store_prompt_is_byte_exact() {
        let (system, facts) = build_store_prompt("en", 30, &metrics());
        assert_eq!(
            system,
            "Use only the facts below. Do not invent numbers or claims. Plain text, 120 to 180 words, no markdown headings. Write the answer in English."
        );
        assert_eq!(
            facts,
            "Period: last 30 days (2026-11-01 to 2026-11-30).\n\
             Revenue: DZD 1234.56.\n\
             Previous period revenue: DZD 1000.00.\n\
             Orders: 9.\n\
             Units sold: 14.\n\
             New customers: 3.\n\
             Top products by units:\n\
             - Serum: 6 units, DZD 900.00 revenue.\n\
             - Second: 5 units, DZD 0.01 revenue.\n\
             - Third: 4 units, DZD 0.01 revenue.\n\
             - Fourth: 3 units, DZD 0.01 revenue.\n\
             - Fifth: 2 units, DZD 0.01 revenue.\n\
             Low stock:\n\
             - Cream: 2 in stock.\n\
             - Extra: 4 in stock.\n\
             - Third: 5 in stock.\n\
             - Fourth: 5 in stock.\n\
             - Fifth: 5 in stock.\n\
             Order status split:\n\
             - paid: 7 orders.\n\
             - pending: 2 orders.\n"
        );
    }

    #[test]
    fn store_prompt_uses_none_for_empty_sections() {
        let empty = Metrics {
            demo: false,
            from: utc(2026, 1, 1),
            to: utc(2026, 1, 7),
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
        let (_, facts) = build_store_prompt("en", 7, &empty);
        assert!(facts.contains("Top products by units:\n- None.\n"));
        assert!(facts.contains("Low stock:\n- None.\n"));
        assert!(facts.contains("Order status split:\n- None.\n"));
    }

    #[test]
    fn product_prompt_is_byte_exact() {
        let (system, facts) = build_product_prompt(
            "fr",
            &product(),
            &ProductSales {
                days: 30,
                units: 9,
                orders: 4,
                revenue_cents: 405_000,
            },
        );
        assert!(system.ends_with("Write the answer in French."));
        assert_eq!(
            facts,
            "Product name: Hydrating serum\n\
             Subtitle: Light daily serum\n\
             Description: A fragrance-free serum with hyaluronic acid.\n\
             Category: Skincare\n\
             Price: DZD 4500.00\n\
             Size: 30 ml\n\
             Stock: 12\n\
             Sales, last 30 days: 9 units, DZD 4050.00 revenue, 4 orders.\n\
             Concerns: dryness\n\
             Ingredients: hyaluronic acid\n\
             Options:\n\
             - scent: rose, unscented\n"
        );
    }

    #[test]
    fn product_prompt_reports_missing_sales_and_empty_fields() {
        let bare = Product {
            name: "Bare".to_string(),
            ..Product::default()
        };
        let (_, facts) = build_product_prompt(
            "ar",
            &bare,
            &ProductSales {
                days: 30,
                ..ProductSales::default()
            },
        );
        assert_eq!(
            facts,
            "Product name: Bare\n\
             Category: \n\
             Price: DZD 0.00\n\
             Stock: 0\n\
             Sales, last 30 days: none recorded.\n"
        );
    }

    #[test]
    fn dzd_formatting_matches_go() {
        assert_eq!(format_dzd(0), "DZD 0.00");
        assert_eq!(format_dzd(5), "DZD 0.05");
        assert_eq!(format_dzd(100), "DZD 1.00");
        assert_eq!(format_dzd(123_456), "DZD 1234.56");
        assert_eq!(format_dzd(-1), "DZD -0.01");
        assert_eq!(format_dzd(-123_456), "DZD -1234.56");
        assert_eq!(format_dzd(1_000_000), "DZD 10000.00");
        assert_eq!(
            format_dzd(i64::MIN),
            "DZD --92233720368547758.-8",
            "Go wraps the negation and prints a signed remainder"
        );
    }
}
