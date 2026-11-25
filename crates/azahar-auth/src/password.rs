//! Argon2id password hashing, compatible with the Go implementation.

use std::sync::{LazyLock, OnceLock};

use argon2::{Algorithm, Argon2, Params, Version, password_hash::PasswordHasher};
use base64::{
    Engine, alphabet,
    engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig},
};
use subtle::ConstantTimeEq;

/// Memory cost in KiB.
const ARGON2_MEMORY: u32 = 64 * 1024;
/// Number of passes.
const ARGON2_TIME: u32 = 3;
/// Degree of parallelism.
const ARGON2_THREADS: u32 = 4;
/// Derived key length in bytes.
const ARGON2_KEY_LEN: usize = 32;
/// Salt length in bytes.
const ARGON2_SALT_LEN: usize = 16;

/// Largest memory cost accepted by [`verify_password`], in KiB.
const MAX_MEMORY: u32 = 1024 * 1024;
/// Largest iteration count accepted by [`verify_password`].
const MAX_ITERATIONS: u32 = 20;
/// Largest parallelism accepted by [`verify_password`].
const MAX_THREADS: u32 = 64;

const DUMMY_PASSWORD: &str = "azahar-dummy-password-for-timing";

/// The Go implementation's exported sentinel for malformed hashes.
///
/// `VerifyPassword` reports malformed input as `false`, so nothing in this
/// crate actually returns it; it exists for callers that match on the sentinel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid password hash")]
pub struct InvalidHash;

/// Failure to hash a password.
#[derive(Debug, thiserror::Error)]
pub enum HashPasswordError {
    /// The operating system CSPRNG could not produce a salt.
    #[error("generate salt: {0}")]
    Salt(#[source] getrandom::Error),
    /// Argon2 rejected the password or parameters.
    #[error("hash password: {0}")]
    Hash(#[source] argon2::password_hash::Error),
}

/// Hashes a password with Argon2id v19 and a fresh 16-byte salt.
///
/// The result is a PHC string of the form
/// `$argon2id$v=19$m=65536,t=3,p=4$<salt>$<key>` with raw standard base64
/// (no padding) for the salt and key.
///
/// # Errors
///
/// Returns an error only if the OS random number generator fails or Argon2
/// rejects the input, mirroring `auth.HashPassword`'s error return.
pub fn hash_password(password: &str) -> Result<String, HashPasswordError> {
    let mut salt = [0u8; ARGON2_SALT_LEN];
    getrandom::fill(&mut salt).map_err(HashPasswordError::Salt)?;

    let params = Params::new(
        ARGON2_MEMORY,
        ARGON2_TIME,
        ARGON2_THREADS,
        Some(ARGON2_KEY_LEN),
    )
    .map_err(|err| HashPasswordError::Hash(err.into()))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let hash = argon2
        .hash_password_with_salt(password.as_bytes(), &salt)
        .map_err(HashPasswordError::Hash)?;
    Ok(hash.to_string())
}

/// Verifies a password against a PHC hash.
///
/// This deliberately reimplements the Go parser rather than using
/// `PasswordVerifier`, because `VerifyPassword` has to preserve its quirks:
/// exactly six `$`-separated fields, the `argon2id` algorithm, version `19`,
/// any parameter order, unknown keys rejected, `p` truncated to a `u8`, the
/// same numeric limits, and raw standard base64 that tolerates CR/LF and
/// non-canonical trailing bits.
///
/// Go's `argon2.IDKey` clamps a memory cost below `8 * p` up to that minimum
/// (while hashing the original cost into the initial block). The Rust argon2
/// crate rejects such parameters instead, so this function returns `false`.
/// The difference is only observable for a hand-built hash that matches Go's
/// clamped derivation; `HashPassword` only emits valid parameters.
#[must_use]
pub fn verify_password(password: &str, encoded: &str) -> bool {
    let parts: Vec<&str> = encoded.split('$').collect();
    if parts.len() != 6 || parts[1] != "argon2id" {
        return false;
    }

    let version_field = parts[2].strip_prefix("v=").unwrap_or(parts[2]);
    let Some(version) = go_atoi(version_field) else {
        return false;
    };
    if version != 19 {
        return false;
    }

    let Some((memory, iterations, threads)) = parse_argon2_params(parts[3]) else {
        return false;
    };
    if memory == 0 || iterations == 0 || threads == 0 {
        return false;
    }
    if memory > MAX_MEMORY || iterations > MAX_ITERATIONS || u32::from(threads) > MAX_THREADS {
        return false;
    }

    let Some(salt) = decode_go_raw_std(parts[4]) else {
        return false;
    };
    if salt.len() < 8 {
        return false;
    }

    let Some(want) = decode_go_raw_std(parts[5]) else {
        return false;
    };
    if want.len() < 16 || want.len() > 128 {
        return false;
    }

    // Params::new rejects `m < 8 * p`; Go's IDKey clamps up to the minimum.
    let Ok(params) = Params::new(memory, iterations, u32::from(threads), Some(want.len())) else {
        return false;
    };
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut got = vec![0u8; want.len()];
    if argon2
        .hash_password_into(password.as_bytes(), &salt, &mut got)
        .is_err()
    {
        return false;
    }

    bool::from(got.ct_eq(&want))
}

/// Returns a real Argon2id hash of a fixed password, cached after first use.
///
/// Login uses this to spend the same work for unknown email addresses. If the
/// RNG fails, Go's `DummyHash` returns the empty string; this does the same.
#[must_use]
pub fn dummy_hash() -> String {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY
        .get_or_init(|| hash_password(DUMMY_PASSWORD).unwrap_or_default())
        .clone()
}

/// Runs the same work factor as a real password check and ignores the result.
pub fn verify_dummy(password: &str) {
    let _ = verify_password(password, &dummy_hash());
}

/// Splits `name=value` fields exactly like Go's `strings.Split` + `strings.Cut`.
fn parse_argon2_params(field: &str) -> Option<(u32, u32, u8)> {
    let mut memory = 0u32;
    let mut iterations = 0u32;
    let mut threads = 0u8;

    for pair in field.split(',') {
        let (key, value) = pair.split_once('=')?;
        let number = go_parse_uint32(value)?;
        match key {
            "m" => memory = number,
            "t" => iterations = number,
            // Go assigns `uint8(n)`, so values above 255 wrap.
            "p" => threads = number as u8,
            _ => return None,
        }
    }

    Some((memory, iterations, threads))
}

/// Parses a decimal integer the way Go's `strconv.Atoi` does.
///
/// Accepts an optional `+`/`-` sign and ASCII digits only. Overflow is
/// rejected; since callers compare against a small constant, that is
/// indistinguishable from Go's range error.
fn go_atoi(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    let (negative, digits) = match bytes.split_first() {
        Some((&b'+', rest)) => (false, rest),
        Some((&b'-', rest)) => (true, rest),
        Some(_) => (false, bytes),
        None => return None,
    };
    if digits.is_empty() {
        return None;
    }

    let mut number: i64 = 0;
    for &byte in digits {
        if !byte.is_ascii_digit() {
            return None;
        }
        number = number.checked_mul(10)?;
        number = number.checked_add(i64::from(byte - b'0'))?;
    }

    if negative {
        number.checked_neg()
    } else {
        Some(number)
    }
}

/// Parses an unsigned 32-bit decimal integer the way Go's
/// `strconv.ParseUint(value, 10, 32)` does: no sign, ASCII digits only.
fn go_parse_uint32(value: &str) -> Option<u32> {
    if value.is_empty() {
        return None;
    }

    let mut number: u32 = 0;
    for byte in value.bytes() {
        if !byte.is_ascii_digit() {
            return None;
        }
        number = number.checked_mul(10)?;
        number = number.checked_add(u32::from(byte - b'0'))?;
    }

    Some(number)
}

/// Go's `base64.RawStdEncoding.DecodeString`.
///
/// Go ignores `\r` and `\n` anywhere in the input, rejects padding, and does
/// not require the final character's unused bits to be zero. The `base64`
/// crate defaults differ on the newline and trailing-bit rules.
fn decode_go_raw_std(value: &str) -> Option<Vec<u8>> {
    static ENGINE: LazyLock<GeneralPurpose> = LazyLock::new(|| {
        GeneralPurpose::new(
            &alphabet::STANDARD,
            GeneralPurposeConfig::new()
                .with_decode_padding_mode(DecodePaddingMode::RequireNone)
                .with_decode_allow_trailing_bits(true),
        )
    });

    let filtered: String = value
        .chars()
        .filter(|character| *character != '\r' && *character != '\n')
        .collect();
    ENGINE.decode(filtered).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use argon2::password_hash::phc::PasswordHash;
    use base64::engine::general_purpose::STANDARD_NO_PAD;

    const PREFIX: &str = "$argon2id$v=19$m=65536,t=3,p=4$";

    fn split_hash(hash: &str) -> Vec<&str> {
        hash.split('$').collect()
    }

    #[test]
    fn hash_and_verify_password() {
        let hash = hash_password("a long enough password").expect("hash");
        assert!(hash.starts_with(PREFIX), "unexpected PHC prefix: {hash}");
        assert!(verify_password("a long enough password", &hash));
        assert!(!verify_password("another password", &hash));
        assert!(!verify_password("a long enough password", "not-a-hash"));
    }

    #[test]
    fn hash_has_expected_salt_and_key_lengths() {
        let hash = hash_password("lengths").expect("hash");
        let parts = split_hash(&hash);
        assert_eq!(parts.len(), 6);
        assert_eq!(parts[1], "argon2id");
        assert_eq!(parts[2], "v=19");
        assert_eq!(parts[3], "m=65536,t=3,p=4");

        let salt = STANDARD_NO_PAD.decode(parts[4]).expect("salt base64");
        let key = STANDARD_NO_PAD.decode(parts[5]).expect("key base64");
        assert_eq!(salt.len(), ARGON2_SALT_LEN);
        assert_eq!(key.len(), ARGON2_KEY_LEN);
    }

    #[test]
    fn hash_is_salted() {
        let first = hash_password("same password twice").expect("first");
        let second = hash_password("same password twice").expect("second");
        assert_ne!(first, second);
    }

    #[test]
    fn verify_rejects_raised_parameters_outside_limits() {
        let hash = "$argon2id$v=19$m=999999999,t=3,p=4$c2FsdHNhbHRzYWx0$aGFzaGhhc2hoYXNoaGFzaA";
        assert!(!verify_password("password", hash));
    }

    #[test]
    fn verify_accepts_go_numeric_quirks() {
        let hash = hash_password("quirks").expect("hash");
        let parts = split_hash(&hash);

        // Atoi accepts a sign and leading zeroes as long as the value is 19.
        for version in ["19", "v=19", "v=019", "v=+19"] {
            let candidate = format!(
                "${}${}${}${}${}",
                parts[1], version, parts[3], parts[4], parts[5]
            );
            assert!(verify_password("quirks", &candidate), "{version}");
        }
    }

    #[test]
    fn verify_accepts_parameter_reordering_and_duplicates() {
        let hash = hash_password("ordering").expect("hash");
        let parts = split_hash(&hash);

        let reordered = format!(
            "${}${}${}${}${}",
            parts[1], parts[2], "p=4,m=65536,t=3", parts[4], parts[5]
        );
        assert!(verify_password("ordering", &reordered));

        let duplicated = format!(
            "${}${}${}${}${}",
            parts[1], parts[2], "m=1,t=1,p=1,m=65536,t=3,p=4", parts[4], parts[5]
        );
        assert!(verify_password("ordering", &duplicated));
    }

    #[test]
    fn verify_truncates_parallelism_to_a_byte() {
        let hash = hash_password("threads").expect("hash");
        let parts = split_hash(&hash);

        // Go stores `p` in a uint8: 260 wraps to 4, but 321 wraps to 65.
        let wrapped = format!(
            "${}${}${}${}${}",
            parts[1], parts[2], "m=65536,t=3,p=260", parts[4], parts[5]
        );
        assert!(verify_password("threads", &wrapped));

        let too_large = format!(
            "${}${}${}${}${}",
            parts[1], parts[2], "m=65536,t=3,p=321", parts[4], parts[5]
        );
        assert!(!verify_password("threads", &too_large));
    }

    #[test]
    fn verify_rejects_unknown_and_malformed_parameters() {
        let hash = hash_password("params").expect("hash");
        let parts = split_hash(&hash);
        let build = |params: &str| {
            format!(
                "${}${}${}${}${}",
                parts[1], parts[2], params, parts[4], parts[5]
            )
        };

        for params in [
            "m=65536,t=3,p=4,x=1",
            "m=65536,t=3",
            "m=65536,t=3,p=4,",
            "m=+65536,t=3,p=4",
            "m=65536,t=3,p=4=5",
            "m=4294967296,t=3,p=4",
            "m=0,t=3,p=4",
            "m=65536,t=0,p=4",
            "m=65536,t=3,p=0",
            "m=65536,t=21,p=4",
            "m=65536,t=3,p=65",
            "M=65536,t=3,p=4",
            "",
        ] {
            assert!(!verify_password("params", &build(params)), "{params}");
        }
    }

    #[test]
    fn verify_requires_exactly_six_fields_but_not_a_leading_empty_one() {
        let hash = hash_password("fields").expect("hash");
        let parts = split_hash(&hash);
        let build = |extra: &str| {
            format!(
                "{extra}${}${}${}${}${}",
                parts[1], parts[2], parts[3], parts[4], parts[5]
            )
        };

        assert!(verify_password("fields", &build("")));
        assert!(verify_password("fields", &build("ignored")));
        assert!(!verify_password("fields", &format!("{}x", build(""))));
        assert!(!verify_password("fields", &build("ignored$again")));
    }

    #[test]
    fn verify_accepts_go_base64_newlines_and_trailing_bits() {
        let hash = hash_password("base64").expect("hash");
        let parts = split_hash(&hash);
        let build = |salt: &str| {
            format!(
                "${}${}${}${}${}",
                parts[1], parts[2], parts[3], salt, parts[5]
            )
        };

        let with_newlines = format!("{}\n\r", parts[4]);
        assert!(verify_password("base64", &build(&with_newlines)));

        // A 16-byte salt encodes as 22 characters. The final character has
        // four unused low bits; flipping them leaves the decoded bytes alone.
        let salt = parts[4];
        let (head, last) = salt.split_at(salt.len() - 1);
        let last = last.as_bytes()[0];
        let index = alphabet::STANDARD
            .as_str()
            .find(char::from(last))
            .expect("alphabet");
        let mutated = alphabet::STANDARD.as_str().as_bytes()[index | 1];
        assert_ne!(last, mutated);
        let noncanonical = format!("{head}{}", char::from(mutated));
        assert!(verify_password("base64", &build(&noncanonical)));
    }

    #[test]
    fn verify_rejects_bad_base64_and_bad_lengths() {
        let hash = hash_password("base64-bad").expect("hash");
        let parts = split_hash(&hash);
        let build = |salt: &str, key: &str| {
            format!("${}${}${}${}${}", parts[1], parts[2], parts[3], salt, key)
        };

        for salt in ["", "c2FsdA", "c2FsdHNhbHQ=", "c2FsdHNhbH RzYWx0", "YQ"] {
            assert!(
                !verify_password("base64-bad", &build(salt, parts[5])),
                "{salt}"
            );
        }
        for key in ["", "aGFzaA", "aGFzaGhhc2hoYXNoaGFzaA="] {
            assert!(
                !verify_password("base64-bad", &build(parts[4], key)),
                "{key}"
            );
        }
    }

    #[test]
    fn verify_rejects_memory_below_the_argon2_minimum() {
        // Go's argon2.IDKey clamps m < 8*p; the Rust argon2 crate rejects it.
        let hash = "$argon2id$v=19$m=1,t=1,p=4$c2FsdHNhbHRzYWx0$c2FsdHNhbHRzYWx0c2FsdA";
        assert!(!verify_password("password", hash));
    }

    #[test]
    fn dummy_hash_is_a_real_hash_of_the_fixed_password() {
        let hash = dummy_hash();
        assert!(hash.starts_with(PREFIX));
        assert!(verify_password(DUMMY_PASSWORD, &hash));
        assert!(!verify_password("not the dummy password", &hash));
        assert_eq!(hash, dummy_hash());
    }

    #[test]
    fn verify_dummy_ignores_the_result() {
        verify_dummy(DUMMY_PASSWORD);
        verify_dummy("wrong");
    }

    #[test]
    fn hash_parses_as_a_phc_password_hash() {
        let hash = hash_password("phc").expect("hash");
        let parsed = PasswordHash::new(&hash).expect("PHC string");
        assert_eq!(parsed.algorithm.as_str(), "argon2id");
        assert_eq!(parsed.version, Some(19));
    }

    proptest::proptest! {
        #[test]
        fn verify_never_panics_on_arbitrary_hashes(
            password in ".{0,64}",
            encoded in ".{0,256}",
        ) {
            let _ = verify_password(&password, &encoded);
        }

        #[test]
        fn verify_never_panics_on_hash_shaped_input(
            password in ".{0,32}",
            version in "[v=0-9+\\-]{0,8}",
            params in "[a-z=0-9,+\\-]{0,48}",
            salt in "[A-Za-z0-9+/\\r\\n=]{0,32}",
            key in "[A-Za-z0-9+/\\r\\n=]{0,140}",
        ) {
            let encoded = format!("$argon2id${version}${params}${salt}${key}");
            let _ = verify_password(&password, &encoded);
        }
    }
}
