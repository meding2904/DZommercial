//! JSON reader matching the semantics and error text of the Go
//! `encoding/json` decoder used by the oracle. The Go decoder accepts a few
//! things a strict RFC reader rejects (a leading `01` is read as `0` followed
//! by trailing input, lone UTF-16 surrogates become U+FFFD, `1e-400`
//! underflows to zero), so the crate carries its own reader instead of relying
//! on serde_json's stricter grammar.

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JsonError {
    pub(crate) message: String,
}

impl JsonError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

pub(crate) fn read_value(raw: &str) -> Result<Json, JsonError> {
    let mut reader = Reader {
        src: raw,
        bytes: raw.as_bytes(),
        pos: 0,
    };
    reader.skip_whitespace();
    reader.value()
}

const MAX_DEPTH: usize = 10_000;

struct Reader<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

// Containers are validated with an explicit stack so deeply nested input
// cannot overflow the call stack. Array contents and objects below the two
// levels the theme decoder reads are discarded after validation.
enum Frame {
    Array,
    Object {
        entries: Vec<(String, Json)>,
        key: Option<String>,
        store: bool,
    },
}

impl Frame {
    fn finish(self) -> Json {
        match self {
            Frame::Array => Json::Array(Vec::new()),
            Frame::Object { entries, store, .. } => {
                Json::Object(if store { entries } else { Vec::new() })
            }
        }
    }
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn unexpected_eof(&self) -> JsonError {
        JsonError::new("unexpected EOF")
    }

    fn invalid_at(&self, pos: usize, context: &str) -> JsonError {
        let ch = self
            .src
            .get(pos..)
            .and_then(|rest| rest.chars().next())
            .unwrap_or('\u{FFFD}');
        JsonError::new(format!("invalid character {} {context}", quote_rune(ch)))
    }

    fn value(&mut self) -> Result<Json, JsonError> {
        let mut stack: Vec<Frame> = Vec::new();
        let mut pending: Option<Json> = None;

        loop {
            if let Some(value) = pending.take() {
                let Some(frame) = stack.last_mut() else {
                    return Ok(value);
                };
                let closed = match frame {
                    Frame::Array => {
                        self.skip_whitespace();
                        match self.peek() {
                            None => return Err(self.unexpected_eof()),
                            Some(b',') => {
                                self.pos += 1;
                                false
                            }
                            Some(b']') => {
                                self.pos += 1;
                                true
                            }
                            Some(_) => {
                                return Err(self.invalid_at(self.pos, "after array element"));
                            }
                        }
                    }
                    Frame::Object {
                        entries,
                        key,
                        store,
                    } => {
                        let Some(name) = key.take() else {
                            return Err(self.unexpected_eof());
                        };
                        if *store {
                            entries.push((name, value));
                        }
                        self.skip_whitespace();
                        match self.peek() {
                            None => return Err(self.unexpected_eof()),
                            Some(b',') => {
                                self.pos += 1;
                                *key = Some(self.object_key()?);
                                false
                            }
                            Some(b'}') => {
                                self.pos += 1;
                                true
                            }
                            Some(_) => {
                                return Err(
                                    self.invalid_at(self.pos, "after object key:value pair")
                                );
                            }
                        }
                    }
                };
                if closed && let Some(frame) = stack.pop() {
                    pending = Some(frame.finish());
                }
                continue;
            }

            self.skip_whitespace();
            match self.peek() {
                None => return Err(self.unexpected_eof()),
                Some(b'"') => pending = Some(Json::String(self.string()?)),
                Some(b'-' | b'0'..=b'9') => pending = Some(self.number()?),
                Some(b't') => pending = Some(self.literal("true", Json::Bool(true))?),
                Some(b'f') => pending = Some(self.literal("false", Json::Bool(false))?),
                Some(b'n') => pending = Some(self.literal("null", Json::Null)?),
                Some(b'[') => {
                    if stack.len() >= MAX_DEPTH {
                        return Err(JsonError::new("exceeded max depth"));
                    }
                    self.pos += 1;
                    stack.push(Frame::Array);
                    self.skip_whitespace();
                    if self.peek() == Some(b']') {
                        self.pos += 1;
                        if let Some(frame) = stack.pop() {
                            pending = Some(frame.finish());
                        }
                    }
                }
                Some(b'{') => {
                    if stack.len() >= MAX_DEPTH {
                        return Err(JsonError::new("exceeded max depth"));
                    }
                    self.pos += 1;
                    let store = stack.len() <= 1;
                    stack.push(Frame::Object {
                        entries: Vec::new(),
                        key: None,
                        store,
                    });
                    self.skip_whitespace();
                    match self.peek() {
                        None => return Err(self.unexpected_eof()),
                        Some(b'}') => {
                            self.pos += 1;
                            if let Some(frame) = stack.pop() {
                                pending = Some(frame.finish());
                            }
                        }
                        Some(_) => {
                            let name = self.object_key()?;
                            if let Some(Frame::Object { key, .. }) = stack.last_mut() {
                                *key = Some(name);
                            }
                        }
                    }
                }
                Some(_) => {
                    return Err(self.invalid_at(self.pos, "looking for beginning of value"));
                }
            }
        }
    }

    fn object_key(&mut self) -> Result<String, JsonError> {
        self.skip_whitespace();
        match self.peek() {
            None => return Err(self.unexpected_eof()),
            Some(b'"') => {}
            Some(_) => {
                return Err(self.invalid_at(self.pos, "looking for beginning of object key string"));
            }
        }
        let key = self.string()?;
        self.skip_whitespace();
        match self.peek() {
            None => Err(self.unexpected_eof()),
            Some(b':') => {
                self.pos += 1;
                Ok(key)
            }
            Some(_) => Err(self.invalid_at(self.pos, "after object key")),
        }
    }

    fn literal(&mut self, word: &'static str, value: Json) -> Result<Json, JsonError> {
        for expected in word.chars() {
            let byte = self.peek().ok_or_else(|| self.unexpected_eof())?;
            if byte != expected as u8 {
                let ch = self
                    .src
                    .get(self.pos..)
                    .and_then(|rest| rest.chars().next())
                    .unwrap_or('\u{FFFD}');
                return Err(JsonError::new(format!(
                    "invalid character {} in literal {word} (expecting '{expected}')",
                    quote_rune(ch)
                )));
            }
            self.pos += 1;
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
            match self.peek() {
                None => return Err(self.unexpected_eof()),
                Some(b'0'..=b'9') => {}
                Some(_) => return Err(self.invalid_at(self.pos, "in numeric literal")),
            }
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                self.pos += 1;
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            _ => return Err(self.invalid_at(self.pos, "in numeric literal")),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            match self.peek() {
                None => return Err(self.unexpected_eof()),
                Some(b'0'..=b'9') => {}
                Some(_) => return Err(self.invalid_at(self.pos, "in numeric literal")),
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            match self.peek() {
                None => return Err(self.unexpected_eof()),
                Some(b'0'..=b'9') => {}
                Some(_) => return Err(self.invalid_at(self.pos, "in numeric literal")),
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        Ok(Json::Number(
            self.src
                .get(start..self.pos)
                .unwrap_or_default()
                .to_string(),
        ))
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.pos += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let byte = self.peek().ok_or_else(|| self.unexpected_eof())?;
            match byte {
                b'"' => {
                    self.pos += 1;
                    return Ok(String::from_utf8_lossy(&out).into_owned());
                }
                b'\\' => {
                    self.pos += 1;
                    let escape = self.peek().ok_or_else(|| self.unexpected_eof())?;
                    self.pos += 1;
                    match escape {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0c),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => self.unicode_escape(&mut out)?,
                        other => {
                            let what = [b'\\', other];
                            return Err(JsonError::new(invalid_text(
                                "escape sequence",
                                &what,
                                "in string",
                            )));
                        }
                    }
                }
                b if b < 0x20 => return Err(self.invalid_at(self.pos, "in string")),
                _ => {
                    out.push(byte);
                    self.pos += 1;
                }
            }
        }
    }

    fn unicode_escape(&mut self, out: &mut Vec<u8>) -> Result<(), JsonError> {
        let first = self.hex_quad()?;
        if (0xD800..=0xDBFF).contains(&first)
            && self.peek() == Some(b'\\')
            && self.bytes.get(self.pos + 1) == Some(&b'u')
        {
            self.pos += 2;
            let second = self.hex_quad()?;
            if (0xDC00..=0xDFFF).contains(&second) {
                let combined = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                push_char(out, char::from_u32(combined).unwrap_or('\u{FFFD}'));
            } else {
                push_char(out, '\u{FFFD}');
                push_code_unit(out, second);
            }
        } else {
            push_code_unit(out, first);
        }
        Ok(())
    }

    fn hex_quad(&mut self) -> Result<u32, JsonError> {
        if self.bytes.len().saturating_sub(self.pos) < 4 {
            return Err(self.unexpected_eof());
        }
        let raw = &self.bytes[self.pos..self.pos + 4];
        self.pos += 4;
        let mut value = 0u32;
        for &byte in raw {
            let digit = match byte {
                b'0'..=b'9' => u32::from(byte - b'0'),
                b'a'..=b'f' => u32::from(byte - b'a') + 10,
                b'A'..=b'F' => u32::from(byte - b'A') + 10,
                _ => {
                    let mut what = Vec::with_capacity(6);
                    what.extend_from_slice(b"\\u");
                    what.extend_from_slice(raw);
                    return Err(JsonError::new(invalid_text(
                        "escape sequence",
                        &what,
                        "in string",
                    )));
                }
            };
            value = (value << 4) | digit;
        }
        Ok(value)
    }
}

fn push_code_unit(out: &mut Vec<u8>, unit: u32) {
    if (0xD800..=0xDFFF).contains(&unit) {
        push_char(out, '\u{FFFD}');
    } else {
        push_char(out, char::from_u32(unit).unwrap_or('\u{FFFD}'));
    }
}

fn push_char(out: &mut Vec<u8>, ch: char) {
    let mut buf = [0u8; 4];
    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
}

fn invalid_text(label: &str, what: &[u8], which: &str) -> String {
    let rendered = if rune_count(what) == 1 {
        match std::str::from_utf8(what) {
            Ok(text) => quote_rune(text.chars().next().unwrap_or('\u{FFFD}')),
            Err(_) => format!("'\\x{:02x}'", what.first().copied().unwrap_or(0)),
        }
    } else if need_escape(what) {
        quote_bytes(what)
    } else {
        format!("`{}`", String::from_utf8_lossy(what))
    };
    format!("invalid {label} {rendered} {which}")
}

fn rune_count(bytes: &[u8]) -> usize {
    let mut rest = bytes;
    let mut count = 0;
    loop {
        if rest.is_empty() {
            return count;
        }
        match std::str::from_utf8(rest) {
            Ok(valid) => return count + valid.chars().count(),
            Err(error) => {
                let valid_len = error.valid_up_to();
                if let Ok(valid) = std::str::from_utf8(&rest[..valid_len]) {
                    count += valid.chars().count();
                }
                count += 1;
                rest = &rest[valid_len + 1..];
            }
        }
    }
}

fn need_escape(bytes: &[u8]) -> bool {
    match std::str::from_utf8(bytes) {
        Ok(valid) => valid.chars().any(needs_escape_char),
        Err(_) => true,
    }
}

fn needs_escape_char(ch: char) -> bool {
    ch == '`' || ch == '\u{FFFD}' || ch.is_whitespace() || !is_go_print(ch)
}

fn quote_bytes(bytes: &[u8]) -> String {
    let mut out = String::from("\"");
    let mut rest = bytes;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(valid) => {
                for ch in valid.chars() {
                    push_quoted_char(&mut out, ch, '"');
                }
                break;
            }
            Err(error) => {
                let valid_len = error.valid_up_to();
                if let Ok(valid) = std::str::from_utf8(&rest[..valid_len]) {
                    for ch in valid.chars() {
                        push_quoted_char(&mut out, ch, '"');
                    }
                }
                out.push_str(&format!("\\x{:02x}", rest[valid_len]));
                rest = &rest[valid_len + 1..];
            }
        }
    }
    out.push('"');
    out
}

fn quote_string(value: &str) -> String {
    quote_bytes(value.as_bytes())
}

fn quote_rune(ch: char) -> String {
    let mut out = String::from("'");
    push_quoted_char(&mut out, ch, '\'');
    out.push('\'');
    out
}

fn push_quoted_char(out: &mut String, ch: char, quote: char) {
    if ch == quote || ch == '\\' {
        out.push('\\');
        out.push(ch);
        return;
    }
    if is_go_print(ch) {
        out.push(ch);
        return;
    }
    match ch {
        '\u{7}' => out.push_str("\\a"),
        '\u{8}' => out.push_str("\\b"),
        '\u{c}' => out.push_str("\\f"),
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\t' => out.push_str("\\t"),
        '\u{b}' => out.push_str("\\v"),
        ch if (ch as u32) < 0x20 || ch as u32 == 0x7f => {
            out.push_str(&format!("\\x{:02x}", ch as u32));
        }
        ch if (ch as u32) < 0x10000 => out.push_str(&format!("\\u{:04x}", ch as u32)),
        ch => out.push_str(&format!("\\U{:08x}", ch as u32)),
    }
}

fn is_go_print(ch: char) -> bool {
    use unicode_general_category::{GeneralCategory, get_general_category};
    if ch == ' ' {
        return true;
    }
    matches!(
        get_general_category(ch),
        GeneralCategory::UppercaseLetter
            | GeneralCategory::LowercaseLetter
            | GeneralCategory::TitlecaseLetter
            | GeneralCategory::ModifierLetter
            | GeneralCategory::OtherLetter
            | GeneralCategory::NonspacingMark
            | GeneralCategory::SpacingMark
            | GeneralCategory::EnclosingMark
            | GeneralCategory::DecimalNumber
            | GeneralCategory::LetterNumber
            | GeneralCategory::OtherNumber
            | GeneralCategory::ConnectorPunctuation
            | GeneralCategory::DashPunctuation
            | GeneralCategory::OpenPunctuation
            | GeneralCategory::ClosePunctuation
            | GeneralCategory::InitialPunctuation
            | GeneralCategory::FinalPunctuation
            | GeneralCategory::OtherPunctuation
            | GeneralCategory::MathSymbol
            | GeneralCategory::CurrencySymbol
            | GeneralCategory::ModifierSymbol
            | GeneralCategory::OtherSymbol
    )
}

pub(crate) fn quote_go_string(value: &str) -> String {
    quote_string(value)
}

#[cfg(test)]
mod tests {
    use super::{Json, read_value};

    #[test]
    fn reads_first_value_and_ignores_trailing_input() {
        assert_eq!(
            read_value("{\"a\":1}{\"b\":2}").unwrap(),
            Json::Object(vec![("a".to_string(), Json::Number("1".to_string()))])
        );
    }

    #[test]
    fn replaces_lone_surrogates() {
        assert_eq!(
            read_value(r#""a\ud800b""#).unwrap(),
            Json::String("a\u{FFFD}b".to_string())
        );
        assert_eq!(
            read_value(r#""\ud83d\ude00""#).unwrap(),
            Json::String("\u{1F600}".to_string())
        );
    }
}
