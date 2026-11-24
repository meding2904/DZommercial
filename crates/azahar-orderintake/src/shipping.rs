//! Storefront and counter delivery rules.

/// Flat delivery fee: 600 DZD.
pub const STANDARD_SHIPPING_CENTS: i64 = 60_000;
/// Subtotal above which delivery is free: 8 000 DZD. A subtotal exactly on
/// the line still pays the fee.
pub const FREE_SHIPPING_OVER_CENTS: i64 = 800_000;

/// A flat 600 DZD, free over 8 000 DZD.
pub fn shipping_cents(subtotal_cents: i64) -> i64 {
    if subtotal_cents > FREE_SHIPPING_OVER_CENTS {
        return 0;
    }
    STANDARD_SHIPPING_CENTS
}

/// The counter sale rule: the customer carries the goods away.
pub fn no_shipping(_subtotal_cents: i64) -> i64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_pays_the_fee() {
        assert_eq!(shipping_cents(0), 60_000);
        assert_eq!(shipping_cents(800_000), 60_000);
        assert_eq!(shipping_cents(800_001), 0);
        assert_eq!(no_shipping(800_001), 0);
    }
}
