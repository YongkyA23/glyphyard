// Typed wrapper for every command and event in docs/desktop-commands.md.
// Outside Tauri (plain `bun run dev` in a browser) calls go to the in-memory mock.
import { invoke as tauriInvoke } from '@tauri-apps/api/core'
import { listen as tauriListen } from '@tauri-apps/api/event'

// ---- Types (copied from the contract) ----
export type License = 'licensed' | 'personal' | 'client' | 'unknown'
export type Source = 'user' | 'admin' | 'project_set' | 'library'
export type Face = {
  hash: string; psName: string; family: string; style: string; fullName: string; version: string
  path: string; active: boolean; source: Source | null; weight: number; italic: boolean; isVariable: boolean
  /** Installed in an OS font folder: always on. */
  system: boolean
  /** On temporarily: 'restart' or the ISO time it turns off. */
  temporaryUntil?: string | null
}
/** How long `setActiveFor` keeps fonts on. */
export type Until = 'restart' | { minutes: number }
export type FontCategory = 'serif' | 'sans' | 'display' | 'handwriting' | 'mono'
export type Family = {
  key: string; family: string; foundry: string; styles: number; isVariable: boolean; license: License
  activeState: 'all' | 'some' | 'none'; adminSet: boolean; hasDuplicates: boolean; faces: Face[]
  /** Every copy is in an OS font folder. */
  system: boolean
  /** From the family's representative face. */
  category: FontCategory | null
  tags: string[]
  /** Sorts first in every sort order. */
  pinned: boolean
}
export type Axis = { tag: string; min: number; default: number; max: number }
export type FileRow = { path: string; version: string; status: 'active' | 'older_version' | 'alternate' | 'grouped' | 'inactive' | 'system' }
export type FamilyDetail = Family & {
  axes: Axis[]; features: string[]; glyphCount: number; designer: string
  files: FileRow[]; licenseInfo: { label: License; file: string | null; note: string }
  details: FontDetails
}
/** From the font's name/head/maxp tables. */
export type FontDetails = {
  copyright: string | null; trademark: string | null; manufacturer: string | null; designer: string | null
  description: string | null; license: string | null; licenseUrl: string | null
  vendorUrl: string | null; designerUrl: string | null; version: string | null
  uniqueId: string | null; format: string; upm: number | null; glyphCount: number
}
/** A Unicode block and the codepoints the face maps in it. */
export type GlyphBlock = { name: string; start: number; end: number; chars: number[] }
export type Folder = { id: number; path: string; status: 'watching' | 'offline' | 'scanning'; fontCount: number; offlineSince: string | null }
export type Collection = { id: number; name: string; parentId: number | null; count: number; /** Families with a font on. */ active: number; builtin: 'favorites' | 'recent' | null }
export type ProjectSet = { id: number; name: string; note: string; familyKeys: string[]; on: boolean; lastUsed: string | null }
export type OrganizeCase = 'exact_copy' | 'different_version' | 'other_format' | 'name_clash' | 'variable_static' | 'in_system' | 'junk' | 'tidy'
export type OrganizeChange = { id: number; case: OrganizeCase; action: { op: string; [k: string]: unknown }; note: string }
export type Connection = {
  status: 'connected' | 'offline' | 'error'; serverUrl: string; installKey: string; editable: boolean
  lastSync: string | null; installId: string; error: string | null
}
export type AppState = {
  onboarded: boolean; name: string; nextNameChange: string | null; nameChangeDays: number
  /** False in standalone mode (no server address): hide what needs the server (admin requests, name rule). */
  serverConfigured: boolean
  connection: Connection; platform: 'windows' | 'macos'; appVersion: string
}
export type ActivationResult = {
  hash: string; ok: boolean
  error: 'file_missing' | 'in_use' | 'invalid_font' | 'blocked' | 'name_conflict' | 'os' | null; message: string | null
}

export type FamilyFilter = 'all' | 'active' | 'inactive' | 'admin' | 'unknown_license' | 'duplicates' | 'system'
export type FamilySort = 'name' | 'name_desc' | 'date' | 'foundry' | 'random'
export type ListFamiliesArgs = {
  search?: string; filter?: FamilyFilter; collectionId?: number; folderId?: number; tag?: string; license?: License; sort?: FamilySort
  /** Folder or subfolder path from folderTree. */ path?: string; keys?: string[]
  category?: FontCategory
  /** With sort 'random': same seed, same order, so paging is stable. */ seed?: number
  /** Default false: fonts in hidden folders are left out unless `path` points inside one. */ includeHidden?: boolean
  /** Glyph search: only families whose representative face has every one of these characters. */ chars?: string
  /** Only families whose representative face fully covers this language (an id from languageCounts). */ lang?: string
}
export type FamilyPage = { items: Family[]; total: number }
/** Local usage statistics (never sent anywhere). `days`: days on which a font of the family was turned on. */
export type UsageStats = {
  period: 'month' | 'year'; since: string; mostUsed: { key: string; family: string; days: number }[]
  neverUsed: { key: string; family: string }[]; neverUsedNote: string | null
  totalFonts: number; addedInPeriod: number; recordingSince: string | null
}
export type Badge = { id: 'collector' | 'archaeologist' | 'quiet'; name: string; description: string; earnedAt: string | null }
export type LibraryCounts = { library: number; active: number; system: number; licenses: Partial<Record<License, number>> }
export type LanguageCount = { id: string; name: string; families: number }
export type FolderNode = { path: string; name: string; fontCount: number; activeCount: number; autoActivate: boolean; hidden: boolean; children: FolderNode[] }
export type Tag = { name: string; count: number }
export type OrganizePlan = {
  /** What Apply will do. */ changes: OrganizeChange[]
  /** Findings that change no file (alternates, groups, already in system). */ notes: OrganizeChange[]
  scanned: number; scannedAt: string
}
export type HistoryEntry = { id: number; kind: string; summary: string; at: string; undone: boolean }
export type Health = { lastCheck: string; active: number; problems: { hash: string; message: string }[]; removed: number; reregistered: number }
export type PdfFont = {
  psName: string; display: string; pages: number; status: 'active' | 'inactive' | 'version' | 'missing'
  detail: string; hash: string | null; google: boolean
}
export type DocFonts = { fonts: PdfFont[]; partial: string | null }
export type GoogleFont = { family: string; category: string; styles: number; variable: boolean; installed: boolean }
export type GooglePage = { items: GoogleFont[]; total: number }
export type Settings = {
  startAtLogin: boolean; startHidden: boolean; autoUpdate: boolean; deactivateOnQuit: boolean
  quickShortcut: string; theme: 'system' | 'light' | 'dark'; sampleText: string
  /** Presets for the sample text picker. */ sampleTexts: string[]
}
export type AdobeApp = 'photoshop' | 'illustrator' | 'indesign'
export type AdobeRow = { app: AdobeApp; label: string; versions: string[]; installed: boolean; paired: boolean }
/** Not specified by the contract; we send Tauri dialog-style filters. */
export type FileFilter = { name: string; extensions: string[] }

/** Every rejection is normalised to this shape. */
export type ApiError = { code: string; message: string; [k: string]: unknown }

export const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

let mockMod: Promise<typeof import('./mock')> | null = null
const mock = () => (mockMod ??= import('./mock'))

function toApiError(e: unknown): ApiError {
  if (e && typeof e === 'object' && 'code' in e) return e as ApiError
  return { code: 'unknown', message: typeof e === 'string' ? e : e instanceof Error ? e.message : 'Something went wrong.' }
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return inTauri ? await tauriInvoke<T>(cmd, args) : await (await mock()).mockInvoke<T>(cmd, args ?? {})
  } catch (e) {
    throw toApiError(e)
  }
}

export const isApiError = (e: unknown, code?: string): e is ApiError =>
  !!e && typeof e === 'object' && 'code' in e && (code === undefined || (e as ApiError).code === code)

// ---- Commands ----
export const api = {
  getAppState: () => call<AppState>('get_app_state'),
  /** `offline`: keep the name here and register once the server answers ("Continue offline"). */
  completeOnboarding: (name: string, offline = false) => call<AppState>('complete_onboarding', { name, offline }),
  changeName: (name: string) => call<{ name: string; nextNameChange: string | null }>('change_name', { name }),

  listFamilies: (query: ListFamiliesArgs = {}) => call<Family[]>('list_families', { query }),
  familyPage: (query: ListFamiliesArgs, offset: number, limit: number) => call<FamilyPage>('family_page', { query, offset, limit }),
  libraryCounts: () => call<LibraryCounts>('library_counts'),
  languageCounts: () => call<LanguageCount[]>('language_counts'),
  getFaces: (hashes: string[]) => call<Face[]>('get_faces', { hashes }),
  folderTree: () => call<FolderNode[]>('folder_tree'),
  setFolderActive: (path: string, active: boolean) =>
    call<ActivationResult[]>('set_folder_active', { path, active }),
  setFolderAuto: (path: string, on: boolean) =>
    call<ActivationResult[]>('set_folder_auto', { path, on }),
  getFamily: (key: string) => call<FamilyDetail>('get_family', { key }),
  setActive: (hashes: string[], active: boolean) =>
    call<ActivationResult[]>('set_active', { hashes, active }),
  setActiveFor: (hashes: string[], until: Until) =>
    call<ActivationResult[]>('set_active_for', { hashes, until }),

  listFolders: () => call<Folder[]>('list_folders'),
  addFolder: (path: string) => call<Folder[]>('add_folder', { path }),
  removeFolder: (id: number) => call<Folder[]>('remove_folder', { id }),
  locateFolder: (id: number, path: string) => call<Folder[]>('locate_folder', { id, path }),

  listCollections: () => call<Collection[]>('list_collections'),
  createCollection: (name: string, parentId?: number | null) => call<Collection[]>('create_collection', { name, parentId }),
  renameCollection: (id: number, name: string) => call<Collection[]>('rename_collection', { id, name }),
  deleteCollection: (id: number) => call<Collection[]>('delete_collection', { id }),
  addToCollection: (id: number, familyKeys: string[]) => call<Collection[]>('add_to_collection', { id, familyKeys }),
  removeFromCollection: (id: number, familyKeys: string[]) => call<Collection[]>('remove_from_collection', { id, familyKeys }),

  listTags: () => call<Tag[]>('list_tags'),
  setTags: (familyKey: string, tags: string[]) => call<string[]>('set_tags', { familyKey, tags }),
  setLicense: (familyKey: string, label: License, file?: string | null, note?: string) =>
    call<FamilyDetail['licenseInfo']>('set_license', { familyKey, label, file, note }),

  listProjectSets: () => call<ProjectSet[]>('list_project_sets'),
  saveProjectSet: (set: { id?: number; name: string; note: string; familyKeys: string[] }) => call<ProjectSet[]>('save_project_set', set),
  deleteProjectSet: (id: number) => call<ProjectSet[]>('delete_project_set', { id }),
  toggleProjectSet: (id: number, on: boolean) =>
    call<ActivationResult[]>('toggle_project_set', { id, on }),

  organizePlan: (tidy: boolean) => call<OrganizePlan>('organize_plan', { tidy }),
  organizeApply: (tidy: boolean, excludeIds: number[]) => call<{ historyId: number; applied: number; failed: string[] }>('organize_apply', { tidy, excludeIds }),
  listHistory: () => call<HistoryEntry[]>('list_history'),
  undoHistory: (id: number) => call<HistoryEntry[]>('undo_history', { id }),

  health: () => call<Health>('health'),
  repairNow: () => call<Health>('repair_now'),
  pdfFonts: (path: string) => call<PdfFont[]>('pdf_fonts', { path }),
  usageStats: (period: 'month' | 'year') => call<UsageStats>('usage_stats', { period }),
  badges: () => call<Badge[]>('badges'),
  documentFonts: (path: string) => call<DocFonts>('document_fonts', { path }),
  requestFont: (name: string, note: string | null) => call<void>('request_font', { name, note }),

  googleCatalog: (search: string | undefined, category: string | undefined, offset: number, limit: number) =>
    call<GooglePage>('google_catalog', { search, category, offset, limit }),
  googleInstall: (family: string) => call<{ family: string; installed: boolean }>('google_install', { family }),
  googleRemove: (family: string) => call<{ family: string; installed: boolean }>('google_remove', { family }),

  backupCreate: (path: string) => call<{ path: string; at: string }>('backup_create', { path }),
  backupRestore: (path: string) => call<{ path: string; at: string }>('backup_restore', { path }),

  getSettings: () => call<Settings>('get_settings'),
  updateSettings: (patch: Partial<Settings>) => call<Settings>('update_settings', { patch }),

  getConnection: () => call<Connection>('get_connection'),
  testConnection: (serverUrl: string, installKey: string) => call<{ ok: boolean; error: string | null }>('test_connection', { serverUrl, installKey }),
  saveConnection: (serverUrl: string, installKey: string) => call<Connection>('save_connection', { serverUrl, installKey }),
  resetConnection: () => call<Connection>('reset_connection'),

  quickSearch: (query: string) => call<Face[]>('quick_search', { query }),
  showInFolder: (path: string) => call<void>('show_in_folder', { path }),
  openUrl: (url: string) => call<void>('open_url', { url }),

  adobeStatus: () => call<AdobeRow[]>('adobe_status'),
  adobeInstall: (app: AdobeApp) => call<AdobeRow>('adobe_install', { app }),
  adobeRemove: (app: AdobeApp) => call<AdobeRow>('adobe_remove', { app }),
  adobePairRespond: (requestId: string, allow: boolean) => call<void>('adobe_pair_respond', { requestId, allow }),
  indesignRunning: () => call<boolean>('indesign_running'),

  pickFolder: () => call<string | null>('pick_folder'),
  pickFile: (filters: FileFilter[]) => call<string | null>('pick_file', { filters }),
  pickSaveFile: (defaultName: string, filters: FileFilter[]) => call<string | null>('pick_save_file', { defaultName, filters }),

  similarFamilies: (key: string, limit: number) => call<Family[]>('similar_families', { key, limit }),
  setCollectionActive: (id: number, active: boolean) =>
    call<ActivationResult[]>('set_collection_active', { id, active }),
  exportCollection: (id: number, dest: string) => call<{ files: number; path: string }>('export_collection', { id, dest }),
  importPaths: (paths: string[]) => call<{ imported: number; skipped: number; folder: string }>('import_paths', { paths }),
  /** Show these files under family `name`; null restores the real family. Never edits the files. */
  setFamilyName: (hashes: string[], name: string | null) => call<void>('set_family_name', { hashes, name }),
  deleteFonts: (hashes: string[]) => call<{ trashed: number; failed: string[] }>('delete_fonts', { hashes }),
  setFolderHidden: (path: string, hidden: boolean) => call<FolderNode[]>('set_folder_hidden', { path, hidden }),
  /** Turns the fonts off and moves the whole folder to the Recycle Bin / Trash; a watched root also leaves the library. */
  deleteFolder: (path: string) => call<{ trashed: number; failed: string[] }>('delete_folder', { path }),

  /** `index`: face inside a TTC (default 0). */
  faceGlyphs: (hash: string, index?: number) => call<{ total: number; blocks: GlyphBlock[] }>('face_glyphs', { hash, index }),
  setPinned: (keys: string[], pinned: boolean) => call<void>('set_pinned', { keys, pinned }),
  pickFiles: (filters: FileFilter[]) => call<string[] | null>('pick_files', { filters }),
  exportCollections: (dest: string) => call<{ collections: number }>('export_collections', { dest }),
  importCollections: (path: string) => call<{ collections: number; families: number; missing: number }>('import_collections', { path }),
}

/** Each family's tags. The contract has no per-family read, so this asks once per existing tag. */
// ponytail: one list_families per tag; add `tags` to Family/FamilyDetail if tag lists grow large.
export async function tagsOf(keys: string[]): Promise<Map<string, string[]>> {
  const out = new Map(keys.map((k) => [k, [] as string[]]))
  for (const f of await api.listFamilies({ keys, includeHidden: true })) out.set(f.key, f.tags)
  return out
}

// ---- Events ----
export type Events = {
  'library-changed': { reason: 'scan' | 'watch' | 'activation' | 'admin' | 'organize' }
  'admin-change': { action: 'activated' | 'deactivated' | 'blocked' | 'delivered'; fonts: { family: string; style: string }[] }
  'organize-requested': Record<string, never>
  'connection-changed': Connection
  'folder-status': Folder
  'activation-progress': { done: number; total: number; active: boolean }
  'activation-done': { total: number; failed: string[] }
  'badge-earned': Badge
  'name-changed': { name: string; nextNameChange: string | null }
  'adobe-pair-request': { requestId: string; app: AdobeApp; appVersion: string }
  'adobe-activated': { app: AdobeApp; document: string; activated: string[]; missing: string[] }
  'events-dropped': { count: number }
}

/** Subscribe; returns an unsubscribe function usable directly as a useEffect cleanup. */
export function on<E extends keyof Events>(event: E, cb: (payload: Events[E]) => void): () => void {
  let off: (() => void) | null = null
  let cancelled = false
  const sub = inTauri
    ? tauriListen<Events[E]>(event, (e) => cb(e.payload))
    : mock().then((m) => m.mockListen(event, cb as (p: unknown) => void))
  sub.then((u) => (cancelled ? u() : (off = u)))
  return () => {
    cancelled = true
    off?.()
  }
}
