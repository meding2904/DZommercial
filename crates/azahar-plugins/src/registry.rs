//! Plugin registry, manifest validation and dependency bookkeeping.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock};

use azahar_store::Product;
use azahar_store::products::{CORE_PLUGIN_ID, normalize_plugin_id};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Manifest, Migration, Plugin, ProductType};

static ID_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9_-]{1,31}$").expect("static plugin id pattern"));
static KEY_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9_]{1,31}$").expect("static setting key pattern"));

/// A validation failure whose text is part of the admin API contract.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct RegistryError(String);

impl RegistryError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// One migration paired with the plugin that contributed it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RegisteredMigration {
    #[serde(rename = "PluginID", alias = "plugin_id")]
    pub plugin_id: String,
    #[serde(rename = "Migration")]
    pub migration: Migration,
}

pub struct Registry {
    order: Vec<Arc<dyn Plugin>>,
    by_id: HashMap<String, Arc<dyn Plugin>>,
    enabled: HashMap<String, bool>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            order: Vec::new(),
            by_id: HashMap::new(),
            enabled: HashMap::new(),
        }
    }

    /// Validates a manifest and its routes, then appends the plugin.
    ///
    /// Checks run in the same order as the Go registry so the first reported
    /// problem is identical. Route handlers are not part of this port, so the
    /// Go "has no handler" rule has no equivalent here.
    pub fn register(&mut self, plugin: Arc<dyn Plugin>) -> Result<(), RegistryError> {
        let manifest = plugin.manifest();
        if !ID_PATTERN.is_match(&manifest.id) {
            return Err(RegistryError::new(format!(
                "register plugin {}: id must be 2 to 32 lowercase letters, numbers, dashes or underscores",
                quote(&manifest.id)
            )));
        }
        if self.by_id.contains_key(&manifest.id) {
            return Err(RegistryError::new(format!(
                "register plugin {}: already registered",
                quote(&manifest.id)
            )));
        }
        if manifest.name.trim().is_empty() || manifest.name.chars().count() > 80 {
            return Err(RegistryError::new(format!(
                "register plugin {}: name must be 1 to 80 characters",
                quote(&manifest.id)
            )));
        }
        if manifest.version.trim().is_empty() || manifest.version.len() > 32 {
            return Err(RegistryError::new(format!(
                "register plugin {}: version must be 1 to 32 characters",
                quote(&manifest.id)
            )));
        }
        if !manifest.admin_path.is_empty() && !manifest.admin_path.starts_with("/admin/") {
            return Err(RegistryError::new(format!(
                "register plugin {}: admin_path must start with /admin/",
                quote(&manifest.id)
            )));
        }

        let mut seen_dependencies = HashSet::with_capacity(manifest.dependencies.len());
        for dependency in &manifest.dependencies {
            let dependency = dependency.trim();
            if !ID_PATTERN.is_match(dependency) {
                return Err(RegistryError::new(format!(
                    "register plugin {}: dependency {} must be a plugin id",
                    quote(&manifest.id),
                    quote(dependency)
                )));
            }
            if dependency == manifest.id {
                return Err(RegistryError::new(format!(
                    "register plugin {}: a plugin cannot depend on itself",
                    quote(&manifest.id)
                )));
            }
            if !seen_dependencies.insert(dependency.to_string()) {
                return Err(RegistryError::new(format!(
                    "register plugin {}: dependency {} is repeated",
                    quote(&manifest.id),
                    quote(dependency)
                )));
            }
        }

        if let Some(product_type) = &manifest.product_type {
            validate_product_type(product_type).map_err(|message| {
                RegistryError::new(format!(
                    "register plugin {}: {message}",
                    quote(&manifest.id)
                ))
            })?;
        }
        validate_categories(&manifest.categories).map_err(|message| {
            RegistryError::new(format!(
                "register plugin {}: {message}",
                quote(&manifest.id)
            ))
        })?;
        validate_storefront_pages(&manifest.storefront_pages).map_err(|message| {
            RegistryError::new(format!(
                "register plugin {}: {message}",
                quote(&manifest.id)
            ))
        })?;

        for existing in &self.order {
            let existing_manifest = existing.manifest();
            for category in &existing_manifest.categories {
                for candidate in &manifest.categories {
                    if candidate.slug == category.slug {
                        return Err(RegistryError::new(format!(
                            "register plugin {}: category {} is already owned by {}",
                            quote(&manifest.id),
                            quote(&candidate.slug),
                            quote(&existing_manifest.id)
                        )));
                    }
                }
            }
            for page in &existing_manifest.storefront_pages {
                for candidate in &manifest.storefront_pages {
                    if candidate.path == page.path {
                        return Err(RegistryError::new(format!(
                            "register plugin {}: storefront page {} is already owned by {}",
                            quote(&manifest.id),
                            quote(&candidate.path),
                            quote(&existing_manifest.id)
                        )));
                    }
                }
            }
        }

        validate_settings(&manifest.settings).map_err(|message| {
            RegistryError::new(format!(
                "register plugin {}: {message}",
                quote(&manifest.id)
            ))
        })?;

        let mut seen_routes = HashSet::new();
        for route in plugin.routes() {
            let method = route.method.trim().to_uppercase();
            if !valid_method(&method) {
                return Err(RegistryError::new(format!(
                    "register plugin {}: route {} {} has an unsupported method",
                    quote(&manifest.id),
                    route.method,
                    quote(&route.pattern)
                )));
            }
            let pattern = route.pattern.trim();
            if !pattern.starts_with('/')
                || pattern.contains("..")
                || pattern.contains("//")
                || pattern.contains([' ', '\t', '\r', '\n'])
            {
                return Err(RegistryError::new(format!(
                    "register plugin {}: route {} must be a clean absolute path",
                    quote(&manifest.id),
                    quote(&route.pattern)
                )));
            }
            if pattern.starts_with("/admin") {
                return Err(RegistryError::new(format!(
                    "register plugin {}: route {} must not include the /admin prefix; use Admin",
                    quote(&manifest.id),
                    quote(pattern)
                )));
            }
            let key = format!("{method} {pattern}");
            if !seen_routes.insert(key.clone()) {
                return Err(RegistryError::new(format!(
                    "register plugin {}: duplicate route {key}",
                    quote(&manifest.id)
                )));
            }
        }

        self.order.push(Arc::clone(&plugin));
        self.by_id.insert(manifest.id.clone(), plugin);
        self.enabled.insert(manifest.id, true);
        Ok(())
    }

    #[must_use]
    pub fn all(&self) -> Vec<Arc<dyn Plugin>> {
        self.order.clone()
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<Arc<dyn Plugin>> {
        self.by_id.get(id).cloned()
    }

    /// Rejects unknown dependencies and cycles after every plugin is registered.
    ///
    /// Like the Go implementation this walks the id map, so when several
    /// problems exist the reported one is unspecified.
    pub fn validate_dependencies(&self) -> Result<(), RegistryError> {
        for (id, plugin) in &self.by_id {
            for dependency in plugin.manifest().dependencies {
                let dependency = normalize_plugin_id(&dependency);
                if !self.by_id.contains_key(&dependency) {
                    return Err(RegistryError::new(format!(
                        "plugin {} depends on unknown plugin {}",
                        quote(id),
                        quote(&dependency)
                    )));
                }
            }
        }

        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Visit {
            Visiting,
            Visited,
        }

        let mut state: HashMap<String, Visit> = HashMap::with_capacity(self.by_id.len());
        fn visit(
            id: &str,
            by_id: &HashMap<String, Arc<dyn Plugin>>,
            state: &mut HashMap<String, Visit>,
        ) -> Result<(), RegistryError> {
            match state.get(id) {
                Some(Visit::Visiting) => {
                    return Err(RegistryError::new(format!(
                        "plugin dependency cycle includes {}",
                        quote(id)
                    )));
                }
                Some(Visit::Visited) => return Ok(()),
                None => {}
            }
            state.insert(id.to_string(), Visit::Visiting);
            let dependencies = by_id
                .get(id)
                .map(|plugin| plugin.manifest().dependencies)
                .unwrap_or_default();
            for dependency in dependencies {
                visit(&normalize_plugin_id(&dependency), by_id, state)?;
            }
            state.insert(id.to_string(), Visit::Visited);
            Ok(())
        }

        for id in self.by_id.keys() {
            visit(id, &self.by_id, &mut state)?;
        }
        Ok(())
    }

    /// Applies persisted enable/disable flags. Plugins without a stored row
    /// stay enabled.
    pub fn set_states(&mut self, states: &HashMap<String, bool>) {
        for id in self.by_id.keys() {
            let value = states.get(id).copied().unwrap_or(true);
            self.enabled.insert(id.clone(), value);
        }
    }

    /// Changes the desired state of one registered plugin. Unknown ids are
    /// ignored and report `false`.
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> bool {
        let id = normalize_plugin_id(id);
        if !self.by_id.contains_key(&id) {
            return false;
        }
        self.enabled.insert(id, enabled);
        true
    }

    /// The admin's desired state. Core products are always enabled, and an
    /// unknown plugin is treated as enabled.
    #[must_use]
    pub fn enabled(&self, id: &str) -> bool {
        self.enabled_locked(&normalize_plugin_id(id))
    }

    fn enabled_locked(&self, id: &str) -> bool {
        if id == CORE_PLUGIN_ID {
            return true;
        }
        if !self.by_id.contains_key(id) {
            return true;
        }
        self.enabled.get(id).copied().unwrap_or(true)
    }

    /// Effective state also requires every dependency to be enabled.
    #[must_use]
    pub fn effective_enabled(&self, id: &str) -> bool {
        self.effective_enabled_locked(&normalize_plugin_id(id), &mut HashSet::new())
    }

    fn effective_enabled_locked(&self, id: &str, seen: &mut HashSet<String>) -> bool {
        if id == CORE_PLUGIN_ID {
            return true;
        }
        let Some(plugin) = self.by_id.get(id) else {
            return true;
        };
        if !seen.insert(id.to_string()) {
            return true;
        }
        if !self.enabled_locked(id) {
            return false;
        }
        for dependency in plugin.manifest().dependencies {
            if !self.effective_enabled_locked(&normalize_plugin_id(&dependency), seen) {
                return false;
            }
        }
        true
    }

    /// Direct and transitive dependents, sorted by id.
    #[must_use]
    pub fn dependents(&self, id: &str) -> Vec<String> {
        self.dependents_locked(&normalize_plugin_id(id), &mut HashSet::new())
    }

    fn dependents_locked(&self, id: &str, seen: &mut HashSet<String>) -> Vec<String> {
        let mut out = Vec::new();
        for (plugin_id, plugin) in &self.by_id {
            if plugin_id == id {
                continue;
            }
            for dependency in plugin.manifest().dependencies {
                if normalize_plugin_id(&dependency) != id {
                    continue;
                }
                if !seen.insert(plugin_id.clone()) {
                    // Go would recurse forever here; a cycle is rejected by
                    // validate_dependencies, so stop instead of overflowing.
                    break;
                }
                out.push(plugin_id.clone());
                out.extend(self.dependents_locked(plugin_id, seen));
                break;
            }
        }
        if out.len() > 1 {
            out.sort();
        }
        out
    }

    /// Dependents that are themselves enabled, the warning set for disabling a
    /// plugin.
    #[must_use]
    pub fn enabled_dependents(&self, id: &str) -> Vec<String> {
        self.dependents(&normalize_plugin_id(id))
            .into_iter()
            .filter(|dependent| self.enabled_locked(dependent))
            .collect()
    }

    /// Disabled dependencies, direct or transitive, that block a plugin.
    #[must_use]
    pub fn blocked_by(&self, id: &str) -> Vec<String> {
        self.blocked_by_locked(&normalize_plugin_id(id), &mut HashSet::new())
    }

    fn blocked_by_locked(&self, id: &str, seen: &mut HashSet<String>) -> Vec<String> {
        let Some(plugin) = self.by_id.get(id) else {
            return Vec::new();
        };
        if !seen.insert(id.to_string()) {
            return Vec::new();
        }
        let mut out = Vec::new();
        for dependency in plugin.manifest().dependencies {
            let dependency = normalize_plugin_id(&dependency);
            if !self.enabled_locked(&dependency) {
                out.push(dependency);
                continue;
            }
            out.extend(self.blocked_by_locked(&dependency, seen));
        }
        if out.len() > 1 {
            out.sort();
        }
        out
    }

    #[must_use]
    pub fn manifests(&self) -> Vec<Manifest> {
        self.order.iter().map(|plugin| plugin.manifest()).collect()
    }

    /// Migrations in registration order, paired with their plugin id.
    #[must_use]
    pub fn migrations(&self) -> Vec<RegisteredMigration> {
        let mut out = Vec::new();
        for plugin in &self.order {
            let plugin_id = plugin.manifest().id;
            for migration in plugin.migrations() {
                out.push(RegisteredMigration {
                    plugin_id: plugin_id.clone(),
                    migration,
                });
            }
        }
        out
    }

    /// Looks up a product type by exact id; the Go method does not normalise.
    #[must_use]
    pub fn product_type(&self, id: &str) -> Option<ProductType> {
        self.by_id
            .get(id)
            .and_then(|plugin| plugin.manifest().product_type)
    }

    /// Computed product fields, or `None` when the owning plugin has no
    /// enricher or the enricher fails.
    #[must_use]
    pub fn enrich_product(&self, product: &Product) -> Option<HashMap<String, Value>> {
        let plugin = self.by_id.get(&product.plugin_id)?;
        plugin.as_product_enricher()?.enrich_product(product).ok()
    }
}

fn validate_product_type(product_type: &ProductType) -> Result<(), String> {
    if product_type.label.trim().is_empty() || product_type.label.chars().count() > 60 {
        return Err("product type label must be 1 to 60 characters".to_string());
    }
    let mut seen = HashSet::new();
    for field in &product_type.fields {
        if !KEY_PATTERN.is_match(&field.key) {
            return Err(format!(
                "option field {} must be lowercase letters, numbers and underscores",
                quote(&field.key)
            ));
        }
        if !seen.insert(field.key.clone()) {
            return Err(format!("option field {} is repeated", quote(&field.key)));
        }
        if field.label.trim().is_empty() || field.label.chars().count() > 60 {
            return Err(format!(
                "option field {} needs a label of 1 to 60 characters",
                quote(&field.key)
            ));
        }
        if !matches!(
            field.field_type.as_str(),
            "select" | "text" | "textarea" | "list"
        ) {
            return Err(format!(
                "option field {} has an unsupported type {}",
                quote(&field.key),
                quote(&field.field_type)
            ));
        }
        if field.field_type == "select" && field.suggestions.len() > 12 {
            return Err(format!(
                "option field {} has more than 12 suggestions",
                quote(&field.key)
            ));
        }
    }
    Ok(())
}

fn validate_categories(categories: &[crate::Category]) -> Result<(), String> {
    let mut seen = HashSet::with_capacity(categories.len());
    for category in categories {
        if !ID_PATTERN.is_match(&category.slug) {
            return Err(format!(
                "category {} must be lowercase letters, numbers, dashes or underscores",
                quote(&category.slug)
            ));
        }
        if !seen.insert(category.slug.clone()) {
            return Err(format!("category {} is repeated", quote(&category.slug)));
        }
        if category.name.trim().is_empty() || category.name.chars().count() > 80 {
            return Err(format!(
                "category {} needs a name of 1 to 80 characters",
                quote(&category.slug)
            ));
        }
        if category.description.chars().count() > 300 {
            return Err(format!(
                "category {} has a description over 300 characters",
                quote(&category.slug)
            ));
        }
    }
    Ok(())
}

fn validate_storefront_pages(pages: &[crate::StorefrontPage]) -> Result<(), String> {
    let reserved = [
        "/",
        "/shop",
        "/about",
        "/contact",
        "/cart",
        "/checkout",
        "/account",
        "/admin",
        "/order",
        "/product",
    ];
    let mut seen = HashSet::with_capacity(pages.len());
    for page in pages {
        if !page.path.starts_with('/')
            || page.path.starts_with("/admin")
            || page.path.contains("..")
            || page.path.contains("//")
            || page
                .path
                .contains([' ', '\t', '\r', '\n', '\\', '%', '?', '#'])
            || page.path.len() > 200
        {
            return Err(format!(
                "storefront page path {} must be a clean public path",
                quote(&page.path)
            ));
        }
        if reserved.contains(&page.path.as_str()) {
            return Err(format!(
                "storefront page path {} is reserved by the core",
                quote(&page.path)
            ));
        }
        if !seen.insert(page.path.clone()) {
            return Err(format!(
                "storefront page path {} is repeated",
                quote(&page.path)
            ));
        }
        if page.label.trim().is_empty() || page.label.chars().count() > 60 {
            return Err(format!(
                "storefront page {} needs a label of 1 to 60 characters",
                quote(&page.path)
            ));
        }
    }
    Ok(())
}

fn validate_settings(settings: &[crate::SettingField]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for field in settings {
        if !KEY_PATTERN.is_match(&field.key) {
            return Err(format!(
                "setting {} must be lowercase letters, numbers and underscores",
                quote(&field.key)
            ));
        }
        if !seen.insert(field.key.clone()) {
            return Err(format!("setting {} is repeated", quote(&field.key)));
        }
        if field.label.trim().is_empty() || field.label.chars().count() > 80 {
            return Err(format!(
                "setting {} needs a label of 1 to 80 characters",
                quote(&field.key)
            ));
        }
        match field.field_type.as_str() {
            "text" | "secret" | "number" | "boolean" => {}
            "select" => {
                if field.options.is_empty() {
                    return Err(format!(
                        "setting {} needs at least one option",
                        quote(&field.key)
                    ));
                }
            }
            other => {
                return Err(format!(
                    "setting {} has an unsupported type {}",
                    quote(&field.key),
                    quote(other)
                ));
            }
        }
        if field.max_length < 0 || field.max_length > 4096 {
            return Err(format!(
                "setting {} has an invalid max length",
                quote(&field.key)
            ));
        }
    }
    Ok(())
}

fn valid_method(method: &str) -> bool {
    matches!(method, "GET" | "POST" | "PUT" | "PATCH" | "DELETE")
}

/// Go's `%q` for a UTF-8 string, limited to the escaping the registry uses.
fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{07}' => out.push_str("\\a"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0b}' => out.push_str("\\v"),
            '\u{2028}' | '\u{2029}' => {
                out.push_str(&format!("\\u{:04x}", character as u32));
            }
            character if character.is_whitespace() && character != ' ' => {
                out.push_str(&format!("\\u{:04x}", character as u32));
            }
            character if character.is_control() => {
                let code = character as u32;
                if code < 0x20 || code == 0x7f {
                    out.push_str(&format!("\\x{code:02x}"));
                } else if code < 0x1_0000 {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    out.push_str(&format!("\\U{code:08x}"));
                }
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}
