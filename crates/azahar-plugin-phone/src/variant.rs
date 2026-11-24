//! Variant keys, labels and the IMEI checksum. Port of
//! `backend/internal/plugins/phone/variant.go`.

use std::collections::HashMap;

/// Option keys in the fixed order the variant key and label walk them.
pub const OPTION_KEYS: [&str; 2] = ["config", "color"];

const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Returns the display label for a known phone option key. The Go map lookup
/// returns an empty string for anything else.
pub(crate) fn option_label(key: &str) -> &str {
    match key {
        "config" => "Memory and storage",
        "color" => "Color",
        _ => "",
    }
}

/// Order-independent stock bucket key. The option keys always appear in the
/// same order, so the same handset always maps to one bucket.
#[must_use]
pub fn variant_key(options: &HashMap<String, String>) -> String {
    if options.is_empty() {
        return String::new();
    }
    let mut parts = Vec::with_capacity(OPTION_KEYS.len());
    for key in OPTION_KEYS {
        let value = options.get(key).map_or("", String::as_str).trim();
        parts.push(format!("{key}={}", query_escape(value)));
    }
    parts.join("&")
}

/// Human label for the selected options. Empty options are skipped.
#[must_use]
pub fn variant_label(options: &HashMap<String, String>) -> String {
    let mut parts = Vec::with_capacity(OPTION_KEYS.len());
    for key in OPTION_KEYS {
        let value = options.get(key).map_or("", String::as_str).trim();
        if value.is_empty() {
            continue;
        }
        parts.push(format!("{}: {value}", option_label(key)));
    }
    parts.join(" · ")
}

/// Luhn checksum over the 15 ASCII digits of an IMEI.
///
/// The byte walk mirrors Go, so a separator, a multi-byte character or a
/// string of the wrong byte length is rejected.
#[must_use]
pub fn valid_imei(imei: &str) -> bool {
    let bytes = imei.as_bytes();
    if bytes.len() != 15 {
        return false;
    }
    let mut sum: u32 = 0;
    for (index, byte) in bytes.iter().enumerate() {
        let mut digit = u32::from(*byte).wrapping_sub(u32::from(b'0'));
        if digit > 9 {
            return false;
        }
        if (bytes.len() - 1 - index) % 2 == 1 {
            digit *= 2;
            if digit > 9 {
                digit -= 9;
            }
        }
        sum += digit;
    }
    sum.is_multiple_of(10)
}

/// Exact `url.QueryEscape`: unreserved bytes stay, space becomes `+`, every
/// other byte is `%XX` with uppercase hex.
fn query_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                escaped.push(char::from(byte));
            }
            b' ' => escaped.push('+'),
            _ => {
                escaped.push('%');
                escaped.push(char::from(HEX[usize::from(byte >> 4)]));
                escaped.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
        }
    }
    escaped
}

pub(crate) fn allowed_option(values: &[String], value: &str) -> bool {
    values.iter().any(|candidate| candidate == value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn valid_imeis_from_the_go_tests_pass() {
        for imei in ["490154203237518", "356938035643809", "352099001761481"] {
            assert!(valid_imei(imei), "{imei}");
        }
    }

    #[test]
    fn bad_checksums_lengths_and_separators_fail() {
        for imei in [
            "",
            "49015420323751",
            "4901542032375181",
            "490154203237519",
            "123456789012345",
            "860000000000000",
            "49015420323751a",
            "49015 203237518",
            "4901-5420-3237-51",
            "490154203237518\n",
        ] {
            assert!(!valid_imei(imei), "{imei:?}");
        }
    }

    #[test]
    fn unicode_digits_are_rejected_byte_wise() {
        // Arabic-Indic digits render as 15 characters but 30 UTF-8 bytes.
        assert!(!valid_imei("٤٩٠١٥٤٢٠٣٢٣٧٥١٨"));
        // A full seven-segment style digit inside an otherwise valid IMEI.
        assert!(!valid_imei("49015420323751８"));
    }

    #[test]
    fn variant_key_is_stable_across_map_order() {
        let first = variant_key(&options(&[
            ("config", "12 GB RAM + 256 GB"),
            ("color", "Midnight"),
        ]));
        let second = variant_key(&options(&[
            ("color", "Midnight"),
            ("config", "12 GB RAM + 256 GB"),
        ]));
        assert_eq!(first, second);
        assert_eq!(first, "config=12+GB+RAM+%2B+256+GB&color=Midnight");
    }

    #[test]
    fn variant_key_keeps_empty_slots_and_escapes_like_go() {
        assert_eq!(
            variant_key(&options(&[("config", "16 GB RAM + 1 TB")])),
            "config=16+GB+RAM+%2B+1+TB&color="
        );
        assert_eq!(variant_key(&HashMap::new()), "");
        assert_eq!(
            variant_key(&options(&[("config", " a/b&c=d "), ("color", "Café")])),
            "config=a%2Fb%26c%3Dd&color=Caf%C3%A9"
        );
    }

    #[test]
    fn variant_label_skips_empty_options() {
        assert_eq!(
            variant_label(&options(&[
                ("config", "12 GB RAM + 256 GB"),
                ("color", "Midnight"),
            ])),
            "Memory and storage: 12 GB RAM + 256 GB · Color: Midnight"
        );
        assert_eq!(
            variant_label(&options(&[("color", " Blue ")])),
            "Color: Blue"
        );
        assert_eq!(variant_label(&HashMap::new()), "");
    }

    #[test]
    fn allowed_option_compares_exact_strings() {
        let values = vec!["Midnight".to_string(), "Blue".to_string()];
        assert!(allowed_option(&values, "Blue"));
        assert!(!allowed_option(&values, "blue"));
        assert!(!allowed_option(&values, ""));
        assert!(!allowed_option(&[], "Blue"));
    }
}
