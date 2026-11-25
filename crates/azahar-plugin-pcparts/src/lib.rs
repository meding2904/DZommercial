#![forbid(unsafe_code)]

//! PC parts product type and the shelves a machine is assembled from. Port of
//! `backend/internal/plugins/pcparts`.

use azahar_plugins::{Category, Manifest, OptionField, Plugin as PluginApi, ProductType};

/// Used by the PC builder to read only these products.
pub const PLUGIN_ID: &str = "pcparts";

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
            name: "PC parts".to_string(),
            version: "1.0.0".to_string(),
            author: "Azahar".to_string(),
            description:
                "Sells the parts a desktop PC is built from, with the specs the builder checks."
                    .to_string(),
            categories: vec![
                category(
                    "cpus",
                    "CPU",
                    "Processors with their socket and power draw.",
                    1,
                ),
                category(
                    "gpus",
                    "Graphics cards",
                    "Graphics cards with their length and power draw.",
                    2,
                ),
                category(
                    "motherboards",
                    "Motherboards",
                    "Boards with their socket, memory type and form factor.",
                    3,
                ),
                category(
                    "memory",
                    "Memory",
                    "Memory kits matched to the board's memory type.",
                    4,
                ),
                category(
                    "storage",
                    "Storage",
                    "Drives with their interface and capacity.",
                    5,
                ),
                category(
                    "psus",
                    "Power supplies",
                    "Power supplies with their rated wattage.",
                    6,
                ),
                category(
                    "cases",
                    "Cases",
                    "Cases with their board support and graphics card clearance.",
                    7,
                ),
                category(
                    "cooling",
                    "Cooling",
                    "Air and liquid coolers with the sockets they fit.",
                    8,
                ),
            ],
            product_type: Some(ProductType {
                label: "PC part".to_string(),
                summary: "A part described by the specs the builder needs to compare it."
                    .to_string(),
                fields: vec![
                    field(
                        "socket",
                        "Socket",
                        "The CPU socket, for example AM5 or LGA1700. A cooler lists every socket it fits.",
                    ),
                    field(
                        "memory_type",
                        "Memory type",
                        "The memory generation, for example DDR4 or DDR5.",
                    ),
                    field(
                        "form_factor",
                        "Form factor",
                        "The board size, for example ATX, micro-ATX or mini-ITX.",
                    ),
                    field(
                        "interface",
                        "Interface",
                        "The connection a drive uses, for example SATA or NVMe.",
                    ),
                    field(
                        "tdp_watts",
                        "TDP (watts)",
                        "The heat a CPU or graphics card is rated to produce.",
                    ),
                    field(
                        "wattage_watts",
                        "Wattage (watts)",
                        "The rated output of a power supply.",
                    ),
                    field("length_mm", "Length (mm)", "The length of a graphics card."),
                    field(
                        "max_gpu_length_mm",
                        "Maximum GPU length (mm)",
                        "The longest graphics card the case accepts.",
                    ),
                    field(
                        "supported_form_factor",
                        "Supported board sizes",
                        "Comma-separated board sizes a case accepts, for example ATX, micro-ATX, mini-ITX.",
                    ),
                ],
            }),
            ..Manifest::default()
        }
    }
}

fn category(slug: &str, name: &str, description: &str, sort_order: i32) -> Category {
    Category {
        slug: slug.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        sort_order,
    }
}

fn field(key: &str, label: &str, help: &str) -> OptionField {
    OptionField {
        key: key.to_string(),
        label: label.to_string(),
        field_type: "text".to_string(),
        required: false,
        help: help.to_string(),
        suggestions: Vec::new(),
    }
}
