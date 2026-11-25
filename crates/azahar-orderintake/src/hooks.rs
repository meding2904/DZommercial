//! Checkout and order-status hooks that plugins implement.
//!
//! The Go registry discovers these interfaces on the plugin object and hands
//! each hook the caller's `*sql.Tx`. The Rust plugin crate has no database
//! dependency, so the intake crate owns the traits and the HTTP layer supplies
//! a [`HookProvider`]. Every hook receives the write transaction's connection:
//! it may read and write through it, but must not commit or roll back.
//!
//! Phone and AI implement these traits when their crates are ported. Until
//! then [`NoopHooks`] lets the API boot with the core path only.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use azahar_plugins::Problem;
use azahar_store::{Product, StoreError};

/// One basket line offered to a checkout hook, with the identifiers the order
/// row already carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutLine {
    pub product: Product,
    pub options: HashMap<String, String>,
    pub quantity: i64,
    pub order_id: String,
    pub item_id: String,
}

/// The order a checkout hook is told about after the row and its items exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderRef {
    pub id: String,
    pub number: String,
    pub status: String,
}

/// A hook failure the intake reports as an internal error.
#[derive(Debug, thiserror::Error)]
pub enum HookError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("{0}")]
    Message(String),
}

impl HookError {
    #[must_use]
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for HookError {
    fn from(value: Box<dyn std::error::Error + Send + Sync>) -> Self {
        Self::Message(value.to_string())
    }
}

/// The two hooks a plugin can offer on the order path.
#[async_trait]
pub trait CheckoutHook: Send + Sync {
    /// Refuses a line the plugin cannot sell, before the stock and price
    /// checks. `None` accepts the line.
    async fn validate_checkout_line(
        &self,
        conn: &libsql::Connection,
        line: &CheckoutLine,
    ) -> Option<Problem>;

    /// Runs after the order and its items are inserted, inside the same
    /// transaction.
    async fn on_order_created(
        &self,
        conn: &libsql::Connection,
        order: &OrderRef,
        lines: &[CheckoutLine],
    ) -> Result<(), HookError>;
}

/// A plugin hook notified about every lifecycle move, including a counter
/// sale's creation-time delivery.
#[async_trait]
pub trait OrderStatusHook: Send + Sync {
    async fn on_order_status_changed(
        &self,
        conn: &libsql::Connection,
        order_id: &str,
        from: &str,
        to: &str,
    ) -> Result<(), HookError>;
}

/// Resolves the hooks that belong to a plugin.
pub trait HookProvider: Send + Sync {
    fn checkout_hook(&self, plugin_id: &str) -> Option<Arc<dyn CheckoutHook>>;
    fn status_hooks(&self) -> Vec<Arc<dyn OrderStatusHook>>;
}

/// A provider with no hooks, for a build without the phone and AI plugins.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopHooks;

impl HookProvider for NoopHooks {
    fn checkout_hook(&self, _plugin_id: &str) -> Option<Arc<dyn CheckoutHook>> {
        None
    }

    fn status_hooks(&self) -> Vec<Arc<dyn OrderStatusHook>> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_hooks_resolve_nothing() {
        let hooks = NoopHooks;
        assert!(hooks.checkout_hook("phone").is_none());
        assert!(hooks.status_hooks().is_empty());
    }

    #[test]
    fn hook_errors_keep_the_underlying_message() {
        let error = HookError::message("phone hook failed");
        assert_eq!(error.to_string(), "phone hook failed");

        let store = HookError::from(StoreError::NotFound);
        assert_eq!(store.to_string(), "no rows in result set");
    }
}
