//! Response headers for the bundled renderer.
//!
//! `desktop.securityHeaders` wrapped the Wails asset server and set seven
//! headers on each response. Tauri exposes four of them declaratively under
//! `app.security.headers` and applies those in the custom-protocol response
//! builder. The remaining two (`X-Frame-Options` and `Referrer-Policy`) have no
//! config key in Tauri 2.9, so they are added by the documented
//! `on_web_resource_request` hook. That hook is only implemented for the
//! `tauri` URI protocol, which is the scheme used on Linux and macOS; the
//! declarative four still cover Windows.
//!
//! The CSP is configured in `tauri.conf.json` rather than overwritten here: on
//! HTML responses Tauri derives its header from that policy and appends the
//! nonces/hashes it needs for its own scripts. Replacing it would break the
//! webview bootstrap. A response that arrives without a CSP gets the Go
//! constant.

use std::borrow::Cow;

use tauri::http::{HeaderMap, HeaderName, HeaderValue, Request, Response};

/// `contentSecurityPolicy` from `desktop/main.go`, including the
/// `frame-ancestors 'none'` directive that a `<meta>` policy cannot express.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'none'; frame-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// The six non-CSP headers from `desktop.securityHeaders`.
pub const SECURITY_HEADERS: [(&str, &str); 6] = [
    ("x-content-type-options", "nosniff"),
    ("x-frame-options", "DENY"),
    ("referrer-policy", "no-referrer"),
    ("cross-origin-opener-policy", "same-origin"),
    ("cross-origin-resource-policy", "same-origin"),
    (
        "permissions-policy",
        "camera=(), microphone=(), geolocation=()",
    ),
];

/// Apply the Go host's headers to one custom-protocol response.
pub fn apply_security_headers(headers: &mut HeaderMap) {
    if !headers.contains_key("content-security-policy") {
        headers.insert(
            HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static(CONTENT_SECURITY_POLICY),
        );
    }
    for (name, value) in SECURITY_HEADERS {
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
}

/// `WebviewWindowBuilder::on_web_resource_request` adapter.
pub fn security_headers(_request: Request<Vec<u8>>, response: &mut Response<Cow<'static, [u8]>>) {
    apply_security_headers(response.headers_mut());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_go_header_is_set() {
        let mut headers = HeaderMap::new();
        apply_security_headers(&mut headers);

        assert_eq!(
            headers.get("content-security-policy").unwrap(),
            CONTENT_SECURITY_POLICY
        );
        for (name, value) in SECURITY_HEADERS {
            assert_eq!(headers.get(name).unwrap(), value, "{name}");
        }
        assert_eq!(headers.len(), SECURITY_HEADERS.len() + 1);
    }

    #[test]
    fn the_tauri_nonce_csp_is_not_replaced() {
        let tauri_csp = "default-src 'none'; script-src 'self' 'nonce-abc'";
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static(tauri_csp),
        );

        apply_security_headers(&mut headers);

        assert_eq!(headers.get("content-security-policy").unwrap(), tauri_csp);
    }

    #[test]
    fn the_web_resource_hook_sets_the_same_headers() {
        let mut response = Response::new(Cow::Owned(Vec::new()));
        security_headers(Request::new(Vec::new()), &mut response);

        for (name, value) in SECURITY_HEADERS {
            assert_eq!(response.headers().get(name).unwrap(), value, "{name}");
        }
        assert_eq!(
            response.headers().get("content-security-policy").unwrap(),
            CONTENT_SECURITY_POLICY
        );
    }

    #[test]
    fn tauri_config_matches_the_go_policy() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let security = &config["app"]["security"];

        assert_eq!(security["csp"].as_str(), Some(CONTENT_SECURITY_POLICY));
        assert!(CONTENT_SECURITY_POLICY.contains("frame-ancestors 'none'"));
        assert_eq!(config["version"].as_str(), Some(crate::DEFAULT_VERSION));
        assert_eq!(config["app"]["withGlobalTauri"], true);
        assert_eq!(config["build"]["frontendDist"], "../../desktop/frontend");
        assert_eq!(
            config["bundle"]["icon"][0],
            "../../desktop/build/appicon.png"
        );

        for (name, value) in [
            ("X-Content-Type-Options", "nosniff"),
            ("Cross-Origin-Opener-Policy", "same-origin"),
            ("Cross-Origin-Resource-Policy", "same-origin"),
            (
                "Permissions-Policy",
                "camera=(), microphone=(), geolocation=()",
            ),
        ] {
            assert_eq!(security["headers"][name].as_str(), Some(value), "{name}");
        }
    }
}
