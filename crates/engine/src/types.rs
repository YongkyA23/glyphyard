//! Serde types matching the TS types in docs/desktop-commands.md.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
pub use typeshelf_core::organize::Change;
pub use typeshelf_core::{Axis, Category, FontDetails, GlyphBlock, GlyphMap};
pub use typeshelf_protocol::Source;

/// Rejection sent to the UI as `{ code, message, ...extra }`.
#[derive(Debug, Clone, Serialize)]
pub struct EngineError {
    pub code: String,
    pub message: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

pub type Result<T, E = EngineError> = std::result::Result<T, E>;

impl EngineError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        EngineError { code: code.into(), message: message.into(), extra: Map::new() }
    }

    pub fn with(mut self, key: &str, value: impl Serialize) -> Self {
        self.extra.insert(key.into(), serde_json::to_value(value).unwrap_or(Value::Null));
        self
    }
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for EngineError {}

impl From<rusqlite::Error> for EngineError {
    fn from(e: rusqlite::Error) -> Self {
        EngineError::new("database", e.to_string())
    }
}

impl From<std::io::Error> for EngineError {
    fn from(e: std::io::Error) -> Self {
        EngineError::new("io", e.to_string())
    }
}

impl From<serde_json::Error> for EngineError {
    fn from(e: serde_json::Error) -> Self {
        EngineError::new("internal", e.to_string())
    }
}

pub(crate) fn not_found(what: &str) -> EngineError {
    EngineError::new("not_found", format!("{what} not found"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum License {
    Licensed,
    Personal,
    Client,
    Unknown,
}

impl License {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            License::Licensed => "licensed",
            License::Personal => "personal",
            License::Client => "client",
            License::Unknown => "unknown",
        }
    }

    pub(crate) fn parse(s: &str) -> License {
        match s {
            "licensed" => License::Licensed,
            "personal" => License::Personal,
            "client" => License::Client,
            _ => License::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Face {
    pub hash: String,
    pub ps_name: String,
    pub family: String,
    pub style: String,
    pub full_name: String,
    pub version: String,
    pub path: String,
    pub active: bool,
    pub source: Option<Source>,
    pub weight: f32,
    pub italic: bool,
    pub is_variable: bool,
    /// Installed in an OS font folder: always on, cannot be turned off here.
    pub system: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveState {
    All,
    Some,
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Family {
    pub key: String,
    pub family: String,
    pub foundry: String,
    pub styles: u32,
    pub is_variable: bool,
    pub license: License,
    pub active_state: ActiveState,
    pub admin_set: bool,
    pub has_duplicates: bool,
    /// Every copy is in an OS font folder.
    pub system: bool,
    /// From the upright, regular-weight face when there is one.
    pub category: Option<Category>,
    pub tags: Vec<String>,
    /// Pinned with `set_pinned`; sorts first in every order.
    pub pinned: bool,
    pub faces: Vec<Face>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Active,
    OlderVersion,
    Alternate,
    Grouped,
    Inactive,
    /// The copy in an OS font folder.
    System,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FileRow {
    pub path: String,
    pub version: String,
    pub status: FileStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LicenseInfo {
    pub label: License,
    pub file: Option<String>,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FamilyDetail {
    #[serde(flatten)]
    pub family: Family,
    pub axes: Vec<Axis>,
    pub features: Vec<String>,
    pub glyph_count: u32,
    pub designer: String,
    pub files: Vec<FileRow>,
    pub license_info: LicenseInfo,
    /// Name table and file facts of the representative face, read from the file.
    pub details: FontDetails,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FamilyFilter {
    #[default]
    All,
    Active,
    Inactive,
    Admin,
    UnknownLicense,
    Duplicates,
    /// Families with a copy in an OS font folder.
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortBy {
    #[default]
    Name,
    NameDesc,
    Date,
    Foundry,
    /// Shuffled by `FamilyQuery::seed`: the same seed gives the same order, so pages line up.
    Random,
}

/// Arguments of `list_families`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FamilyQuery {
    pub search: Option<String>,
    pub filter: FamilyFilter,
    pub collection_id: Option<i64>,
    pub folder_id: Option<i64>,
    pub tag: Option<String>,
    pub license: Option<License>,
    pub sort: SortBy,
    /// Only families with a file in this folder or below it (a node of `folder_tree`).
    pub path: Option<String>,
    /// Only these families (system and hidden families included).
    pub keys: Option<Vec<String>>,
    pub category: Option<Category>,
    /// For `SortBy::Random`. A JS number, so any float works.
    pub seed: Option<f64>,
    /// Also families whose every file is in a hidden folder (implied when `path` is inside one).
    pub include_hidden: bool,
    /// Glyph search: the representative face maps every character (whitespace ignored).
    pub chars: Option<String>,
}

/// One page of `list_families`.
#[derive(Debug, Clone, Serialize)]
pub struct FamilyPage {
    pub items: Vec<Family>,
    pub total: usize,
}

/// Sidebar and status-bar numbers, without sending every family over.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryCounts {
    pub library: usize,
    pub active: usize,
    pub system: usize,
    pub licenses: std::collections::BTreeMap<String, usize>,
}

/// A watched folder or a subfolder of one that holds fonts.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderNode {
    pub path: String,
    pub name: String,
    pub font_count: u32,
    pub active_count: u32,
    /// New fonts in this folder are turned on automatically.
    pub auto_activate: bool,
    /// Hidden by `set_folder_hidden` (its subfolders are hidden with it).
    pub hidden: bool,
    pub children: Vec<FolderNode>,
}

/// `export_collection`: the zip written.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportResult {
    pub files: u32,
    pub path: String,
}

/// `export_collections`: collections written (nested ones included).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CollectionsExport {
    pub collections: u32,
}

/// `import_collections`: collections merged or created, family entries matched, and entries with
/// no matching family in the library.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CollectionsImport {
    pub collections: u32,
    pub families: u32,
    pub missing: u32,
}

/// `import_paths`: font files copied into "Imported fonts" and those already in the library.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImportResult {
    pub imported: u32,
    pub skipped: u32,
    pub folder: String,
}

/// `delete_fonts` / `delete_folder`: files sent to the Recycle Bin, and "path: reason" for the rest.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeleteResult {
    pub trashed: u32,
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderStatus {
    Watching,
    Offline,
    Scanning,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: i64,
    pub path: String,
    pub status: FolderStatus,
    pub font_count: u32,
    pub offline_since: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Builtin {
    Favorites,
    Recent,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    pub count: u32,
    pub builtin: Option<Builtin>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TagCount {
    pub name: String,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSet {
    pub id: i64,
    pub name: String,
    pub note: String,
    pub family_keys: Vec<String>,
    pub on: bool,
    pub last_used: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationErrorCode {
    FileMissing,
    InUse,
    InvalidFont,
    Blocked,
    NameConflict,
    Os,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActivationResult {
    pub hash: String,
    pub ok: bool,
    pub error: Option<ActivationErrorCode>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OrganizeChange {
    pub id: i64,
    #[serde(flatten)]
    pub change: Change,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizePlan {
    /// Things Apply will do.
    pub changes: Vec<OrganizeChange>,
    /// Findings that need no file change (kept as alternate, grouped, already in the system).
    pub notes: Vec<OrganizeChange>,
    pub scanned: u32,
    pub scanned_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizeResult {
    pub history_id: i64,
    pub applied: u32,
    /// Changes that could not be done, as "file: reason" (at most 20).
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub kind: String,
    pub summary: String,
    pub at: String,
    pub undone: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Problem {
    pub hash: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub last_check: Option<String>,
    pub active: u32,
    pub problems: Vec<Problem>,
    pub removed: u32,
    pub reregistered: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PdfFontStatus {
    Active,
    Inactive,
    Version,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfFont {
    pub ps_name: String,
    pub display: String,
    pub pages: u32,
    pub status: PdfFontStatus,
    pub detail: String,
    pub hash: Option<String>,
    pub google: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GoogleFamily {
    pub family: String,
    pub category: String,
    pub styles: u32,
    pub variable: bool,
    pub installed: bool,
}

/// One page of the Google catalog; `total` counts every match so the UI knows when to stop.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GooglePage {
    pub items: Vec<GoogleFamily>,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GoogleInstall {
    pub family: String,
    pub installed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BackupInfo {
    pub path: String,
    pub at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    System,
    Light,
    Dark,
}

/// Stored settings missing a field (written by an older version) take its default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub start_at_login: bool,
    pub start_hidden: bool,
    pub auto_update: bool,
    pub deactivate_on_quit: bool,
    pub quick_shortcut: String,
    pub theme: Theme,
    /// The current sample text.
    pub sample_text: String,
    /// Presets for the sample picker.
    pub sample_texts: Vec<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            start_at_login: true,
            start_hidden: false,
            auto_update: true,
            deactivate_on_quit: false,
            quick_shortcut: "CmdOrCtrl+Alt+Space".into(),
            theme: Theme::System,
            sample_text: "The quick brown fox jumps over the lazy dog".into(),
            sample_texts: [
                "The quick brown fox jumps over the lazy dog",
                "ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz",
                "0123456789 !\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~",
                "Hamburgefonstiv",
                "Type sets the tone before a single word is read. A good face stays out of the way and keeps its shape at every size.",
            ]
            .map(String::from)
            .to_vec(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    Connected,
    Offline,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub status: ConnectionStatus,
    pub server_url: String,
    pub install_key: String,
    pub editable: bool,
    pub last_sync: Option<String>,
    pub install_id: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Windows,
    Macos,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub onboarded: bool,
    pub name: String,
    pub next_name_change: Option<String>,
    pub name_change_days: u32,
    pub connection: Connection,
    pub platform: Platform,
    pub app_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FontLabel {
    pub family: String,
    pub style: String,
}

/// Backend → UI events. Serializes as `{ "event": <name>, "payload": {...} }`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", content = "payload", rename_all = "kebab-case")]
pub enum UiEvent {
    LibraryChanged { reason: &'static str },
    AdminChange { action: &'static str, fonts: Vec<FontLabel> },
    OrganizeRequested {},
    FolderStatus(Folder),
    /// Progress of a big on/off batch (more than one chunk).
    ActivationProgress { done: usize, total: usize, active: bool },
}

impl UiEvent {
    pub fn name(&self) -> &'static str {
        match self {
            UiEvent::LibraryChanged { .. } => "library-changed",
            UiEvent::AdminChange { .. } => "admin-change",
            UiEvent::OrganizeRequested {} => "organize-requested",
            UiEvent::FolderStatus(_) => "folder-status",
            UiEvent::ActivationProgress { .. } => "activation-progress",
        }
    }

    pub fn payload(&self) -> Value {
        serde_json::to_value(self).map(|mut v| v["payload"].take()).unwrap_or(Value::Null)
    }
}
