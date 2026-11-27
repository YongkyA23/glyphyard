//! One command per row of docs/desktop-commands.md. Engine calls run on the blocking pool and return
//! the engine's types unchanged; every error rejects as `{ code, message, ...extra }`.


use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;
use typeshelf_adobe_bridge::install::{self, AdobeApp};
use typeshelf_adobe_bridge::App;
use typeshelf_engine::*;

use crate::sync::{self, sync_err, DEFAULT_INSTALL_KEY, DEFAULT_SERVER_URL};
use crate::{set_autostart, set_shortcut, Ctx};

type R<T> = Result<T, EngineError>;
type St<'a> = State<'a, Ctx>;

pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        get_app_state, complete_onboarding, change_name,
        list_families, family_page, query_targets, edit_tags, library_counts, language_counts, get_faces, get_family, set_active, similar_families,
        set_active_for,
        set_family_name, delete_fonts, import_paths, face_glyphs, glyph_svg, set_pinned,
        pairing_suggestions, list_axis_presets, save_axis_preset, delete_axis_preset,
        folder_tree, set_folder_active, set_folder_auto, set_folder_hidden, delete_folder,
        list_folders, add_folder, remove_folder, locate_folder,
        list_collections, create_collection, rename_collection, delete_collection, add_to_collection, remove_from_collection,
        set_collection_active, export_collection, export_collections, import_collections,
        list_tags, set_tags, set_license,
        list_project_sets, save_project_set, delete_project_set, toggle_project_set,
        organize_plan, organize_apply, list_history, undo_history,
        health, repair_now, pdf_fonts, document_fonts, request_font,
        usage_stats, badges,
        google_catalog, google_install, google_remove,
        backup_create, backup_restore,
        get_settings, update_settings,
        get_connection, test_connection, save_connection, reset_connection, unlock_connection, set_tray_language,
        check_update, restart_to_update,
        quick_search, show_in_folder, open_url,
        adobe_status, adobe_install, adobe_remove, adobe_pair_respond, indesign_running,
        pick_folder, pick_file, pick_files, pick_save_file,
    ]
}

// ---- Identity ----

/// The app state plus whether this computer holds a server token (the name rule applies only then).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStateView {
    #[serde(flatten)]
    state: AppState,
    registered: bool,
}

fn app_state(e: &Engine) -> R<AppStateView> {
    Ok(AppStateView { state: e.get_app_state()?, registered: e.token()?.is_some() })
}

#[tauri::command]
pub async fn get_app_state(ctx: St<'_>) -> R<AppStateView> {
    ctx.run(app_state).await
}

/// `offline`: the person chose "Continue offline". Without a server, or offline, the name is kept here and
/// the sync loop registers with it once the server answers.
#[tauri::command]
pub async fn complete_onboarding(app: AppHandle, ctx: St<'_>, name: String, offline: Option<bool>) -> R<AppStateView> {
    let name = typeshelf_core::name_rule::validate_name(&name).map_err(|m| EngineError::new("invalid_name", m))?;
    let state = ctx.run(|e| e.get_app_state()).await?;
    if state.server_configured && !offline.unwrap_or(false) {
        let conn = state.connection;
        let client = sync::new_client(&ctx.data_dir, conn.server_url, conn.install_key, None);
        sync::register(&ctx.engine, &client, &name).await?;
    } else {
        ctx.run(move |e| e.set_identity(&name, None)).await?;
    }
    sync::restart(&app).await?;
    ctx.run(app_state).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NameResult {
    name: String,
    next_name_change: Option<String>,
}

#[tauri::command]
pub async fn change_name(app: AppHandle, ctx: St<'_>, name: String) -> R<NameResult> {
    let name = typeshelf_core::name_rule::validate_name(&name).map_err(|m| EngineError::new("invalid_name", m))?;
    // Not registered (no server, or not reachable since onboarding): the name is only kept here, so it
    // changes freely; restarting sync registers with the new name right away when there is a server.
    if ctx.run(|e| e.token()).await?.is_none() {
        let n = name.clone();
        ctx.run(move |e| e.set_identity(&n, None)).await?;
        sync::restart(&app).await?;
        return Ok(NameResult { name, next_name_change: None });
    }
    let res = ctx.client().change_name(&name).await.map_err(sync_err)?;
    let out = NameResult { name: res.name.clone(), next_name_change: res.next_name_change.map(|d| d.to_rfc3339()) };
    ctx.run(move |e| e.set_identity(&res.name, res.next_name_change)).await?;
    Ok(out)
}

// ---- Library ----

#[tauri::command]
pub async fn list_families(ctx: St<'_>, query: FamilyQuery) -> R<Vec<Family>> {
    ctx.run(move |e| e.list_families(&query)).await
}

#[tauri::command]
pub async fn family_page(ctx: St<'_>, query: FamilyQuery, offset: usize, limit: usize) -> R<FamilyPage> {
    ctx.run(move |e| e.family_page(&query, offset, limit)).await
}

#[tauri::command]
pub async fn query_targets(ctx: St<'_>, query: FamilyQuery) -> R<QueryTargets> {
    ctx.run(move |e| e.query_targets(&query)).await
}

#[tauri::command]
pub async fn edit_tags(ctx: St<'_>, family_keys: Vec<String>, add: Vec<String>, remove: Vec<String>) -> R<usize> {
    ctx.run(move |e| e.edit_tags(&family_keys, &add, &remove)).await
}

#[tauri::command]
pub async fn library_counts(ctx: St<'_>) -> R<LibraryCounts> {
    ctx.run(|e| e.library_counts()).await
}

#[tauri::command]
pub async fn language_counts(ctx: St<'_>) -> R<Vec<LanguageCount>> {
    ctx.run(|e| e.language_counts()).await
}

#[tauri::command]
pub async fn get_faces(ctx: St<'_>, hashes: Vec<String>) -> R<Vec<Face>> {
    ctx.run(move |e| e.faces(&hashes)).await
}

#[tauri::command]
pub async fn folder_tree(ctx: St<'_>) -> R<Vec<FolderNode>> {
    ctx.run(|e| e.folder_tree()).await
}

#[tauri::command]
pub async fn set_folder_active(ctx: St<'_>, path: String, active: bool) -> R<Vec<ActivationResult>> {
    ctx.run(move |e| e.set_folder_active(&path, active)).await
}

#[tauri::command]
pub async fn set_folder_auto(ctx: St<'_>, path: String, on: bool) -> R<Vec<ActivationResult>> {
    ctx.run(move |e| e.set_folder_auto(&path, on)).await
}

#[tauri::command]
pub async fn get_family(ctx: St<'_>, key: String) -> R<FamilyDetail> {
    ctx.run(move |e| e.get_family(&key)).await
}

#[tauri::command]
pub async fn set_active(ctx: St<'_>, hashes: Vec<String>, active: bool) -> R<Vec<ActivationResult>> {
    ctx.run(move |e| e.set_active(&hashes, active)).await
}

#[tauri::command]
pub async fn set_active_for(ctx: St<'_>, hashes: Vec<String>, until: Until) -> R<Vec<ActivationResult>> {
    ctx.run(move |e| e.set_active_for(&hashes, until)).await
}

#[tauri::command]
pub async fn similar_families(ctx: St<'_>, key: String, limit: usize) -> R<Vec<Family>> {
    ctx.run(move |e| e.similar_families(&key, limit)).await
}

#[tauri::command]
pub async fn pairing_suggestions(ctx: St<'_>, key: String) -> R<Vec<Family>> {
    ctx.run(move |e| e.pairing_suggestions(&key)).await
}

#[tauri::command]
pub async fn list_axis_presets(ctx: St<'_>, key: String) -> R<Vec<AxisPreset>> {
    ctx.run(move |e| e.axis_presets(&key)).await
}

#[tauri::command]
pub async fn save_axis_preset(ctx: St<'_>, key: String, name: String, values: std::collections::BTreeMap<String, f32>) -> R<Vec<AxisPreset>> {
    ctx.run(move |e| e.save_axis_preset(&key, &name, &values)).await
}

#[tauri::command]
pub async fn delete_axis_preset(ctx: St<'_>, key: String, name: String) -> R<Vec<AxisPreset>> {
    ctx.run(move |e| e.delete_axis_preset(&key, &name)).await
}

#[tauri::command]
pub async fn set_family_name(ctx: St<'_>, hashes: Vec<String>, name: Option<String>) -> R<()> {
    ctx.run(move |e| e.set_family_name(&hashes, name.as_deref())).await
}

#[tauri::command]
pub async fn delete_fonts(ctx: St<'_>, hashes: Vec<String>) -> R<DeleteResult> {
    ctx.run(move |e| e.delete_fonts(&hashes)).await
}

#[tauri::command]
pub async fn import_paths(ctx: St<'_>, paths: Vec<String>) -> R<ImportResult> {
    ctx.run(move |e| e.import_paths(&paths)).await
}

#[tauri::command]
pub async fn face_glyphs(ctx: St<'_>, hash: String, index: Option<u32>) -> R<GlyphMap> {
    ctx.run(move |e| e.face_glyphs(&hash, index.unwrap_or(0))).await
}

#[tauri::command]
pub async fn glyph_svg(ctx: St<'_>, hash: String, index: Option<u32>, cp: u32) -> R<String> {
    ctx.run(move |e| e.glyph_svg(&hash, index.unwrap_or(0), cp)).await
}

#[tauri::command]
pub async fn set_pinned(ctx: St<'_>, keys: Vec<String>, pinned: bool) -> R<()> {
    ctx.run(move |e| e.set_pinned(&keys, pinned)).await
}

#[tauri::command]
pub async fn quick_search(ctx: St<'_>, query: String) -> R<Vec<Face>> {
    ctx.run(move |e| e.quick_search(&query)).await
}

// ---- Folders ----

#[tauri::command]
pub async fn set_folder_hidden(ctx: St<'_>, path: String, hidden: bool) -> R<Vec<FolderNode>> {
    ctx.run(move |e| e.set_folder_hidden(&path, hidden)).await
}

#[tauri::command]
pub async fn delete_folder(ctx: St<'_>, path: String) -> R<DeleteResult> {
    ctx.run(move |e| e.delete_folder(&path)).await
}

#[tauri::command]
pub async fn list_folders(ctx: St<'_>) -> R<Vec<Folder>> {
    ctx.run(|e| e.list_folders()).await
}

#[tauri::command]
pub async fn add_folder(ctx: St<'_>, path: String) -> R<Vec<Folder>> {
    ctx.run(move |e| e.add_folder(&path)).await
}

#[tauri::command]
pub async fn remove_folder(ctx: St<'_>, id: i64) -> R<Vec<Folder>> {
    ctx.run(move |e| e.remove_folder(id)).await
}

#[tauri::command]
pub async fn locate_folder(ctx: St<'_>, id: i64, path: String) -> R<Vec<Folder>> {
    ctx.run(move |e| e.locate_folder(id, &path)).await
}

// ---- Collections, tags, licenses ----

#[tauri::command]
pub async fn list_collections(ctx: St<'_>) -> R<Vec<Collection>> {
    ctx.run(|e| e.list_collections()).await
}

#[tauri::command]
pub async fn create_collection(ctx: St<'_>, name: String, parent_id: Option<i64>) -> R<Vec<Collection>> {
    ctx.run(move |e| e.create_collection(&name, parent_id)).await
}

#[tauri::command]
pub async fn rename_collection(ctx: St<'_>, id: i64, name: String) -> R<Vec<Collection>> {
    ctx.run(move |e| e.rename_collection(id, &name)).await
}

#[tauri::command]
pub async fn delete_collection(ctx: St<'_>, id: i64) -> R<Vec<Collection>> {
    ctx.run(move |e| e.delete_collection(id)).await
}

#[tauri::command]
pub async fn add_to_collection(ctx: St<'_>, id: i64, family_keys: Vec<String>) -> R<Vec<Collection>> {
    ctx.run(move |e| e.add_to_collection(id, &family_keys)).await
}

#[tauri::command]
pub async fn remove_from_collection(ctx: St<'_>, id: i64, family_keys: Vec<String>) -> R<Vec<Collection>> {
    ctx.run(move |e| e.remove_from_collection(id, &family_keys)).await
}

#[tauri::command]
pub async fn set_collection_active(ctx: St<'_>, id: i64, active: bool) -> R<Vec<ActivationResult>> {
    ctx.run(move |e| e.set_collection_active(id, active)).await
}

#[tauri::command]
pub async fn export_collection(ctx: St<'_>, id: i64, dest: String) -> R<ExportResult> {
    ctx.run(move |e| e.export_collection(id, &dest)).await
}

#[tauri::command]
pub async fn export_collections(ctx: St<'_>, dest: String) -> R<CollectionsExport> {
    ctx.run(move |e| e.export_collections(&dest)).await
}

#[tauri::command]
pub async fn import_collections(ctx: St<'_>, path: String) -> R<CollectionsImport> {
    ctx.run(move |e| e.import_collections(&path)).await
}

#[tauri::command]
pub async fn list_tags(ctx: St<'_>) -> R<Vec<TagCount>> {
    ctx.run(|e| e.list_tags()).await
}

#[tauri::command]
pub async fn set_tags(ctx: St<'_>, family_key: String, tags: Vec<String>) -> R<Vec<String>> {
    ctx.run(move |e| e.set_tags(&family_key, &tags)).await
}

#[tauri::command]
pub async fn set_license(
    ctx: St<'_>,
    family_key: String,
    label: License,
    file: Option<String>,
    note: Option<String>,
) -> R<LicenseInfo> {
    ctx.run(move |e| e.set_license(&family_key, label, file.as_deref(), note.as_deref())).await
}

// ---- Project sets ----

#[tauri::command]
pub async fn list_project_sets(ctx: St<'_>) -> R<Vec<ProjectSet>> {
    ctx.run(|e| e.list_project_sets()).await
}

#[tauri::command]
pub async fn save_project_set(
    ctx: St<'_>,
    id: Option<i64>,
    name: String,
    note: String,
    family_keys: Vec<String>,
) -> R<Vec<ProjectSet>> {
    ctx.run(move |e| e.save_project_set(id, &name, &note, &family_keys)).await
}

#[tauri::command]
pub async fn delete_project_set(ctx: St<'_>, id: i64) -> R<Vec<ProjectSet>> {
    ctx.run(move |e| e.delete_project_set(id)).await
}

#[tauri::command]
pub async fn toggle_project_set(ctx: St<'_>, id: i64, on: bool) -> R<Vec<ActivationResult>> {
    ctx.run(move |e| e.toggle_project_set(id, on)).await
}

// ---- Organize, history, health ----

#[tauri::command]
pub async fn organize_plan(ctx: St<'_>, tidy: bool) -> R<OrganizePlan> {
    ctx.run(move |e| e.organize_plan(tidy)).await
}

#[tauri::command]
pub async fn organize_apply(ctx: St<'_>, tidy: bool, exclude_ids: Vec<i64>) -> R<OrganizeResult> {
    ctx.run(move |e| e.organize_apply(tidy, &exclude_ids)).await
}

#[tauri::command]
pub async fn list_history(ctx: St<'_>) -> R<Vec<HistoryEntry>> {
    ctx.run(|e| e.list_history()).await
}

#[tauri::command]
pub async fn undo_history(ctx: St<'_>, id: i64) -> R<Vec<HistoryEntry>> {
    ctx.run(move |e| e.undo_history(id)).await
}

#[tauri::command]
pub async fn health(ctx: St<'_>) -> R<Health> {
    ctx.run(|e| e.health()).await
}

#[tauri::command]
pub async fn repair_now(ctx: St<'_>) -> R<Health> {
    let health = ctx.run(|e| e.repair_now()).await?;
    // Only a registered install has a server to tell (no token in standalone mode).
    if ctx.run(|e| e.token()).await?.is_some() {
        let (engine, client) = (ctx.engine.clone(), ctx.client());
        tauri::async_runtime::spawn(async move { sync::push_snapshot(&engine, &client).await });
    }
    Ok(health)
}

#[tauri::command]
pub async fn pdf_fonts(ctx: St<'_>, path: String) -> R<Vec<PdfFont>> {
    ctx.run(move |e| e.pdf_fonts(&path)).await
}

#[tauri::command]
pub async fn document_fonts(ctx: St<'_>, path: String) -> R<typeshelf_engine::DocFonts> {
    ctx.run(move |e| e.document_fonts(&path)).await
}

#[tauri::command]
pub async fn usage_stats(ctx: St<'_>, period: UsagePeriod) -> R<UsageStats> {
    ctx.run(move |e| e.usage_stats(period, chrono::Utc::now())).await
}

#[tauri::command]
pub async fn badges(ctx: St<'_>) -> R<Vec<Badge>> {
    ctx.run(|e| e.badges(chrono::Utc::now())).await
}

#[tauri::command]
pub async fn request_font(ctx: St<'_>, name: String, note: Option<String>) -> R<()> {
    ctx.run(move |e| e.request_font(&name, note.as_deref())).await
}

// ---- Google Fonts, backup ----

#[tauri::command]
pub async fn google_catalog(
    ctx: St<'_>,
    search: Option<String>,
    category: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
) -> R<GooglePage> {
    ctx.run(move |e| e.google_catalog(search.as_deref(), category.as_deref(), offset.unwrap_or(0), limit.unwrap_or(30))).await
}

#[tauri::command]
pub async fn google_install(ctx: St<'_>, family: String) -> R<GoogleInstall> {
    ctx.run(move |e| e.google_install(&family)).await
}

#[tauri::command]
pub async fn google_remove(ctx: St<'_>, family: String) -> R<GoogleInstall> {
    ctx.run(move |e| e.google_remove(&family)).await
}

#[tauri::command]
pub async fn backup_create(ctx: St<'_>, path: String) -> R<BackupInfo> {
    ctx.run(move |e| e.backup_create(&path)).await
}

#[tauri::command]
pub async fn backup_restore(ctx: St<'_>, path: String) -> R<BackupInfo> {
    ctx.run(move |e| e.backup_restore(&path)).await
}

// ---- Settings ----

#[tauri::command]
pub async fn get_settings(app: AppHandle, ctx: St<'_>) -> R<crate::update::SettingsView> {
    Ok(crate::update::view(&app, ctx.run(|e| e.get_settings()).await?))
}

/// Saves, then applies the OS side (shortcut, login item); a failed side effect rolls the field back.
#[tauri::command]
pub async fn update_settings(app: AppHandle, ctx: St<'_>, patch: Value) -> R<crate::update::SettingsView> {
    let old = ctx.run(|e| e.get_settings()).await?;
    let new = ctx.run(move |e| e.update_settings(patch)).await?;
    let mut revert = serde_json::Map::new();
    let mut err = None;
    if new.quick_shortcut != old.quick_shortcut {
        if let Err(e) = set_shortcut(&app, &old.quick_shortcut, &new.quick_shortcut) {
            revert.insert("quickShortcut".into(), json!(old.quick_shortcut));
            err = Some(e);
        }
    }
    if new.start_at_login != old.start_at_login {
        if let Err(e) = set_autostart(&app, new.start_at_login) {
            revert.insert("startAtLogin".into(), json!(old.start_at_login));
            err = Some(e);
        }
    }
    match err {
        Some(e) => {
            ctx.run(move |en| en.update_settings(Value::Object(revert))).await?;
            Err(e)
        }
        None => Ok(crate::update::view(&app, new)),
    }
}

// ---- Connection ----

#[tauri::command]
pub async fn get_connection(ctx: St<'_>) -> R<Connection> {
    ctx.run(|e| e.get_connection()).await
}

#[derive(Serialize)]
pub struct TestResult {
    ok: bool,
    error: Option<String>,
}

#[tauri::command]
pub async fn test_connection(ctx: St<'_>, server_url: String, install_key: String) -> R<TestResult> {
    let client = sync::new_client(&ctx.data_dir, server_url.trim().into(), install_key.trim().into(), None);
    Ok(match client.health().await {
        Ok(()) => TestResult { ok: true, error: None },
        Err(e) => TestResult { ok: false, error: Some(sync_err(e).message) },
    })
}

/// A different server/key means a different registration: the token is dropped and `sync::restart`
/// registers again with the stored name. So does saving the same values once the admin removed this computer.
async fn change_connection(app: &AppHandle, ctx: &Ctx, server_url: &str, install_key: &str, password: Option<String>) -> R<Connection> {
    let (url, key) = (server_url.trim().to_string(), install_key.trim().to_string());
    let cleared = url.is_empty();
    let changed = ctx
        .run(move |e| {
            e.authorize_connection_change(password.as_deref())?;
            let c = e.get_connection()?;
            if c.server_url == url && c.install_key == key && c.error.as_deref() != Some(sync::REVOKED) {
                return Ok(false);
            }
            e.set_connection(&url, &key)?;
            // A changed connection is a new install on that server (the UI warns about this).
            e.set_token(None)?;
            e.new_install_id()?;
            Ok(true)
        })
        .await?;
    // An emptied address stays empty after a restart (the installer defaults are not filled in again).
    sync::set_server_cleared(&ctx.data_dir, cleared);
    if changed {
        tracing::info!(server_url, "connection changed");
        sync::restart(app).await?;
    }
    ctx.run(|e| e.get_connection()).await
}

#[tauri::command]
pub async fn save_connection(app: AppHandle, ctx: St<'_>, server_url: String, install_key: String, password: Option<String>) -> R<Connection> {
    change_connection(&app, &ctx, &server_url, &install_key, password).await
}

#[tauri::command]
pub async fn reset_connection(app: AppHandle, ctx: St<'_>, password: Option<String>) -> R<Connection> {
    change_connection(&app, &ctx, DEFAULT_SERVER_URL, DEFAULT_INSTALL_KEY, password).await
}

/// Checks the admin's unlock password without changing anything (the UI then shows the editable form).
#[tauri::command]
pub async fn unlock_connection(ctx: St<'_>, password: String) -> R<()> {
    ctx.run(move |e| e.verify_connection_password(&password)).await
}

pub struct TrayItems {
    pub open: tauri::menu::MenuItem<tauri::Wry>,
    pub quit: tauri::menu::MenuItem<tauri::Wry>,
    pub menu: tauri::menu::Menu<tauri::Wry>,
    /// "Restart to update": not in `menu` until an update is ready.
    pub update: tauri::menu::MenuItem<tauri::Wry>,
    pub shown: std::sync::atomic::AtomicBool,
}

/// Open, Quit, Restart to update. The update label carries no version: it is fixed text.
pub fn tray_labels(indonesian: bool) -> (&'static str, &'static str, &'static str) {
    if indonesian { ("Buka Glyphyard", "Keluar", "Restart untuk update") } else { ("Open Glyphyard", "Quit", "Restart to update") }
}

/// Adds "Restart to update" above Quit, once.
pub fn show_update_item(app: &AppHandle) {
    let items = app.state::<TrayItems>();
    if items.shown.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    if let Err(e) = items.menu.insert(&items.update, 1) {
        items.shown.store(false, std::sync::atomic::Ordering::SeqCst);
        tracing::warn!("tray update item: {e}");
    }
}

#[tauri::command]
pub fn set_tray_language(app: AppHandle, lang: String) -> R<()> {
    let items = app.state::<TrayItems>();
    let (open, quit, update) = tray_labels(lang == "id");
    items.open.set_text(open).map_err(|e| EngineError::new("tray", e.to_string()))?;
    items.update.set_text(update).map_err(|e| EngineError::new("tray", e.to_string()))?;
    items.quit.set_text(quit).map_err(|e| EngineError::new("tray", e.to_string()))?;
    Ok(())
}

// ---- Updates ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckView {
    /// "upToDate" or "ready".
    status: &'static str,
    version: String,
}

/// Settings › "Check for updates": one check now, downloading even when "Update automatically" is off.
#[tauri::command]
pub async fn check_update(app: AppHandle, ctx: St<'_>) -> R<CheckView> {
    let err = |m: &str| EngineError::new("update", m);
    if crate::update::key().is_none() {
        return Err(err("this build has no update key"));
    }
    let (server, token) = ctx.run(|e| Ok((e.get_app_state()?.server_configured, e.token()?.is_some()))).await?;
    if !server || !token {
        return Err(err("connect to your team server first"));
    }
    match crate::update::check_now(&app).await.map_err(|e| err(&e.to_string()))? {
        Some(v) => Ok(CheckView { status: "ready", version: v.to_string() }),
        None => Ok(CheckView { status: "upToDate", version: env!("CARGO_PKG_VERSION").into() }),
    }
}

/// Quits, installs the waiting update and starts the app again (the current one if nothing was installed).
#[tauri::command]
pub fn restart_to_update(app: AppHandle) {
    crate::update::request_restart(&app);
}

// ---- OS integration ----

#[tauri::command]
pub fn show_in_folder(path: String) -> R<()> {
    tauri_plugin_opener::reveal_item_in_dir(&path).map_err(|e| EngineError::new("io", e.to_string()))
}

/// Opens a web link from a font's metadata in the default browser. Only http(s): the URL comes from a font file.
#[tauri::command]
pub fn open_url(url: String) -> R<()> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err(EngineError::new("invalid", "only http and https links can be opened"));
    }
    tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|e| EngineError::new("io", e.to_string()))
}

#[derive(Deserialize)]
pub struct FileFilter {
    name: String,
    extensions: Vec<String>,
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> R<Option<String>> {
    let (tx, rx) = oneshot::channel();
    let mut dialog = app.dialog().file();
    if let Some(w) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&w);
    }
    dialog.pick_folder(move |p| {
        let _ = tx.send(p);
    });
    Ok(rx.await.ok().flatten().and_then(|p| p.into_path().ok()).map(|p| p.display().to_string()))
}

fn file_dialog(app: &AppHandle, filters: Option<Vec<FileFilter>>) -> tauri_plugin_dialog::FileDialogBuilder<tauri::Wry> {
    let mut dialog = app.dialog().file();
    if let Some(w) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&w);
    }
    for f in filters.unwrap_or_default() {
        let ext: Vec<&str> = f.extensions.iter().map(String::as_str).collect();
        dialog = dialog.add_filter(f.name, &ext);
    }
    dialog
}

#[tauri::command]
pub async fn pick_file(app: AppHandle, filters: Option<Vec<FileFilter>>) -> R<Option<String>> {
    let (tx, rx) = oneshot::channel();
    file_dialog(&app, filters).pick_file(move |p| {
        let _ = tx.send(p);
    });
    Ok(rx.await.ok().flatten().and_then(|p| p.into_path().ok()).map(|p| p.display().to_string()))
}

#[tauri::command]
pub async fn pick_files(app: AppHandle, filters: Option<Vec<FileFilter>>) -> R<Option<Vec<String>>> {
    let (tx, rx) = oneshot::channel();
    file_dialog(&app, filters).pick_files(move |p| {
        let _ = tx.send(p);
    });
    Ok(rx
        .await
        .ok()
        .flatten()
        .map(|ps| ps.into_iter().filter_map(|p| p.into_path().ok()).map(|p| p.display().to_string()).collect()))
}

#[tauri::command]
pub async fn pick_save_file(app: AppHandle, default_name: Option<String>, filters: Option<Vec<FileFilter>>) -> R<Option<String>> {
    let (tx, rx) = oneshot::channel();
    let mut dialog = file_dialog(&app, filters);
    if let Some(name) = default_name {
        dialog = dialog.set_file_name(name);
    }
    dialog.save_file(move |p| {
        let _ = tx.send(p);
    });
    Ok(rx.await.ok().flatten().and_then(|p| p.into_path().ok()).map(|p| p.display().to_string()))
}

// ---- Adobe ----

#[derive(Serialize)]
pub struct AdobeRow {
    #[serde(flatten)]
    app: AdobeApp,
    paired: bool,
}

async fn adobe_rows(ctx: &Ctx) -> R<Vec<AdobeRow>> {
    let apps = tauri::async_runtime::spawn_blocking(install::detect_apps)
        .await
        .map_err(|e| EngineError::new("internal", e.to_string()))?;
    let bridge = ctx.bridge.lock().unwrap();
    Ok(apps
        .into_iter()
        .map(|a| AdobeRow { paired: bridge.as_ref().is_some_and(|b| b.is_paired(a.app)), app: a })
        .collect())
}

async fn adobe_row(ctx: &Ctx, app: App) -> R<AdobeRow> {
    adobe_rows(ctx).await?.into_iter().find(|r| r.app.app == app).ok_or_else(|| EngineError::new("not_found", "unknown app"))
}

#[tauri::command]
pub async fn adobe_status(ctx: St<'_>) -> R<Vec<AdobeRow>> {
    adobe_rows(&ctx).await
}

#[tauri::command]
pub async fn adobe_install(handle: AppHandle, ctx: St<'_>, app: App) -> R<AdobeRow> {
    let dist = handle.path().resource_dir().map_err(|e| EngineError::new("io", e.to_string()))?.join("plugins");
    tauri::async_runtime::spawn_blocking(move || install::install(app, &dist))
        .await
        .map_err(|e| EngineError::new("internal", e.to_string()))?
        .map_err(|e| EngineError::new("adobe", e.to_string()))?;
    adobe_row(&ctx, app).await
}

#[tauri::command]
pub async fn adobe_remove(ctx: St<'_>, app: App) -> R<AdobeRow> {
    tauri::async_runtime::spawn_blocking(move || install::remove(app))
        .await
        .map_err(|e| EngineError::new("internal", e.to_string()))?
        .map_err(|e| EngineError::new("adobe", e.to_string()))?;
    if let Some(b) = ctx.bridge.lock().unwrap().as_ref() {
        let _ = b.revoke(app);
    }
    adobe_row(&ctx, app).await
}

#[tauri::command]
pub async fn indesign_running() -> bool {
    tauri::async_runtime::spawn_blocking(install::indesign_running).await.unwrap_or(false)
}

#[tauri::command]
pub fn adobe_pair_respond(ctx: St<'_>, request_id: String, allow: bool) {
    if let Some(tx) = ctx.pairs.lock().unwrap().remove(&request_id) {
        let _ = tx.send(allow);
    }
}
