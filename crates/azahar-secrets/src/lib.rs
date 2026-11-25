//! AES-256-GCM encryption for small settings values, ported from
//! `backend/internal/secrets`.
//!
//! The key is the SHA-256 digest of the key string. Sealed values are
//! `enc:v1:` followed by raw URL-safe base64 of the 12-byte nonce and the
//! ciphertext (including its authentication tag), matching Go's
//! `gcm.Seal(nonce, nonce, plain, nil)` layout.

use std::borrow::Cow;

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::{
    Engine, alphabet,
    engine::{
        DecodePaddingMode,
        general_purpose::{GeneralPurpose, GeneralPurposeConfig},
    },
};
use sha2::{Digest, Sha256};

/// Prefix every encrypted value starts with.
pub const PREFIX: &str = "enc:v1:";

/// AES-GCM nonce length in bytes.
pub const NONCE_LEN: usize = 12;

/// Length of the SHA-256 key in bytes.
pub const KEY_LEN: usize = 32;

/// Message used for every malformed or unauthentic value.
pub const INVALID_MESSAGE: &str = "the encrypted value is not valid";

/// Go's raw URL-safe base64 decoder ignores CR and LF and does not require the
/// unused trailing bits of the final symbol to be zero. Rust's default decoder
/// rejects both, so use a configuration that matches the oracle.
const GO_BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::URL_SAFE,
    GeneralPurposeConfig::new()
        .with_encode_padding(false)
        .with_decode_allow_trailing_bits(true)
        .with_decode_padding_mode(DecodePaddingMode::RequireNone),
);

/// Error returned when a value cannot be sealed or opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The hashed key could not initialise AES-256.
    Cipher(String),
    /// AES-GCM could not be used.
    Gcm(String),
    /// The operating system random number generator failed.
    Nonce(String),
    /// The value is not a well-formed, authenticated `enc:v1:` ciphertext.
    Invalid,
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cipher(message) => write!(formatter, "cipher: {message}"),
            Self::Gcm(message) => write!(formatter, "gcm: {message}"),
            Self::Nonce(message) => write!(formatter, "nonce: {message}"),
            Self::Invalid => formatter.write_str(INVALID_MESSAGE),
        }
    }
}

impl std::error::Error for Error {}

/// Encrypts and decrypts settings values with AES-256-GCM.
///
/// A box is cheap to create; build one per operation with [`SecretBox::new`].
pub struct SecretBox {
    key: [u8; KEY_LEN],
}

impl SecretBox {
    /// Derives the 32-byte AES key by hashing the supplied key string.
    #[must_use]
    pub fn new(key: &str) -> Self {
        let mut hashed = [0_u8; KEY_LEN];
        hashed.copy_from_slice(&Sha256::digest(key.as_bytes()));
        Self { key: hashed }
    }

    fn cipher(&self) -> Result<Aes256Gcm, Error> {
        Aes256Gcm::new_from_slice(&self.key).map_err(|error| Error::Cipher(error.to_string()))
    }

    /// Encrypts `plaintext` and returns `enc:v1:` plus raw URL-safe base64.
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<String, Error> {
        let cipher = self.cipher()?;
        let mut nonce = [0_u8; NONCE_LEN];
        getrandom::fill(&mut nonce).map_err(|error| Error::Nonce(error.to_string()))?;
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce), plaintext)
            .map_err(|error| Error::Gcm(error.to_string()))?;

        let mut sealed = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        sealed.extend_from_slice(&nonce);
        sealed.extend_from_slice(&ciphertext);
        Ok(format!("{PREFIX}{}", GO_BASE64.encode(sealed)))
    }

    /// Decrypts a value produced by [`SecretBox::encrypt`] or by the Go
    /// `secrets.Box.Encrypt`.
    ///
    /// Any wrong prefix, malformed base64, short input, wrong key or modified
    /// ciphertext yields [`Error::Invalid`], the same error the Go package
    /// returns.
    pub fn decrypt(&self, value: &str) -> Result<Vec<u8>, Error> {
        let encoded = value.strip_prefix(PREFIX).ok_or(Error::Invalid)?;
        let encoded = if encoded.contains('\r') || encoded.contains('\n') {
            Cow::Owned(encoded.replace(['\r', '\n'], ""))
        } else {
            Cow::Borrowed(encoded)
        };
        let raw = GO_BASE64
            .decode(encoded.as_ref())
            .map_err(|_| Error::Invalid)?;
        if raw.len() < NONCE_LEN {
            return Err(Error::Invalid);
        }

        let cipher = self.cipher()?;
        let (nonce, ciphertext) = raw.split_at(NONCE_LEN);
        cipher
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| Error::Invalid)
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, INVALID_MESSAGE, PREFIX, SecretBox};

    #[test]
    fn round_trip() {
        let secret_box = SecretBox::new("a long development key for tests");
        let sealed = secret_box
            .encrypt(br#"{"api_key":"sk-test"}"#)
            .expect("encrypt");
        assert!(sealed.starts_with(PREFIX));
        assert_ne!(sealed, r#"{"api_key":"sk-test"}"#);
        assert_eq!(
            secret_box.decrypt(&sealed).expect("decrypt"),
            br#"{"api_key":"sk-test"}"#
        );
    }

    #[test]
    fn wrong_key_is_rejected() {
        let sealed = SecretBox::new("key one")
            .encrypt(b"value")
            .expect("encrypt");
        assert_eq!(
            SecretBox::new("key two").decrypt(&sealed),
            Err(Error::Invalid)
        );
    }

    #[test]
    fn invalid_values_use_the_go_error() {
        let secret_box = SecretBox::new("key");
        assert_eq!(
            secret_box.decrypt(r#"{"api_key":"sk-test"}"#),
            Err(Error::Invalid)
        );
        assert_eq!(Error::Invalid.to_string(), INVALID_MESSAGE);
    }

    #[test]
    fn tampering_is_rejected() {
        let secret_box = SecretBox::new("key");
        let sealed = secret_box.encrypt(b"value").expect("encrypt");
        let mut tampered = sealed.into_bytes();
        let last = tampered.last_mut().expect("sealed value");
        *last = if *last == b'A' { b'B' } else { b'A' };
        let tampered = String::from_utf8(tampered).expect("utf-8");
        assert_eq!(secret_box.decrypt(&tampered), Err(Error::Invalid));
    }
}
