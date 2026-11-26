//! App settings plus the identity/connection values the sync crate reads and writes.

use crate::{store, AppSettings, AppState, Connection, ConnectionStatus, Engine, EngineError, Platform, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use typeshelf_protocol::Policy;

fn default_policy() -> Policy {
    Policy { name_change_days: 30, allow_connection_edit: true, notify_admin_changes: true, brand_name: String::new(), brand_color: String::new(), brand_logo: String::new() }
}

impl Engine {
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
        Ok(Connection {
            status,
            server_url: store::get(&db, "server_url")?.unwrap_or_default(),
            install_key: store::get(&db, "install_key")?.unwrap_or_default(),
            editable: store::get::<Policy>(&db, "policy")?.unwrap_or_else(default_policy).allow_connection_edit,
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
