//! Order lifecycle statuses and the transitions allowed between them.
//!
//! Port of `backend/internal/orderstatus`. The order of [`STATUSES`] is the
//! lifecycle order used by the API and the codemode planner.

/// Every status an order can hold, in lifecycle order.
pub const STATUSES: [&str; 6] = [
    "pending",
    "confirmed",
    "packed",
    "shipped",
    "delivered",
    "cancelled",
];

/// Maps a status to the statuses it may move to next. Terminal statuses carry
/// an empty list.
const ALLOWED: [(&str, &[&str]); 6] = [
    ("pending", &["confirmed", "cancelled"]),
    ("confirmed", &["packed", "cancelled"]),
    ("packed", &["shipped", "cancelled"]),
    ("shipped", &["delivered"]),
    ("delivered", &[]),
    ("cancelled", &[]),
];

/// Reports whether `status` is one of [`STATUSES`].
#[must_use]
pub fn known(status: &str) -> bool {
    STATUSES.contains(&status)
}

/// Reports whether an order may move from one status to another.
#[must_use]
pub fn can_transition(from: &str, to: &str) -> bool {
    ALLOWED
        .iter()
        .find(|(status, _)| *status == from)
        .is_some_and(|(_, targets)| targets.contains(&to))
}

#[cfg(test)]
mod tests {
    use super::{STATUSES, can_transition, known};

    #[test]
    fn terminal_statuses_have_no_transitions() {
        for from in ["delivered", "cancelled"] {
            for to in STATUSES {
                assert!(!can_transition(from, to));
            }
        }
    }

    #[test]
    fn unknown_statuses_are_rejected() {
        assert!(!known(""));
        assert!(!known("Pending"));
        assert!(!can_transition("unknown", "pending"));
        assert!(!can_transition("pending", "unknown"));
    }

    #[test]
    fn allowed_transitions() {
        for (from, to) in [
            ("pending", "confirmed"),
            ("pending", "cancelled"),
            ("confirmed", "packed"),
            ("confirmed", "cancelled"),
            ("packed", "shipped"),
            ("packed", "cancelled"),
            ("shipped", "delivered"),
        ] {
            assert!(can_transition(from, to), "{from} -> {to}");
        }
        assert!(!can_transition("pending", "shipped"));
    }
}
