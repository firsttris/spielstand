//! Two kinds of callers: the admin in the browser (session cookie) and agents (device token).

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::{
    extract::FromRequestParts,
    http::{Method, header, request::Parts},
};
use axum_extra::extract::CookieJar;
use rand::Rng;
use sha2::{Digest, Sha256};

use crate::{app::AppState, db, error::AppError};

pub const SESSION_COOKIE: &str = "spielstand_session";
/// Browsers can't add custom headers to cross-site requests without CORS, so this blocks CSRF.
pub const CSRF_HEADER: &str = "x-spielstand-csrf";
pub const SESSION_DAYS: i64 = 30;

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    // A random salt from the operating system
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("{e}"))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// The browser session of the one admin.
pub struct Admin;

impl FromRequestParts<AppState> for Admin {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let mutating = !matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS);
        if mutating && !parts.headers.contains_key(CSRF_HEADER) {
            return Err(AppError::Unauthorized);
        }
        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar
            .get(SESSION_COOKIE)
            .map(|c| c.value().to_string())
            .ok_or(AppError::Unauthorized)?;
        let valid: Option<(String,)> =
            sqlx::query_as("SELECT token_hash FROM sessions WHERE token_hash = ? AND expires_at > ?")
                .bind(token_hash(&token))
                .bind(db::now())
                .fetch_optional(&state.db)
                .await?;
        valid.map(|_| Admin).ok_or(AppError::Unauthorized)
    }
}

/// An agent, identified by its device token.
pub struct Agent {
    pub device_id: String,
    pub name: String,
}

pub async fn agent_from_token(state: &AppState, token: &str) -> Result<Agent, AppError> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT id, name FROM devices WHERE token_hash = ? AND removed_at IS NULL")
            .bind(token_hash(token))
            .fetch_optional(&state.db)
            .await?;
    let (device_id, name) = row.ok_or(AppError::Unauthorized)?;
    sqlx::query("UPDATE devices SET last_seen = ? WHERE id = ?")
        .bind(db::now())
        .bind(&device_id)
        .execute(&state.db)
        .await?;
    Ok(Agent { device_id, name })
}

impl FromRequestParts<AppState> for Agent {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(AppError::Unauthorized)?;
        agent_from_token(state, token).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_verify() {
        let hash = hash_password("hunter22").unwrap();
        assert!(verify_password("hunter22", &hash));
        assert!(!verify_password("wrong", &hash));
    }
}
