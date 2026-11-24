//! Basket line normalisation and merging.

use std::collections::HashMap;

/// One basket line. `expected_price_cents` is None when the caller does not
/// pin a price, as the counter sale does not.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Line {
    pub product_id: String,
    pub quantity: i64,
    pub options: HashMap<String, String>,
    pub expected_price_cents: Option<i64>,
}

/// Trims product ids and options, merges repeated lines and returns the
/// `items` field message callers send when a line is invalid.
pub fn build_lines(items: Vec<Line>) -> (Vec<Line>, HashMap<String, String>) {
    let mut fields = HashMap::new();
    let mut lines: Vec<Line> = Vec::with_capacity(items.len());
    let mut index: HashMap<String, usize> = HashMap::new();

    for item in items {
        let product_id = item.product_id.trim().to_string();
        if product_id.is_empty() {
            fields.insert(
                "items".to_string(),
                "Choose a product for every line.".to_string(),
            );
            continue;
        }
        if item.quantity < 1 || item.quantity > 10 {
            fields.insert(
                "items".to_string(),
                "Use between 1 and 10 units per product.".to_string(),
            );
            continue;
        }
        let options = normalize_options(&item.options);
        let line_key = format!("{product_id}|{}", options_key(&options));
        if let Some(existing) = index.get(&line_key) {
            lines[*existing].quantity += item.quantity;
            if lines[*existing].quantity > 10 {
                fields.insert(
                    "items".to_string(),
                    "Use between 1 and 10 units per product.".to_string(),
                );
            }
            continue;
        }
        index.insert(line_key, lines.len());
        lines.push(Line {
            product_id,
            quantity: item.quantity,
            options,
            expected_price_cents: item.expected_price_cents,
        });
    }
    (lines, fields)
}

fn normalize_options(options: &HashMap<String, String>) -> HashMap<String, String> {
    let mut out = HashMap::with_capacity(options.len());
    for (key, value) in options {
        out.insert(key.trim().to_string(), value.trim().to_string());
    }
    out
}

/// Go `net/url.QueryEscape` over the sorted keys.
fn options_key(options: &HashMap<String, String>) -> String {
    let mut keys: Vec<&String> = options.keys().collect();
    keys.sort();
    keys.iter()
        .map(|key| format!("{}={}", key, query_escape(&options[*key])))
        .collect::<Vec<_>>()
        .join("&")
}

fn query_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(product_id: &str, quantity: i64, options: &[(&str, &str)]) -> Line {
        Line {
            product_id: product_id.to_string(),
            quantity,
            options: options
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                .collect(),
            expected_price_cents: None,
        }
    }

    #[test]
    fn trims_and_merges_repeated_lines() {
        let (lines, fields) = build_lines(vec![
            line(" prd_a ", 2, &[(" colour ", " rose ")]),
            line("prd_a", 3, &[("colour", "rose")]),
            line("prd_b", 1, &[]),
        ]);
        assert!(fields.is_empty());
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].product_id, "prd_a");
        assert_eq!(lines[0].quantity, 5);
        assert_eq!(
            lines[0].options.get("colour").map(String::as_str),
            Some("rose")
        );
        assert_eq!(lines[1].product_id, "prd_b");
    }

    #[test]
    fn rejects_empty_products_and_bad_quantities() {
        let (lines, fields) = build_lines(vec![
            line(" ", 1, &[]),
            line("prd", 0, &[]),
            line("prd", 11, &[]),
        ]);
        assert!(lines.is_empty());
        assert_eq!(
            fields.get("items").map(String::as_str),
            Some("Use between 1 and 10 units per product.")
        );
    }

    #[test]
    fn merging_past_ten_reports_the_cap_but_keeps_the_line() {
        let (lines, fields) = build_lines(vec![line("prd", 6, &[]), line("prd", 6, &[])]);
        assert_eq!(lines[0].quantity, 12);
        assert_eq!(
            fields.get("items").map(String::as_str),
            Some("Use between 1 and 10 units per product.")
        );
    }

    #[test]
    fn option_keys_use_go_query_escaping() {
        let options = HashMap::from([
            ("b".to_string(), "x y".to_string()),
            ("a".to_string(), "é&".to_string()),
        ]);
        assert_eq!(options_key(&options), "a=%C3%A9%26&b=x+y");
    }
}
