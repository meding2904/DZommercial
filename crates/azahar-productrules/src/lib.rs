//! Product write rules shared by the admin API and the code-mode planner,
//! ported from `backend/internal/productrules`.

#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};

use azahar_plugins::{OptionField, ProductType};

pub const MIN_NAME_RUNES: usize = 2;
pub const MAX_NAME_RUNES: usize = 120;
pub const MAX_SUBTITLE_RUNES: usize = 160;
pub const MAX_DESCRIPTION_RUNES: usize = 4000;
pub const MAX_PRICE_CENTS: i64 = 99_999_999;
pub const MAX_STOCK: i64 = 100_000;

const MAX_IMAGE_PATH_BYTES: usize = 500;
const MAX_OPTION_VALUES: usize = 12;
const MAX_OPTION_VALUE_RUNES: usize = 80;
const MAX_TEXT_RUNES: usize = 120;
const MAX_TEXTAREA_RUNES: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    MissingRequired,
    SingleValue,
    TooManyValues,
    InvalidText,
    InvalidValue,
    DuplicateValue,
    UnknownField,
    EmptyOptionName,
    DuplicateOptionName,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub field: String,
    pub kind: Kind,
    pub label: String,
    pub limit: usize,
}

impl Problem {
    fn new(kind: Kind) -> Self {
        Self {
            field: String::new(),
            kind,
            label: String::new(),
            limit: 0,
        }
    }

    fn field(field: &OptionField, kind: Kind) -> Self {
        Self {
            field: field.key.clone(),
            kind,
            label: field.label.clone(),
            limit: 0,
        }
    }
}

pub fn valid_name(name: &str) -> bool {
    let length = name.chars().count();
    (MIN_NAME_RUNES..=MAX_NAME_RUNES).contains(&length)
}

pub fn valid_price(cents: i64) -> bool {
    (1..=MAX_PRICE_CENTS).contains(&cents)
}

pub fn valid_stock(stock: i64) -> bool {
    (0..=MAX_STOCK).contains(&stock)
}

pub fn valid_image_id(id: &str) -> bool {
    let Some(hex) = id.strip_prefix("img_") else {
        return false;
    };
    hex.len() == 24
        && hex
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
}

/// A bundled image under `/images/` or an uploaded one under `/api/images/`.
pub fn valid_image_path(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_IMAGE_PATH_BYTES {
        return false;
    }
    if value.contains(['\\', '%', '?', '#']) || value.contains("..") {
        return false;
    }
    if let Some(id) = value.strip_prefix("/api/images/") {
        return valid_image_id(id);
    }
    value.starts_with("/images/")
}

/// Free of control runes other than the newline and tab textareas allow.
pub fn plain_text(value: &str) -> bool {
    value.chars().all(|character| {
        character == '\n' || character == '\t' || (character >= ' ' && character != '\u{7f}')
    })
}

/// Trims keys and values; reports empty or duplicate keys but returns every
/// entry so callers that ignore problems keep the previous overwrite.
pub fn normalize_attributes(
    attributes: &HashMap<String, Vec<String>>,
) -> (HashMap<String, Vec<String>>, Vec<Problem>) {
    if attributes.is_empty() {
        return (HashMap::new(), Vec::new());
    }
    let mut normalized = HashMap::with_capacity(attributes.len());
    let mut problems = Vec::new();
    let mut keys: Vec<&String> = attributes.keys().collect();
    keys.sort();
    for key in keys {
        let values: Vec<String> = attributes[key]
            .iter()
            .map(|value| value.trim().to_string())
            .collect();
        let key = key.trim();
        if key.is_empty() {
            problems.push(Problem::new(Kind::EmptyOptionName));
        } else if normalized.contains_key(key) {
            let mut problem = Problem::new(Kind::DuplicateOptionName);
            problem.field = key.to_string();
            problems.push(problem);
        }
        normalized.insert(key.to_string(), values);
    }
    (normalized, problems)
}

/// Checks options against a product type. At most one problem per field, in
/// manifest order, with an unknown key reported last.
pub fn validate_attributes(
    product_type: &ProductType,
    attributes: &HashMap<String, Vec<String>>,
) -> Vec<Problem> {
    let known: HashSet<&str> = product_type
        .fields
        .iter()
        .map(|field| field.key.as_str())
        .collect();
    let mut problems = Vec::new();
    for field in &product_type.fields {
        let values = attributes.get(&field.key);
        if field.required && values.is_none() {
            problems.push(Problem::field(field, Kind::MissingRequired));
            continue;
        }
        let Some(values) = values else {
            continue;
        };
        match field.field_type.as_str() {
            "text" | "textarea" => {
                let limit = if field.field_type == "textarea" {
                    MAX_TEXTAREA_RUNES
                } else {
                    MAX_TEXT_RUNES
                };
                if values.len() > 1 {
                    problems.push(Problem::field(field, Kind::SingleValue));
                    continue;
                }
                if values.len() == 1 {
                    let value = values[0].trim();
                    if !valid_option_value(value, limit) {
                        let mut problem = Problem::field(field, Kind::InvalidText);
                        problem.limit = limit;
                        problems.push(problem);
                    }
                }
            }
            "select" | "list" => {
                if values.len() > MAX_OPTION_VALUES {
                    problems.push(Problem::field(field, Kind::TooManyValues));
                    continue;
                }
                let mut seen = HashSet::new();
                for value in values {
                    let value = value.trim();
                    if !valid_option_value(value, MAX_OPTION_VALUE_RUNES) {
                        problems.push(Problem::field(field, Kind::InvalidValue));
                        break;
                    }
                    if !seen.insert(value) {
                        problems.push(Problem::field(field, Kind::DuplicateValue));
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    let mut keys: Vec<&String> = attributes.keys().collect();
    keys.sort();
    if keys.iter().any(|key| !known.contains(key.as_str())) {
        problems.push(Problem::new(Kind::UnknownField));
    }
    problems
}

fn valid_option_value(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.chars().count() <= limit && plain_text(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(key: &str, label: &str, field_type: &str, required: bool) -> OptionField {
        OptionField {
            key: key.to_string(),
            label: label.to_string(),
            field_type: field_type.to_string(),
            required,
            help: String::new(),
            suggestions: Vec::new(),
        }
    }

    fn schema() -> ProductType {
        ProductType {
            label: "Oils".to_string(),
            summary: String::new(),
            fields: vec![
                field("storage", "Storage", "select", true),
                field("size", "Size", "text", false),
                field("notes", "Notes", "textarea", false),
                field("concerns", "Concerns", "list", false),
            ],
        }
    }

    fn options(pairs: &[(&str, &[&str])]) -> HashMap<String, Vec<String>> {
        pairs
            .iter()
            .map(|(key, values)| {
                (
                    (*key).to_string(),
                    values.iter().map(|value| (*value).to_string()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn bounds_match_the_go_tests() {
        assert!(!valid_name(""));
        assert!(!valid_name("x"));
        assert!(valid_name("olive oil"));
        assert!(valid_name("éé"));
        assert!(!valid_price(0));
        assert!(valid_price(MAX_PRICE_CENTS));
        assert!(!valid_price(MAX_PRICE_CENTS + 1));
        assert!(valid_stock(0));
        assert!(valid_stock(MAX_STOCK));
        assert!(!valid_stock(-1));
        assert!(valid_image_id("img_0123456789abcdef01234567"));
        assert!(!valid_image_id("img_0123456789ABCDEF01234567"));
        assert!(valid_image_path("/images/products/a.webp"));
        assert!(valid_image_path("/api/images/img_0123456789abcdef01234567"));
        assert!(!valid_image_path("/images/../secret"));
        assert!(!plain_text("bell\u{7}"));
    }

    #[test]
    fn validates_options_in_manifest_order() {
        let valid = options(&[
            ("storage", &["128 GB"]),
            ("size", &["100 ml"]),
            ("notes", &["Apply daily."]),
            ("concerns", &["dry", "sensitive"]),
        ]);
        assert!(validate_attributes(&schema(), &valid).is_empty());

        let cases: Vec<(HashMap<String, Vec<String>>, Problem)> = vec![
            (
                options(&[("size", &["100 ml"])]),
                Problem {
                    field: "storage".into(),
                    kind: Kind::MissingRequired,
                    label: "Storage".into(),
                    limit: 0,
                },
            ),
            (
                options(&[("storage", &["128 GB"]), ("size", &["100 ml", "50 ml"])]),
                Problem {
                    field: "size".into(),
                    kind: Kind::SingleValue,
                    label: "Size".into(),
                    limit: 0,
                },
            ),
            (
                options(&[("storage", &["128 GB"]), ("concerns", &["dry", " dry "])]),
                Problem {
                    field: "concerns".into(),
                    kind: Kind::DuplicateValue,
                    label: "Concerns".into(),
                    limit: 0,
                },
            ),
            (
                options(&[("storage", &["128 GB"]), ("battery", &["5000 mAh"])]),
                Problem::new(Kind::UnknownField),
            ),
        ];
        for (attributes, expected) in cases {
            assert_eq!(validate_attributes(&schema(), &attributes), vec![expected]);
        }
    }

    #[test]
    fn normalizes_and_reports_bad_keys() {
        let (normalized, problems) =
            normalize_attributes(&options(&[(" storage ", &[" 128 GB "])]));
        assert!(problems.is_empty());
        assert_eq!(normalized.get("storage"), Some(&vec!["128 GB".to_string()]));

        let (_, problems) = normalize_attributes(&options(&[("  ", &["128 GB"])]));
        assert_eq!(problems[0].kind, Kind::EmptyOptionName);
    }
}
