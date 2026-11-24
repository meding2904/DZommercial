//! URL canonicalisation, host policy and API path validation.
//!
//! The parser follows `net/url` closely enough for the client's decisions:
//! schemes are lowercased, opaque URLs are rejected, the authority is split at
//! the last `@`, HTTP and HTTPS reject extra colons in the host, and percent
//! escapes are validated in the host, path, query and user info.

use std::net::IpAddr;

use crate::types::{ApiError, Settings};

pub const DEFAULT_API_URL: &str = "http://127.0.0.1:8080";
pub const FALLBACK_ORIGIN: &str = "http://localhost:3000";
pub const MAX_API_PATH_LENGTH: usize = 4096;

const INVALID_API_URL: &str = "The API address must be an absolute http or https URL.";
const INVALID_ORIGIN: &str = "The origin must be an absolute http or https URL.";

/// The parsed pieces of a URL that `net/url` exposes to this package.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedUrl {
    pub scheme: String,
    pub userinfo: Option<String>,
    /// Authority without user info, percent-decoded, including an optional port.
    pub host: String,
    /// Path exactly as written, before percent decoding.
    pub path: String,
    pub raw_query: String,
    pub force_query: bool,
    pub fragment: String,
    pub opaque: Option<String>,
}

/// A validated `/api/...` reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiPath {
    /// Percent-decoded path, matching `url.URL.Path`.
    pub path: String,
    pub raw_path: String,
    pub raw_query: String,
    pub has_query: bool,
}

/// Absolute target resolved against the configured API address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetUrl {
    pub url: String,
    pub generation: u64,
}

pub fn invalid_api_url() -> ApiError {
    ApiError::validation("invalid_api_url", INVALID_API_URL)
}

pub fn invalid_origin() -> ApiError {
    ApiError::validation("invalid_origin", INVALID_ORIGIN)
}

pub fn invalid_path() -> ApiError {
    ApiError::validation(
        "invalid_path",
        "The desktop client only calls the API namespace.",
    )
}

pub fn network_consent_error() -> ApiError {
    ApiError::validation(
        "network_consent_required",
        "Allow network access before connecting to a server outside this device.",
    )
}

/// Reject ASCII control bytes, including DEL, exactly like Go's `containsControl`.
pub fn contains_control(value: &str) -> bool {
    value.bytes().any(|byte| byte < 0x20 || byte == 0x7f)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn valid_optional_port(port: &str) -> bool {
    if port.is_empty() {
        return true;
    }
    match port.strip_prefix(':') {
        Some(digits) => digits.bytes().all(|byte| byte.is_ascii_digit()),
        None => false,
    }
}

fn is_host_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'-' | b'.'
                | b'_'
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
                | b':'
                | b'['
                | b']'
        )
}

/// Percent-decode a host component with `net/url`'s host restrictions.
fn decode_host(raw: &str) -> Result<String, ()> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            if index + 2 >= bytes.len() {
                return Err(());
            }
            let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            else {
                return Err(());
            };
            let decoded = (high << 4) | low;
            // go's unescape only permits host escapes for non-ASCII bytes,
            // plus the RFC 6874 zone separator "%25".
            if decoded < 0x80 && decoded != b'%' {
                return Err(());
            }
            out.push(decoded);
            index += 3;
            continue;
        }
        if byte < 0x80 && !is_host_char(byte) {
            return Err(());
        }
        out.push(byte);
        index += 1;
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn escape_host(host: &str) -> String {
    let mut out = String::with_capacity(host.len());
    for byte in host.bytes() {
        if byte < 0x80 && is_host_char(byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn percent_decode(raw: &str) -> Result<Vec<u8>, ()> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(());
            }
            let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            else {
                return Err(());
            };
            out.push((high << 4) | low);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    Ok(out)
}

fn validate_userinfo(raw: &str) -> Result<(), ()> {
    percent_decode(raw)?;
    Ok(())
}

fn parse_host(host: &str, scheme: &str) -> Result<String, ()> {
    if let Some(open) = host.rfind('[')
        && open > 0
    {
        return Err(());
    }
    if host.starts_with('[') {
        let close = host.rfind(']').ok_or(())?;
        let colon_port = &host[close + 1..];
        if !valid_optional_port(colon_port) {
            return Err(());
        }
        let decoded = decode_host(&host[1..close])?;
        let address = decoded.split('%').next().unwrap_or_default();
        match address.parse::<IpAddr>() {
            Ok(IpAddr::V4(_)) | Err(_) => return Err(()),
            Ok(IpAddr::V6(_)) => {}
        }
        let port = decode_host(colon_port)?;
        return Ok(format!("[{decoded}]{port}"));
    }

    if let Some(first) = host.find(':') {
        let last = host.rfind(':').unwrap_or(first);
        if last != first && (scheme == "http" || scheme == "https") {
            return Err(());
        }
        if !valid_optional_port(&host[last..]) {
            return Err(());
        }
    }
    decode_host(host)
}

fn split_scheme(raw: &str) -> Result<(&str, &str), ()> {
    let bytes = raw.as_bytes();
    for (index, &byte) in bytes.iter().enumerate() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' => {}
            b'0'..=b'9' | b'+' | b'-' | b'.' => {
                if index == 0 {
                    return Ok(("", raw));
                }
            }
            b':' => {
                if index == 0 {
                    return Err(());
                }
                return Ok((&raw[..index], &raw[index + 1..]));
            }
            _ => return Ok(("", raw)),
        }
    }
    Ok(("", raw))
}

/// URL syntax rejected by the Go-compatible parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid URL")]
pub struct UrlParseError;

/// Parse a URL with the decision-relevant subset of `net/url.Parse`.
pub fn parse_url(raw: &str) -> Result<ParsedUrl, UrlParseError> {
    parse_url_inner(raw).map_err(|()| UrlParseError)
}

fn parse_url_inner(raw: &str) -> Result<ParsedUrl, ()> {
    if contains_control(raw) {
        return Err(());
    }
    let (without_fragment, fragment) = match raw.split_once('#') {
        Some((head, tail)) => (head, tail),
        None => (raw, ""),
    };
    let (scheme, rest) = split_scheme(without_fragment)?;
    let scheme = scheme.to_ascii_lowercase();

    let (rest, raw_query, force_query) =
        if rest.ends_with('?') && rest.bytes().filter(|byte| *byte == b'?').count() == 1 {
            (&rest[..rest.len() - 1], "", true)
        } else if let Some((head, tail)) = rest.split_once('?') {
            (head, tail, false)
        } else {
            (rest, "", false)
        };

    let mut parsed = ParsedUrl {
        scheme,
        force_query,
        raw_query: raw_query.to_owned(),
        fragment: if fragment.is_empty() {
            String::new()
        } else {
            let decoded = percent_decode(fragment)?;
            String::from_utf8_lossy(&decoded).into_owned()
        },
        ..ParsedUrl::default()
    };

    if !rest.starts_with('/') {
        if !parsed.scheme.is_empty() {
            parsed.opaque = Some(rest.to_owned());
            return Ok(parsed);
        }
        let first_segment = rest.split('/').next().unwrap_or_default();
        if first_segment.contains(':') {
            return Err(());
        }
        percent_decode(rest)?;
        parsed.path = rest.to_owned();
        return Ok(parsed);
    }

    let authority_allowed = !parsed.scheme.is_empty() || !rest.starts_with("///");
    if authority_allowed && let Some(after) = rest.strip_prefix("//") {
        let (authority, remainder) = match after.find('/') {
            Some(index) => (&after[..index], &after[index..]),
            None => (after, ""),
        };
        let (userinfo, host) = match authority.rfind('@') {
            Some(index) => {
                validate_userinfo(&authority[..index])?;
                (Some(authority[..index].to_owned()), &authority[index + 1..])
            }
            None => (None, authority),
        };
        parsed.userinfo = userinfo;
        parsed.host = parse_host(host, &parsed.scheme)?;
        percent_decode(remainder)?;
        parsed.path = remainder.to_owned();
        return Ok(parsed);
    }

    percent_decode(rest)?;
    parsed.path = rest.to_owned();
    Ok(parsed)
}

/// `url.Hostname`: authority without the port and IPv6 brackets.
pub fn hostname(host: &str) -> String {
    let (name, _) = split_host_port(host);
    if name.starts_with('[') && name.ends_with(']') && name.len() >= 2 {
        name[1..name.len() - 1].to_owned()
    } else {
        name.to_owned()
    }
}

/// `url.Port`: the numeric port, or an empty string.
pub fn port(host: &str) -> String {
    split_host_port(host).1.to_owned()
}

fn split_host_port(host: &str) -> (&str, &str) {
    if let Some(colon) = host.rfind(':')
        && valid_optional_port(&host[colon..])
    {
        return (&host[..colon], &host[colon + 1..]);
    }
    (host, "")
}

/// Canonical scheme + host, with the default port removed.
pub fn canonical_origin(parsed: &ParsedUrl) -> String {
    let scheme = parsed.scheme.to_ascii_lowercase();
    if parsed.host.is_empty() {
        return if scheme.is_empty() {
            String::new()
        } else {
            format!("{scheme}:")
        };
    }
    let mut host = parsed.host.to_ascii_lowercase();
    let port = port(&host);
    if (scheme == "http" && port == "80") || (scheme == "https" && port == "443") {
        host.truncate(host.len() - port.len() - 1);
    }
    if host.is_empty() {
        return if scheme.is_empty() {
            String::new()
        } else {
            format!("{scheme}:")
        };
    }
    if scheme.is_empty() {
        format!("//{}", escape_host(&host))
    } else {
        format!("{scheme}://{}", escape_host(&host))
    }
}

pub fn normalize_api_url(raw: &str) -> Result<String, ApiError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(ApiError::validation(
            "invalid_api_url",
            "Enter the API address.",
        ));
    }
    if contains_control(raw) || raw.contains(['\\', '#']) {
        return Err(invalid_api_url());
    }
    let parsed = parse_url(raw).map_err(|_| invalid_api_url())?;
    if parsed.scheme.is_empty() || parsed.host.is_empty() || parsed.opaque.is_some() {
        return Err(invalid_api_url());
    }
    if parsed.scheme != "http" && parsed.scheme != "https" {
        return Err(ApiError::validation(
            "invalid_api_url",
            "The API address must use http or https.",
        ));
    }
    if parsed.userinfo.is_some() {
        return Err(ApiError::validation(
            "invalid_api_url",
            "The API address must not contain credentials.",
        ));
    }
    if hostname(&parsed.host).is_empty() || parsed.host.ends_with(':') {
        return Err(invalid_api_url());
    }
    if (!parsed.path.is_empty() && parsed.path != "/")
        || !parsed.raw_query.is_empty()
        || parsed.force_query
        || !parsed.fragment.is_empty()
    {
        return Err(ApiError::validation(
            "invalid_api_url",
            "Use the API root, without a path, query or fragment.",
        ));
    }
    Ok(canonical_origin(&parsed))
}

pub fn normalize_origin(raw: &str) -> Result<String, ApiError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(String::new());
    }
    if contains_control(raw) || raw.contains('\\') {
        return Err(invalid_origin());
    }
    let parsed = parse_url(raw).map_err(|_| invalid_origin())?;
    if parsed.scheme.is_empty() || parsed.host.is_empty() || parsed.opaque.is_some() {
        return Err(invalid_origin());
    }
    if parsed.scheme != "http" && parsed.scheme != "https" {
        return Err(ApiError::validation(
            "invalid_origin",
            "The origin must use http or https.",
        ));
    }
    if parsed.userinfo.is_some() {
        return Err(ApiError::validation(
            "invalid_origin",
            "The origin must not contain credentials.",
        ));
    }
    if hostname(&parsed.host).is_empty() || parsed.host.ends_with(':') {
        return Err(invalid_origin());
    }
    Ok(canonical_origin(&parsed))
}

pub fn validate_server_address(api_url: &str, allow_network: bool) -> Option<ApiError> {
    consent_host_for_update(&Settings::default(), api_url, allow_network).err()
}

/// The normalized host of an API URL, after the transport check.
pub fn api_host(api_url: &str) -> Result<String, ApiError> {
    let parsed = parse_url(api_url).map_err(|_| invalid_api_url())?;
    if hostname(&parsed.host).is_empty() {
        return Err(invalid_api_url());
    }
    validate_transport_address(&parsed)?;
    Ok(normalize_host(&hostname(&parsed.host)))
}

pub fn consent_host_for_update(
    previous: &Settings,
    api_url: &str,
    allow_network: bool,
) -> Result<String, ApiError> {
    let host = api_host(api_url)?;
    if is_loopback_host(&host) {
        return Ok(String::new());
    }
    if allow_network {
        return Ok(host);
    }
    if normalize_host(&previous.network_allowed_host).eq_ignore_ascii_case(&host) {
        return Ok(String::new());
    }
    Err(network_consent_error())
}

pub fn settings_consent_allowed(settings: &Settings) -> bool {
    let Ok(host) = api_host(&settings.api_url) else {
        return false;
    };
    is_loopback_host(&host)
        || host.eq_ignore_ascii_case(&normalize_host(&settings.network_allowed_host))
}

pub fn normalize_host(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

fn validate_transport_address(parsed: &ParsedUrl) -> Result<(), ApiError> {
    if parsed.scheme == "http" && !is_local_host(&hostname(&parsed.host)) {
        return Err(ApiError::validation(
            "invalid_api_url",
            "Use an https address for a server that is not on this device.",
        ));
    }
    Ok(())
}

pub fn is_loopback_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    let host = host.strip_suffix('.').unwrap_or(&host);
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    match parse_host_ip(host) {
        Some(IpAddr::V4(address)) => address.is_loopback(),
        Some(IpAddr::V6(address)) => match address.to_ipv4_mapped() {
            Some(mapped) => mapped.is_loopback(),
            None => address.is_loopback(),
        },
        None => false,
    }
}

pub fn is_local_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    let host = host.strip_suffix('.').unwrap_or(&host);
    if is_loopback_host(host)
        || host == "localhost"
        || host.ends_with(".local")
        || host.ends_with(".localhost")
    {
        return true;
    }
    let Some(address) = parse_host_ip(host) else {
        return false;
    };
    match address {
        IpAddr::V4(address) => {
            address.is_private()
                || address.is_loopback()
                || address.is_unspecified()
                || address.is_link_local()
                || is_ipv4_link_local_multicast(&address)
        }
        IpAddr::V6(address) => {
            let mapped = address.to_ipv4_mapped();
            if let Some(mapped) = mapped {
                return mapped.is_private()
                    || mapped.is_loopback()
                    || mapped.is_unspecified()
                    || mapped.is_link_local()
                    || is_ipv4_link_local_multicast(&mapped);
            }
            address.is_unique_local()
                || address.is_loopback()
                || address.is_unspecified()
                || address.is_unicast_link_local()
                || is_ipv6_link_local_multicast(&address)
        }
    }
}

fn is_ipv4_link_local_multicast(address: &std::net::Ipv4Addr) -> bool {
    let octets = address.octets();
    octets[0] == 224 && octets[1] == 0 && octets[2] == 0
}

fn is_ipv6_link_local_multicast(address: &std::net::Ipv6Addr) -> bool {
    let segments = address.segments();
    segments[0] == 0xff02
}

fn parse_host_ip(host: &str) -> Option<IpAddr> {
    let host = match host.find('%') {
        Some(index) if index > 0 && host[..index].contains(':') => &host[..index],
        _ => host,
    };
    host.parse().ok()
}

/// Validate a renderer-supplied API path.
pub fn validate_api_path(raw: &str) -> Result<ApiPath, ApiError> {
    if raw.is_empty()
        || raw.len() > MAX_API_PATH_LENGTH
        || contains_control(raw)
        || raw.contains('\\')
    {
        return Err(invalid_path());
    }
    if !raw.starts_with("/api/") || raw.starts_with("//") {
        return Err(invalid_path());
    }
    let (raw_path, raw_query) = match raw.split_once('?') {
        Some((path, query)) => (path, query),
        None => (raw, ""),
    };
    let lower_path = raw_path.to_ascii_lowercase();
    if raw.contains('#')
        || lower_path.contains("%2e")
        || lower_path.contains("%2f")
        || lower_path.contains("%25")
        || lower_path.contains("%5c")
    {
        return Err(invalid_path());
    }

    let decoded_path = percent_decode(raw_path).map_err(|()| invalid_path())?;
    if decoded_path
        .iter()
        .any(|byte| *byte < 0x20 || *byte == 0x7f)
    {
        return Err(invalid_path());
    }
    let decoded_path = String::from_utf8_lossy(&decoded_path).into_owned();
    if !decoded_path.starts_with("/api/") || decoded_path.contains('\\') {
        return Err(invalid_path());
    }
    if decoded_path
        .split('/')
        .any(|segment| segment == "." || segment == "..")
    {
        return Err(invalid_path());
    }

    let decoded_query = percent_decode(raw_query).map_err(|()| invalid_path())?;
    if decoded_query
        .iter()
        .any(|byte| *byte < 0x20 || *byte == 0x7f)
    {
        return Err(invalid_path());
    }

    Ok(ApiPath {
        path: decoded_path,
        raw_path: raw_path.to_owned(),
        raw_query: raw_query.to_owned(),
        has_query: raw.contains('?'),
    })
}

fn is_path_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'-' | b'.'
                | b'_'
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
                | b':'
                | b'@'
                | b'/'
                | b'['
                | b']'
        )
}

/// `url.URL.String` for the escaped path: keep the original escaping when it
/// is a valid path encoding, otherwise re-encode the decoded path.
fn escaped_path(raw_path: &str) -> String {
    let decoded = percent_decode(raw_path).unwrap_or_else(|()| raw_path.as_bytes().to_vec());
    let canonical = escape_path(&decoded);
    if raw_path == canonical {
        return raw_path.to_owned();
    }
    let valid = raw_path
        .bytes()
        .all(|byte| byte == b'%' || is_path_char(byte));
    if valid {
        raw_path.to_owned()
    } else {
        canonical
    }
}

fn escape_path(decoded: &[u8]) -> String {
    let mut out = String::with_capacity(decoded.len());
    for &byte in decoded {
        if byte < 0x80 && is_path_char(byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn clean_path(raw: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for segment in raw.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            segment => parts.push(segment),
        }
    }
    if parts.is_empty() {
        "/".to_owned()
    } else {
        format!("/{}", parts.join("/"))
    }
}

/// True when a path addresses the CSRF endpoint, including its aliases.
pub fn is_csrf_endpoint(raw: &str) -> bool {
    let Ok(path) = validate_api_path(raw) else {
        return false;
    };
    let cleaned = clean_path(&path.path);
    let rest = cleaned.strip_prefix("/api/").unwrap_or(&cleaned);
    let segment = rest.split('/').next().unwrap_or_default();
    let segment = match segment.find(';') {
        Some(index) => &segment[..index],
        None => segment,
    };
    segment
        .trim_end_matches(['.', ' '])
        .eq_ignore_ascii_case("csrf")
}

/// Resolve an API path against the configured address without a `Client`.
pub fn target_url_with(
    api_url: &str,
    allowed_host: &str,
    generation: u64,
    raw: &str,
) -> Result<TargetUrl, ApiError> {
    let relative = validate_api_path(raw)?;
    let base = parse_url(api_url).map_err(|_| invalid_api_url())?;
    if hostname(&base.host).is_empty() {
        return Err(invalid_api_url());
    }
    validate_transport_address(&base)?;
    let host = normalize_host(&hostname(&base.host));
    if !is_loopback_host(&host) && !host.eq_ignore_ascii_case(&normalize_host(allowed_host)) {
        return Err(network_consent_error());
    }
    if base.userinfo.is_some() {
        return Err(invalid_path());
    }

    let mut url = format!(
        "{}://{}{}",
        base.scheme,
        escape_host(&base.host),
        escaped_path(&relative.raw_path)
    );
    if relative.has_query {
        url.push('?');
        url.push_str(&relative.raw_query);
    }
    Ok(TargetUrl { url, generation })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_style_url_table() {
        let cases = [
            ("http://127.0.0.1:8080", Some("http://127.0.0.1:8080")),
            ("http://127.0.0.1:8080/", Some("http://127.0.0.1:8080")),
            ("http://[::1]:8080", Some("http://[::1]:8080")),
            ("http://localhost:8080", Some("http://localhost:8080")),
            ("http://192.168.1.10:8080", Some("http://192.168.1.10:8080")),
            (
                "http://[fe80::1%25eth0]:8080",
                Some("http://[fe80::1%25eth0]:8080"),
            ),
            ("https://api.example.com", Some("https://api.example.com")),
            (
                "https://api.example.com:443",
                Some("https://api.example.com"),
            ),
            ("http://api.example.com:80", Some("http://api.example.com")),
            ("//api.example.com", None),
            ("http:////api.example.com", None),
            ("http:example.com", None),
            ("https://user:pass@api.example.com", None),
            ("https://api.example.com/api", None),
            ("https://api.example.com?x=1", None),
            ("https://api.example.com#x", None),
            ("ftp://api.example.com", None),
            (
                "http://127.0.0.1.nip.io:8080",
                Some("http://127.0.0.1.nip.io:8080"),
            ),
            ("http://api.local:8080", Some("http://api.local:8080")),
            ("http://example.com:99999", Some("http://example.com:99999")),
        ];
        for (raw, want) in cases {
            let got = normalize_api_url(raw);
            match want {
                Some(want) => assert_eq!(got.as_deref().ok(), Some(want), "input {raw}"),
                None => assert!(got.is_err(), "input {raw} produced {got:?}"),
            }
        }
        assert!(validate_server_address("http://127.0.0.1.nip.io:8080", true).is_some());
        assert!(validate_server_address("http://api.example.com", true).is_some());
    }

    #[test]
    fn host_policy() {
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("api.localhost"));
        assert!(is_loopback_host("127.1.2.3"));
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("::ffff:127.0.0.1"));
        assert!(!is_loopback_host("192.168.1.1"));
        assert!(is_local_host("192.168.1.1"));
        assert!(is_local_host("10.20.30.40"));
        assert!(is_local_host("172.16.0.1"));
        assert!(!is_local_host("172.32.0.1"));
        assert!(is_local_host("169.254.1.1"));
        assert!(is_local_host("fe80::1"));
        assert!(is_local_host("fd00::1"));
        assert!(is_local_host("api.local"));
        assert!(!is_local_host("api.example.com"));
    }

    #[test]
    fn csrf_aliases() {
        for path in [
            "/api/csrf",
            "/api/csrf?x=1",
            "/api/csrf/",
            "/api/csrf;foo",
            "/api/csrf%3bfoo",
            "/api/csrf.",
            "/api/csrf%20",
            "/api/CSRF",
            "/api//csrf",
            "/api/%63srf",
        ] {
            assert!(is_csrf_endpoint(path), "{path}");
        }
        assert!(!is_csrf_endpoint("/api/csrfx"));
    }

    #[test]
    fn path_table() {
        for path in [
            "",
            "/api",
            "/api/../admin",
            "/api/%2e%2e/admin",
            "/api/%2E%2E/admin",
            "/api/..%2f..%2fetc",
            "/api/%2f%2fevil.com",
            "/api/%252e%252e/admin",
            "/api/x%5Cy",
            "/api/x\\y",
            "/api/x\u{0}y",
            "//evil.com/api/x",
            "http://evil.com/api/x",
            "/other/x",
        ] {
            assert!(validate_api_path(path).is_err(), "{path}");
        }
        let path = validate_api_path("/api/ok?q=%2e%2e%2f&page=2").expect("query path");
        assert_eq!(path.raw_query, "q=%2e%2e%2f&page=2");
    }
}
