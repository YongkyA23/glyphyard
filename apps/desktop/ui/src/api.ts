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
}
export type Family = {
  key: string; family: string; foundry: string; styles: number; isVariable: boolean; license: License
  activeState: 'all' | 'some' | 'none'; adminSet: boolean; hasDuplicates: boolean; faces: Face[]
  /** Every copy is in an OS font folder. */
  system: boolean
}
export type Axis = { tag: string; min: number; default: number; max: number }
export type FileRow = { path: string; version: string; status: 'active' | 'older_version' | 'alternate' | 'grouped' | 'inactive' | 'system' }
export type FamilyDetail = Family & {
  axes: Axis[]; features: string[]; glyphCount: number; designer: string
  files: FileRow[]; licenseInfo: { label: License; file: string | null; note: string }
}
export type Folder = { id: number; path: string; status: 'watching' | 'offline' | 'scanning'; fontCount: number; offlineSince: string | null }
export type Collection = { id: number; name: string; parentId: number | null; count: number; builtin: 'favorites' | 'recent' | null }
export type ProjectSet = { id: number; name: string; note: string; familyKeys: string[]; on: boolean; lastUsed: string | null }
export type OrganizeCase = 'exact_copy' | 'different_version' | 'other_format' | 'name_clash' | 'variable_static' | 'in_system' | 'junk' | 'tidy'
export type OrganizeChange = { id: number; case: OrganizeCase; action: { op: string; [k: string]: unknown }; note: string }
export type Connection = {
  status: 'connected' | 'offline' | 'error'; serverUrl: string; installKey: string; editable: boolean
  lastSync: string | null; installId: string; error: string | null
}
export type AppState = {
  onboarded: boolean; name: string; nextNameChange: string | null; nameChangeDays: number
  connection: Connection; platform: 'windows' | 'macos'; appVersion: string
}
export type ActivationResult = {
  hash: string; ok: boolean
  error: 'file_missing' | 'in_use' | 'invalid_font' | 'blocked' | 'name_conflict' | 'os' | null; message: string | null
}

export type FamilyFilter = 'all' | 'active' | 'inactive' | 'admin' | 'unknown_license' | 'duplicates' | 'system'
export type FamilySort = 'name' | 'name_desc' | 'date' | 'foundry'
export type ListFamiliesArgs = {
  search?: string; filter?: FamilyFilter; collectionId?: number; folderId?: number; tag?: string; license?: License; sort?: FamilySort
}
export type Tag = { name: string; count: number }
export type OrganizePlan = { changes: OrganizeChange[]; scanned: number; scannedAt: string }
export type HistoryEntry = { id: number; kind: string; summary: string; at: string; undone: boolean }
export type Health = { lastCheck: string; active: number; problems: { hash: string; message: string }[]; removed: number; reregistered: number }
export type PdfFont = {
  psName: string; display: string; pages: number; status: 'active' | 'inactive' | 'version' | 'missing'
  detail: string; hash: string | null; google: boolean
}
export type GoogleFont = { family: string; category: string; styles: number; variable: boolean; installed: boolean }
export type GooglePage = { items: GoogleFont[]; total: number }
export type Settings = {
  startAtLogin: boolean; startHidden: boolean; autoUpdate: boolean; deactivateOnQuit: boolean
  quickShortcut: string; theme: 'system' | 'light' | 'dark'; sampleText: string
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
  completeOnboarding: (name: string) => call<AppState>('complete_onboarding', { name }),
  changeName: (name: string) => call<{ name: string; nextNameChange: string | null }>('change_name', { name }),

  listFamilies: (args: ListFamiliesArgs = {}) => call<Family[]>('list_families', args),
  getFamily: (key: string) => call<FamilyDetail>('get_family', { key }),
  setActive: (hashes: string[], active: boolean, confirmUnknownLicense?: boolean) =>
    call<ActivationResult[]>('set_active', { hashes, active, confirmUnknownLicense }),

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
  toggleProjectSet: (id: number, on: boolean, confirmUnknownLicense?: boolean) =>
    call<ActivationResult[]>('toggle_project_set', { id, on, confirmUnknownLicense }),

  organizePlan: (tidy: boolean) => call<OrganizePlan>('organize_plan', { tidy }),
  organizeApply: (tidy: boolean, excludeIds: number[]) => call<{ historyId: number; applied: number }>('organize_apply', { tidy, excludeIds }),
  listHistory: () => call<HistoryEntry[]>('list_history'),
  undoHistory: (id: number) => call<HistoryEntry[]>('undo_history', { id }),

  health: () => call<Health>('health'),
  repairNow: () => call<Health>('repair_now'),
  pdfFonts: (path: string) => call<PdfFont[]>('pdf_fonts', { path }),

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

  adobeStatus: () => call<AdobeRow[]>('adobe_status'),
  adobeInstall: (app: AdobeApp) => call<AdobeRow>('adobe_install', { app }),
  adobeRemove: (app: AdobeApp) => call<AdobeRow>('adobe_remove', { app }),
  adobePairRespond: (requestId: string, allow: boolean) => call<void>('adobe_pair_respond', { requestId, allow }),
  indesignRunning: () => call<boolean>('indesign_running'),

  pickFolder: () => call<string | null>('pick_folder'),
  pickFile: (filters: FileFilter[]) => call<string | null>('pick_file', { filters }),
}

// ---- Events ----
export type Events = {
  'library-changed': { reason: 'scan' | 'watch' | 'activation' | 'admin' | 'organize' }
  'admin-change': { action: 'activated' | 'deactivated' | 'blocked' | 'delivered'; fonts: { family: string; style: string }[] }
  'organize-requested': Record<string, never>
  'connection-changed': Connection
  'folder-status': Folder
  'name-changed': { name: string; nextNameChange: string | null }
  'adobe-pair-request': { requestId: string; app: AdobeApp; appVersion: string }
  'adobe-activated': { app: AdobeApp; document: string; activated: string[]; missing: string[] }
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
