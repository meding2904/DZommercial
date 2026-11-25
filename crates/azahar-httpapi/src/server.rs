//! Listener and serve helpers for the `azahar-api` binary.

use std::future::Future;
use std::net::SocketAddr;

use axum::Router;
use tokio::net::TcpListener;
use tower_sessions::SessionManagerLayer;

use crate::router::build_router;
use crate::session_layer::session_layer;
use crate::session_store::LibsqlSessionStore;
use crate::state::{AppState, configured_ttl};

/// The session layer configured from [`AppState`].
///
/// Handlers still call `login_with_ttl`, which re-anchors each session's
/// absolute deadline at login time; this layer value is only the default for
/// sessions that never sign in.
#[must_use]
pub fn session_layer_for(state: &AppState) -> SessionManagerLayer<LibsqlSessionStore> {
    session_layer(
        LibsqlSessionStore::new(state.store.conn().clone()),
        state.cfg.cookie_secure,
        configured_ttl(&state.cfg),
    )
}

/// A bound API server. `run` owns the listener, so `azahar-api` can log the
/// actual address before serving.
pub struct Server {
    listener: TcpListener,
    app: Router,
    state: AppState,
}

impl Server {
    /// Binds `API_ADDR`. Go accepts `:8080`; Rust needs an explicit host, so a
    /// leading colon is expanded to all IPv4 interfaces.
    pub async fn bind(
        address: &str,
        state: AppState,
        plugin_routes: Router<AppState>,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind(normalize_bind_address(address)).await?;
        Ok(Self::with_listener(listener, state, plugin_routes))
    }

    /// Builds the app on an already-bound listener.
    #[must_use]
    pub fn with_listener(
        listener: TcpListener,
        state: AppState,
        plugin_routes: Router<AppState>,
    ) -> Self {
        let sessions = session_layer_for(&state);
        let app = build_router(state.clone(), sessions, plugin_routes);
        Self {
            listener,
            app,
            state,
        }
    }

    /// Go's `Server.Close`: stops the captcha sweep and lets the limiters
    /// release their state. Axum closes the listener when the server drops.
    pub fn close(&self) {
        shutdown_state(&self.state);
    }

    /// The address the OS assigned, useful with port `0` in tests.
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Serves until the process is stopped.
    pub async fn run(self) -> std::io::Result<()> {
        let state = self.state.clone();
        let result = axum::serve(
            self.listener,
            self.app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
        shutdown_state(&state);
        result
    }

    /// Serves until `signal` resolves, then drains in-flight requests.
    pub async fn run_with_shutdown<F>(self, signal: F) -> std::io::Result<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let state = self.state.clone();
        let result = axum::serve(
            self.listener,
            self.app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(signal)
        .await;
        shutdown_state(&state);
        result
    }
}

fn normalize_bind_address(address: &str) -> String {
    if address.starts_with(':') {
        format!("0.0.0.0{address}")
    } else {
        address.to_string()
    }
}

fn shutdown_state(state: &AppState) {
    state.captcha.close();
    state.general_limiter.close();
    state.auth_limiter.close();
    state.checkout_limiter.close();
    state.captcha_limiter.close();
    state.admin_limiter.close();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_style_port_only_addresses_get_a_host() {
        assert_eq!(normalize_bind_address(":8080"), "0.0.0.0:8080");
        assert_eq!(normalize_bind_address("127.0.0.1:8080"), "127.0.0.1:8080");
        assert_eq!(normalize_bind_address("[::1]:8080"), "[::1]:8080");
    }
}
