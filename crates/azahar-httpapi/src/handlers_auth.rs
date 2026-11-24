//! Authentication routes: register, login, logout and the session probe.

use std::collections::HashMap;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use azahar_auth::{hash_password, verify_dummy, verify_password};
use azahar_ids::new as new_id;
use azahar_store::{StoreError, User};
use chrono::Utc;
use serde::Deserialize;
use tower_sessions::Session;

use crate::dto::UserEnvelope;
use crate::extract::JsonBody;
use crate::middleware::{CurrentUser, internal_error};
use crate::respond::{ApiProblem, json_response, problem, problem_fields};
use crate::session_layer::{login_with_ttl, logout as end_session, ttl_for_role};
use crate::state::AppState;
use crate::validate::{fields, rune_len, valid_email};

/// `POST /api/auth/register` body.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RegisterPayload {
    pub name: Option<String>,
    pub email: Option<String>,
    pub password: Option<String>,
}

/// `POST /api/auth/login` body.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoginPayload {
    pub email: Option<String>,
    pub password: Option<String>,
}

/// `POST /api/auth/register`.
pub async fn register(
    State(state): State<AppState>,
    session: Session,
    body: JsonBody<RegisterPayload>,
) -> Result<Response, ApiProblem> {
    let payload = body.payload;
    let name = payload.name.unwrap_or_default().trim().to_string();
    let email = go_to_lower(payload.email.unwrap_or_default().trim());
    let password = payload.password.unwrap_or_default();

    let mut problems = Vec::new();
    if !(2..=80).contains(&rune_len(&name)) {
        problems.push(("name", "Use between 2 and 80 characters."));
    }
    if !valid_email(&email) {
        problems.push(("email", "Enter a valid email."));
    }
    if !(10..=200).contains(&rune_len(&password)) {
        problems.push(("password", "Use between 10 and 200 characters."));
    }
    if !problems.is_empty() {
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            fields(&problems),
        ));
    }

    match state.store.user_by_email(&email).await {
        Ok(_) => return Err(duplicate_email()),
        Err(err) if err.is_not_found() => {}
        Err(_) => return Err(internal_error()),
    }

    let hash = tokio::task::spawn_blocking(move || hash_password(&password))
        .await
        .map_err(|_| internal_error())?
        .map_err(|_| internal_error())?;

    let now = Utc::now();
    let user = User {
        id: new_id("usr"),
        name,
        email,
        password_hash: hash,
        role: "customer".to_string(),
        theme_preset: "cellar".to_string(),
        created_at: Some(now),
        updated_at: Some(now),
    };

    let transaction = state
        .store
        .begin_write()
        .await
        .map_err(|_| internal_error())?;
    if let Err(err) = state.store.create_user(transaction.conn(), &user).await {
        return Err(if err.is_unique_violation() {
            duplicate_email()
        } else {
            internal_error()
        });
    }
    transaction.commit().await.map_err(|_| internal_error())?;

    login_with_ttl(
        &session,
        &user.id,
        ttl_for_role(&user.role, state.session_ttl()),
    )
    .await
    .map_err(|error| {
        tracing::error!(error = %error, "login session write failed");
        internal_error()
    })?;

    Ok(json_response(201, &UserEnvelope::from(&user)))
}

/// `POST /api/auth/login`.
pub async fn login(
    State(state): State<AppState>,
    session: Session,
    body: JsonBody<LoginPayload>,
) -> Result<Response, ApiProblem> {
    let payload = body.payload;
    let email = go_to_lower(payload.email.unwrap_or_default().trim());
    let password = payload.password.unwrap_or_default();

    let mut problems = Vec::new();
    if !valid_email(&email) {
        problems.push(("email", "Enter a valid email."));
    }
    if password.is_empty() || rune_len(&password) > 200 {
        problems.push(("password", "Enter your password."));
    }
    if !problems.is_empty() {
        return Err(problem_fields(
            422,
            "validation_error",
            "Check the marked fields.",
            fields(&problems),
        ));
    }

    let user = match state.store.user_by_email(&email).await {
        Ok(user) => user,
        Err(StoreError::NotFound) => {
            // Spend the same hashing time as a real account, then fail
            // generically so the response cannot enumerate emails.
            tokio::task::spawn_blocking(move || verify_dummy(&password))
                .await
                .map_err(|_| internal_error())?;
            return Err(bad_credentials());
        }
        Err(_) => return Err(internal_error()),
    };

    let password_hash = user.password_hash.clone();
    let verified = tokio::task::spawn_blocking(move || verify_password(&password, &password_hash))
        .await
        .map_err(|_| internal_error())?;
    if !verified {
        return Err(bad_credentials());
    }

    login_with_ttl(
        &session,
        &user.id,
        ttl_for_role(&user.role, state.session_ttl()),
    )
    .await
    .map_err(|error| {
        tracing::error!(error = %error, "login session write failed");
        internal_error()
    })?;

    Ok(json_response(200, &UserEnvelope::from(&user)))
}

/// `POST /api/auth/logout`: flushes the session row and lets the middleware
/// emit the cookie-removal header.
pub async fn logout(session: Session) -> Result<Response, ApiProblem> {
    end_session(&session).await.map_err(|error| {
        tracing::error!(error = %error, "logout session delete failed");
        internal_error()
    })?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `GET /api/auth/me`, mounted behind [`crate::middleware::require_user`].
pub async fn me(CurrentUser(user): CurrentUser) -> Response {
    json_response(200, &UserEnvelope::from(&user))
}

fn duplicate_email() -> ApiProblem {
    problem_fields(
        409,
        "conflict",
        "An account with this email already exists.",
        HashMap::from([(
            "email".to_string(),
            "This email is already registered.".to_string(),
        )]),
    )
}

fn bad_credentials() -> ApiProblem {
    problem(401, "unauthorized", "Email or password is incorrect.")
}

/// Go's `strings.ToLower` applies the one-rune simple mapping.
fn go_to_lower(value: &str) -> String {
    value
        .chars()
        .map(|character| character.to_lowercase().next().unwrap_or(character))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_normalisation_matches_go() {
        assert_eq!(go_to_lower("  Amina@Example.COM "), "  amina@example.com ");
        assert_eq!(go_to_lower("ADMİN@example.com"), "admin@example.com");
    }

    #[test]
    fn duplicate_email_problem_shape() {
        let problem = duplicate_email();
        assert_eq!(problem.status, StatusCode::CONFLICT);
        assert_eq!(problem.code, "conflict");
        assert_eq!(
            problem
                .fields
                .as_ref()
                .and_then(|fields| fields.get("email")),
            Some(&"This email is already registered.".to_string())
        );
    }

    #[test]
    fn credentials_error_is_generic() {
        let problem = bad_credentials();
        assert_eq!(problem.status, StatusCode::UNAUTHORIZED);
        assert_eq!(problem.message, "Email or password is incorrect.");
    }
}
