//! Port of `backend/internal/store`: SQL access, models and pricing rules.
//!
//! `Store` holds one libsql `Connection` for its whole lifetime. A shared
//! connection means reads on that connection can observe an open write
//! transaction (its uncommitted rows) while it is running, whereas the Go
//! `database/sql` pool hands reads a different connection and sees the last
//! committed state. Remote Turso uses a separate stream per connection in any
//! case, so the difference is confined to the local database. Write paths that
//! must not read their own uncommitted rows take a `Connection` argument so the
//! caller decides which connection to use.

pub mod audit;
pub mod audit_flush;
pub mod images;
pub mod metrics;
pub mod orders;
pub mod plugin_states;
pub mod pricing;
pub mod products;
pub mod sessions;
pub mod settings;
pub mod store;
pub mod types;
pub mod users;

pub use products::{CORE_PLUGIN_ID, ProductFilter, normalize_plugin_id};
pub use store::{Store, StoreError};
pub use types::*;
