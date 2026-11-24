//! Router assembly and the Go middleware order.
//!
//! `build_router` mirrors `Server.routes` for the routes this crate owns. The
//! layers are added innermost-first: the last call becomes the outermost
//! middleware, so requests flow through security headers, request id, body
//! limit, the general limiter, CSRF, the read-cache invalidation and finally
//! the session layer before a handler runs.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::middleware::{from_fn, from_fn_with_state};
use axum::routing::{get, patch, post, put};
use tower_sessions::SessionManagerLayer;

use crate::cache::{
    CacheHandle, CacheState, READ_CACHE_MAX_ENTRIES, ReadCache, cache_read,
    invalidate_read_cache_on_write,
};
use crate::extract::BODY_LIMIT;
use crate::handlers_admin;
use crate::handlers_admin_products;
use crate::handlers_auth;
use crate::handlers_catalog;
use crate::handlers_client;
use crate::handlers_framework;
use crate::handlers_images;
use crate::handlers_metrics;
use crate::handlers_orders;
use crate::handlers_shipping;
use crate::middleware;
use crate::respond::{ApiProblem, problem};
use crate::session_store::LibsqlSessionStore;
use crate::state::AppState;

const CATEGORIES_TTL: Duration = Duration::from_secs(5);
const PRODUCTS_TTL: Duration = Duration::from_secs(2);
const PLUGINS_TTL: Duration = Duration::from_secs(5);
const THEME_TTL: Duration = Duration::from_secs(2);
const ADMIN_METRICS_TTL: Duration = Duration::from_secs(3);
const ADMIN_PRODUCTS_TTL: Duration = Duration::from_secs(1);
const ADMIN_ORDERS_TTL: Duration = Duration::from_secs(1);
const ADMIN_CUSTOMERS_TTL: Duration = Duration::from_secs(5);

/// Builds the complete API router.
pub fn build_router(state: AppState, sessions: SessionManagerLayer<LibsqlSessionStore>) -> Router {
    let cache = Arc::new(ReadCache::new(READ_CACHE_MAX_ENTRIES));

    let credential_routes = Router::new()
        .route("/api/auth/register", post(handlers_auth::register))
        .route("/api/auth/login", post(handlers_auth::login))
        .route_layer(from_fn_with_state(
            state.clone(),
            middleware::auth_rate_limit,
        ));

    let signed_in_routes = Router::new()
        .route("/api/auth/me", get(handlers_auth::me))
        .route("/api/account/orders", get(handlers_orders::account_orders))
        .route_layer(from_fn_with_state(state.clone(), middleware::require_user));

    let admin_only = Router::new()
        .route("/api/admin/products", post(handlers_admin_products::create))
        .route(
            "/api/admin/products/{id}",
            patch(handlers_admin_products::update).delete(handlers_admin_products::archive),
        )
        .route(
            "/api/admin/storefront/theme",
            get(handlers_framework::admin_storefront_theme)
                .put(handlers_framework::put_storefront_theme),
        )
        .route("/api/admin/team", get(handlers_admin::team))
        .route(
            "/api/admin/team/{id}",
            patch(handlers_admin::update_team_role),
        )
        .route("/api/admin/plugins", get(handlers_framework::admin_plugins))
        .route(
            "/api/admin/plugins/{pluginID}/state",
            put(handlers_framework::plugin_state),
        )
        .route(
            "/api/admin/plugins/{pluginID}/settings",
            get(handlers_framework::get_plugin_settings)
                .put(handlers_framework::put_plugin_settings),
        )
        .route(
            "/api/admin/product-images",
            post(handlers_images::upload).layer(DefaultBodyLimit::max(
                handlers_images::MAX_UPLOAD_BODY_BYTES,
            )),
        )
        .route_layer(from_fn(middleware::require_admin));

    let admin_routes = Router::new()
        .route(
            "/api/admin/metrics",
            get(handlers_metrics::metrics).layer(from_fn_with_state(
                CacheState::new(cache.clone(), ADMIN_METRICS_TTL),
                cache_read,
            )),
        )
        .route("/api/admin/sales", post(handlers_admin::counter_sale))
        .route(
            "/api/admin/products",
            get(handlers_admin_products::list).layer(from_fn_with_state(
                CacheState::new(cache.clone(), ADMIN_PRODUCTS_TTL),
                cache_read,
            )),
        )
        .route(
            "/api/admin/orders",
            get(handlers_admin::orders).layer(from_fn_with_state(
                CacheState::new(cache.clone(), ADMIN_ORDERS_TTL),
                cache_read,
            )),
        )
        .route(
            "/api/admin/orders/{id}/status",
            patch(handlers_admin::order_status),
        )
        .route(
            "/api/admin/customers",
            get(handlers_admin::customers).layer(from_fn_with_state(
                CacheState::new(cache.clone(), ADMIN_CUSTOMERS_TTL),
                cache_read,
            )),
        )
        .route(
            "/api/admin/preferences",
            get(handlers_admin::get_preferences).put(handlers_admin::put_preferences),
        )
        .merge(admin_only)
        .route_layer(from_fn_with_state(
            state.clone(),
            middleware::admin_mutation_limit,
        ))
        .route_layer(from_fn(middleware::require_staff))
        .route_layer(from_fn_with_state(state.clone(), middleware::require_user));

    Router::new()
        .route("/api/health", get(handlers_client::health))
        .route("/api/csrf", get(handlers_client::csrf))
        .route("/api/auth/logout", post(handlers_auth::logout))
        .route(
            "/api/categories",
            get(handlers_catalog::categories).layer(from_fn_with_state(
                CacheState::new(cache.clone(), CATEGORIES_TTL),
                cache_read,
            )),
        )
        .route(
            "/api/products",
            get(handlers_catalog::products).layer(from_fn_with_state(
                CacheState::new(cache.clone(), PRODUCTS_TTL),
                cache_read,
            )),
        )
        .route(
            "/api/products/{slug}",
            get(handlers_catalog::product).layer(from_fn_with_state(
                CacheState::new(cache.clone(), PRODUCTS_TTL),
                cache_read,
            )),
        )
        .route(
            "/api/plugins",
            get(handlers_catalog::plugins).layer(from_fn_with_state(
                CacheState::new(cache.clone(), PLUGINS_TTL),
                cache_read,
            )),
        )
        .route(
            "/api/storefront/theme",
            get(handlers_catalog::storefront_theme).layer(from_fn_with_state(
                CacheState::new(cache.clone(), THEME_TTL),
                cache_read,
            )),
        )
        .route("/api/images/{id}", get(handlers_images::by_id))
        .route("/api/shipping/wilayas", get(handlers_shipping::wilayas))
        .route("/api/shipping/communes", get(handlers_shipping::communes))
        .route("/api/shipping/desks", get(handlers_shipping::desks))
        .route("/api/orders/{token}", get(handlers_orders::by_token))
        .merge(credential_routes)
        .merge(signed_in_routes)
        .merge(admin_routes)
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .with_state(state.clone())
        .layer(sessions)
        .layer(from_fn_with_state(
            CacheHandle::new(cache),
            invalidate_read_cache_on_write,
        ))
        .layer(from_fn_with_state(state.clone(), middleware::csrf))
        .layer(from_fn_with_state(
            state.clone(),
            middleware::general_rate_limit,
        ))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(from_fn(middleware::request_id))
        .layer(from_fn_with_state(state, middleware::security_headers))
}

async fn not_found() -> ApiProblem {
    problem(404, "not_found", "Not found.")
}

async fn method_not_allowed() -> ApiProblem {
    problem(405, "method_not_allowed", "Method not allowed.")
}
