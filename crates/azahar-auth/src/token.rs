//! Bearer tokens and their stored hashes.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

/// Number of random bytes in a bearer token.
pub const TOKEN_BYTES: usize = 32;

/// Failure to generate a token.
#[derive(Debug, thiserror::Error)]
#[error("generate token: {0}")]
pub struct TokenError(#[source] getrandom::Error);

/// Returns 32 random bytes encoded as unpadded base64url.
///
/// # Errors
///
/// Returns an error if the operating system random number generator fails,
/// mirroring `auth.NewToken`.
pub fn new_token() -> Result<String, TokenError> {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(TokenError)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Returns the lowercase hex SHA-256 of a bearer token.
///
/// Only this value is stored in the database.
#[must_use]
pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_token_is_unique_base64url_of_32_bytes() {
        let first = new_token().expect("first token");
        let second = new_token().expect("second token");
        assert_ne!(first, second);

        let raw = URL_SAFE_NO_PAD.decode(&first).expect("base64url");
        assert_eq!(raw.len(), TOKEN_BYTES);
        assert_eq!(first.len(), 43);
        assert!(
            first
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );
    }

    #[test]
    fn hash_token_is_lowercase_hex_sha256() {
        let hash = hash_token("token-value");
        assert_eq!(hash.len(), 64);
        assert!(
            hash.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        assert_eq!(hash_token("token-value"), hash);
        assert_ne!(hash_token("token-other"), hash);
    }

    #[test]
    fn hash_token_matches_known_sha256_vector() {
        assert_eq!(
            hash_token(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hash_token("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    proptest::proptest! {
        #[test]
        fn tokens_decode_to_32_bytes(_seed in 0u8..16) {
            let token = new_token().expect("token");
            let raw = URL_SAFE_NO_PAD.decode(&token).expect("base64url");
            proptest::prop_assert_eq!(raw.len(), TOKEN_BYTES);
        }

        #[test]
        fn hash_token_is_always_64_lowercase_hex(token in ".{0,128}") {
            let hash = hash_token(&token);
            proptest::prop_assert_eq!(hash.len(), 64);
            proptest::prop_assert!(
                hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            );
        }
    }
}
