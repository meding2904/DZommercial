//! Password hashing and bearer tokens.
//!
//! Mirrors the Go package `azahar/internal/auth`.

#![forbid(unsafe_code)]

mod password;
mod token;

pub use password::{
    HashPasswordError, InvalidHash, dummy_hash, hash_password, verify_dummy, verify_password,
};
pub use token::{TOKEN_BYTES, TokenError, hash_token, new_token};
