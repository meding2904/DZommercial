//! Input validation shared by the HTTP handlers.

use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};

/// Go `utf8.RuneCountInString`.
pub fn rune_len(value: &str) -> usize {
    value.chars().count()
}

/// Port of `validEmail`: a bare addr-spec accepted by `net/mail.ParseAddress`
/// whose parsed address equals the input. Comments and quoted local parts are
/// rejected because Go's parser normalises them, so the equality check fails.
pub fn valid_email(value: &str) -> bool {
    if value.is_empty() || value.len() > 254 || value.contains([' ', '\t', '\r', '\n']) {
        return false;
    }
    let mut parts = value.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    if local.is_empty() || domain.is_empty() {
        return false;
    }
    if !valid_dot_atom(local) {
        return false;
    }
    if domain.starts_with('[') && domain.ends_with(']') {
        return valid_domain_literal(&domain[1..domain.len() - 1]);
    }
    valid_dot_atom(domain)
}

/// Go's `net/mail` accepts an IPv4 literal, a v4-mapped IPv6 literal without a
/// prefix, and any IP after an explicit `IPv6:` tag.
fn valid_domain_literal(value: &str) -> bool {
    if let Some(inner) = value.strip_prefix("IPv6:") {
        return inner.parse::<Ipv6Addr>().is_ok() || inner.parse::<Ipv4Addr>().is_ok();
    }
    if value.parse::<Ipv4Addr>().is_ok() {
        return true;
    }
    value
        .parse::<Ipv6Addr>()
        .map(|address| address.to_ipv4_mapped().is_some())
        .unwrap_or(false)
}

fn valid_dot_atom(value: &str) -> bool {
    value
        .split('.')
        .all(|atom| !atom.is_empty() && atom.chars().all(is_atext))
}

/// The atext set used by Go's `net/mail` parser: printable ASCII except the
/// specials, plus every non-ASCII rune.
fn is_atext(character: char) -> bool {
    match character {
        '!' | '#' | '$' | '%' | '&' | '\'' | '*' | '+' | '-' | '/' | '=' | '?' | '^' | '_'
        | '`' | '{' | '|' | '}' | '~' => true,
        '0'..='9' | 'a'..='z' | 'A'..='Z' => true,
        character if character.is_ascii() => false,
        _ => true,
    }
}

pub fn valid_slug(value: &str) -> bool {
    if value.is_empty() || value.len() > 120 {
        return false;
    }
    value.split('-').all(|part| {
        !part.is_empty()
            && part
                .chars()
                .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
    })
}

pub fn valid_phone(value: &str) -> bool {
    let length = value.chars().count();
    (6..=30).contains(&length)
        && value
            .chars()
            .all(|character| character.is_ascii_digit() || " +()-".contains(character))
}

/// A local 10 digit mobile number. The courier API expects 05, 06 or 07
/// prefixes; spaces and punctuation are accepted on the way in.
pub fn normalized_algerian_mobile(value: &str) -> Option<String> {
    let mut buffer = String::new();
    for character in value.chars() {
        match character {
            '0'..='9' => buffer.push(character),
            '+' if buffer.is_empty() => buffer.push(character),
            ' ' | '-' | '(' | ')' => continue,
            _ => return None,
        }
    }

    let mut digits = buffer.strip_prefix('+').unwrap_or(&buffer).to_string();
    if digits.starts_with("213") && digits.chars().count() == 12 {
        digits = format!("0{}", &digits[3..]);
    }
    if digits.chars().count() != 10 {
        return None;
    }
    let mut characters = digits.chars();
    if characters.next() != Some('0') {
        return None;
    }
    if !matches!(characters.next(), Some('5' | '6' | '7')) {
        return None;
    }
    Some(digits)
}

pub fn valid_algerian_mobile(value: &str) -> bool {
    normalized_algerian_mobile(value).is_some()
}

pub fn truncate_runes(value: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if value.chars().count() <= max {
        return value.to_string();
    }
    value.chars().take(max).collect()
}

pub fn trim_all(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// Utility used by handlers that build field-error maps.
pub fn fields(entries: &[(&str, &str)]) -> HashMap<String, String> {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_cases_match_go_net_mail() {
        let valid = [
            "a@b",
            "a@b.c",
            "a.b@c.d",
            "a@-b",
            "a@b-",
            "a@b_c",
            "a@[1.2.3.4]",
            "a@[IPv6:::1]",
            "a@[IPv6:2001:db8::1]",
            "a@[IPv6:1.2.3.4]",
            "a@[::ffff:1.2.3.4]",
            "a@[::FFFF:1.2.3.4]",
            "a@[0:0:0:0:0:ffff:102:304]",
            "a@[IPv6:::ffff:1.2.3.4]",
            "a+b@c.d",
            "ü@b.c",
            "a@ü.c",
            "a!#$%&'*+-/=?^_`{|}~@b.c",
            "a@b/c",
            "A@B.C",
            "a@127.0.0.1",
            "a@b-c.d",
            "a@b--c.d",
            "a@b-.c",
            "a@b._c",
        ];
        for value in valid {
            assert!(valid_email(value), "{value} should be valid");
        }

        let invalid = [
            "",
            "a..b@c.d",
            ".a@b",
            "a.@b",
            "\"ab\"@c.d",
            "a@b@c",
            "a@",
            "@b",
            "a@b.",
            "a@b..c",
            "a@b c",
            "a@b:c",
            "a@b;c",
            "a@b,c",
            "a@b.c.",
            "a@.b.c",
            "a@b(c)",
            "a@[abc]",
            "a@[::1]",
            "a@[2001:db8::1]",
            "a@[IPv6:zz]",
            "a@[IPv6:]",
            "a@[1.2.3.4]x",
            "a@b..",
        ];
        for value in invalid {
            assert!(!valid_email(value), "{value} should be invalid");
        }
    }

    #[test]
    fn slug_rules() {
        for value in ["a", "abc-123", "a1-b2"] {
            assert!(valid_slug(value), "{value}");
        }
        for value in ["", "A", "-a", "a-", "a--b", "a_b", "a.b", &"a".repeat(121)] {
            assert!(!valid_slug(value), "{value}");
        }
    }

    #[test]
    fn algerian_mobile_normalisation() {
        assert_eq!(
            normalized_algerian_mobile("+213 555 12 34 56").as_deref(),
            Some("0555123456")
        );
        assert_eq!(
            normalized_algerian_mobile("(06) 12-34-56-78").as_deref(),
            Some("0612345678")
        );
        assert_eq!(
            normalized_algerian_mobile("0712345678").as_deref(),
            Some("0712345678")
        );
        assert_eq!(normalized_algerian_mobile("0412345678"), None);
        assert_eq!(normalized_algerian_mobile("712345678"), None);
        assert_eq!(normalized_algerian_mobile("07123456789"), None);
        assert_eq!(normalized_algerian_mobile("07a1234567"), None);
    }

    #[test]
    fn truncate_counts_characters() {
        assert_eq!(truncate_runes("héllo", 3), "hél");
        assert_eq!(truncate_runes("abc", 0), "");
        assert_eq!(truncate_runes("abc", 5), "abc");
    }
}
