//! Delivery map and parcel client for the Azahar store.
//!
//! `Client` answers the checkout and shipment flows from the dzship HTTP API
//! and falls back to the bundled wilaya/commune snapshot when the courier
//! cannot. The local sandbox (desks, shipments, tracking) is only available
//! when the configured courier key is `sandbox` and `allow_local_sandbox` is
//! set, so a real courier never receives a fake parcel.

mod cache;
mod client;
mod dzship;
mod options;
mod snapshot;
mod types;

pub use client::{Client, Courier};
pub use options::{Clock, FallbackCallback, Options};
pub use types::{
    APIError, Commune, Desk, Error, OrderRequest, Recipient, Shipment, Tracking, TrackingEvent,
    TransportError, Wilaya,
};
