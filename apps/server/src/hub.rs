//! Command queue + live WebSocket push, keyed by install id.

use crate::db;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use typeshelf_protocol::{Command, CommandKind, FONT_NOT_HERE_CODE, FONT_NOT_HERE_SUFFIX};

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

/// Stores the command (so it survives offline periods) for the install and for every install merged into it
/// (each is still a computer of its own), and pushes it to those connected. Returns the command id of `install_id`.
/// The copies of a fan-out share `fanout` = that id, so `ack` can judge the group as a whole.
pub fn queue(c: &Connection, hub: &Hub, install_id: &str, kind: CommandKind) -> rusqlite::Result<i64> {
    let payload = serde_json::to_value(&kind).expect("command serializes");
    let kind_name = payload["kind"].as_str().unwrap_or_default().to_string();
    let mut stmt = c.prepare("SELECT id FROM installs WHERE merged_into = ?1")?;
    let merged: Vec<String> = stmt.query_map([install_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
    let mut first = None;
    for target in std::iter::once(install_id).chain(merged.iter().map(String::as_str)) {
        c.execute(
            "INSERT INTO commands (install_id, kind, payload_json, created_at, fanout) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![target, kind_name, payload.to_string(), db::now(), first],
        )?;
        let id = c.last_insert_rowid();
        first.get_or_insert(id);
        hub.push(target, Command { id, kind: kind.clone() });
    }
    let first = first.expect("the install itself is always queued");
    if !merged.is_empty() {
        // Acks wait for the caller's DB lock, so none can arrive before this.
        c.execute("UPDATE commands SET fanout = ?1 WHERE id = ?1", [first])?;
    }
    Ok(first)
}

/// The engine's ack error when the font is not in that computer's library (`EngineError` "code: message", from
/// `apply_command`'s `Activate` arm); old desktops send the same text. A file missing on disk is not this.
fn font_not_here(error: &str) -> bool {
    error.strip_prefix(FONT_NOT_HERE_CODE).and_then(|e| e.strip_prefix(": ")).is_some_and(|e| e.ends_with(FONT_NOT_HERE_SUFFIX))
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
/// The first ack of a command stands; a repeat (a socket ack retried over REST) changes nothing.
/// "Font not on this computer" from one copy of a fan-out waits while another copy succeeded or was delivered
/// and is not acked yet: another computer of the same person may have the font. A copy never delivered (that
/// computer offline, gone or split away) does not hold it back, so a computer that succeeds later leaves the
/// failure on the timeline. The update and its event are one transaction (callers hold none).
pub fn ack(c: &Connection, install_id: &str, id: i64, ok: bool, error: Option<&str>) -> rusqlite::Result<bool> {
    let tx = c.unchecked_transaction()?;
    let found = record_ack(&tx, install_id, id, ok, error)?;
    tx.commit()?;
    Ok(found)
}

fn record_ack(c: &Connection, install_id: &str, id: i64, ok: bool, error: Option<&str>) -> rusqlite::Result<bool> {
    let now = db::now();
    let n = c.execute(
        "UPDATE commands SET acked_at = ?1, ok = ?2, error = ?3 WHERE id = ?4 AND install_id = ?5 AND acked_at IS NULL",
        params![now, ok, error, id, install_id],
    )?;
    if n == 0 {
        return c.query_row("SELECT EXISTS(SELECT 1 FROM commands WHERE id = ?1 AND install_id = ?2)", params![id, install_id], |r| r.get(0));
    }
    if ok {
        return Ok(true);
    }
    let (kind, fanout): (String, Option<i64>) = c.query_row("SELECT kind, fanout FROM commands WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let (kind, error) = (kind.replace('_', " "), error.unwrap_or("unknown error"));
    let mut text = format!("Admin command “{kind}” failed: {error}");
    if let (Some(fanout), true) = (fanout, font_not_here(error)) {
        let (open_or_ok, unacked): (bool, bool) = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM commands WHERE fanout = ?1 AND (ok OR (acked_at IS NULL AND delivered_at IS NOT NULL))),
                    EXISTS(SELECT 1 FROM commands WHERE fanout = ?1 AND acked_at IS NULL)",
            [fanout],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if open_or_ok {
            return Ok(true);
        }
        if !unacked {
            text = format!("Admin command “{kind}” failed on every computer: {error}");
        }
    }
    db::insert_event(c, install_id, "failure", "system", &text, "{}", &now)?;
    Ok(true)
}
