//! The Wails compatibility shim injected before the renderer runs.

/// `window.go.main.App` method -> Tauri command name, in `bridge.js` order.
pub const WAILS_METHODS: [(&str, &str); 8] = [
    ("Info", "info"),
    ("Health", "health"),
    ("Request", "request"),
    ("Image", "image"),
    ("UpdateSettings", "update_settings"),
    ("ClearSession", "clear_session"),
    ("FirewallGuide", "firewall_guide"),
    ("OpenExternal", "open_external"),
];

/// Initialization script installed with `withGlobalTauri: true` and passed to
/// `WebviewWindowBuilder::initialization_script`.
pub const BRIDGE_SCRIPT: &str = include_str!("bridge.js");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_wails_method_reaches_a_tauri_command() {
        assert_eq!(WAILS_METHODS.len(), 8);
        for (method, command) in WAILS_METHODS {
            assert!(
                BRIDGE_SCRIPT.contains(&format!("{method}: function")),
                "bridge.js is missing {method}"
            );
            assert!(
                BRIDGE_SCRIPT.contains(&format!("'{command}'")),
                "bridge.js does not invoke {command}"
            );
        }
    }

    #[test]
    fn positional_arguments_become_camel_case_command_args() {
        assert!(BRIDGE_SCRIPT.contains("method: text(method)"));
        assert!(BRIDGE_SCRIPT.contains("path: text(path)"));
        assert!(BRIDGE_SCRIPT.contains("body: body == null ? {} : body"));
        assert!(BRIDGE_SCRIPT.contains("apiUrl: text(apiUrl)"));
        assert!(BRIDGE_SCRIPT.contains("origin: text(origin)"));
        assert!(BRIDGE_SCRIPT.contains("allowNetwork: Boolean(allowNetwork)"));
        assert!(BRIDGE_SCRIPT.contains("url: text(url)"));
    }

    #[test]
    fn the_shim_uses_the_tauri_global_api() {
        assert!(BRIDGE_SCRIPT.contains("window.__TAURI__.core"));
        assert!(BRIDGE_SCRIPT.contains("window.go = { main: { App: app } };"));
        assert!(BRIDGE_SCRIPT.contains("throw new Error(message)"));
    }
}
