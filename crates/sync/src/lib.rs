//! The desktop app's client for the server's Desktop API (`/api/v1`, see docs/api.md).

use std::collections::HashSet;
use std::future::Future;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch, Mutex, Notify};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{header::AUTHORIZATION, HeaderValue};
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use typeshelf_core::organizer::{self, OrganizerDoc};
use typeshelf_core::update::Platform;
use typeshelf_protocol::{
    ClientMessage, Command, EventBatch, FontSnapshot, NameChange, NameStatus, Policy, RegisterRequest,
    RegisterResponse, ServerMessage, TimedEvent, INSTALL_KEY_HEADER,
};

pub use tokio_util::sync::CancellationToken;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const PING_EVERY: Duration = Duration::from_secs(30);
/// No frame (not even a pong) for this long means the socket is dead, e.g. after sleep/wake.
const IDLE_LIMIT: Duration = Duration::from_secs(75);
const BATCH: usize = 200;
// ponytail: 10,000 events, oldest dropped past it; the outbox is rewritten whole per batch, so it must stay small.
const OUTBOX_MAX: usize = 10_000;
/// A release manifest is a few hundred bytes; a larger `GET /update` body is not read to the end.
const MANIFEST_MAX: usize = 64 * 1024;

// Shared by all clients, so a client replaced at runtime can't race its successor on the same file.
static OUTBOX_FILE: Mutex<()> = Mutex::const_new(());
static FLUSHING: Mutex<()> = Mutex::const_new(());

#[derive(Debug, Clone)]
pub struct Config {
    /// e.g. `https://fonts.example.com`, without `/api/v1`.
    pub server_url: String,
    pub install_key: String,
    pub token: Option<String>,
    /// JSON-lines file of events the server has not accepted yet.
    pub outbox_path: PathBuf,
}

/// `GET /me` response.
#[derive(Debug, Clone, Deserialize)]
pub struct Me {
    pub name: String,
    pub next_name_change: Option<DateTime<Utc>>,
    pub policy: Policy,
}

/// `GET` / `PUT /me/organizer`: the person's synced collections and project sets.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Organizer {
    pub rev: i64,
    pub doc: OrganizerDoc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Connected,
    Offline,
    /// The server answered but refused us (bad key/token, TLS error, bad URL...).
    Error(String),
}

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("server unreachable")]
    Offline,
    #[error("name can be changed again on {next_name_change}")]
    NameLocked { next_name_change: DateTime<Utc> },
    #[error("server error {status} {code}: {message}")]
    Server { status: u16, code: String, message: String },
    /// `PUT /me/organizer` with a stale `base_rev`: another computer pushed first. Holds the server's current one.
    #[error("the organizer changed on another computer (rev {})", .0.rev)]
    OrganizerConflict(Organizer),
    #[error(transparent)]
    Http(reqwest::Error),
    #[error("outbox: {0}")]
    Io(#[from] io::Error),
}

fn http_err(e: reqwest::Error) -> SyncError {
    if e.is_connect() || e.is_timeout() {
        SyncError::Offline
    } else {
        SyncError::Http(e)
    }
}

async fn check(res: Response) -> Result<Response, SyncError> {
    if res.status().is_success() {
        return Ok(res);
    }
    #[derive(Deserialize, Default)]
    struct ErrorBody {
        #[serde(default)]
        error: String,
        #[serde(default)]
        message: String,
    }
    let status = res.status().as_u16();
    let body: ErrorBody = res.json().await.unwrap_or_default();
    Err(SyncError::Server { status, code: body.error, message: body.message })
}

fn status_for(e: &SyncError) -> Status {
    match e {
        SyncError::Offline => Status::Offline,
        e => Status::Error(e.to_string()),
    }
}

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// What [`SyncClient::sync_organizer`] hands back: the document to adopt, and whether this computer's changes
/// reached the server (when not, they stay dirty).
#[derive(Debug, Clone, PartialEq)]
pub struct OrganizerSync {
    pub organizer: Organizer,
    pub pushed: bool,
}

pub struct SyncClient {
    config: Config,
    token: RwLock<Option<String>>,
    http: reqwest::Client,
    status: watch::Sender<Status>,
    flush_wanted: Notify,
    dropped: watch::Sender<u64>,
    /// Set by `notices()`; until then non-command messages are dropped.
    notices: std::sync::Mutex<Option<mpsc::UnboundedSender<ServerMessage>>>,
}

impl SyncClient {
    /// To apply a changed server URL / key: cancel the old `run`, drop the client, make a new one.
    pub fn new(config: Config) -> Self {
        // tokio-tungstenite builds its TLS config from rustls' process default. In a host that links
        // both rustls backends (ring + aws-lc-rs, e.g. the desktop app) there is no default, and the
        // WebSocket connect panics — the app then stays "Offline" forever. Pick one; ignore "already set".
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(Duration::from_secs(30))
            .build()
            .expect("TLS backend failed to initialize");
        Self {
            token: RwLock::new(config.token.clone()),
            config,
            http,
            status: watch::Sender::new(Status::Offline),
            flush_wanted: Notify::new(),
            dropped: watch::Sender::new(0),
            notices: std::sync::Mutex::new(None),
        }
    }

    pub fn status(&self) -> watch::Receiver<Status> {
        self.status.subscribe()
    }

    /// Every server message that is not a command (`identity`, `organizer_changed`, ...), as it arrives over the
    /// WebSocket. One subscriber: the first call gets the receiver, later calls get `None`. Messages that arrive
    /// before the first call, or after the receiver is dropped, are dropped.
    pub fn notices(&self) -> Option<mpsc::UnboundedReceiver<ServerMessage>> {
        let mut slot = self.notices.lock().unwrap();
        if slot.is_some() {
            return None;
        }
        let (tx, rx) = mpsc::unbounded_channel();
        *slot = Some(tx);
        Some(rx)
    }

    fn base(&self) -> &str {
        self.config.server_url.trim_end_matches('/')
    }

    fn token(&self) -> Option<String> {
        self.token.read().unwrap().clone()
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let rb = self
            .http
            .request(method, format!("{}/api/v1{path}", self.base()))
            .header(INSTALL_KEY_HEADER, &self.config.install_key);
        match self.token() {
            Some(t) => rb.bearer_auth(t),
            None => rb,
        }
    }

    async fn send(&self, rb: RequestBuilder) -> Result<Response, SyncError> {
        check(rb.send().await.map_err(http_err)?).await
    }

    /// "Test connection": checks the server is reachable and the install key is right.
    pub async fn health(&self) -> Result<(), SyncError> {
        self.send(self.request(Method::GET, "/health")).await.map(drop)
    }

    /// Registers this install; the returned token is used by this client from now on.
    pub async fn register(&self, req: RegisterRequest) -> Result<RegisterResponse, SyncError> {
        let res = self.send(self.request(Method::POST, "/installs").json(&req)).await?;
        let res: RegisterResponse = res.json().await.map_err(http_err)?;
        *self.token.write().unwrap() = Some(res.token.clone());
        Ok(res)
    }

    pub async fn me(&self) -> Result<Me, SyncError> {
        let res = self.send(self.request(Method::GET, "/me")).await?;
        res.json().await.map_err(http_err)
    }

    pub async fn change_name(&self, name: &str) -> Result<NameStatus, SyncError> {
        let rb = self.request(Method::PUT, "/me/name").json(&NameChange { name: name.into() });
        let res = rb.send().await.map_err(http_err)?;
        if res.status() == StatusCode::CONFLICT {
            #[derive(Deserialize)]
            struct Locked {
                next_name_change: DateTime<Utc>,
            }
            let locked: Locked = res.json().await.map_err(http_err)?;
            return Err(SyncError::NameLocked { next_name_change: locked.next_name_change });
        }
        check(res).await?.json().await.map_err(http_err)
    }

    /// `None` from a server without organizer sync: 404, or an older server's admin UI (200, not JSON).
    pub async fn get_organizer(&self) -> Result<Option<Organizer>, SyncError> {
        let res = self.request(Method::GET, "/me/organizer").send().await.map_err(http_err)?;
        if res.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let body = check(res).await?.bytes().await.map_err(http_err)?;
        if let Ok(o) = serde_json::from_slice(&body) {
            return Ok(Some(o));
        }
        // An HTML admin UI is an older server; JSON of another shape is a server bug worth seeing in the log.
        if serde_json::from_slice::<serde_json::Value>(&body).is_ok() {
            tracing::warn!("GET /me/organizer answered JSON that is not an organizer; organizer sync stays off until the next connect");
        }
        Ok(None)
    }

    /// `Err(OrganizerConflict(current))` when `base_rev` is not the server's rev any more.
    pub async fn put_organizer(&self, base_rev: i64, doc: &OrganizerDoc) -> Result<Organizer, SyncError> {
        let body = serde_json::json!({ "base_rev": base_rev, "doc": doc });
        let res = self.request(Method::PUT, "/me/organizer").json(&body).send().await.map_err(http_err)?;
        if res.status() == StatusCode::CONFLICT {
            return Err(SyncError::OrganizerConflict(res.json().await.map_err(http_err)?));
        }
        check(res).await?.json().await.map_err(http_err)
    }

    /// One sync round: pull, merge with `local`, and push only when the merge has something the server lacks or
    /// `dirty` is set. A conflict is merged with the server's current document and pushed once more. `Ok(Some)`
    /// is the server's document to adopt (also when nothing was pushed); a push that failed (a second conflict
    /// included) still returns the pulled document, with `pushed` false so the caller keeps its changes dirty.
    /// `Ok(None)` means the server has no organizer sync.
    pub async fn sync_organizer(&self, local: &OrganizerDoc, dirty: bool) -> Result<Option<OrganizerSync>, SyncError> {
        let Some(remote) = self.get_organizer().await? else { return Ok(None) };
        let merged = organizer::merge(&remote.doc, local);
        if merged == remote.doc && !dirty {
            return Ok(Some(OrganizerSync { organizer: remote, pushed: true }));
        }
        let put = match self.put_organizer(remote.rev, &merged).await {
            Err(SyncError::OrganizerConflict(current)) => {
                let merged = organizer::merge(&current.doc, &merged);
                self.put_organizer(current.rev, &merged).await
            }
            res => res,
        };
        Ok(Some(match put {
            Ok(organizer) => OrganizerSync { organizer, pushed: true },
            Err(e) => {
                tracing::warn!("pushing collections and project sets: {e}");
                OrganizerSync { organizer: remote, pushed: false }
            }
        }))
    }

    pub async fn put_snapshot(&self, snapshot: &FontSnapshot) -> Result<(), SyncError> {
        self.send(self.request(Method::PUT, "/me/fonts").json(snapshot)).await.map(drop)
    }

    pub async fn pending_commands(&self) -> Result<Vec<Command>, SyncError> {
        let res = self.send(self.request(Method::GET, "/me/commands")).await?;
        res.json().await.map_err(http_err)
    }

    /// REST ack; `run` acks over the socket and only falls back to this.
    pub async fn ack(&self, id: i64, ok: bool, error: Option<String>) -> Result<(), SyncError> {
        let body = serde_json::json!({ "ok": ok, "error": error });
        let path = format!("/me/commands/{id}/ack");
        self.send(self.request(Method::POST, &path).json(&body)).await.map(drop)
    }

    pub async fn download_library_file(&self, hash: &str) -> Result<Vec<u8>, SyncError> {
        let res = self.send(self.request(Method::GET, &format!("/library/files/{hash}"))).await?;
        Ok(res.bytes().await.map_err(http_err)?.to_vec())
    }

    /// `GET /update` (Windows) or `GET /update?platform=macos`: the current release's `(manifest, signature)`
    /// exactly as the server sent them, or `None` when it has none. The caller verifies them; nothing here is trusted.
    pub async fn update_release(&self, platform: Platform) -> Result<Option<(String, String)>, SyncError> {
        #[derive(Deserialize)]
        struct Release {
            manifest: String,
            signature: String,
        }
        let res = self.request(Method::GET, &update_path("/update", platform)).send().await.map_err(http_err)?;
        if res.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        // A server without the update routes answers with its admin UI (200 HTML): no release, not an error.
        let mut res = check(res).await?;
        let mut body = Vec::new();
        while let Some(chunk) = res.chunk().await.map_err(http_err)? {
            body.extend_from_slice(&chunk);
            if body.len() > MANIFEST_MAX {
                return Ok(None);
            }
        }
        Ok(serde_json::from_slice::<Release>(&body).ok().map(|r| (r.manifest, r.signature)))
    }

    /// Streams `GET /update/file` (with `?platform=macos` on a Mac) into `to`, giving up once more than `max`
    /// bytes arrive. On any error the partial file is removed.
    pub async fn download_update(&self, to: &Path, max: u64, platform: Platform) -> Result<(), SyncError> {
        let res = self.download_to(to, max, platform).await;
        if res.is_err() {
            let _ = tokio::fs::remove_file(to).await;
        }
        res
    }

    async fn download_to(&self, to: &Path, max: u64, platform: Platform) -> Result<(), SyncError> {
        let mut res = self.send(self.request(Method::GET, &update_path("/update/file", platform))).await?;
        let mut f = tokio::fs::File::create(to).await?;
        let mut size = 0u64;
        while let Some(chunk) = res.chunk().await.map_err(http_err)? {
            size += chunk.len() as u64;
            if size > max {
                return Err(io::Error::other("the download is larger than its manifest says").into());
            }
            f.write_all(&chunk).await?;
        }
        f.sync_all().await?;
        Ok(())
    }

    /// Durably appends events to the outbox; they are sent by `flush` (run calls it while connected).
    pub async fn queue_events(&self, events: Vec<TimedEvent>) -> Result<(), SyncError> {
        {
            let _file = OUTBOX_FILE.lock().await;
            let mut lines = read_lines(&self.config.outbox_path).await?;
            lines.extend(events.iter().map(|e| serde_json::to_string(e).expect("events serialize")));
            // Not while a flush runs: it removes the lines it sent by count, which a drop here would shift.
            if let Ok(_flushing) = FLUSHING.try_lock() {
                lines.drain(..lines.len().saturating_sub(OUTBOX_MAX));
            }
            write_lines(&self.config.outbox_path, &lines).await?;
        }
        self.flush_wanted.notify_one();
        Ok(())
    }

    /// Sends queued events in batches, removing each batch once the server accepted it. A batch the
    /// server rejects for good is resent one event at a time, so only the rejected events are dropped.
    /// Returns how many events were dropped; the same drops also count into `dropped_events`.
    pub async fn flush(&self) -> Result<usize, SyncError> {
        let _flushing = FLUSHING.lock().await;
        let path = &self.config.outbox_path;
        let mut dropped = 0;
        loop {
            let lines = {
                let _file = OUTBOX_FILE.lock().await;
                read_lines(path).await?
            };
            if lines.is_empty() {
                return Ok(dropped);
            }
            let n = lines.len().min(BATCH);
            if !self.post_events(&lines[..n]).await? {
                if n > 1 {
                    // ponytail: one request per event (200 for a rejected batch); bisect the batch if that is slow.
                    let mut done = 0;
                    let mut sent = Ok(());
                    for line in &lines[..n] {
                        match self.post_events(std::slice::from_ref(line)).await {
                            Ok(true) => {}
                            Ok(false) => {
                                dropped += 1;
                                self.dropped.send_modify(|t| *t += 1);
                            }
                            Err(e) => {
                                sent = Err(e);
                                break;
                            }
                        }
                        done += 1;
                    }
                    // One rewrite for the handled lines, also after an error; if it fails they are only resent.
                    let removed = self.remove_first(done).await;
                    sent?;
                    removed?;
                    continue;
                }
                dropped += 1;
                self.dropped.send_modify(|t| *t += 1);
            }
            self.remove_first(n).await?;
        }
    }

    /// Total events dropped by `flush` (including the ones `run` flushes) since this client was made.
    pub fn dropped_events(&self) -> watch::Receiver<u64> {
        self.dropped.subscribe()
    }

    /// Sends outbox lines as one batch: true if the server took it, false if it rejected it for good.
    /// A client error that retrying cannot fix (unknown event kind, payload too large) would block every
    /// later batch. Auth, timeout and rate-limit statuses recover, so they stay errors.
    async fn post_events(&self, lines: &[String]) -> Result<bool, SyncError> {
        // A line that doesn't parse would block the outbox forever, so it is dropped with its batch.
        let events = lines.iter().filter_map(|l| serde_json::from_str(l).ok()).collect();
        let batch = EventBatch { events };
        match self.send(self.request(Method::POST, "/me/events").json(&batch)).await {
            Ok(_) => Ok(true),
            Err(SyncError::Server { status, .. }) if (400..500).contains(&status) && ![401, 403, 408, 429].contains(&status) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Only appends happen to the outbox while we hold FLUSHING, so its first `n` lines are still ours.
    async fn remove_first(&self, n: usize) -> Result<(), SyncError> {
        let _file = OUTBOX_FILE.lock().await;
        let mut lines = read_lines(&self.config.outbox_path).await?;
        lines.drain(..n);
        write_lines(&self.config.outbox_path, &lines).await?;
        Ok(())
    }

    /// Keeps the command WebSocket connected until `cancel` fires, reconnecting with backoff.
    /// Each command goes to `handler`, then is acked with its result. A command may be handed
    /// over again after a crash or lost ack, so handlers must be idempotent.
    pub async fn run<H, F>(&self, cancel: CancellationToken, mut handler: H)
    where
        H: FnMut(Command) -> F,
        F: Future<Output = Result<(), String>>,
    {
        let mut failures = 0;
        loop {
            let status = tokio::select! {
                _ = cancel.cancelled() => break,
                s = self.session(&mut handler, &mut failures) => s,
            };
            self.status.send_replace(status);
            failures += 1;
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tokio::time::sleep(backoff(failures)) => {}
            }
        }
        self.status.send_replace(Status::Offline);
    }

    /// One connection's lifetime; returns the status to show once it ends.
    async fn session<H, F>(&self, handler: &mut H, failures: &mut u32) -> Status
    where
        H: FnMut(Command) -> F,
        F: Future<Output = Result<(), String>>,
    {
        let mut ws = match self.connect_ws().await {
            Ok(ws) => ws,
            Err(status) => return status,
        };
        *failures = 0;
        self.status.send_replace(Status::Connected);

        let pending = match self.pending_commands().await {
            Ok(p) => p,
            Err(e) => return status_for(&e),
        };
        // Commands pushed right after connect can also be in `pending`.
        let mut seen = HashSet::new();
        for cmd in pending {
            self.handle(&mut ws, handler, cmd, &mut seen).await;
        }
        // Errors leave events queued; they're retried on the next queue_events or reconnect.
        let _ = self.flush().await;

        let mut ping = tokio::time::interval(PING_EVERY);
        let mut last_frame = Instant::now();
        loop {
            tokio::select! {
                msg = ws.next() => {
                    let Some(Ok(msg)) = msg else { return Status::Offline };
                    last_frame = Instant::now();
                    match msg {
                        Message::Text(text) => match serde_json::from_str(&text) {
                            Ok(ServerMessage::Command(cmd)) => self.handle(&mut ws, handler, cmd, &mut seen).await,
                            Ok(notice) => {
                                if let Some(tx) = &*self.notices.lock().unwrap() {
                                    let _ = tx.send(notice);
                                }
                            }
                            // A message type newer than this client.
                            Err(_) => {}
                        },
                        Message::Close(_) => return Status::Offline,
                        _ => {}
                    }
                }
                _ = self.flush_wanted.notified() => {
                    let _ = self.flush().await;
                }
                _ = ping.tick() => {
                    if last_frame.elapsed() > IDLE_LIMIT
                        || ws.send(Message::Ping(Default::default())).await.is_err()
                    {
                        return Status::Offline;
                    }
                }
            }
        }
    }

    async fn handle<H, F>(&self, ws: &mut Ws, handler: &mut H, cmd: Command, seen: &mut HashSet<i64>)
    where
        H: FnMut(Command) -> F,
        F: Future<Output = Result<(), String>>,
    {
        if !seen.insert(cmd.id) {
            return;
        }
        let id = cmd.id;
        let (ok, error) = match handler(cmd).await {
            Ok(()) => (true, None),
            Err(e) => (false, Some(e)),
        };
        let ack = ClientMessage::Ack { id, ok, error: error.clone() };
        let text = serde_json::to_string(&ack).expect("ack serializes");
        if ws.send(Message::text(text)).await.is_err() {
            // Unacked commands come back via pending_commands after reconnect.
            let _ = self.ack(id, ok, error).await;
        }
    }

    async fn connect_ws(&self) -> Result<Ws, Status> {
        fn err(e: impl std::fmt::Display) -> Status {
            Status::Error(e.to_string())
        }
        // http:// -> ws://, https:// -> wss://
        let url = format!("{}/api/v1/me/ws", self.base()).replacen("http", "ws", 1);
        let mut req = url.into_client_request().map_err(err)?;
        let headers = req.headers_mut();
        headers.insert(INSTALL_KEY_HEADER, HeaderValue::from_str(&self.config.install_key).map_err(err)?);
        if let Some(t) = self.token() {
            headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {t}")).map_err(err)?);
        }
        match tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(req)).await {
            Ok(Ok((ws, _))) => Ok(ws),
            Ok(Err(tungstenite::Error::Io(_))) | Err(_) => Err(Status::Offline),
            Ok(Err(tungstenite::Error::Http(res))) => {
                Err(Status::Error(format!("server refused the connection ({})", res.status())))
            }
            Ok(Err(e)) => Err(err(e)),
        }
    }
}

/// Capped exponential backoff with jitter: 50–100% of min(0.5s * 2^failures, 60s).
fn backoff(failures: u32) -> Duration {
    let max = Duration::from_millis(500 << failures.min(7)).min(Duration::from_secs(60));
    // RandomState is randomly seeded, which is plenty for jitter without a rand dependency.
    let r = std::collections::hash_map::RandomState::new().build_hasher().finish();
    max / 2 + max.mul_f64((r % 1000) as f64 / 2000.0)
}

async fn read_lines(path: &Path) -> io::Result<Vec<String>> {
    match tokio::fs::read_to_string(path).await {
        Ok(s) => Ok(s.lines().filter(|l| !l.trim().is_empty()).map(String::from).collect()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Write-then-rename, so a crash leaves either the old or the new outbox, never a torn one.
// ponytail: rewrites the whole file per change; fine for an outbox that drains while online.
async fn write_lines(path: &Path, lines: &[String]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut f = tokio::fs::File::create(&tmp).await?;
    let buf: String = lines.iter().map(|l| format!("{l}\n")).collect();
    f.write_all(buf.as_bytes()).await?;
    f.sync_all().await?;
    drop(f);
    tokio::fs::rename(&tmp, path).await
}

/// Windows keeps the bare path, so an older server sees today's request; only a Mac asks for its slot.
fn update_path(path: &str, platform: Platform) -> String {
    match platform {
        Platform::Windows => path.into(),
        Platform::Macos => format!("{path}?platform=macos"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_is_capped_and_jittered() {
        let first = backoff(1);
        assert!(first >= Duration::from_millis(500) && first <= Duration::from_secs(1));
        let late = backoff(40);
        assert!(late >= Duration::from_secs(30) && late <= Duration::from_secs(60));
    }
}
