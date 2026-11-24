//! The subset of Go's `net/url` needed by the configuration package.
//!
//! `url.Parse`, `URL.String`, `URL.Query` and `Values.Encode` have observable
//! behaviour that the WHATWG URL parser used by the `url` crate does not share:
//! default ports are preserved, an empty path stays empty, invalid escapes are
//! tolerated or rejected in the same places, and query values are re-encoded
//! with Go's exact escaping rules. The port therefore follows `net/url`
//! directly, operating on bytes so percent-encoded non-UTF-8 values round-trip.

use std::collections::BTreeMap;

pub(crate) type QueryValues = BTreeMap<Vec<u8>, Vec<Vec<u8>>>;

const UPPER_HEX: &[u8; 16] = b"0123456789ABCDEF";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Path,
    Host,
    Zone,
    UserPassword,
    Query,
    Fragment,
}

#[derive(Debug, Clone, Default)]
struct Userinfo {
    username: Vec<u8>,
    password: Vec<u8>,
    password_set: bool,
}

impl Userinfo {
    fn encoded(&self) -> Vec<u8> {
        let mut out = escape(&self.username, Mode::UserPassword);
        if self.password_set {
            out.push(b':');
            out.extend(escape(&self.password, Mode::UserPassword));
        }
        out
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Url {
    scheme: Vec<u8>,
    opaque: Vec<u8>,
    user: Option<Userinfo>,
    host: Vec<u8>,
    path: Vec<u8>,
    raw_path: Vec<u8>,
    raw_query: Vec<u8>,
    fragment: Vec<u8>,
    raw_fragment: Vec<u8>,
    force_query: bool,
    omit_host: bool,
}

impl Url {
    pub(crate) fn scheme(&self) -> &[u8] {
        &self.scheme
    }

    pub(crate) fn host(&self) -> &[u8] {
        &self.host
    }

    pub(crate) fn has_user(&self) -> bool {
        self.user.is_some()
    }

    pub(crate) fn query(&self) -> QueryValues {
        parse_query(&self.raw_query)
    }

    pub(crate) fn set_raw_query(&mut self, query: Vec<u8>) {
        self.raw_query = query;
    }

    pub(crate) fn string(&self) -> String {
        let mut buf: Vec<u8> = Vec::new();

        if !self.scheme.is_empty() {
            buf.extend_from_slice(&self.scheme);
            buf.push(b':');
        }

        if !self.opaque.is_empty() {
            buf.extend_from_slice(&self.opaque);
        } else {
            if !self.scheme.is_empty() || !self.host.is_empty() || self.user.is_some() {
                let omit_empty_host = self.omit_host && self.host.is_empty() && self.user.is_none();
                if !omit_empty_host {
                    if !self.host.is_empty() || !self.path.is_empty() || self.user.is_some() {
                        buf.extend_from_slice(b"//");
                    }
                    if let Some(user) = &self.user {
                        buf.extend_from_slice(&user.encoded());
                        buf.push(b'@');
                    }
                    if !self.host.is_empty() {
                        buf.extend_from_slice(&escape(&self.host, Mode::Host));
                    }
                }
            }

            let mut path = self.escaped_path();
            if self.omit_host
                && self.host.is_empty()
                && self.user.is_none()
                && path.starts_with(b"//")
            {
                buf.extend_from_slice(b"%2F");
                path.remove(0);
            }
            if !path.is_empty() && path[0] != b'/' && !self.host.is_empty() {
                buf.push(b'/');
            }
            if buf.is_empty() {
                let first_segment = match path.iter().position(|&b| b == b'/') {
                    Some(i) => &path[..i],
                    None => &path[..],
                };
                if first_segment.contains(&b':') {
                    buf.extend_from_slice(b"./");
                }
            }
            buf.extend_from_slice(&path);
        }

        if self.force_query || !self.raw_query.is_empty() {
            buf.push(b'?');
            buf.extend_from_slice(&self.raw_query);
        }
        if !self.fragment.is_empty() {
            buf.push(b'#');
            buf.extend_from_slice(&self.escaped_fragment());
        }

        String::from_utf8_lossy(&buf).into_owned()
    }

    fn escaped_path(&self) -> Vec<u8> {
        self.escaped_component(&self.raw_path, &self.path, Mode::Path)
    }

    fn escaped_fragment(&self) -> Vec<u8> {
        self.escaped_component(&self.raw_fragment, &self.fragment, Mode::Fragment)
    }

    fn escaped_component(&self, raw: &[u8], decoded: &[u8], mode: Mode) -> Vec<u8> {
        if !raw.is_empty()
            && valid_encoded(raw, mode)
            && let Ok(candidate) = unescape(raw, mode)
            && candidate == decoded
        {
            return raw.to_vec();
        }
        if mode == Mode::Path && decoded == b"*" {
            return b"*".to_vec();
        }
        escape(decoded, mode)
    }
}

pub(crate) fn parse(raw: &str) -> Result<Url, ()> {
    let bytes = raw.as_bytes();
    let (base, fragment) = match bytes.iter().position(|&b| b == b'#') {
        Some(i) => (&bytes[..i], Some(&bytes[i + 1..])),
        None => (bytes, None),
    };
    let mut url = parse_base(base)?;
    if let Some(fragment) = fragment
        && !fragment.is_empty()
    {
        set_fragment(&mut url, fragment)?;
    }
    Ok(url)
}

fn parse_base(raw: &[u8]) -> Result<Url, ()> {
    if raw.iter().any(|&b| b < b' ' || b == 0x7f) {
        return Err(());
    }

    let mut url = Url::default();
    if raw == b"*" {
        url.path = b"*".to_vec();
        return Ok(url);
    }

    let (scheme, mut rest) = get_scheme(raw)?;
    url.scheme = scheme;

    if rest.last() == Some(&b'?') && rest.iter().filter(|&&b| b == b'?').count() == 1 {
        url.force_query = true;
        rest = &rest[..rest.len() - 1];
    } else if let Some(i) = rest.iter().position(|&b| b == b'?') {
        url.raw_query = rest[i + 1..].to_vec();
        rest = &rest[..i];
    }

    if !rest.starts_with(b"/") {
        if !url.scheme.is_empty() {
            url.opaque = rest.to_vec();
            return Ok(url);
        }
        let first_segment = match rest.iter().position(|&b| b == b'/') {
            Some(i) => &rest[..i],
            None => rest,
        };
        if first_segment.contains(&b':') {
            return Err(());
        }
    }

    let has_authority = (url.scheme.is_empty() && !rest.starts_with(b"///")
        || !url.scheme.is_empty())
        && rest.starts_with(b"//");
    if has_authority {
        let after_slashes = &rest[2..];
        let (authority, remainder) = match after_slashes.iter().position(|&b| b == b'/') {
            Some(i) => (&after_slashes[..i], &after_slashes[i..]),
            None => (after_slashes, &[][..]),
        };
        let (user, host) = parse_authority(&url.scheme, authority)?;
        url.user = user;
        url.host = host;
        rest = remainder;
    } else if !url.scheme.is_empty() && rest.starts_with(b"/") {
        url.omit_host = true;
    }

    set_path(&mut url, rest)?;
    Ok(url)
}

fn get_scheme(raw: &[u8]) -> Result<(Vec<u8>, &[u8]), ()> {
    for (i, &c) in raw.iter().enumerate() {
        match c {
            b'a'..=b'z' | b'A'..=b'Z' => {}
            b'0'..=b'9' | b'+' | b'-' | b'.' => {
                if i == 0 {
                    return Ok((Vec::new(), raw));
                }
            }
            b':' => {
                if i == 0 {
                    return Err(());
                }
                return Ok((raw[..i].to_ascii_lowercase(), &raw[i + 1..]));
            }
            _ => return Ok((Vec::new(), raw)),
        }
    }
    Ok((Vec::new(), raw))
}

fn parse_authority(scheme: &[u8], authority: &[u8]) -> Result<(Option<Userinfo>, Vec<u8>), ()> {
    let at = authority.iter().rposition(|&b| b == b'@');
    let host_part = match at {
        Some(i) => &authority[i + 1..],
        None => authority,
    };
    let host = parse_host(scheme, host_part)?;

    let Some(i) = at else {
        return Ok((None, host));
    };

    let raw_userinfo = &authority[..i];
    if !valid_userinfo(raw_userinfo) {
        return Err(());
    }
    if let Some(colon) = raw_userinfo.iter().position(|&b| b == b':') {
        let username = unescape(&raw_userinfo[..colon], Mode::UserPassword)?;
        let password = unescape(&raw_userinfo[colon + 1..], Mode::UserPassword)?;
        Ok((
            Some(Userinfo {
                username,
                password,
                password_set: true,
            }),
            host,
        ))
    } else {
        let username = unescape(raw_userinfo, Mode::UserPassword)?;
        Ok((
            Some(Userinfo {
                username,
                password: Vec::new(),
                password_set: false,
            }),
            host,
        ))
    }
}

fn valid_userinfo(s: &[u8]) -> bool {
    s.iter().all(|&c| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                b'-' | b'.'
                    | b'_'
                    | b':'
                    | b'~'
                    | b'!'
                    | b'$'
                    | b'&'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
                    | b'%'
                    | b'@'
            )
    })
}

fn parse_host(scheme: &[u8], host: &[u8]) -> Result<Vec<u8>, ()> {
    if let Some(open) = host.iter().rposition(|&b| b == b'[') {
        if open > 0 {
            return Err(());
        }
        let close = host.iter().rposition(|&b| b == b']').ok_or(())?;
        let colon_port = &host[close + 1..];
        if !valid_optional_port(colon_port) {
            return Err(());
        }
        let unescaped_colon_port = unescape(colon_port, Mode::Host)?;
        let hostname = &host[1..close];
        let unescaped_hostname = match find_subslice(hostname, b"%25") {
            Some(zone_idx) => {
                let mut value = unescape(&hostname[..zone_idx], Mode::Host)?;
                value.extend(unescape(&hostname[zone_idx..], Mode::Zone)?);
                value
            }
            None => unescape(hostname, Mode::Host)?,
        };
        validate_ip_literal(&unescaped_hostname)?;

        let mut out = Vec::with_capacity(unescaped_hostname.len() + unescaped_colon_port.len() + 2);
        out.push(b'[');
        out.extend_from_slice(&unescaped_hostname);
        out.push(b']');
        out.extend_from_slice(&unescaped_colon_port);
        Ok(out)
    } else if let Some(first_colon) = host.iter().position(|&b| b == b':') {
        let colon = if scheme == b"http" || scheme == b"https" {
            first_colon
        } else {
            host.iter().rposition(|&b| b == b':').unwrap_or(first_colon)
        };
        if !valid_optional_port(&host[colon..]) {
            return Err(());
        }
        unescape(host, Mode::Host)
    } else {
        unescape(host, Mode::Host)
    }
}

fn validate_ip_literal(host: &[u8]) -> Result<(), ()> {
    let host = std::str::from_utf8(host).map_err(|_| ())?;
    let (address, zone) = match host.split_once('%') {
        Some((address, zone)) => (address, Some(zone)),
        None => (host, None),
    };
    if let Some(zone) = zone
        && zone.is_empty()
    {
        return Err(());
    }
    if address.parse::<std::net::Ipv4Addr>().is_ok() {
        return Err(());
    }
    address
        .parse::<std::net::Ipv6Addr>()
        .map(|_| ())
        .map_err(|_| ())
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn valid_optional_port(port: &[u8]) -> bool {
    match port.first() {
        None => true,
        Some(b':') => port[1..].iter().all(u8::is_ascii_digit),
        Some(_) => false,
    }
}

fn set_path(url: &mut Url, escaped: &[u8]) -> Result<(), ()> {
    let path = unescape(escaped, Mode::Path)?;
    let default_encoding = escape(&path, Mode::Path);
    url.raw_path = if escaped == default_encoding {
        Vec::new()
    } else {
        escaped.to_vec()
    };
    url.path = path;
    Ok(())
}

fn set_fragment(url: &mut Url, escaped: &[u8]) -> Result<(), ()> {
    let fragment = unescape(escaped, Mode::Fragment)?;
    let default_encoding = escape(&fragment, Mode::Fragment);
    url.raw_fragment = if escaped == default_encoding {
        Vec::new()
    } else {
        escaped.to_vec()
    };
    url.fragment = fragment;
    Ok(())
}

fn host_allows(c: u8) -> bool {
    matches!(
        c,
        b'!' | b'$'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b'+'
            | b','
            | b';'
            | b'='
            | b':'
            | b'['
            | b']'
            | b'<'
            | b'>'
            | b'"'
    )
}

fn should_escape(c: u8, mode: Mode) -> bool {
    if c.is_ascii_alphanumeric() {
        return false;
    }

    if matches!(mode, Mode::Host | Mode::Zone) && host_allows(c) {
        return false;
    }

    match c {
        b'-' | b'_' | b'.' | b'~' => false,
        b'$' | b'&' | b'+' | b',' | b'/' | b':' | b';' | b'=' | b'?' | b'@' => match mode {
            Mode::Path => c == b'?',
            Mode::UserPassword => matches!(c, b'@' | b'/' | b'?' | b':'),
            Mode::Query => true,
            Mode::Fragment => false,
            Mode::Host | Mode::Zone => true,
        },
        _ => {
            if mode == Mode::Fragment && matches!(c, b'!' | b'(' | b')' | b'*') {
                return false;
            }
            true
        }
    }
}

fn escape(s: &[u8], mode: Mode) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for &c in s {
        if mode == Mode::Query && c == b' ' {
            out.push(b'+');
        } else if should_escape(c, mode) {
            out.push(b'%');
            out.push(UPPER_HEX[usize::from(c >> 4)]);
            out.push(UPPER_HEX[usize::from(c & 0x0f)]);
        } else {
            out.push(c);
        }
    }
    out
}

fn unescape(s: &[u8], mode: Mode) -> Result<Vec<u8>, ()> {
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'%' => {
                if i + 2 >= s.len() || !is_hex(s[i + 1]) || !is_hex(s[i + 2]) {
                    return Err(());
                }
                let value = unhex(s[i + 1]) << 4 | unhex(s[i + 2]);
                // Host escapes may only introduce non-ASCII bytes; `%25` is
                // allowed for IPv6 zone identifiers.
                if mode == Mode::Host && unhex(s[i + 1]) < 8 && &s[i..i + 3] != b"%25" {
                    return Err(());
                }
                if mode == Mode::Zone
                    && &s[i..i + 3] != b"%25"
                    && value != b' '
                    && should_escape(value, Mode::Host)
                {
                    return Err(());
                }
                i += 3;
            }
            b'+' => i += 1,
            c => {
                if matches!(mode, Mode::Host | Mode::Zone) && c < 0x80 && should_escape(c, mode) {
                    return Err(());
                }
                i += 1;
            }
        }
    }

    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'%' => {
                out.push(unhex(s[i + 1]) << 4 | unhex(s[i + 2]));
                i += 3;
            }
            b'+' if mode == Mode::Query => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(out)
}

fn is_hex(c: u8) -> bool {
    c.is_ascii_hexdigit()
}

fn unhex(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

fn valid_encoded(s: &[u8], mode: Mode) -> bool {
    s.iter().all(|&c| {
        matches!(
            c,
            b'!' | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'@'
                | b'['
                | b']'
                | b'%'
        ) || !should_escape(c, mode)
    })
}

fn parse_query(raw: &[u8]) -> QueryValues {
    let mut values = QueryValues::new();
    let parameter_count = raw.iter().filter(|&&b| b == b'&').count() + 1;
    if parameter_count > 10_000 {
        return values;
    }

    for pair in raw.split(|&b| b == b'&') {
        if pair.is_empty() || pair.contains(&b';') {
            continue;
        }
        let (raw_key, raw_value) = match pair.iter().position(|&b| b == b'=') {
            Some(i) => (&pair[..i], &pair[i + 1..]),
            None => (pair, &[][..]),
        };
        let Ok(key) = unescape(raw_key, Mode::Query) else {
            continue;
        };
        let Ok(value) = unescape(raw_value, Mode::Query) else {
            continue;
        };
        values.entry(key).or_default().push(value);
    }
    values
}

pub(crate) fn encode_query(values: &QueryValues) -> Vec<u8> {
    let mut out = Vec::new();
    for (key, list) in values {
        let escaped_key = escape(key, Mode::Query);
        for value in list {
            if !out.is_empty() {
                out.push(b'&');
            }
            out.extend_from_slice(&escaped_key);
            out.push(b'=');
            out.extend_from_slice(&escape(value, Mode::Query));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dsn(raw: &str, token: &str) -> String {
        let Ok(mut url) = parse(raw) else {
            return raw.to_string();
        };
        let mut query = url.query();
        if query
            .get(b"authToken".as_slice())
            .and_then(|values| values.first())
            .is_none_or(Vec::is_empty)
        {
            query.insert(b"authToken".to_vec(), vec![token.as_bytes().to_vec()]);
        }
        url.set_raw_query(encode_query(&query));
        url.string()
    }

    #[test]
    fn dsn_matches_go_shapes() {
        assert_eq!(
            dsn("http://127.0.0.1:8880", "tok"),
            "http://127.0.0.1:8880?authToken=tok"
        );
        assert_eq!(
            dsn("https://shop.example:443/a", "tok"),
            "https://shop.example:443/a?authToken=tok"
        );
        assert_eq!(
            dsn("http://example.com/?authToken=existing&z=1", "tok"),
            "http://example.com/?authToken=existing&z=1"
        );
        assert_eq!(
            dsn("http://example.com/?authToken=&z=1", "tok"),
            "http://example.com/?authToken=tok&z=1"
        );
        assert_eq!(
            dsn("http://example.com/%FF", "tok"),
            "http://example.com/%FF?authToken=tok"
        );
        assert_eq!(dsn("http://[::1", "tok"), "http://[::1");
        assert_eq!(
            dsn("http://example.com/#frag", "tok"),
            "http://example.com/?authToken=tok#frag"
        );
        assert_eq!(dsn("*", "tok"), "*?authToken=tok");
        assert_eq!(dsn("", "tok"), "?authToken=tok");
    }
}
