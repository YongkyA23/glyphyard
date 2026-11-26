// Typed client for the Admin API (docs/api.md, "Admin API").

export type By = 'user' | 'admin' | 'system'
export type Activity = { id: number; install_id: string; install_name: string; kind: string; text: string; by: By; at: string }
export type Flag = 'new' | 'possible_duplicate' | 'override' | 'failures' | 'stale'
export type InstallSummary = {
  id: string; name: string; previous_names: string[]; os: string; app_version: string
  active_fonts: number; last_seen: string; online: boolean; next_name_change: string | null
  flags: Flag[]
}
export type FontSource = 'user' | 'admin' | 'project_set' | 'library'
export type FontState = { hash: string; ps_name: string; family: string; style: string; active: boolean; source: FontSource; overridden: boolean }
export type InstallDetail = InstallSummary & {
  first_seen: string; merged_from: string[]
  name_history: { name: string; from: string; to: string | null }[]
  overrides: { font: FontState; admin_at: string; user_at: string }[]
}
export type TimelineKind = 'name' | 'font' | 'override' | 'failure' | 'organize' | 'system'
export type TimelineEvent = { id: number; at: string; by: By; kind: TimelineKind; text: string }
export type NameChangeBy = 'user' | 'first' | 'merge' | 'admin_reset'
export type NameChangeRow = { at: string; install_ids: string[]; old_name: string | null; new_name: string; by: NameChangeBy; next_allowed: string | null }
export type License = 'licensed' | 'personal' | 'client' | 'unknown'
export type LibraryFile = {
  hash: string; filename: string; ps_name: string; family: string; style: string; version: string
  license: License; size: number; delivered_to: 'all' | 'none' | string[]; active_on: number; blocked: boolean; uploaded_at: string
}
export type BulkAction = 'deliver' | 'block' | 'unblock' | 'delete'
/** Selected files by hash, or every file the library search `q` matches. */
export type BulkSelection = { hashes: string[] } | { q: string }
export type OrganizeChange = { case: string; action: { op: string; [k: string]: unknown }; note: string }
export type Settings = {
  name_change_days: number; allow_connection_edit: boolean; deliver_library_to_new: boolean
  activate_delivered: boolean; notify_admin_changes: boolean; timeline_retention_days: number | null
  server_url: string; install_key: string
  /** Shown in every desktop app; '' = Glyphyard's own. brand_color is '#rrggbb'. */
  brand_name: string; brand_color: string
  /** Image data URL (PNG, JPEG, WebP or SVG, at most 200 KB) or ''. */
  brand_logo: string
}
export type SortDir = 'asc' | 'desc'
export type Sort<K extends string> = { key: K; dir: SortDir } | null
export type InstallSort = 'name' | 'last_seen' | 'fonts'
export type LibrarySort = 'family' | 'uploaded' | 'size'
export type RequestSort = 'at' | 'font'
export type TrendDay = { day: string; activity: number; failures: number; new_installs: number }
export type Overview = {
  /** Last 30 UTC days, oldest first, today last. */
  trend: TrendDay[]
  installs: number; online: number; os: { windows: number; macos: number }
  name_changes_month: number; overrides_week: number; failures_week: number
  recent: Activity[]
  attention: { overrides: number; failures: number; possible_duplicates: number; stale: number; requests: number }
  top_fonts: { family: string; users: number }[]
}
/** "I need this font" from a desktop; `install_id`/`install_name` are the person it belongs to now. */
export type FontRequest = { id: number; install_id: string; install_name: string; font_name: string; note: string | null; at: string; done_at: string | null }
export type RequestState = 'open' | 'done' | 'all'
export type Role = 'owner' | 'admin'
export type Me = { email: string; role: Role }
export type Admin = { id: number; email: string; role: Role; last_login: string | null }
export type AuditEntry = { id: number; at: string; admin: string; action: string; target: string; detail: string }
export type AuditPage = Page<AuditEntry> & { admins: string[]; actions: string[] }
export type UpdateRelease = { version: string; size: number }
export type OrganizeReport = { install_id: string; name: string; duplicates: number; version_conflicts: number; name_clashes: number; last_organized: string | null }
/** One page of a server-side list; `total` counts the whole filtered list. */
export type Page<T> = { items: T[]; total: number }
export type InstallFilter = 'all' | 'online' | 'overrides' | 'duplicates' | 'stale'
export type TimelineFilter = 'all' | 'names' | 'fonts' | 'overrides' | 'failures'

export class ApiError extends Error {
  status: number
  code: string
  constructor(status: number, code: string, message: string) {
    super(message)
    this.status = status
    this.code = code
  }
}

const BASE = '/api/admin'

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const init: RequestInit = { method, credentials: 'include', headers: {} }
  if (body instanceof FormData) init.body = body
  else if (body !== undefined) {
    init.body = JSON.stringify(body)
    init.headers = { 'Content-Type': 'application/json' }
  }
  let res: Response
  try {
    res = await fetch(BASE + path, init)
  } catch {
    throw new ApiError(0, 'network', 'Can’t reach the server. Check your connection and try again.')
  }
  if (res.status === 401 && path !== '/login') {
    if (location.pathname !== '/login') location.assign('/login')
    throw new ApiError(401, 'unauthorized', 'Your session has ended. Sign in again.')
  }
  if (!res.ok) {
    const err = await res.json().catch(() => null) as { error?: string; message?: string } | null
    throw new ApiError(res.status, err?.error ?? 'http_' + res.status, err?.message ?? `Request failed (${res.status}).`)
  }
  if (res.status === 204 || res.headers.get('content-length') === '0') return undefined as T
  const text = await res.text()
  return (text ? JSON.parse(text) : undefined) as T
}

const qs = (params: Record<string, string | number | boolean | undefined>) => {
  const p = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) if (v !== undefined && v !== '') p.set(k, String(v))
  const s = p.toString()
  return s ? '?' + s : ''
}
const id = encodeURIComponent
export const PAGE_SIZE = 50
/** `page` is 1-based. */
const sortParams = (sort?: Sort<string>) => (sort ? { sort: sort.key, dir: sort.dir } : {})
const paging = (page: number, size = PAGE_SIZE) => ({ offset: (page - 1) * size, limit: size })

export const api = {
  login: (email: string, password: string) => request<void>('POST', '/login', { email, password }),
  logout: () => request<void>('POST', '/logout'),
  me: () => request<Me>('GET', '/me'),
  overview: () => request<Overview>('GET', '/overview'),
  installs: (q = '', filter: InstallFilter = 'all', page = 1, sort?: Sort<InstallSort>) =>
    request<Page<InstallSummary>>('GET', '/installs' + qs({ q, filter, ...sortParams(sort), ...paging(page) })),
  install: (installId: string) => request<InstallDetail>('GET', `/installs/${id(installId)}`),
  fonts: (installId: string, q = '', page = 1, active?: boolean) =>
    request<Page<FontState>>('GET', `/installs/${id(installId)}/fonts` + qs({ q, active, ...paging(page) })),
  timeline: (installId: string, kind: TimelineFilter = 'all', page = 1, size = PAGE_SIZE) =>
    request<Page<TimelineEvent>>('GET', `/installs/${id(installId)}/timeline` + qs({ kind, ...paging(page, size) })),
  setFont: (installId: string, hash: string, active: boolean) =>
    request<{ command_id: number | string }>('POST', `/installs/${id(installId)}/fonts`, { hash, active }),
  resetNameLimit: (installId: string) => request<void>('POST', `/installs/${id(installId)}/reset-name-limit`),
  requestOrganize: (installId: string) => request<{ command_id: number | string }>('POST', `/installs/${id(installId)}/organize-request`),
  merge: (primary: string, secondary: string, keep_name: 'primary' | 'secondary') =>
    request<InstallDetail>('POST', '/installs/merge', { primary, secondary, keep_name }),
  split: (retiredId: string) => request<void>('POST', `/installs/${id(retiredId)}/split`),
  removeInstall: (installId: string) => request<void>('DELETE', `/installs/${id(installId)}`),
  nameChanges: (q = '', since = '', page = 1) => request<Page<NameChangeRow>>('GET', '/name-changes' + qs({ q, since, ...paging(page) })),
  nameChangesCsvUrl: (q = '', since = '') => BASE + '/name-changes.csv' + qs({ q, since }),
  library: (q = '', page = 1, size = PAGE_SIZE, sort?: Sort<LibrarySort>) =>
    request<Page<LibraryFile>>('GET', '/library' + qs({ q, ...sortParams(sort), ...paging(page, size) })),
  upload: (files: FileList | File[]) => {
    const form = new FormData()
    for (const f of Array.from(files)) form.append('files', f)
    return request<LibraryFile[]>('POST', '/library', form)
  },
  setLicense: (hash: string, license: License) => request<LibraryFile>('PUT', `/library/${id(hash)}`, { license }),
  block: (hash: string, blocked: boolean) => request<LibraryFile>('POST', `/library/${id(hash)}/block`, { blocked }),
  deliver: (hash: string, target: 'all' | string[], activate: boolean) =>
    request<LibraryFile>('POST', `/library/${id(hash)}/deliver`, { target, activate }),
  deleteFile: (hash: string) => request<void>('DELETE', `/library/${id(hash)}`),
  bulk: (action: BulkAction, selection: BulkSelection, target?: 'all' | string[], activate?: boolean) =>
    request<{ changed: number; skipped: number; failed: number }>('POST', '/library/bulk', { action, ...selection, target, activate }),
  organizePlan: () => request<{ changes: OrganizeChange[] }>('GET', '/library/organize'),
  applyOrganize: () => request<{ applied: number }>('POST', '/library/organize/apply'),
  fontRequests: (state: RequestState = 'open', page = 1, sort?: Sort<RequestSort>) =>
    request<Page<FontRequest>>('GET', '/requests' + qs({ state, ...sortParams(sort), ...paging(page) })),
  setRequestDone: (requestId: number, done: boolean) => request<void>('POST', `/requests/${requestId}/${done ? 'done' : 'reopen'}`),
  organizeReports: (page = 1) => request<Page<OrganizeReport>>('GET', '/organize/reports' + qs(paging(page))),
  settings: () => request<Settings>('GET', '/settings'),
  saveSettings: (s: Partial<Omit<Settings, 'install_key'>>) => request<Settings>('PUT', '/settings', s),
  regenerateKey: () => request<{ install_key: string }>('POST', '/settings/install-key'),
  updateRelease: () => request<UpdateRelease | null>('GET', '/update'),
  /** Fields `file` (setup program), `manifest` and `signature` (from `typeshelf-server update-sign`). */
  uploadUpdate: (form: FormData) => request<UpdateRelease>('POST', '/update', form),
  admins: () => request<Admin[]>('GET', '/admins'),
  addAdmin: (email: string, password: string) => request<void>('POST', '/admins', { email, password }),
  removeAdmin: (adminId: number) => request<void>('DELETE', `/admins/${adminId}`),
  setRole: (adminId: number, role: Role) => request<void>('PUT', `/admins/${adminId}/role`, { role }),
  resetPassword: (adminId: number, password: string) => request<void>('POST', `/admins/${adminId}/password`, { password }),
  changePassword: (current: string, password: string) => request<void>('PUT', '/me/password', { current, password }),
  audit: (admin = '', action = '', page = 1) => request<AuditPage>('GET', '/audit' + qs({ admin, action, ...paging(page) })),
}

export const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e))
