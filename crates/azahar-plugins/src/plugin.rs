//! Plugin traits and the shared error envelope.

use std::collections::HashMap;

use azahar_store::Product;
use serde_json::Value;

use crate::{Manifest, Migration, Route};

/// Everything the registry needs from a plugin.
///
/// `routes` and `migrations` have defaults so a data-only plugin only writes
/// `manifest`. Route handlers are not part of this trait: the HTTP layer owns a
/// per-plugin dispatch table keyed by the `(method, pattern, admin)`
/// registration.
pub trait Plugin: Send + Sync {
    fn manifest(&self) -> Manifest;

    fn routes(&self) -> Vec<Route> {
        Vec::new()
    }

    fn migrations(&self) -> Vec<Migration> {
        Vec::new()
    }

    /// Hooks used by [`crate::Registry::enrich_product`]. A plugin that wants
    /// enrichment overrides this with `Some(self)`.
    fn as_product_enricher(&self) -> Option<&dyn ProductEnricher> {
        None
    }

    /// Hooks used by the HTTP layer before saving settings. A plugin that
    /// validates its own settings overrides this with `Some(self)`.
    fn as_settings_validator(&self) -> Option<&dyn SettingsValidator> {
        None
    }
}

/// Optional computed fields merged into a product DTO.
///
/// The Go hook is synchronous and the port keeps that shape. Async enrichment
/// is deferred with the phone/AI plugin port.
pub trait ProductEnricher: Send + Sync {
    fn enrich_product(
        &self,
        product: &Product,
    ) -> Result<HashMap<String, Value>, Box<dyn std::error::Error + Send + Sync>>;
}

/// Optional semantic checks on top of the schema checks in [`crate::merge_settings`].
pub trait SettingsValidator: Send + Sync {
    fn validate_settings(&self, settings: &HashMap<String, String>) -> HashMap<String, String>;
}

/// A field-level failure the HTTP layer renders as the shared error envelope.
///
/// `fields` is optional so an empty or absent map is omitted from the JSON,
/// matching the Go `omitempty` tag.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Problem {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub fields: Option<HashMap<String, String>>,
}

impl Problem {
    #[must_use]
    pub fn new(
        status: u16,
        code: impl Into<String>,
        message: impl Into<String>,
        fields: Option<HashMap<String, String>>,
    ) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
            fields,
        }
    }
}
