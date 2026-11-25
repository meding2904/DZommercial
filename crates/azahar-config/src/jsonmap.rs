//! JSON decoding for `COURIER_CREDENTIALS` with Go's `encoding/json` semantics.
//!
//! `serde_json` rejects lone UTF-16 surrogates, while Go replaces them with
//! U+FFFD, and Go accepts JSON `null` as a string value (decoding to the zero
//! string). Those rules are visible through `COURIER_CREDENTIALS`, so the small
//! grammar that method accepts is decoded directly.

use std::collections::HashMap;

const REPLACEMENT: char = '\u{FFFD}';

/// Decode a JSON object of string values.
///
/// `Ok(None)` mirrors Go unmarshalling `null` into a map: nil map, no error.
pub(crate) fn parse_credentials_object(input: &str) -> Result<Option<HashMap<String, String>>, ()> {
    let bytes = input.as_bytes();
    let mut cursor = 0;
    skip_whitespace(bytes, &mut cursor);

    if bytes[cursor..].starts_with(b"null") {
        cursor += 4;
        skip_whitespace(bytes, &mut cursor);
        return if cursor == bytes.len() {
            Ok(None)
        } else {
            Err(())
        };
    }

    if bytes.get(cursor) != Some(&b'{') {
        return Err(());
    }
    cursor += 1;
    skip_whitespace(bytes, &mut cursor);

    let mut values = HashMap::new();
    if bytes.get(cursor) == Some(&b'}') {
        cursor += 1;
        skip_whitespace(bytes, &mut cursor);
        return if cursor == bytes.len() {
            Ok(Some(values))
        } else {
            Err(())
        };
    }

    loop {
        let key = parse_string(bytes, &mut cursor)?;
        skip_whitespace(bytes, &mut cursor);
        if bytes.get(cursor) != Some(&b':') {
            return Err(());
        }
        cursor += 1;
        skip_whitespace(bytes, &mut cursor);

        let value = if bytes.get(cursor) == Some(&b'"') {
            parse_string(bytes, &mut cursor)?
        } else if bytes[cursor..].starts_with(b"null") {
            cursor += 4;
            String::new()
        } else {
            return Err(());
        };
        values.insert(key, value);

        skip_whitespace(bytes, &mut cursor);
        match bytes.get(cursor) {
            Some(b',') => {
                cursor += 1;
                skip_whitespace(bytes, &mut cursor);
            }
            Some(b'}') => {
                cursor += 1;
                break;
            }
            _ => return Err(()),
        }
    }

    skip_whitespace(bytes, &mut cursor);
    if cursor == bytes.len() {
        Ok(Some(values))
    } else {
        Err(())
    }
}

fn skip_whitespace(bytes: &[u8], cursor: &mut usize) {
    while matches!(bytes.get(*cursor), Some(b' ' | b'\t' | b'\n' | b'\r')) {
        *cursor += 1;
    }
}

fn parse_string(bytes: &[u8], cursor: &mut usize) -> Result<String, ()> {
    if bytes.get(*cursor) != Some(&b'"') {
        return Err(());
    }
    *cursor += 1;

    let mut out: Vec<u8> = Vec::new();
    loop {
        let Some(&c) = bytes.get(*cursor) else {
            return Err(());
        };
        match c {
            b'"' => {
                *cursor += 1;
                return String::from_utf8(out).map_err(|_| ());
            }
            b'\\' => {
                *cursor += 1;
                let Some(&escape) = bytes.get(*cursor) else {
                    return Err(());
                };
                match escape {
                    b'"' | b'\\' | b'/' => {
                        out.push(escape);
                        *cursor += 1;
                    }
                    b'b' => {
                        out.push(0x08);
                        *cursor += 1;
                    }
                    b'f' => {
                        out.push(0x0c);
                        *cursor += 1;
                    }
                    b'n' => {
                        out.push(b'\n');
                        *cursor += 1;
                    }
                    b'r' => {
                        out.push(b'\r');
                        *cursor += 1;
                    }
                    b't' => {
                        out.push(b'\t');
                        *cursor += 1;
                    }
                    b'u' => {
                        *cursor += 1;
                        let rune = parse_unicode_escape(bytes, cursor)?;
                        let mut encoded = [0u8; 4];
                        out.extend_from_slice(rune.encode_utf8(&mut encoded).as_bytes());
                    }
                    _ => return Err(()),
                }
            }
            c if c < 0x20 => return Err(()),
            _ => {
                out.push(c);
                *cursor += 1;
            }
        }
    }
}

fn parse_unicode_escape(bytes: &[u8], cursor: &mut usize) -> Result<char, ()> {
    let first = parse_hex4(bytes, cursor)?;
    if (0xD800..=0xDBFF).contains(&first) {
        if bytes.get(*cursor) == Some(&b'\\') && bytes.get(*cursor + 1) == Some(&b'u') {
            let retry = *cursor;
            *cursor += 2;
            let second = parse_hex4(bytes, cursor)?;
            if (0xDC00..=0xDFFF).contains(&second) {
                let codepoint =
                    0x10000 + (u32::from(first - 0xD800) << 10) + u32::from(second - 0xDC00);
                return Ok(char::from_u32(codepoint).unwrap_or(REPLACEMENT));
            }
            // Go consumes only the first escape and decodes the second on its
            // own, which may itself be a lone surrogate.
            *cursor = retry;
        }
        return Ok(REPLACEMENT);
    }
    if (0xDC00..=0xDFFF).contains(&first) {
        return Ok(REPLACEMENT);
    }
    Ok(char::from_u32(u32::from(first)).unwrap_or(REPLACEMENT))
}

fn parse_hex4(bytes: &[u8], cursor: &mut usize) -> Result<u16, ()> {
    let mut value = 0u16;
    for _ in 0..4 {
        let Some(&c) = bytes.get(*cursor) else {
            return Err(());
        };
        let digit = match c {
            b'0'..=b'9' => u16::from(c - b'0'),
            b'a'..=b'f' => u16::from(c - b'a') + 10,
            b'A'..=b'F' => u16::from(c - b'A') + 10,
            _ => return Err(()),
        };
        value = (value << 4) | digit;
        *cursor += 1;
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_object_and_values() {
        assert_eq!(parse_credentials_object("null"), Ok(None));
        assert_eq!(parse_credentials_object("{}"), Ok(Some(HashMap::new())));
        let parsed = parse_credentials_object(r#"{"a":null,"b":"x"}"#)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.get("a").map(String::as_str), Some(""));
        assert_eq!(parsed.get("b").map(String::as_str), Some("x"));
    }

    #[test]
    fn surrogates_are_replaced() {
        let parsed = parse_credentials_object(r#"{"a":"\ud800\uZZZZ"}"#);
        assert_eq!(parsed, Err(()));
        let parsed = parse_credentials_object(r#"{"a":"\ud800x"}"#)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.get("a").map(String::as_str), Some("\u{FFFD}x"));
        let parsed = parse_credentials_object(r#"{"a":"\ud83d\ude00"}"#)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.get("a").map(String::as_str), Some("😀"));
    }

    #[test]
    fn rejects_non_object_and_trailing_data() {
        for value in [
            r#"["x"]"#,
            r#""x""#,
            "123",
            "true",
            "{not json",
            r#"{"a":"b"} x"#,
            r#"{"a":1}"#,
            r#"{"a":true}"#,
            r#"{"a":{}}"#,
            r#"{"a":[]}"#,
            r#"{"a":"b",}"#,
        ] {
            assert_eq!(parse_credentials_object(value), Err(()), "{value}");
        }
    }

    #[test]
    fn duplicates_and_escapes() {
        let parsed = parse_credentials_object(r#"{"a":"x","a":"y"}"#)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.get("a").map(String::as_str), Some("y"));
        let parsed = parse_credentials_object(r#"{"\u0061":"x\ny"}"#)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.get("a").map(String::as_str), Some("x\ny"));
    }
}
