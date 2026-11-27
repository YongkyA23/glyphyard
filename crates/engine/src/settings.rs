//! App settings plus the identity/connection values the sync crate reads and writes.

use crate::{store, AppSettings, AppState, Connection, ConnectionStatus, Engine, EngineError, Platform, Result};
use argon2::password_hash::{phc::PasswordHash, PasswordVerifier};
use argon2::Argon2;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use typeshelf_protocol::Policy;

fn default_policy() -> Policy {
    Policy { name_change_days: 30, allow_connection_edit: true, notify_admin_changes: true, brand_name: String::new(), brand_color: String::new(), brand_logo: String::new(), connection_password_hash: String::new() }
}

const THROTTLE_KEY: &str = "unlock_throttle";
const MAX_FAILURES: u32 = 5;
const LOCK_MINUTES: i64 = 5;

#[derive(Default, Serialize, Deserialize)]
struct Throttle {
    failures: u32,
    locked_until: Option<DateTime<Utc>>,
}

impl Engine {
    /// Checks the admin's unlock password against the policy hash. Five wrong passwords in a row
    /// lock further attempts for five minutes (kept in the store, so a restart does not reset it).
    pub fn verify_connection_password(&self, password: &str) -> Result<()> {
        // Attempts are serialized (not the db mutex: argon2 is slow and other db work must go on), so
        // parallel guesses cannot all start from the same failure count.
        let _attempt = self.unlock_lock.lock().unwrap_or_else(|p| p.into_inner());
        let (hash, mut throttle) = {
            let db = self.db();
            let hash = store::get::<Policy>(&db, "policy")?.unwrap_or_else(default_policy).connection_password_hash;
            (hash, store::get::<Throttle>(&db, THROTTLE_KEY)?.unwrap_or_default())
        };
        let unavailable = || EngineError::new("unlock_unavailable", "No unlock password is set for this computer.");
        if hash.is_empty() {
            return Err(unavailable());
        }
        if let Some(until) = throttle.locked_until.filter(|u| *u > Utc::now()) {
            // A clock moved back must not leave a lock far longer than the lock time.
            let secs = (until - Utc::now()).num_seconds().clamp(1, LOCK_MINUTES * 60);
            let mins = (secs + 59) / 60;
            return Err(EngineError::new("unlock_throttled", format!("Too many wrong passwords. Try again in {mins} minute{}.", if mins == 1 { "" } else { "s" }))
                .with("retryAfterSecs", secs));
        }
        let parsed = PasswordHash::new(&hash).map_err(|_| unavailable())?;
        let ok = Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok();
        let db = self.db();
        if ok {
            store::remove(&db, THROTTLE_KEY)?;
            return Ok(());
        }
        throttle.failures += 1;
        if throttle.failures >= MAX_FAILURES {
            throttle = Throttle { failures: 0, locked_until: Some(Utc::now() + Duration::minutes(LOCK_MINUTES)) };
        }
        store::set(&db, THROTTLE_KEY, &throttle)?;
        Err(EngineError::new("wrong_password", "That password isn't right."))
    }

    /// Gate for saving or resetting the connection: free when the admin allows edits, otherwise the unlock
    /// password must be given and right.
    pub fn authorize_connection_change(&self, password: Option<&str>) -> Result<()> {
        if self.policy()?.allow_connection_edit {
            return Ok(());
        }
        match password {
            None => Err(EngineError::new("connection_locked", "Your admin has locked the connection settings.")),
            Some(p) => self.verify_connection_password(p),
        }
    }

    pub(crate) fn ensure_install_id(&self) -> Result<()> {
        let db = self.db();
        if store::get::<String>(&db, "install_id")?.is_none() {
            store::set(&db, "install_id", &uuid::Uuid::new_v4().to_string())?;
        }
        Ok(())
    }

    /// Starts over as a new install, e.g. after switching servers. Returns the new id.
    pub fn new_install_id(&self) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        store::set(&self.db(), "install_id", &id)?;
        Ok(id)
    }

    pub fn install_id(&self) -> Result<String> {
        Ok(store::get(&self.db(), "install_id")?.unwrap_or_default())
    }

    pub fn get_settings(&self) -> Result<AppSettings> {
        Ok(store::get(&self.db(), "app")?.unwrap_or_default())
    }

    /// Merges a partial camelCase object into the settings.
    pub fn update_settings(&self, patch: Value) -> Result<AppSettings> {
        let db = self.db();
        let current: AppSettings = store::get(&db, "app")?.unwrap_or_default();
        let mut merged = serde_json::to_value(current)?;
        if let (Value::Object(m), Value::Object(p)) = (&mut merged, patch) {
            m.extend(p);
        }
        let next: AppSettings = serde_json::from_value(merged)
            .map_err(|e| EngineError::new("invalid", format!("invalid settings: {e}")))?;
        store::set(&db, "app", &next)?;
        Ok(next)
    }

    /// Stores the name confirmed by the server (registration or name change).
    pub fn set_identity(&self, name: &str, next_name_change: Option<DateTime<Utc>>) -> Result<()> {
        let db = self.db();
        store::set(&db, "name", name)?;
        store::set(&db, "next_name_change", &next_name_change)?;
        Ok(())
    }

    pub fn token(&self) -> Result<Option<String>> {
        Ok(store::get(&self.db(), "token")?)
    }

    pub fn set_token(&self, token: Option<&str>) -> Result<()> {
        let db = self.db();
        match token {
            Some(t) => store::set(&db, "token", t)?,
            None => store::remove(&db, "token")?,
        }
        Ok(())
    }

    pub fn policy(&self) -> Result<Policy> {
        Ok(store::get(&self.db(), "policy")?.unwrap_or_else(default_policy))
    }

    pub fn set_policy(&self, policy: &Policy) -> Result<()> {
        Ok(store::set(&self.db(), "policy", policy)?)
    }

    pub fn set_connection(&self, server_url: &str, install_key: &str) -> Result<()> {
        let db = self.db();
        store::set(&db, "server_url", server_url)?;
        store::set(&db, "install_key", install_key)?;
        Ok(())
    }

    /// Live status reported by the sync client (not persisted).
    pub fn set_connection_status(&self, status: ConnectionStatus, error: Option<String>) {
        *self.connection_state.lock().unwrap() = (status, error);
    }

    pub fn set_last_sync(&self, at: DateTime<Utc>) -> Result<()> {
        Ok(store::set(&self.db(), "last_sync", &at.to_rfc3339())?)
    }

    pub fn get_connection(&self) -> Result<Connection> {
        let (status, error) = self.connection_state.lock().unwrap().clone();
        let db = self.db();
        let policy = store::get::<Policy>(&db, "policy")?.unwrap_or_else(default_policy);
        Ok(Connection {
            status,
            server_url: store::get(&db, "server_url")?.unwrap_or_default(),
            install_key: store::get(&db, "install_key")?.unwrap_or_default(),
            editable: policy.allow_connection_edit,
            unlockable: !policy.allow_connection_edit && PasswordHash::new(&policy.connection_password_hash).is_ok(),
            last_sync: store::get(&db, "last_sync")?,
            install_id: store::get(&db, "install_id")?.unwrap_or_default(),
            error,
        })
    }

    pub fn get_app_state(&self) -> Result<AppState> {
        let connection = self.get_connection()?;
        let db = self.db();
        let name: String = store::get(&db, "name")?.unwrap_or_default();
        let next: Option<DateTime<Utc>> = store::get(&db, "next_name_change")?.flatten();
        let policy = store::get::<Policy>(&db, "policy")?.unwrap_or_else(default_policy);
        Ok(AppState {
            // A name is enough: without a server (or before it is reachable) the name is kept here and the
            // sync loop registers with it later.
            onboarded: !name.is_empty(),
            server_configured: !connection.server_url.trim().is_empty(),
            name,
            next_name_change: next.map(|d| d.to_rfc3339()),
            name_change_days: policy.name_change_days,
            brand_name: policy.brand_name,
            brand_color: policy.brand_color,
            brand_logo: policy.brand_logo,
            connection,
            platform: if cfg!(target_os = "macos") { Platform::Macos } else { Platform::Windows },
            app_version: env!("CARGO_PKG_VERSION").into(),
        })
    }
}
