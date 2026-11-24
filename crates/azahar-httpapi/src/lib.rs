//! Port of `backend/internal/httpapi`: routing, middleware and handlers.

#![forbid(unsafe_code)]

pub mod cache;
pub mod captcha;
pub mod dto;
pub mod extract;
pub mod handlers_admin;
pub mod handlers_admin_products;
pub mod handlers_auth;
pub mod handlers_catalog;
pub mod handlers_checkout;
pub mod handlers_client;
pub mod handlers_framework;
pub mod handlers_images;
pub mod handlers_metrics;
pub mod handlers_orders;
pub mod handlers_plugins;
pub mod handlers_shipments;
pub mod handlers_shipping;
pub mod middleware;
pub mod ratelimit;
pub mod respond;
pub mod router;
pub mod server;
pub mod session_layer;
pub mod session_store;
pub mod state;
pub mod turnstile;
pub mod validate;

pub use dto::{
    AdminPluginDto, CategoryDto, CustomerDto, OrderDto, OrderItemDto, ProductDto, PublicPluginDto,
    TeamMemberDto, UserDto, UserEnvelope, to_order, to_product, to_products, to_user,
};
pub use extract::{BODY_LIMIT, JsonBody, RawFields, client_ip, decode_json};
pub use middleware::{CurrentUser, RequestId};
pub use respond::{ApiProblem, problem, problem_fields};
pub use router::{build_router, build_router_with_extra};
pub use server::{Server, session_layer_for};
pub use session_layer::{
    SESSION_COOKIE_NAME, STAFF_SESSION_CAP_HOURS, SessionError, SessionUser, USER_ID_KEY,
    current_user_id, login, login_with_ttl, logout, session_layer, ttl_for_role,
};
pub use session_store::{LibsqlSessionStore, SESSION_TABLE};
pub use state::AppState;
