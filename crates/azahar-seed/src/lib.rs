//! Demo seed for a fresh Azahar database, ported from `backend/internal/seed`.
//!
//! [`run`] applies the same four stages in the same order: the oils catalogue,
//! the plugin demo catalogue, the demo accounts, then the demo orders. Each
//! stage reads its table first and returns zero when it already holds rows, so
//! a second run is a no-op that returns an empty summary.
//!
//! The order stage seeds Go's `math/rand` with the fixed value `20261111`, so
//! the plan is deterministic but the drawn values depend on Go's generator and
//! shuffle. This port keeps the same plan, ranges and branch logic with a
//! seeded [`rand::rngs::StdRng`]; concrete draws differ. Tests assert
//! invariants rather than byte equality.
//!
//! Logging moves from `*slog.Logger` to the `tracing` facade because Rust has
//! no equivalent of passing a handler through every call. The event name and
//! fields are unchanged: `demo seed applied` with categories, products, users
//! and orders.

#![forbid(unsafe_code)]

mod catalog;
mod orders;
mod plugins;
mod users;

use azahar_store::{Store, StoreError};

/// Demo account credentials. Emails are used verbatim; the Go config layer
/// lowercases and trims them before it calls the seed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub admin_email: String,
    pub admin_password: String,
    pub customer_email: String,
    pub customer_password: String,
    pub employee_email: String,
    pub employee_password: String,
}

/// Rows created by one [`run`]. A second run against the same database
/// returns all zeroes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    pub categories: usize,
    pub products: usize,
    pub users: usize,
    pub orders: usize,
}

/// A stage failure with the same context prefix as the Go wrapper.
#[derive(Debug, thiserror::Error)]
pub enum SeedError {
    #[error("seed catalog: {0}")]
    Catalog(String),
    #[error("seed plugin catalog: {0}")]
    PluginCatalog(String),
    #[error("seed users: {0}")]
    Users(String),
    #[error("seed orders: {0}")]
    Orders(String),
}

pub(crate) type StageResult<T> = Result<T, StageError>;

/// An internal failure before [`run`] adds its stage prefix.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(crate) struct StageError(String);

impl StageError {
    pub(crate) fn message(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl From<StoreError> for StageError {
    fn from(value: StoreError) -> Self {
        Self(value.to_string())
    }
}

impl From<libsql::Error> for StageError {
    fn from(value: libsql::Error) -> Self {
        Self(value.to_string())
    }
}

impl From<azahar_auth::HashPasswordError> for StageError {
    fn from(value: azahar_auth::HashPasswordError) -> Self {
        Self(value.to_string())
    }
}

impl From<azahar_auth::TokenError> for StageError {
    fn from(value: azahar_auth::TokenError) -> Self {
        Self(value.to_string())
    }
}

/// Seed the demo catalogue, accounts and orders.
///
/// Safe to call on every start: each stage checks its own tables first, so an
/// existing catalogue, user list or order list is left untouched.
///
/// The demo layout is fixed: 8 oils products, one phone and eight PC parts,
/// 15 accounts and 48 orders. Orders go through `Store::create_order`, which
/// (like the Go seed) writes no audit rows; order intake records those.
///
/// # Errors
///
/// Returns [`SeedError`] with the failing stage prefix. Partial stages are
/// rolled back; earlier stages stay committed, exactly like the Go version.
pub async fn run(store: &Store, opts: &Options) -> Result<Summary, SeedError> {
    let mut summary = Summary::default();

    let catalog = catalog::seed_catalog(store)
        .await
        .map_err(|err| SeedError::Catalog(err.to_string()))?;
    summary.categories = catalog.categories;
    summary.products = catalog.products;

    let plugin_products = plugins::seed_plugin_catalog(store)
        .await
        .map_err(|err| SeedError::PluginCatalog(err.to_string()))?;
    summary.products += plugin_products;

    summary.users = users::seed_users(store, opts)
        .await
        .map_err(|err| SeedError::Users(err.to_string()))?;

    summary.orders = orders::seed_orders(store)
        .await
        .map_err(|err| SeedError::Orders(err.to_string()))?;

    if summary.categories + summary.products + summary.users + summary.orders > 0 {
        tracing::info!(
            categories = summary.categories,
            products = summary.products,
            users = summary.users,
            orders = summary.orders,
            "demo seed applied"
        );
    }

    Ok(summary)
}
