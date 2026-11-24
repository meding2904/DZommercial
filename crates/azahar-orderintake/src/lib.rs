//! Port of `backend/internal/orderintake`: basket lines, option schemas,
//! selling channels, shipping rules and the order placement/status path.

#![forbid(unsafe_code)]

pub mod channel;
pub mod hooks;
pub mod intake;
pub mod lines;
pub mod options;
pub mod problems;
pub mod shipping;

pub use channel::{Audit, AuditDetail, Channel, ShippingRule, checkout_channel, counter_channel};
pub use hooks::{
    CheckoutHook, CheckoutLine, HookError, HookProvider, NoopHooks, OrderRef, OrderStatusHook,
};
pub use intake::{
    Change, Changed, Customer, Destination, Intake, IntakeError, PlaceResult, Request,
};
pub use lines::{Line, build_lines};
pub use options::{option_label, option_schema, selected_option_fields};
pub use problems::Problem;
pub use shipping::{no_shipping, shipping_cents};
