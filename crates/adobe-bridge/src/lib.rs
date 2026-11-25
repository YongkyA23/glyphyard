//! Loopback-only HTTP bridge between the Adobe plug-ins (`plugins/`) and the Typeshelf app.
//!
//! Unlike an open `0.0.0.0` server with `CORS: *`, every request must come from a loopback peer, name
//! `127.0.0.1`/`localhost` as Host (defeats DNS rebinding) and carry `X-Typeshelf-Client` (forces a CORS
//! preflight, which gets 403, so web pages can't call it). Document requests also need a token that the
//! user approved once per plug-in.

pub mod install;

use std::collections::{HashMap, HashSet};
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

pub const DEFAULT_PORT: u16 = 27716;
/// Plug-ins probe `DEFAULT_PORT..=DEFAULT_PORT + PORT_FALLBACKS`.
pub const PORT_FALLBACKS: u16 = 4;
/// `{"port": n}`, written to the data dir once the bridge listens.
pub const DISCOVERY_FILE: &str = "adobe-bridge.json";
const TOKENS_FILE: &str = "adobe-tokens.json";
const CLIENT_HEADER: &str = "x-typeshelf-client";
const PAIR_TIMEOUT: Duration = Duration::from_secs(60);
/// An app not heard from for this long (plug-ins ping every 15 s) has its documents treated as closed.
pub const EXPIRY: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum App {
    Photoshop,
    Illustrator,
    Indesign,
}

pub struct Config {
    /// Holds the token and discovery files.
    pub data_dir: PathBuf,
    /// First port to try; the next `PORT_FALLBACKS` are tried if it is taken. 0 = any (tests).
    pub port: u16,
    /// Typeshelf version reported by `/v1/ping`.
    pub version: String,
    /// `EXPIRY`; tests shorten it. Also the window after start for re-claiming fonts owned by the last run.
    pub expiry: Duration,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairRequest {
    pub app: App,
    pub app_version: String,
    pub client_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocFont {
    pub postscript_name: String,
    #[serde(default)]
    pub family: String,
    #[serde(default)]
    pub style: String,
    /// The Adobe app reported it as missing/substituted.
    #[serde(default)]
    pub missing: bool,
}

#[derive(Clone, Debug)]
pub struct DocRequest {
    pub app: App,
    pub document: String,
    pub fonts: Vec<DocFont>,
}

/// PostScript names, as returned to the plug-in.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocResult {
    /// Fonts this call turned on (they were off before). Only these may later be released.
    pub activated: Vec<String>,
    /// Fonts neither active nor in the library.
    pub missing: Vec<String>,
    pub already_active: Vec<String>,
}

/// Implemented by the desktop shell.
#[async_trait]
pub trait BridgeHandler: Send + Sync + 'static {
    /// Ask the user ("Allow Photoshop to turn on fonts automatically?"). The bridge treats no answer within
    /// 60 s, or the plug-in disconnecting, as a denial and drops this future.
    async fn approve_pairing(&self, req: PairRequest) -> bool;
    /// Turn on the document's fonts that the library has.
    async fn activate(&self, req: DocRequest) -> DocResult;
    /// Turn off fonts that an earlier `activate` turned on and no open document needs any more.
    async fn release(&self, postscript_names: Vec<String>);
}

pub struct BridgeHandle {
    addr: SocketAddr,
    shared: Arc<Shared>,
    task: JoinHandle<io::Result<()>>,
}

impl BridgeHandle {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn is_paired(&self, app: App) -> bool {
        self.shared.tokens.lock().unwrap().iter().any(|t| t.app == app)
    }

    /// Forget every token issued to `app`; its plug-in has to pair again.
    pub fn revoke(&self, app: App) -> io::Result<()> {
        let mut tokens = self.shared.tokens.lock().unwrap();
        tokens.retain(|t| t.app != app);
        save_tokens(&self.shared.config.data_dir, &tokens)
    }

    pub fn shutdown(self) {
        // ponytail: hard stop; a graceful drain could wait out a 60 s pairing prompt at app quit.
        self.task.abort();
    }
}

/// Bind `127.0.0.1` and serve in the background on the current Tokio runtime.
pub async fn start(config: Config, handler: Arc<dyn BridgeHandler>) -> io::Result<BridgeHandle> {
    std::fs::create_dir_all(&config.data_dir)?;
    let listener = bind(config.port).await?;
    let addr = listener.local_addr()?;
    // Fonts the last run turned on; released by `expire` unless a document re-claims them in time.
    let owned = load_state(&config.data_dir).owned;
    save_state(&config.data_dir, addr.port(), &owned)?;
    let shared = Arc::new(Shared {
        tokens: Mutex::new(load_tokens(&config.data_dir)),
        config,
        port: addr.port(),
        handler,
        docs: tokio::sync::Mutex::new(Docs { open: HashMap::new(), owned }),
        pairing: AtomicBool::new(false),
        heard: Mutex::default(),
        started: Instant::now(),
    });
    let router = Router::new()
        .route("/v1/ping", get(ping))
        .route("/v1/pair", post(pair))
        .route("/v1/document", post(document))
        .layer(middleware::from_fn(guard))
        .with_state(shared.clone());
    let sweeper = shared.clone();
    // One task, so `shutdown` stops the expiry checks along with the server.
    let task = tokio::spawn(async move {
        tokio::select! {
            res = axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>()) => res,
            () = expire_loop(sweeper) => Ok(()),
        }
    });
    Ok(BridgeHandle { addr, shared, task })
}

async fn expire_loop(s: Arc<Shared>) {
    let mut tick = tokio::time::interval(s.config.expiry / 4);
    loop {
        tick.tick().await;
        expire(&s).await;
    }
}

/// Closes the documents of apps not heard from within `expiry`, and after the start-up window releases
/// fonts owned by the last run that no open document re-claimed.
async fn expire(s: &Shared) {
    let expiry = s.config.expiry;
    let mut docs = s.docs.lock().await;
    let silent: Vec<(App, String)> = {
        let heard = s.heard.lock().unwrap();
        docs.open
            .keys()
            .filter(|(app, _)| heard.get(app).is_none_or(|t| t.elapsed() > expiry))
            .cloned()
            .collect()
    };
    let mut released: Vec<String> = silent.iter().flat_map(|key| docs.close(key)).collect();
    if s.started.elapsed() > expiry {
        released.extend(docs.orphans());
    }
    released.sort();
    s.release(&docs, &released).await;
}

async fn bind(port: u16) -> io::Result<TcpListener> {
    let mut last_err = None;
    for p in port..=port.saturating_add(PORT_FALLBACKS) {
        match TcpListener::bind((Ipv4Addr::LOCALHOST, p)).await {
            Ok(listener) => return Ok(listener),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.expect("range is never empty"))
}

struct Shared {
    config: Config,
    handler: Arc<dyn BridgeHandler>,
    tokens: Mutex<Vec<TokenEntry>>,
    // ponytail: one lock across the host calls serializes document requests, so open/close bookkeeping
    // can't interleave with an activate/release in flight. Fine at a few documents per minute.
    docs: tokio::sync::Mutex<Docs>,
    pairing: AtomicBool,
    /// Bound port, rewritten into the discovery file with `owned`.
    port: u16,
    /// When each paired app last made an authenticated request.
    heard: Mutex<HashMap<App, Instant>>,
    started: Instant,
}

impl Shared {
    /// The app the token belongs to; also records that this app was just heard from.
    fn token_app(&self, token: &str) -> Option<App> {
        let hash = sha256_hex(token);
        let app = self.tokens.lock().unwrap().iter().find(|t| t.hash == hash).map(|t| t.app)?;
        self.heard.lock().unwrap().insert(app, Instant::now());
        Some(app)
    }

    fn save(&self, docs: &Docs) {
        // ponytail: a failed write only costs the restart clean-up of these fonts; no logger in this crate.
        let _ = save_state(&self.config.data_dir, self.port, &docs.owned);
    }

    /// Turns off fonts `docs` just dropped from `owned` (a close, an expiry, or the start-up clean-up).
    async fn release(&self, docs: &Docs, names: &[String]) {
        if !names.is_empty() {
            self.handler.release(names.to_vec()).await;
            self.save(docs);
        }
    }
}

async fn guard(ConnectInfo(peer): ConnectInfo<SocketAddr>, req: Request, next: Next) -> Response {
    let host_ok = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(|h| h.rsplit_once(':').map_or(h, |(name, _)| name))
        .is_some_and(|name| name == "127.0.0.1" || name == "localhost");
    if req.method() == Method::OPTIONS
        || !peer.ip().is_loopback()
        || !host_ok
        || !req.headers().contains_key(CLIENT_HEADER)
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    next.run(req).await
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Bearer ")
}

async fn ping(State(s): State<Arc<Shared>>, headers: HeaderMap) -> Json<serde_json::Value> {
    let app = bearer(&headers).and_then(|t| s.token_app(t));
    let mut body = json!({ "app": "typeshelf", "version": s.config.version, "paired": app.is_some() });
    // How many of the caller's documents the bridge has open; 0 tells a plug-in that still tracks some that
    // they were forgotten (restart, expiry) and must be re-sent.
    // ponytail: skipped while a slow activate holds the lock; the plug-in just checks again on its next ping.
    if let Some(app) = app {
        if let Ok(docs) = tokio::time::timeout(Duration::from_millis(500), s.docs.lock()).await {
            body["documents"] = docs.open.keys().filter(|(a, _)| *a == app).count().into();
        }
    }
    Json(body)
}

async fn pair(State(s): State<Arc<Shared>>, Json(req): Json<PairRequest>) -> Response {
    // One prompt at a time, so a misbehaving client can't stack dialogs.
    if s.pairing.swap(true, Ordering::SeqCst) {
        return StatusCode::CONFLICT.into_response();
    }
    let _busy = ClearOnDrop(&s.pairing);
    let approved = tokio::time::timeout(PAIR_TIMEOUT, s.handler.approve_pairing(req.clone()))
        .await
        .unwrap_or(false);
    if !approved {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("OS random number generator");
    let token = hex(&bytes);
    let mut tokens = s.tokens.lock().unwrap();
    tokens.retain(|t| !(t.app == req.app && t.client_id == req.client_id));
    tokens.push(TokenEntry { app: req.app, client_id: req.client_id, hash: sha256_hex(&token) });
    match save_tokens(&s.config.data_dir, &tokens) {
        Ok(()) => Json(json!({ "token": token })).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

struct ClearOnDrop<'a>(&'a AtomicBool);

impl Drop for ClearOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum DocAction {
    Open,
    Close,
}

#[derive(Deserialize)]
struct DocBody {
    document: String,
    action: DocAction,
    #[serde(default)]
    fonts: Vec<DocFont>,
}

async fn document(State(s): State<Arc<Shared>>, headers: HeaderMap, Json(body): Json<DocBody>) -> Response {
    // The token decides the app; the body's `app` field is informational.
    let Some(app) = bearer(&headers).and_then(|t| s.token_app(t)) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let key = (app, body.document.clone());
    let mut docs = s.docs.lock().await;
    match body.action {
        DocAction::Open => {
            let names: Vec<String> = body.fonts.iter().map(|f| f.postscript_name.clone()).collect();
            let req = DocRequest { app, document: body.document, fonts: body.fonts };
            let result = s.handler.activate(req).await;
            docs.open(key, names, &result.activated);
            if !result.activated.is_empty() {
                s.save(&docs);
            }
            Json(result).into_response()
        }
        DocAction::Close => {
            let released = docs.close(&key);
            s.release(&docs, &released).await;
            Json(json!({ "released": released })).into_response()
        }
    }
}

/// Which fonts each open document uses, and which of them the bridge turned on (and so may turn off).
#[derive(Default)]
struct Docs {
    open: HashMap<(App, String), HashSet<String>>,
    owned: HashSet<String>,
}

impl Docs {
    fn open(&mut self, key: (App, String), fonts: Vec<String>, activated: &[String]) {
        // Re-checking an open document only adds fonts, so a font is never dropped while still shown.
        self.open.entry(key).or_default().extend(fonts);
        self.owned.extend(activated.iter().cloned());
    }

    /// Fonts to release: owned, used by this document, needed by no other open document.
    fn close(&mut self, key: &(App, String)) -> Vec<String> {
        let Some(fonts) = self.open.remove(key) else {
            return Vec::new();
        };
        let mut release: Vec<String> = fonts
            .into_iter()
            .filter(|f| self.owned.contains(f) && !self.open.values().any(|other| other.contains(f)))
            .collect();
        release.sort();
        for f in &release {
            self.owned.remove(f);
        }
        release
    }

    /// Owned fonts no open document uses. Only fonts loaded from the last run can be in this state, since
    /// `close` drops a font from `owned` together with its last document.
    fn orphans(&mut self) -> Vec<String> {
        let open = &self.open;
        let (orphans, kept) = self.owned.drain().partition(|f| !open.values().any(|fonts| fonts.contains(f)));
        self.owned = kept;
        orphans.into_iter().collect()
    }
}

/// The discovery file, which also holds the fonts the bridge turned on so a restart can release them.
#[derive(Default, Deserialize)]
struct Saved {
    #[serde(default)]
    owned: HashSet<String>,
}

/// A missing or unreadable file means the bridge owns nothing.
fn load_state(dir: &Path) -> Saved {
    std::fs::read(dir.join(DISCOVERY_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save_state(dir: &Path, port: u16, owned: &HashSet<String>) -> io::Result<()> {
    let mut owned: Vec<&String> = owned.iter().collect();
    owned.sort();
    let path = dir.join(DISCOVERY_FILE);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json!({ "port": port, "owned": owned }).to_string())?;
    std::fs::rename(tmp, path)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenEntry {
    app: App,
    client_id: String,
    /// SHA-256 of the token; the token itself is only known to the plug-in.
    hash: String,
}

/// A missing or unreadable file means "nothing paired"; plug-ins simply pair again.
fn load_tokens(dir: &Path) -> Vec<TokenEntry> {
    std::fs::read(dir.join(TOKENS_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save_tokens(dir: &Path, tokens: &[TokenEntry]) -> io::Result<()> {
    let path = dir.join(TOKENS_FILE);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(tokens)?)?;
    std::fs::rename(tmp, path)
}

fn sha256_hex(s: &str) -> String {
    hex(&Sha256::digest(s.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn font_is_kept_while_another_document_needs_it() {
        let a = (App::Photoshop, "a.psd".to_string());
        let b = (App::Indesign, "b.indd".to_string());
        let mut docs = Docs::default();
        docs.open(a.clone(), s(&["X", "Y", "Sys"]), &s(&["X", "Y"]));
        docs.open(b.clone(), s(&["X"]), &[]);
        assert_eq!(docs.close(&a), s(&["Y"]));
        assert_eq!(docs.close(&a), Vec::<String>::new(), "closing twice is a no-op");
        assert_eq!(docs.close(&b), s(&["X"]));
    }

    #[test]
    fn fonts_not_activated_by_the_bridge_are_never_released() {
        let a = (App::Illustrator, "a.ai".to_string());
        let mut docs = Docs::default();
        docs.open(a.clone(), s(&["UserFont"]), &[]);
        assert!(docs.close(&a).is_empty());
    }
}
