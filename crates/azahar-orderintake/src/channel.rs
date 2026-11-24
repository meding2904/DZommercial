//! Selling channels: what the web checkout and the counter sale change about
//! the shared order path, ported from `backend/internal/orderintake/orderintake.go`.

use azahar_store::Order;
use azahar_store::format_time;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// A delivery rule a channel applies to a subtotal.
pub type ShippingRule = fn(i64) -> i64;

/// The audit row one channel writes with an order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditDetail {
    /// Storefront checkout: the terms version and the instant of acceptance.
    TermsAccepted { version: String },
    /// Counter sale: the channel name and the sale totals.
    CounterSale,
}

impl AuditDetail {
    #[must_use]
    pub fn render(&self, order: &Order, item_count: usize, now: DateTime<Utc>) -> Value {
        match self {
            Self::TermsAccepted { version } => serde_json::json!({
                "version": version,
                "accepted_at": format_time(&now),
            }),
            Self::CounterSale => serde_json::json!({
                "channel": "counter",
                "total_cents": order.total_cents,
                "items": item_count,
            }),
        }
    }
}

/// The audit row one channel writes with an order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Audit {
    pub action: String,
    pub detail: Option<AuditDetail>,
}

/// What one way of selling changes about the shared order path.
#[derive(Debug, Clone, Default)]
pub struct Channel {
    /// The noun refusals use for the lines: `bag` for the web checkout, `sale`
    /// for the counter.
    pub basket: String,
    /// The delivery fee for a subtotal. `None` is free delivery.
    pub shipping: Option<ShippingRule>,
    /// Whether a paid sale is recorded as delivered in the same transaction.
    pub delivered: bool,
    pub audit: Audit,
}

impl Channel {
    /// Applies the channel's delivery rule.
    #[must_use]
    pub fn shipping_cents(&self, subtotal_cents: i64) -> i64 {
        match self.shipping {
            Some(rule) => rule(subtotal_cents),
            None => 0,
        }
    }

    /// The audit detail for a placed order, or an empty object when the
    /// channel records no detail.
    #[must_use]
    pub fn audit_detail(&self, order: &Order, item_count: usize, now: DateTime<Utc>) -> Value {
        match &self.audit.detail {
            Some(detail) => detail.render(order, item_count, now),
            None => Value::Object(serde_json::Map::new()),
        }
    }
}

/// The storefront checkout: the bag, the standard shipping rule, an order left
/// pending and the accepted terms recorded in the audit.
#[must_use]
pub fn checkout_channel(terms_version: &str) -> Channel {
    Channel {
        basket: "bag".to_string(),
        shipping: Some(crate::shipping::shipping_cents),
        delivered: false,
        audit: Audit {
            action: "order.terms_accepted".to_string(),
            detail: Some(AuditDetail::TermsAccepted {
                version: terms_version.to_string(),
            }),
        },
    }
}

/// The in-store sale: no delivery, delivered in the same transaction, with the
/// sale's own audit row.
#[must_use]
pub fn counter_channel() -> Channel {
    Channel {
        basket: "sale".to_string(),
        shipping: Some(crate::shipping::no_shipping),
        delivered: true,
        audit: Audit {
            action: "order.counter_sale".to_string(),
            detail: Some(AuditDetail::CounterSale),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use azahar_store::Order;

    fn order(total_cents: i64) -> Order {
        Order {
            total_cents,
            ..Order::default()
        }
    }

    #[test]
    fn checkout_uses_the_bag_rule_and_terms_audit() {
        let channel = checkout_channel("2026-11-14");
        assert_eq!(channel.basket, "bag");
        assert!(!channel.delivered);
        assert_eq!(channel.shipping_cents(800_000), 60_000);
        assert_eq!(channel.shipping_cents(800_001), 0);
        assert_eq!(channel.audit.action, "order.terms_accepted");

        let now = DateTime::parse_from_rfc3339("2026-11-24T10:00:00Z")
            .expect("timestamp")
            .with_timezone(&Utc);
        assert_eq!(
            channel.audit_detail(&order(0), 0, now),
            serde_json::json!({"version": "2026-11-14", "accepted_at": "2026-11-24T10:00:00Z"})
        );
    }

    #[test]
    fn counter_has_no_shipping_and_a_sale_audit() {
        let channel = counter_channel();
        assert_eq!(channel.basket, "sale");
        assert!(channel.delivered);
        assert_eq!(channel.shipping_cents(0), 0);
        assert_eq!(channel.audit.action, "order.counter_sale");

        let now = Utc::now();
        assert_eq!(
            channel.audit_detail(&order(246_800), 2, now),
            serde_json::json!({"channel": "counter", "total_cents": 246_800, "items": 2})
        );
    }

    #[test]
    fn a_channel_without_a_rule_or_detail_is_free_and_quiet() {
        let channel = Channel::default();
        assert_eq!(channel.shipping_cents(1_000), 0);
        assert_eq!(
            channel.audit_detail(&order(1_000), 1, Utc::now()),
            serde_json::json!({})
        );
    }
}
