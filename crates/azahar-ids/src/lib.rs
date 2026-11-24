//! Prefixed random identifiers.
//!
//! Mirrors the Go package `azahar/internal/ids`.

#![forbid(unsafe_code)]

/// Number of random bytes in an identifier (24 hex characters).
const RANDOM_BYTES: usize = 12;

/// Returns a prefixed random identifier, for example `prd_9f2c...`.
///
/// The random part is 12 bytes from the operating system CSPRNG, encoded as
/// lowercase hexadecimal.
///
/// # Panics
///
/// Panics if the system random number generator is unavailable. The Go
/// original also treats `crypto/rand` failure as fatal, so this is the one
/// documented panic in the crate.
#[must_use]
pub fn new(prefix: &str) -> String {
    let mut bytes = [0u8; RANDOM_BYTES];
    getrandom::fill(&mut bytes).expect("crypto/rand unavailable");

    let mut id = String::with_capacity(prefix.len() + 1 + RANDOM_BYTES * 2);
    id.push_str(prefix);
    id.push('_');
    id.push_str(&hex::encode(bytes));
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_is_prefix_underscore_and_24_lowercase_hex() {
        let id = new("prd");
        assert_eq!(id.len(), 4 + 24);
        assert!(id.starts_with("prd_"));

        let random = &id["prd_".len()..];
        assert_eq!(random.len(), 24);
        assert!(
            random
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
    }

    #[test]
    fn identifiers_are_unique() {
        let first = new("ses");
        let second = new("ses");
        assert_ne!(first, second);
    }

    #[test]
    fn empty_and_odd_prefixes_are_kept_verbatim() {
        let empty = new("");
        assert!(empty.starts_with('_'));
        assert_eq!(empty.len(), 25);

        let odd = new("a_b$c");
        assert!(odd.starts_with("a_b$c_"));
        assert_eq!(odd.len(), 5 + 1 + 24);
    }

    proptest::proptest! {
        #[test]
        fn generated_ids_match_the_go_shape(prefix in ".{0,64}") {
            let id = new(&prefix);
            let expected_prefix = format!("{prefix}_");
            proptest::prop_assert!(id.starts_with(&expected_prefix));

            let random = &id[expected_prefix.len()..];
            proptest::prop_assert_eq!(random.len(), 24);
            proptest::prop_assert!(
                random.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            );
        }
    }
}
