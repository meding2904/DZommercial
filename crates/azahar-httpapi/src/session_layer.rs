//! Session middleware configuration and login helpers.
//!
//! [`session_layer`] builds the `tower-sessions` middleware with the cookie
//! contract the Go API used: `azahar_session`, `HttpOnly`, `SameSite=Lax`,
//! `Path=/`, and `Secure` controlled by configuration. Session records live in
//! libSQL through [`LibsqlSessionStore`].
//!
//! # Expiry
//!
//! Sessions use an absolute deadline, not a sliding inactivity window. The
//! middleware's default expiry is anchored when the layer is built; because a
//! process can run for days, handlers that sign a user in must call
//! [`login_with_ttl`] so the deadline is anchored at the login instead. The
//! role rule is in [`ttl_for_role`]: customers keep the configured TTL, staff
//! are capped at 12 hours.

use std::convert::Infallible;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use chrono::{Duration as ChronoDuration, Utc};
use time::OffsetDateTime;
use tower_sessions::SessionManagerLayer;
use tower_sessions::cookie::SameSite;
use tower_sessions::session::{Expiry, Session};

use crate::session_store::LibsqlSessionStore;

/// Cookie name used by both the Go service and the Rust port.
pub const SESSION_COOKIE_NAME: &str = "azahar_session";

/// Key under which the authenticated user's id is stored in the session.
pub const USER_ID_KEY: &str = "user_id";

/// Staff sessions never live longer than this, regardless of `SESSION_TTL_HOURS`.
pub const STAFF_SESSION_CAP_HOURS: i64 = 12;

/// Errors from reading or changing session state.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error(transparent)]
    Store(#[from] tower_sessions::session::Error),
}

/// The effective session lifetime for a role.
///
/// `customer` gets the configured TTL; every other role (`employee`, `admin`)
/// is staff and gets `min(ttl, 12h)`, matching `Config::AdminSessionTTL`. A
/// non-positive TTL is treated as zero so a misconfiguration cannot mint a
/// session that outlives the configuration.
pub fn ttl_for_role(role: &str, ttl: ChronoDuration) -> ChronoDuration {
    let ttl = ChronoDuration::seconds(ttl.num_seconds().max(0));
    if role == "customer" {
        ttl
    } else {
        ttl.min(ChronoDuration::hours(STAFF_SESSION_CAP_HOURS))
    }
}

/// A `tower-sessions` layer with the Azahar cookie and expiry contract.
///
/// `secure` should come from `COOKIE_SECURE` (or production). `ttl` is the
/// customer lifetime; staff handlers pass `ttl_for_role("admin" | "employee",
/// ttl)` to [`login_with_ttl`].
pub fn session_layer(
    store: LibsqlSessionStore,
    secure: bool,
    ttl: ChronoDuration,
) -> SessionManagerLayer<LibsqlSessionStore> {
    SessionManagerLayer::new(store)
        .with_name(SESSION_COOKIE_NAME)
        .with_http_only(true)
        .with_same_site(SameSite::Lax)
        .with_path("/")
        .with_secure(secure)
        .with_expiry(Expiry::AtDateTime(deadline(ttl)))
}

/// Reads the authenticated user id from the session, if there is one.
///
/// A store failure and an unauthenticated session both yield `None`; callers
/// that need to distinguish them should use [`Session::get`] directly.
pub async fn current_user_id(session: &Session) -> Option<String> {
    session
        .get::<String>(USER_ID_KEY)
        .await
        .ok()
        .flatten()
        .filter(|user_id| !user_id.is_empty())
}

/// Starts an authenticated session.
///
/// `cycle_id` replaces any pre-authentication session id, so a fixated cookie
/// cannot become an authenticated one. Existing session data is discarded
/// before the user id is inserted.
///
/// This uses the middleware's default expiry. Prefer [`login_with_ttl`] when
/// the login time should anchor the absolute deadline.
pub async fn login(session: &Session, user_id: &str) -> Result<(), SessionError> {
    session.cycle_id().await?;
    session.clear().await;
    session.insert(USER_ID_KEY, user_id).await?;
    Ok(())
}

/// Starts an authenticated session with an absolute deadline of `now + ttl`.
///
/// Handlers should pass `ttl_for_role(user.role, configured_ttl)`.
pub async fn login_with_ttl(
    session: &Session,
    user_id: &str,
    ttl: ChronoDuration,
) -> Result<(), SessionError> {
    login(session, user_id).await?;
    session.set_expiry(Some(Expiry::AtDateTime(deadline(ttl))));
    Ok(())
}

/// Ends the session: clears its data, deletes its row, and drops its id so the
/// middleware sends the cookie-removal header on the response.
pub async fn logout(session: &Session) -> Result<(), SessionError> {
    session.flush().await?;
    Ok(())
}

/// Extractor that yields the authenticated user id, or `None`.
///
/// The `Session` extractor itself only fails when the middleware is missing,
/// which is a wiring bug rather than a request error, so this extractor never
/// rejects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionUser(pub Option<String>);

impl SessionUser {
    pub fn id(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

impl<S> FromRequestParts<S> for SessionUser
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match Session::from_request_parts(parts, state).await {
            Ok(session) => Ok(Self(current_user_id(&session).await)),
            Err(_) => Ok(Self(None)),
        }
    }
}

fn deadline(ttl: ChronoDuration) -> OffsetDateTime {
    let seconds = ttl.num_seconds().max(0);
    let target = Utc::now()
        .checked_add_signed(ChronoDuration::seconds(seconds))
        .unwrap_or_else(Utc::now);
    OffsetDateTime::from_unix_timestamp(target.timestamp())
        .unwrap_or_else(|_| OffsetDateTime::now_utc())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn customer_keeps_the_configured_ttl() {
        assert_eq!(
            ttl_for_role("customer", ChronoDuration::hours(48)),
            ChronoDuration::hours(48)
        );
    }

    #[test]
    fn staff_is_capped_at_twelve_hours() {
        assert_eq!(
            ttl_for_role("admin", ChronoDuration::hours(48)),
            ChronoDuration::hours(12)
        );
        assert_eq!(
            ttl_for_role("employee", ChronoDuration::hours(2)),
            ChronoDuration::hours(2)
        );
    }

    #[test]
    fn negative_ttl_becomes_zero() {
        assert_eq!(
            ttl_for_role("customer", ChronoDuration::hours(-1)),
            ChronoDuration::zero()
        );
    }
}
