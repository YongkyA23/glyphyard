//! Install key + install tokens for desktops, password + session cookie for admins.

use crate::{db, ApiError, AppState};
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use axum::extract::{FromRequestParts, Query};
use axum::http::header::{AUTHORIZATION, COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use typeshelf_protocol::INSTALL_KEY_HEADER;

pub const SESSION_COOKIE: &str = "ts_session";
pub const SESSION_DAYS: i64 = 30;

pub const LOGIN_MAX_FAILURES: u32 = 5;
pub const LOGIN_WINDOW: Duration = Duration::from_secs(15 * 60);
const LOGIN_MAX_TRACKED: usize = 10_000;

/// Failed admin sign-ins per email (trimmed, lowercased): `(failures, first failure in the window)`.
#[derive(Default)]
pub struct LoginThrottle(Mutex<HashMap<String, (u32, Instant)>>);

impl LoginThrottle {
    fn key(email: &str) -> String {
        email.trim().to_lowercase()
    }

    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, (u32, Instant)>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// How long this email stays locked, if it is.
    pub fn locked_for(&self, email: &str, now: Instant) -> Option<Duration> {
        let (n, start) = *self.map().get(&Self::key(email))?;
        let left = (start + LOGIN_WINDOW).checked_duration_since(now).filter(|d| !d.is_zero())?;
        (n >= LOGIN_MAX_FAILURES).then_some(left)
    }

    pub fn failed(&self, email: &str, now: Instant) {
        let mut map = self.map();
        let live = |start: &Instant| now.duration_since(*start) < LOGIN_WINDOW;
        let key = Self::key(email);
        if let Some((n, _)) = map.get_mut(&key).filter(|(_, start)| live(start)) {
            *n += 1;
            return;
        }
        map.retain(|_, (_, start)| live(start));
        // ponytail: at the cap a flood of distinct emails stops new emails being tracked; add a per-IP limit if that matters.
        if map.len() < LOGIN_MAX_TRACKED || map.contains_key(&key) {
            map.insert(key, (1, now));
        }
    }

    pub fn clear(&self, email: &str) {
        self.map().remove(&Self::key(email));
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

pub fn random_hex(len: usize) -> String {
    let bytes: Vec<u8> = (0..len).map(|_| rand::random()).collect();
    hex(&bytes)
}

/// Tokens are stored only as their SHA-256.
pub fn token_hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

pub fn hash_password(password: &str) -> String {
    Argon2::default().hash_password(password.as_bytes()).expect("argon2 hashing").to_string()
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
}

fn query(parts: &Parts) -> HashMap<String, String> {
    Query::<HashMap<String, String>>::try_from_uri(&parts.uri).map(|q| q.0).unwrap_or_default()
}

/// Header `X-Install-Key` (or `?key=`) matches the server's install key.
pub struct InstallKey;

impl FromRequestParts<AppState> for InstallKey {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let given = match parts.headers.get(INSTALL_KEY_HEADER).and_then(|v| v.to_str().ok()) {
            Some(k) => k.to_string(),
            None => query(parts).remove("key").unwrap_or_default(),
        };
        let expected = db::settings(&state.db())?.install_key;
        if given.is_empty() || given != expected {
            return Err(ApiError::new(StatusCode::FORBIDDEN, "bad_install_key", "the install key is not valid for this server"));
        }
        Ok(InstallKey)
    }
}

/// A registered desktop: install key + `Authorization: Bearer <token>` (or `?token=`). Updates `last_seen`.
pub struct Install {
    pub id: String,
}

impl FromRequestParts<AppState> for Install {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        InstallKey::from_request_parts(parts, state).await?;
        let token = match parts.headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()) {
            Some(h) => h.strip_prefix("Bearer ").unwrap_or_default().trim().to_string(),
            None => query(parts).remove("token").unwrap_or_default(),
        };
        let db = state.db();
        let id: Option<String> = db
            .query_row("SELECT id FROM installs WHERE token_hash = ?1", [token_hash(&token)], |r| r.get(0))
            .optional()?;
        let id = id.ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "bad_token", "unknown install token"))?;
        db.execute("UPDATE installs SET last_seen = ?1 WHERE id = ?2", [db::now(), id.clone()])?;
        Ok(Install { id })
    }
}

/// A signed-in admin (session cookie).
pub struct Admin {
    pub id: i64,
    pub email: String,
    pub role: String,
}

pub fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|c| c.trim().strip_prefix(SESSION_COOKIE)?.strip_prefix('=').map(str::to_string))
}

impl FromRequestParts<AppState> for Admin {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let unauthorized = || ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "sign in first");
        let token = session_token(&parts.headers).ok_or_else(unauthorized)?;
        let admin = state
            .db()
            .query_row(
                "SELECT a.id, a.email, a.role FROM sessions s JOIN admins a ON a.id = s.admin_id
                 WHERE s.token_hash = ?1 AND s.expires_at > ?2",
                [token_hash(&token), db::now()],
                |r| Ok(Admin { id: r.get(0)?, email: r.get(1)?, role: r.get(2)? }),
            )
            .optional()?;
        admin.ok_or_else(unauthorized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_round_trip() {
        let h = hash_password("correct horse");
        assert!(verify_password("correct horse", &h));
        assert!(!verify_password("wrong", &h));
        assert_eq!(token_hash("abc").len(), 64);
    }

    #[test]
    fn login_lock_ends_with_the_window() {
        let t = LoginThrottle::default();
        let start = Instant::now();
        for _ in 0..LOGIN_MAX_FAILURES {
            assert_eq!(t.locked_for("a@x", start), None);
            t.failed("A@x ", start);
        }
        assert!(t.locked_for("a@x", start + Duration::from_secs(60)).is_some());
        assert_eq!(t.locked_for("a@x", start + LOGIN_WINDOW), None);
        // A failure after the window starts a new count.
        t.failed("a@x", start + LOGIN_WINDOW);
        assert_eq!(t.locked_for("a@x", start + LOGIN_WINDOW), None);
        assert_eq!(t.map().len(), 1);
    }
}
