//! JSON text helpers that reproduce Go's `encoding/json` and `strconv.Quote`.
//!
//! The code-mode plan is stored and returned as the exact text Go produced, so
//! raw patch values keep their key order and duplicate keys. `serde_json`'s
//! value types reorder objects and collapse duplicates, which is why the plan
//! encoder and every fingerprint are written by hand here.

use std::fmt::Write as _;

/// Go `encoding/json` string escaping, including the HTML-safe escapes the Go
/// encoder applies by default.
#[must_use]
pub fn string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            other if (other as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", other as u32);
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Go `fmt`'s `%q` for a UTF-8 string (`strconv.Quote`).
///
/// Printable characters are kept as-is and control characters use the Go
/// escapes. Code-mode only quotes identifiers, slugs and names, so the
/// printability test is intentionally the simpler "not a control or separator"
/// rule instead of the full Unicode `IsPrint` tables.
#[must_use]
pub fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{07}' => out.push_str("\\a"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0b}' => out.push_str("\\v"),
            other if (other as u32) < 0x20 || other == '\u{7f}' => {
                let _ = write!(out, "\\x{:02x}", other as u32);
            }
            '\u{2028}' | '\u{2029}' => {
                let _ = write!(out, "\\u{:04x}", character as u32);
            }
            other if other.is_control() => {
                let code = other as u32;
                if code < 0x1_0000 {
                    let _ = write!(out, "\\u{code:04x}");
                } else {
                    let _ = write!(out, "\\U{code:08x}");
                }
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Go's `json.Marshal` compaction of a raw JSON message: whitespace outside
/// strings is dropped, and the HTML-sensitive characters inside strings are
/// escaped.
#[must_use]
pub fn compact(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut characters = raw.chars();
    let mut in_string = false;
    while let Some(character) = characters.next() {
        if in_string {
            match character {
                '"' => {
                    in_string = false;
                    out.push('"');
                }
                '\\' => {
                    out.push('\\');
                    if let Some(next) = characters.next() {
                        out.push(next);
                    }
                }
                '<' => out.push_str("\\u003c"),
                '>' => out.push_str("\\u003e"),
                '&' => out.push_str("\\u0026"),
                '\u{2028}' => out.push_str("\\u2028"),
                '\u{2029}' => out.push_str("\\u2029"),
                other if (other as u32) < 0x20 => {
                    let _ = write!(out, "\\u{:04x}", other as u32);
                }
                other => out.push(other),
            }
            continue;
        }
        match character {
            '"' => {
                in_string = true;
                out.push('"');
            }
            ' ' | '\t' | '\n' | '\r' => {}
            other => out.push(other),
        }
    }
    out
}

/// Incremental object writer that keeps a caller-chosen field order.
pub struct Object {
    text: String,
    first: bool,
}

impl Object {
    #[must_use]
    pub fn new() -> Self {
        Self {
            text: String::from("{"),
            first: true,
        }
    }

    /// Appends a field whose value is already-encoded JSON text.
    pub fn raw_field(&mut self, name: &str, value: &str) {
        if self.first {
            self.first = false;
        } else {
            self.text.push(',');
        }
        self.text.push_str(&string(name));
        self.text.push(':');
        self.text.push_str(value);
    }

    pub fn string_field(&mut self, name: &str, value: &str) {
        self.raw_field(name, &string(value));
    }

    pub fn int_field(&mut self, name: &str, value: i64) {
        self.raw_field(name, &value.to_string());
    }

    pub fn bool_field(&mut self, name: &str, value: bool) {
        self.raw_field(name, if value { "true" } else { "false" });
    }

    #[must_use]
    pub fn finish(mut self) -> String {
        self.text.push('}');
        self.text
    }
}

impl Default for Object {
    fn default() -> Self {
        Self::new()
    }
}

/// Go map encoding for `map[string]string`: sorted keys, HTML-safe strings.
#[must_use]
pub fn string_map(values: &std::collections::BTreeMap<String, String>) -> String {
    let mut object = Object::new();
    for (key, value) in values {
        object.string_field(key, value);
    }
    object.finish()
}

/// A raw JSON text wrapper that serialises verbatim.
///
/// Building the response body by hand and handing it to [`serde_json`] through
/// this type keeps the plan's patch objects byte-identical to Go's output.
#[must_use]
pub fn raw(text: String) -> Box<serde_json::value::RawValue> {
    serde_json::value::RawValue::from_string(text).unwrap_or_else(|_| {
        serde_json::value::RawValue::from_string("null".to_string()).expect("null is valid JSON")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_string_escapes_like_go() {
        assert_eq!(string("a<b>&c"), "\"a\\u003cb\\u003e\\u0026c\"");
        assert_eq!(string("tab\tnl\n"), "\"tab\\tnl\\n\"");
        assert_eq!(string("\u{2028}\u{2029}"), "\"\\u2028\\u2029\"");
        assert_eq!(string("é"), "\"é\"");
    }

    #[test]
    fn compact_drops_whitespace_and_keeps_order() {
        assert_eq!(
            compact(" { \"b\" : 1 , \"a\" : [ 2, 3 ] } "),
            "{\"b\":1,\"a\":[2,3]}"
        );
        assert_eq!(compact("{\"x\":\"a < b\"}"), "{\"x\":\"a \\u003c b\"}");
        assert_eq!(compact("{\"x\":\"\\\\\"}"), "{\"x\":\"\\\\\"}");
    }

    #[test]
    fn quote_matches_go_for_identifiers() {
        assert_eq!(quote("plain"), "\"plain\"");
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote("a\nb"), "\"a\\nb\"");
        assert_eq!(quote("\u{1}"), "\"\\x01\"");
    }
}
