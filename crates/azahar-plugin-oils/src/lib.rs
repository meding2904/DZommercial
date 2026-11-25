#![forbid(unsafe_code)]

//! Skincare product type: a single formula with its size, concerns, materials
//! and instructions. Port of `backend/internal/plugins/oils`.

use azahar_plugins::{Category, Manifest, OptionField, Plugin as PluginApi, ProductType};

const PLUGIN_ID: &str = "oils";

#[derive(Debug, Default, Clone, Copy)]
pub struct Plugin;

#[must_use]
pub fn new() -> Plugin {
    Plugin
}

impl PluginApi for Plugin {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PLUGIN_ID.to_string(),
            name: "Oil shelf".to_string(),
            version: "1.0.0".to_string(),
            author: "Azahar".to_string(),
            description:
                "Sells a single formula with its size, concerns, materials and instructions."
                    .to_string(),
            admin_path: "/admin/plugins/oils".to_string(),
            categories: vec![
                Category {
                    slug: "face".to_string(),
                    name: "Face".to_string(),
                    description: "Oils, serums and masks for the face.".to_string(),
                    sort_order: 1,
                },
                Category {
                    slug: "body".to_string(),
                    name: "Body".to_string(),
                    description: "Balms, oils and creams for the body.".to_string(),
                    sort_order: 2,
                },
                Category {
                    slug: "ritual".to_string(),
                    name: "Ritual".to_string(),
                    description: "Soaks for the bath.".to_string(),
                    sort_order: 3,
                },
            ],
            product_type: Some(ProductType {
                label: "Oil".to_string(),
                summary: "A formula with its size, concerns, materials and instructions."
                    .to_string(),
                fields: vec![
                    OptionField {
                        key: "size".to_string(),
                        label: "Size".to_string(),
                        field_type: "text".to_string(),
                        required: true,
                        help: "The volume or weight shown on the product page, for example 100 ml."
                            .to_string(),
                        suggestions: Vec::new(),
                    },
                    OptionField {
                        key: "concerns".to_string(),
                        label: "Concerns".to_string(),
                        field_type: "list".to_string(),
                        required: false,
                        help:
                            "The skin concerns this formula answers. The shop filter reads these."
                                .to_string(),
                        suggestions: vec![
                            "dry".to_string(),
                            "sensitive".to_string(),
                            "dull".to_string(),
                            "oily".to_string(),
                            "tired".to_string(),
                        ],
                    },
                    OptionField {
                        key: "ingredients".to_string(),
                        label: "Ingredients".to_string(),
                        field_type: "list".to_string(),
                        required: false,
                        help: "The materials that carry the formula.".to_string(),
                        suggestions: Vec::new(),
                    },
                    OptionField {
                        key: "how_to".to_string(),
                        label: "How to use".to_string(),
                        field_type: "textarea".to_string(),
                        required: false,
                        help: "One short paragraph: how much, where and when.".to_string(),
                        suggestions: Vec::new(),
                    },
                ],
            }),
            ..Manifest::default()
        }
    }
}
