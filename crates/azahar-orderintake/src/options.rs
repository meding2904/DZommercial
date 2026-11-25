//! Option schemas a product's plugin declares and the choices a basket line
//! sends, ported from `backend/internal/orderintake/options.go`.

use std::collections::{HashMap, HashSet};

use azahar_plugins::{OptionField, Registry};
use azahar_store::Product;
use azahar_store::products::CORE_PLUGIN_ID;

/// The option fields a product's plugin exposes. Core products and plugins
/// without a product type take no options.
#[must_use]
pub fn option_schema(registry: &Registry, product: &Product) -> Vec<OptionField> {
    if product.plugin_id.is_empty() || product.plugin_id == CORE_PLUGIN_ID {
        return Vec::new();
    }
    registry
        .product_type(&product.plugin_id)
        .map(|product_type| product_type.fields)
        .unwrap_or_default()
}

/// Checks the choices a customer sends with a line. The result is the `items`
/// field message the caller renders, empty when every choice is valid.
#[must_use]
pub fn selected_option_fields(
    schema: &[OptionField],
    attributes: &HashMap<String, Vec<String>>,
    options: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    let select_fields: Vec<&OptionField> = schema
        .iter()
        .filter(|field| field.field_type == "select")
        .collect();
    if select_fields.is_empty() {
        if !options.is_empty() {
            fields.insert(
                "items".to_string(),
                "This product does not take options.".to_string(),
            );
        }
        return fields;
    }

    let known: HashSet<&str> = select_fields
        .iter()
        .map(|field| field.key.as_str())
        .collect();
    for field in &select_fields {
        let value = options
            .get(&field.key)
            .map(|value| value.trim())
            .unwrap_or("");
        if value.is_empty() {
            if field.required {
                fields.insert(
                    "items".to_string(),
                    format!("Choose a {} for every product.", field.label.to_lowercase()),
                );
            }
            continue;
        }
        let allowed = attributes
            .get(&field.key)
            .is_some_and(|candidates| candidates.iter().any(|candidate| candidate == value));
        if !allowed {
            fields.insert(
                "items".to_string(),
                format!("Choose an available {}.", field.label.to_lowercase()),
            );
        }
    }
    for key in options.keys() {
        if !known.contains(key.as_str()) {
            fields.insert(
                "items".to_string(),
                "This product does not take that option.".to_string(),
            );
            break;
        }
    }
    fields
}

/// The stored description of a line's choices: `Label: value` for every
/// answered select field, joined by a middle dot.
#[must_use]
pub fn option_label(schema: &[OptionField], options: &HashMap<String, String>) -> String {
    let parts: Vec<String> = schema
        .iter()
        .filter(|field| field.field_type == "select")
        .filter_map(|field| {
            let value = options
                .get(&field.key)
                .map(|value| value.trim())
                .unwrap_or("");
            (!value.is_empty()).then(|| format!("{}: {value}", field.label))
        })
        .collect();
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use azahar_plugins::{ProductType, Registry};
    use azahar_store::Product;

    fn field(key: &str, label: &str, field_type: &str, required: bool) -> OptionField {
        OptionField {
            key: key.to_string(),
            label: label.to_string(),
            field_type: field_type.to_string(),
            required,
            ..OptionField::default()
        }
    }

    fn schema() -> Vec<OptionField> {
        vec![
            field("scent", "Scent", "select", true),
            field("finish", "Finish", "select", false),
            field("engraving", "Engraving", "text", false),
        ]
    }

    fn attributes() -> HashMap<String, Vec<String>> {
        HashMap::from([
            (
                "scent".to_string(),
                vec!["Rose".to_string(), "Oud".to_string()],
            ),
            (
                "finish".to_string(),
                vec!["Matte".to_string(), "Gloss".to_string()],
            ),
        ])
    }

    fn options(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    fn message(fields: &HashMap<String, String>) -> Option<&str> {
        fields.get("items").map(String::as_str)
    }

    #[test]
    fn core_products_and_unknown_plugins_have_no_schema() {
        let registry = Registry::new();
        let core = Product {
            plugin_id: "core".to_string(),
            ..Product::default()
        };
        let empty = Product::default();
        let unknown = Product {
            plugin_id: "missing".to_string(),
            ..Product::default()
        };
        assert!(option_schema(&registry, &core).is_empty());
        assert!(option_schema(&registry, &empty).is_empty());
        assert!(option_schema(&registry, &unknown).is_empty());
    }

    #[test]
    fn schema_returns_the_plugin_fields() {
        struct Fixture(OptionField);
        impl azahar_plugins::Plugin for Fixture {
            fn manifest(&self) -> azahar_plugins::Manifest {
                azahar_plugins::Manifest {
                    id: "fixture".to_string(),
                    name: "Fixture".to_string(),
                    version: "1.0.0".to_string(),
                    product_type: Some(ProductType {
                        label: "Fixture".to_string(),
                        fields: vec![self.0.clone()],
                        ..ProductType::default()
                    }),
                    ..azahar_plugins::Manifest::default()
                }
            }
        }

        let mut registry = Registry::new();
        registry
            .register(std::sync::Arc::new(Fixture(field(
                "scent", "Scent", "select", true,
            ))))
            .expect("register fixture");
        let product = Product {
            plugin_id: "fixture".to_string(),
            ..Product::default()
        };
        assert_eq!(
            option_schema(&registry, &product),
            vec![schema()[0].clone()]
        );
    }

    #[test]
    fn select_fields_are_required_and_restricted() {
        let schema = schema();
        let attributes = attributes();

        let missing = selected_option_fields(&schema, &attributes, &options(&[]));
        assert_eq!(message(&missing), Some("Choose a scent for every product."));

        let unavailable =
            selected_option_fields(&schema, &attributes, &options(&[("scent", "Lavender")]));
        assert_eq!(message(&unavailable), Some("Choose an available scent."));

        let unknown = selected_option_fields(
            &schema,
            &attributes,
            &options(&[("scent", "Rose"), ("size", "50 ml")]),
        );
        assert_eq!(
            message(&unknown),
            Some("This product does not take that option.")
        );

        let valid = selected_option_fields(
            &schema,
            &attributes,
            &options(&[("scent", " Rose "), ("finish", "Matte")]),
        );
        assert!(valid.is_empty());

        let optional_missing =
            selected_option_fields(&schema, &attributes, &options(&[("scent", "Oud")]));
        assert!(optional_missing.is_empty());

        let last_problem_wins =
            selected_option_fields(&schema, &attributes, &options(&[("finish", "Satin")]));
        assert_eq!(
            message(&last_problem_wins),
            Some("Choose an available finish.")
        );
    }

    #[test]
    fn products_without_select_fields_refuse_any_option() {
        let no_select = vec![field("engraving", "Engraving", "text", false)];
        let fields = selected_option_fields(
            &no_select,
            &attributes(),
            &options(&[("engraving", "A name")]),
        );
        assert_eq!(
            message(&fields),
            Some("This product does not take options.")
        );
        assert!(selected_option_fields(&no_select, &attributes(), &options(&[])).is_empty());
    }

    #[test]
    fn label_joins_answered_select_fields_in_schema_order() {
        let schema = schema();
        assert_eq!(option_label(&schema, &options(&[])), "");
        assert_eq!(
            option_label(
                &schema,
                &options(&[
                    ("finish", " Matte "),
                    ("scent", "Rose"),
                    ("engraving", "ignored"),
                ]),
            ),
            "Scent: Rose · Finish: Matte"
        );
        assert_eq!(
            option_label(&schema, &options(&[("finish", "Gloss")])),
            "Finish: Gloss"
        );
    }
}
