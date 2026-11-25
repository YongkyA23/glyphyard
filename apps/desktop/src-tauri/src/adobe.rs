//! Adobe plug-in bridge: pairing prompts go to the UI, document fonts are turned on as user activations.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;
use typeshelf_adobe_bridge::{install, App, BridgeHandler, DocRequest, DocResult, PairRequest};
use typeshelf_engine::{Engine, Face, FamilyQuery, Result, Source};

use crate::{show_main, Ctx};

pub async fn start(app: AppHandle) {
    let config = typeshelf_adobe_bridge::Config {
        data_dir: app.state::<Ctx>().data_dir.clone(),
        port: typeshelf_adobe_bridge::DEFAULT_PORT,
        version: env!("CARGO_PKG_VERSION").into(),
        expiry: typeshelf_adobe_bridge::EXPIRY,
    };
    match typeshelf_adobe_bridge::start(config, Arc::new(Bridge(app.clone()))).await {
        Ok(handle) => {
            tracing::info!(addr = %handle.addr(), "adobe bridge listening");
            *app.state::<Ctx>().bridge.lock().unwrap() = Some(handle);
        }
        Err(e) => tracing::error!("adobe bridge: {e}"),
    }
}

struct Bridge(AppHandle);

/// Removes an unanswered prompt when the bridge gives up on it.
struct Pending<'a>(&'a Mutex<HashMap<String, oneshot::Sender<bool>>>, String);

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        self.0.lock().unwrap().remove(&self.1);
    }
}

/// Library faces with exactly this PostScript name.
fn faces(e: &Engine, ps_name: &str) -> Result<Vec<Face>> {
    let q = FamilyQuery { search: Some(ps_name.into()), ..Default::default() };
    Ok(e.list_families(&q)?
        .into_iter()
        .flat_map(|f| f.faces)
        .filter(|f| f.ps_name.eq_ignore_ascii_case(ps_name))
        .collect())
}

#[async_trait]
impl BridgeHandler for Bridge {
    async fn approve_pairing(&self, req: PairRequest) -> bool {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let ctx = self.0.state::<Ctx>();
        let id = format!("pair-{}", NEXT.fetch_add(1, Ordering::Relaxed));
        let (tx, rx) = oneshot::channel();
        ctx.pairs.lock().unwrap().insert(id.clone(), tx);
        let _pending = Pending(&ctx.pairs, id.clone());
        show_main(&self.0);
        let _ = self.0.emit("adobe-pair-request", json!({ "requestId": id, "app": req.app, "appVersion": req.app_version }));
        let allowed = matches!(tokio::time::timeout(Duration::from_secs(60), rx).await, Ok(Ok(true)));
        tracing::info!(app = ?req.app, allowed, "adobe pairing");
        allowed
    }

    async fn activate(&self, req: DocRequest) -> DocResult {
        let fonts = req.fonts.clone();
        let res = self
            .0
            .state::<Ctx>()
            .run(move |e| {
                let mut out = DocResult::default();
                let mut todo: HashMap<String, Vec<String>> = HashMap::new(); // hash -> PostScript names
                let mut seen = HashSet::new();
                for f in fonts.iter().filter(|f| seen.insert(f.postscript_name.to_lowercase())) {
                    let ps = f.postscript_name.clone();
                    let found = faces(e, &ps)?;
                    if found.iter().any(|x| x.active) {
                        out.already_active.push(ps);
                    } else if let Some(face) = found.first() {
                        // ponytail: first match; the engine still keeps one file per PostScript name active.
                        todo.entry(face.hash.clone()).or_default().push(ps);
                    } else if f.missing {
                        out.missing.push(ps);
                    } else {
                        // Not in the library but the Adobe app has it (a system font).
                        out.already_active.push(ps);
                    }
                }
                if !todo.is_empty() {
                    let hashes: Vec<String> = todo.keys().cloned().collect();
                    // The plug-in can't show the unknown-license prompt; the user approved the pairing.
                    for r in e.set_active_now(&hashes, true)? {
                        let names = todo.remove(&r.hash).unwrap_or_default();
                        if r.ok {
                            out.activated.extend(names);
                        } else {
                            out.missing.extend(names);
                        }
                    }
                }
                Ok(out)
            })
            .await;
        let result = res.unwrap_or_else(|e| {
            tracing::error!("adobe activate: {e}");
            DocResult::default()
        });
        if !result.activated.is_empty() || !result.missing.is_empty() {
            let _ = self.0.emit(
                "adobe-activated",
                json!({ "app": req.app, "document": req.document, "activated": result.activated, "missing": result.missing }),
            );
        }
        result
    }

    async fn release(&self, postscript_names: Vec<String>) {
        let res = self
            .0
            .state::<Ctx>()
            .run(move |e| {
                let mut hashes = Vec::new();
                for ps in &postscript_names {
                    // Only user activations; an admin may have turned the font on since.
                    hashes.extend(faces(e, ps)?.into_iter().filter(|f| f.active && f.source == Some(Source::User)).map(|f| f.hash));
                }
                if !hashes.is_empty() {
                    e.deactivate_unrecorded(&hashes)?;
                }
                Ok(())
            })
            .await;
        if let Err(e) = res {
            tracing::error!("adobe release: {e}");
        }
    }

    async fn app_running(&self, app: App) -> bool {
        match tauri::async_runtime::spawn_blocking(move || install::running(app)).await {
            Ok(Ok(running)) => running,
            // Never release fonts on doubt.
            Ok(Err(e)) => {
                tracing::warn!(?app, "adobe process check: {e}");
                true
            }
            Err(_) => true,
        }
    }
}
