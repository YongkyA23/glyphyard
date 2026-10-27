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
use tokio::sync::{watch, Mutex, Notify};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{header::AUTHORIZATION, HeaderValue};
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
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

pub struct SyncClient {
    config: Config,
    token: RwLock<Option<String>>,
    http: reqwest::Client,
    status: watch::Sender<Status>,
    flush_wanted: Notify,
}

impl SyncClient {
    /// To apply a changed server URL / key: cancel the old `run`, drop the client, make a new one.
    pub fn new(config: Config) -> Self {
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
        }
    }

    pub fn status(&self) -> watch::Receiver<Status> {
        self.status.subscribe()
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

    /// Durably appends events to the outbox; they are sent by `flush` (run calls it while connected).
    pub async fn queue_events(&self, events: Vec<TimedEvent>) -> Result<(), SyncError> {
        {
            let _file = OUTBOX_FILE.lock().await;
            let mut lines = read_lines(&self.config.outbox_path).await?;
            lines.extend(events.iter().map(|e| serde_json::to_string(e).expect("events serialize")));
            write_lines(&self.config.outbox_path, &lines).await?;
        }
        self.flush_wanted.notify_one();
        Ok(())
    }

    /// Sends queued events in batches, removing each batch once the server accepted it.
    pub async fn flush(&self) -> Result<(), SyncError> {
        let _flushing = FLUSHING.lock().await;
        let path = &self.config.outbox_path;
        loop {
            let lines = {
                let _file = OUTBOX_FILE.lock().await;
                read_lines(path).await?
            };
            if lines.is_empty() {
                return Ok(());
            }
            let n = lines.len().min(BATCH);
            // A line that doesn't parse would block the outbox forever, so it is dropped with its batch.
            let events = lines[..n].iter().filter_map(|l| serde_json::from_str(l).ok()).collect();
            self.send(self.request(Method::POST, "/me/events").json(&EventBatch { events })).await?;

            // Only appends happened meanwhile (we hold FLUSHING), so the first n lines are still ours.
            let _file = OUTBOX_FILE.lock().await;
            let mut lines = read_lines(path).await?;
            lines.drain(..n);
            write_lines(path, &lines).await?;
        }
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
                        Message::Text(text) => {
                            if let Ok(ServerMessage::Command(cmd)) = serde_json::from_str(&text) {
                                self.handle(&mut ws, handler, cmd, &mut seen).await;
                            }
                        }
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
