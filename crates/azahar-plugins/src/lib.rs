#![forbid(unsafe_code)]

//! Port of `backend/internal/plugins`: the plugin extension surface and its
//! registry.
//!
//! The Go package also defines the HTTP `Context`, body decoding and JSON
//! writing. Those stay with the HTTP crate in this port; a plugin contributes
//! its manifest, route registrations and migrations, and the mount layer
//! supplies the handlers.

mod manifest;
mod plugin;
mod registry;
mod settings;

pub use manifest::*;
pub use plugin::*;
pub use registry::*;
pub use settings::*;
