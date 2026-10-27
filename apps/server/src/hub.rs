//! Command queue + live WebSocket push, keyed by install id.

use crate::db;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use typeshelf_protocol::{Command, CommandKind};

#[derive(Default)]
pub struct Hub {
    /// One live socket per install; a reconnect replaces the old sender (which ends the old socket loop).
    conns: Mutex<HashMap<String, (u64, UnboundedSender<Command>)>>,
    next_conn: AtomicU64,
}

impl Hub {
    pub fn connect(&self, install_id: &str) -> (u64, UnboundedReceiver<Command>) {
        let (tx, rx) = unbounded_channel();
        let conn = self.next_conn.fetch_add(1, Ordering::Relaxed);
        self.lock().insert(install_id.to_string(), (conn, tx));
        (conn, rx)
    }

    pub fn disconnect(&self, install_id: &str, conn: u64) {
        let mut conns = self.lock();
        if conns.get(install_id).is_some_and(|(c, _)| *c == conn) {
            conns.remove(install_id);
        }
    }

    pub fn is_connected(&self, install_id: &str) -> bool {
        self.lock().contains_key(install_id)
    }

    fn push(&self, install_id: &str, cmd: Command) {
        if let Some((_, tx)) = self.lock().get(install_id) {
            let _ = tx.send(cmd);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, (u64, UnboundedSender<Command>)>> {
        self.conns.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Stores the command (so it survives offline periods) and pushes it if the install is connected.
pub fn queue(c: &Connection, hub: &Hub, install_id: &str, kind: CommandKind) -> rusqlite::Result<i64> {
    let payload = serde_json::to_value(&kind).expect("command serializes");
    let kind_name = payload["kind"].as_str().unwrap_or_default().to_string();
    c.execute(
        "INSERT INTO commands (install_id, kind, payload_json, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![install_id, kind_name, payload.to_string(), db::now()],
    )?;
    let id = c.last_insert_rowid();
    hub.push(install_id, Command { id, kind });
    Ok(id)
}

/// Not yet acknowledged commands, oldest first; marks them delivered.
pub fn pending(c: &Connection, install_id: &str) -> rusqlite::Result<Vec<Command>> {
    let mut stmt = c.prepare("SELECT id, payload_json FROM commands WHERE install_id = ?1 AND acked_at IS NULL ORDER BY id")?;
    let rows = stmt.query_map([install_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut cmds = Vec::new();
    for row in rows {
        let (id, payload) = row?;
        match serde_json::from_str(&payload) {
            Ok(kind) => cmds.push(Command { id, kind }),
            Err(e) => tracing::warn!("command {id} has an unreadable payload: {e}"),
        }
    }
    c.execute(
        "UPDATE commands SET delivered_at = ?1 WHERE install_id = ?2 AND acked_at IS NULL AND delivered_at IS NULL",
        params![db::now(), install_id],
    )?;
    Ok(cmds)
}

pub fn mark_delivered(c: &Connection, id: i64) -> rusqlite::Result<()> {
    c.execute("UPDATE commands SET delivered_at = ?1 WHERE id = ?2 AND delivered_at IS NULL", params![db::now(), id])?;
    Ok(())
}

/// Records an ack; a failed command shows up as a failure on the timeline. `false` = no such command.
pub fn ack(c: &Connection, install_id: &str, id: i64, ok: bool, error: Option<&str>) -> rusqlite::Result<bool> {
    let now = db::now();
    let n = c.execute(
        "UPDATE commands SET acked_at = ?1, ok = ?2, error = ?3 WHERE id = ?4 AND install_id = ?5",
        params![now, ok, error, id, install_id],
    )?;
    if n > 0 && !ok {
        let kind: String = c.query_row("SELECT kind FROM commands WHERE id = ?1", [id], |r| r.get(0))?;
        let text = format!("Admin command “{}” failed: {}", kind.replace('_', " "), error.unwrap_or("unknown error"));
        db::insert_event(c, install_id, "failure", "system", &text, "{}", &now)?;
    }
    Ok(n > 0)
}
