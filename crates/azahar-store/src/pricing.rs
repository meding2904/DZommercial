//! Product sale window rules, ported from `backend/internal/store/pricing.go`.

use chrono::{DateTime, Utc};

use crate::types::{Product, Timestamp};

pub const SALE_PRICE_INVALID: u8 = 1;
pub const SALE_PRICE_MISSING: u8 = 2;
pub const SALE_START_MISSING: u8 = 4;
pub const SALE_END_MISSING: u8 = 8;
pub const SALE_END_NOT_AFTER_START: u8 = 16;

pub type SaleProblem = u8;

/// A product's optional sale price and the interval during which it applies.
/// A missing start or end means that endpoint is not set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaleWindow {
    pub price_cents: Option<i64>,
    pub starts_at: Timestamp,
    pub ends_at: Timestamp,
}

impl Product {
    pub fn sale_window(&self) -> SaleWindow {
        SaleWindow {
            price_cents: self.sale_price_cents,
            starts_at: self.sale_starts_at,
            ends_at: self.sale_ends_at,
        }
    }

    /// The price a customer pays at the given instant.
    pub fn effective_price_cents(&self, now: DateTime<Utc>) -> i64 {
        if self.sale_active(now) {
            self.sale_price_cents.unwrap_or(self.price_cents)
        } else {
            self.price_cents
        }
    }

    /// Whether the product has a valid limited-time price at the instant.
    pub fn sale_active(&self, now: DateTime<Utc>) -> bool {
        self.sale_window().active(now, self.price_cents)
    }
}

impl SaleWindow {
    /// Whether a sale price is set above zero. Comparison with the regular
    /// price is part of `price_valid`.
    pub fn price_positive(&self) -> bool {
        matches!(self.price_cents, Some(price) if price > 0)
    }

    /// Whether the sale price is above zero and below the regular price.
    pub fn price_valid(&self, regular_price_cents: i64) -> bool {
        self.price_positive() && self.price_cents.unwrap_or_default() < regular_price_cents
    }

    /// Whether both endpoints are set and the window ends after it starts. The
    /// start is inclusive and the end is exclusive.
    pub fn ends_after_starts(&self) -> bool {
        match (self.starts_at, self.ends_at) {
            (Some(start), Some(end)) => end > start,
            _ => false,
        }
    }

    /// Every sale rule the window breaks for a product priced at
    /// `regular_price_cents`. A window without a sale price only reports
    /// `SALE_PRICE_MISSING` when an endpoint is set.
    pub fn problems(&self, regular_price_cents: i64) -> Vec<SaleProblem> {
        if self.price_cents.is_none() {
            if self.starts_at.is_some() || self.ends_at.is_some() {
                return vec![SALE_PRICE_MISSING];
            }
            return Vec::new();
        }

        let mut problems = Vec::with_capacity(4);
        if !self.price_valid(regular_price_cents) {
            problems.push(SALE_PRICE_INVALID);
        }
        if self.starts_at.is_none() {
            problems.push(SALE_START_MISSING);
        }
        if self.ends_at.is_none() {
            problems.push(SALE_END_MISSING);
        } else if self.starts_at.is_some() && !self.ends_after_starts() {
            problems.push(SALE_END_NOT_AFTER_START);
        }
        problems
    }

    /// Whether the whole sale window is valid data. Expired and future windows
    /// are valid; use `active` to tell whether the sale is running.
    pub fn valid(&self, regular_price_cents: i64) -> bool {
        self.problems(regular_price_cents).is_empty()
    }

    /// Whether the product has a valid limited-time price at the given
    /// instant. The start is inclusive and the end is exclusive.
    pub fn active(&self, now: DateTime<Utc>, regular_price_cents: i64) -> bool {
        if !self.valid(regular_price_cents) {
            return false;
        }
        let (Some(start), Some(end)) = (self.starts_at, self.ends_at) else {
            return false;
        };
        now >= start && now < end
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(year: i32, month: u32, day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, 10, 0, 0).unwrap()
    }

    #[test]
    fn effective_price_uses_active_sale_window() {
        let start = at(2026, 11, 20);
        let end = at(2026, 11, 27);
        let product = Product {
            price_cents: 120_000,
            sale_price_cents: Some(90_000),
            sale_starts_at: Some(start),
            sale_ends_at: Some(end),
            ..Default::default()
        };

        assert_eq!(
            product.effective_price_cents(start - chrono::Duration::seconds(1)),
            120_000
        );
        assert_eq!(product.effective_price_cents(start), 90_000);
        assert_eq!(
            product.effective_price_cents(end - chrono::Duration::seconds(1)),
            90_000
        );
        assert_eq!(product.effective_price_cents(end), 120_000);
    }

    #[test]
    fn effective_price_rejects_invalid_sale() {
        let start = at(2026, 11, 20);
        let end = start + chrono::Duration::hours(24);
        let cases = [
            Product {
                price_cents: 10_000,
                ..Default::default()
            },
            Product {
                price_cents: 10_000,
                sale_price_cents: Some(0),
                sale_starts_at: Some(start),
                sale_ends_at: Some(end),
                ..Default::default()
            },
            Product {
                price_cents: 10_000,
                sale_price_cents: Some(10_000),
                sale_starts_at: Some(start),
                sale_ends_at: Some(end),
                ..Default::default()
            },
            Product {
                price_cents: 10_000,
                sale_price_cents: Some(9_000),
                ..Default::default()
            },
            Product {
                price_cents: 10_000,
                sale_price_cents: Some(9_000),
                sale_starts_at: Some(start),
                ..Default::default()
            },
        ];
        for (index, product) in cases.iter().enumerate() {
            assert_eq!(
                product.effective_price_cents(start + chrono::Duration::hours(1)),
                product.price_cents,
                "case {index}"
            );
        }
    }

    #[test]
    fn sale_window_problems_match_the_go_table() {
        let start = at(2026, 11, 20);
        let end = start + chrono::Duration::hours(24);
        const REGULAR: i64 = 10_000;
        let cases: Vec<(&str, SaleWindow, Vec<SaleProblem>)> = vec![
            ("no sale", SaleWindow::default(), vec![]),
            (
                "valid",
                SaleWindow {
                    price_cents: Some(9_000),
                    starts_at: Some(start),
                    ends_at: Some(end),
                },
                vec![],
            ),
            (
                "missing price with start",
                SaleWindow {
                    starts_at: Some(start),
                    ..Default::default()
                },
                vec![SALE_PRICE_MISSING],
            ),
            (
                "zero price",
                SaleWindow {
                    price_cents: Some(0),
                    starts_at: Some(start),
                    ends_at: Some(end),
                },
                vec![SALE_PRICE_INVALID],
            ),
            (
                "price equal to regular",
                SaleWindow {
                    price_cents: Some(REGULAR),
                    starts_at: Some(start),
                    ends_at: Some(end),
                },
                vec![SALE_PRICE_INVALID],
            ),
            (
                "missing start",
                SaleWindow {
                    price_cents: Some(9_000),
                    ends_at: Some(end),
                    ..Default::default()
                },
                vec![SALE_START_MISSING],
            ),
            (
                "missing end",
                SaleWindow {
                    price_cents: Some(9_000),
                    starts_at: Some(start),
                    ..Default::default()
                },
                vec![SALE_END_MISSING],
            ),
            (
                "missing window",
                SaleWindow {
                    price_cents: Some(9_000),
                    ..Default::default()
                },
                vec![SALE_START_MISSING, SALE_END_MISSING],
            ),
            (
                "reversed window",
                SaleWindow {
                    price_cents: Some(9_000),
                    starts_at: Some(end),
                    ends_at: Some(start),
                },
                vec![SALE_END_NOT_AFTER_START],
            ),
            (
                "equal endpoints",
                SaleWindow {
                    price_cents: Some(9_000),
                    starts_at: Some(start),
                    ends_at: Some(start),
                },
                vec![SALE_END_NOT_AFTER_START],
            ),
        ];
        for (name, window, want) in cases {
            assert_eq!(window.problems(REGULAR), want, "{name}");
            assert_eq!(window.valid(REGULAR), want.is_empty(), "{name}");
        }
    }

    #[test]
    fn active_boundaries_are_start_inclusive_end_exclusive() {
        let start = at(2026, 11, 20);
        let end = start + chrono::Duration::hours(24);
        let window = SaleWindow {
            price_cents: Some(9_000),
            starts_at: Some(start),
            ends_at: Some(end),
        };
        assert!(!window.active(start - chrono::Duration::nanoseconds(1), 10_000));
        assert!(window.active(start, 10_000));
        assert!(window.active(start + chrono::Duration::hours(1), 10_000));
        assert!(window.active(end - chrono::Duration::nanoseconds(1), 10_000));
        assert!(!window.active(end, 10_000));

        let future = SaleWindow {
            price_cents: Some(9_000),
            starts_at: Some(end),
            ends_at: Some(end + chrono::Duration::hours(1)),
        };
        assert!(future.valid(10_000));
        assert!(!future.active(start, 10_000));
    }
}
