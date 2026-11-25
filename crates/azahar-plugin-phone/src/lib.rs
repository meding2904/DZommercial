//! Phone catalog plugin: manifest, IMEI storage, device operations and order
//! hooks. Port of `backend/internal/plugins/phone`.
//!
//! The HTTP device endpoints are mounted by `azahar-httpapi`; this crate keeps
//! the domain layer, the same SQL and the same `Problem` refusals so the
//! handlers only translate JSON and status codes.

#![forbid(unsafe_code)]

mod devices;
mod hooks;
mod phone;
mod variant;

pub use devices::{
    CreateDevicePayload, DEVICE_SELECT, Device, DeviceCounts, DeviceListResponse,
    OrderDevicesResponse, scan_device,
};
pub use hooks::{PhoneHooks, VariantCount, product_enrichment};
pub use phone::{PLUGIN_ID, PhoneService, Plugin, new, reconcile_stock, repair_legacy_attributes};
pub use variant::{OPTION_KEYS, valid_imei, variant_key, variant_label};
