// Dev-only fixtures (?mock=1). Patches fetch for /api/admin so pages render without a server.
import type { AuditEntry, FamilyRow, FontRequest, FontState, InstallDetail, InstallSummary, LibraryFile, Overview, Page, Settings } from './api'

const now = Date.now()
const at = (mins: number) => new Date(now - mins * 60000).toISOString()
const day = 1440

const installs: InstallSummary[] = [
  { id: '7F3A-2C9E-91C2', name: 'Budi Santoso', previous_names: ['Budi S.'], os: 'Windows 11', app_version: '1.0.0', active_fonts: 86, last_seen: at(1), online: true, next_name_change: at(-15 * day), flags: ['override'] },
  { id: '1B77-40DA-0E15', name: 'Sari Wulandari', previous_names: ['Sari W.'], os: 'Windows 11', app_version: '1.0.0', active_fonts: 54, last_seen: at(2), online: true, next_name_change: at(-30 * day), flags: [] },
  { id: 'C902-7E11-5A3B', name: 'Rizky Pratama', previous_names: [], os: 'Windows 10', app_version: '1.0.0', active_fonts: 112, last_seen: at(3), online: true, next_name_change: null, flags: ['failures'] },
  { id: 'E41D-03B7-7C6A', name: 'Budi S.', previous_names: [], os: 'macOS · Intel', app_version: '1.0.0', active_fonts: 0, last_seen: at(3 * day), online: false, next_name_change: null, flags: ['possible_duplicate'] },
  { id: '9AF0-6D25-3E8B', name: 'Putri Ananda', previous_names: [], os: 'Windows 11', app_version: '0.9.2', active_fonts: 61, last_seen: at(16 * day), online: false, next_name_change: null, flags: ['stale'] },
  // Filler so the pagers have several pages.
  ...Array.from({ length: 130 }, (_, i): InstallSummary => ({
    id: `F${String(i).padStart(3, '0')}-0000-0000`, name: `Designer ${i + 1}`, previous_names: [], os: i % 9 ? 'Windows 11' : 'macOS · Apple silicon',
    app_version: '1.0.0', active_fonts: (i * 7) % 90, last_seen: at(i * 37), online: i % 4 === 0, next_name_change: null, flags: i % 11 ? [] : ['stale'],
  })),
]
const FAMILIES = ['Sample Sans', 'Aster Serif', 'Boreal Mono', 'Cinder', 'Dovetail', 'Ember Display', 'Fjord', 'Gable', 'Harbor Slab', 'Iris', 'Juniper', 'Kestrel', 'Lumen', 'Marlow', 'Nimbus', 'Onyx Condensed', 'Pavilion', 'Quarry', 'Reed', 'Saffron', 'Tundra', 'Umber', 'Vesper', 'Willow', 'Yarrow']
const STYLES = ['Regular', 'Italic', 'Bold', 'Bold Italic', 'Light', 'Medium', 'SemiBold', 'Black']
const SOURCES: FontState['source'][] = ['user', 'user', 'admin', 'library', 'project_set']
const mk = (hash: string, family: string, style: string, active: boolean, source: FontState['source'], mins: number, overridden = false, in_library = false, blocked = false): FontState =>
  ({ hash, ps_name: `${family.replace(/ /g, '')}-${style.replace(/ /g, '')}`, family, style, active, source, overridden, updated_at: at(mins), in_library, blocked })
const fonts: FontState[] = [
  mk('h1', 'Fraunces', 'Regular', false, 'admin', 1440, true),
  mk('h2', 'Space Grotesk', 'Medium', true, 'admin', 30, false, true),
  mk('h3', 'IBM Plex Mono', 'Medium', true, 'user', 90),
  mk('h4', 'Syne', 'SemiBold', true, 'project_set', 200, false, true),
  ...Array.from({ length: 160 }, (_, i) => {
    const fam = i % FAMILIES.length
    const n = 4 + (fam % 5)
    const style = STYLES[Math.floor(i / FAMILIES.length) % n]
    const source = SOURCES[(fam + i) % SOURCES.length]
    return mk(`hf${i}`, FAMILIES[fam], style, i % 3 !== 0, source, 5 + ((i * 53) % 4000), source === 'admin' && i % 7 === 0, source === 'library', source === 'library' && i % 6 === 0)
  }),
]
const detail = (s: InstallSummary): InstallDetail => ({
  ...s, first_seen: at(26 * day), merged_from: [],
  name_history: [{ name: s.name, from: at(15 * day), to: null }, ...s.previous_names.map((n) => ({ name: n, from: at(26 * day), to: at(15 * day) }))],
  overrides: s.flags.includes('override') ? [{ font: fonts[0], admin_at: at(day), user_at: at(5) }] : [],
})
const library: LibraryFile[] = [
  { hash: 'h2', filename: 'SpaceGrotesk-Medium.otf', ps_name: 'SpaceGrotesk-Medium', family: 'Space Grotesk', style: 'Medium', version: '2.0', license: 'licensed', size: 84000, delivered_to: 'all', active_on: 41, blocked: false, uploaded_at: at(9 * day) },
  { hash: 'h4', filename: 'Syne-SemiBold.otf', ps_name: 'Syne-SemiBold', family: 'Syne', style: 'SemiBold', version: '2.0', license: 'client', size: 52000, delivered_to: ['7F3A-2C9E-91C2'], active_on: 12, blocked: false, uploaded_at: at(9 * day) },
  { hash: 'h9', filename: 'BebasNeue.otf', ps_name: 'BebasNeue', family: 'Bebas Neue', style: 'Regular', version: '1.0', license: 'unknown', size: 31000, delivered_to: 'none', active_on: 0, blocked: true, uploaded_at: at(2 * day) },
]
const overview: Overview = {
  installs: 48, online: 31, os: { windows: 46, macos: 2 }, name_changes_month: 6, overrides_week: 4, failures_week: 3,
  recent: [
    { id: 1, install_id: installs[0].id, install_name: 'Budi Santoso', kind: 'override', text: 'Budi Santoso turned off Fraunces, which you turned on', by: 'user', at: at(5) },
    { id: 2, install_id: installs[2].id, install_name: 'Rizky Pratama', kind: 'failure', text: 'Montserrat Bold failed to activate for Rizky Pratama (file missing)', by: 'system', at: at(60) },
    { id: 3, install_id: installs[1].id, install_name: 'Sari', kind: 'font', text: 'You turned on Space Grotesk for 12 users', by: 'admin', at: at(130) },
  ],
  attention: { overrides: 4, failures: 3, possible_duplicates: 2, stale: 5, requests: 2 },
  top_fonts: [{ family: 'Space Grotesk', users: 41 }, { family: 'Fraunces', users: 33 }, { family: 'Syne', users: 19 }],
  trend: Array.from({ length: 30 }, (_, i) => ({
    day: new Date(now - (29 - i) * day * 60000).toISOString().slice(0, 10),
    activity: 8 + ((i * 7) % 13),
    failures: i % 6 === 0 ? 2 : 0,
    new_installs: i % 5 === 0 ? 1 : 0,
  })),
}
const fontRequests: FontRequest[] = [
  { id: 3, install_id: installs[1].id, install_name: 'Sari Wulandari', font_name: 'Söhne Buch', note: 'For the Acme rebrand deck, due Friday.', at: at(40), done_at: null },
  { id: 2, install_id: installs[0].id, install_name: 'Budi Santoso', font_name: 'GT America Condensed', note: null, at: at(2 * day), done_at: null },
  { id: 1, install_id: installs[2].id, install_name: 'Rizky Pratama', font_name: 'Founders Grotesk', note: 'Client sent a PDF that uses it.', at: at(6 * day), done_at: at(5 * day) },
]
let settings: Settings = {
  name_change_days: 30, allow_connection_edit: true, deliver_library_to_new: true, activate_delivered: false,
  notify_admin_changes: true, timeline_retention_days: null, server_url: 'https://fonts.example.test/api', install_key: 'tsk_example_key_1234',
  brand_name: '', brand_color: '', brand_logo: '', connection_password_set: false,
}

const audit: AuditEntry[] = [
  { id: 8, at: at(2), admin: 'admin@example.test', action: 'connection_password_cleared', target: '', detail: '' },
  { id: 7, at: at(3), admin: 'admin@example.test', action: 'connection_password_set', target: '', detail: '' },
  { id: 6, at: at(4), admin: 'admin@example.test', action: 'font_delivered', target: 'h2', detail: 'SpaceGrotesk-Medium to all' },
  { id: 5, at: at(40), admin: 'second@example.test', action: 'font_blocked', target: 'h9', detail: 'BebasNeue' },
  { id: 4, at: at(90), admin: 'second@example.test', action: 'sign_in', target: '', detail: '' },
  { id: 3, at: at(day), admin: 'admin@example.test', action: 'settings_changed', target: '', detail: 'name_change_days: 14 → 30' },
  { id: 2, at: at(2 * day), admin: 'admin@example.test', action: 'admin_added', target: 'second@example.test', detail: 'role admin' },
  { id: 1, at: at(3 * day), admin: 'admin@example.test', action: 'install_key_regenerated', target: '', detail: '' },
  ...Array.from({ length: 70 }, (_, i): AuditEntry => ({ id: -i, at: at(4 * day + i * 60), admin: 'admin@example.test', action: 'sign_in', target: '', detail: '' })),
]

// Each overview poll nudges the numbers and, every other poll, adds an event (so live-update motion is visible).
let polls = 0
function livePoll(): Overview {
  polls++
  if (polls > 2 && polls % 2 === 0) {
    overview.online = 27 + (polls % 7)
    overview.top_fonts[2].users = 15 + (polls % 9)
    const font = ['Syne', 'Fraunces', 'Space Grotesk'][polls % 3]
    overview.recent = [{ id: 100 + polls, install_id: installs[1].id, install_name: 'Sari Wulandari', kind: 'font', text: `Sari Wulandari turned on ${font}`, by: 'user' as const, at: new Date().toISOString() }, ...overview.recent].slice(0, 6)
  }
  return overview
}

/** Filters by `q` over `text(item)` and slices by offset/limit, like the server. */
/** `cmps` maps each `sort` key to an ascending comparator; `dir` flips it. */
function paged<T>(list: T[], sp: URLSearchParams, text: (x: T) => string = () => '', cmps: Record<string, (a: T, b: T) => number> = {}): Page<T> {
  const q = (sp.get('q') ?? '').toLowerCase()
  let hits = q ? list.filter((x) => text(x).toLowerCase().includes(q)) : list
  const cmp = cmps[sp.get('sort') ?? '']
  if (cmp) {
    const sign = sp.get('dir') === 'desc' ? -1 : 1
    hits = [...hits].sort((a, b) => sign * cmp(a, b))
  }
  const offset = Number(sp.get('offset') ?? 0)
  const limit = Math.min(200, Math.max(1, Number(sp.get('limit') ?? 50)))
  return { items: hits.slice(offset, offset + limit), total: hits.length }
}

const fontText = (f: FontState) => `${f.family} ${f.style} ${f.ps_name}`
/** The state / source / override / search filters shared by /fonts, /families and /fonts/bulk. */
function matches(f: FontState, sp: URLSearchParams): boolean {
  const active = sp.get('active'), source = sp.get('source'), q = (sp.get('q') ?? '').toLowerCase()
  return (active === null || f.active === (active === 'true')) && (!source || f.source === source)
    && (sp.get('overridden') !== 'true' || f.overridden) && (!q || fontText(f).toLowerCase().includes(q))
}

/** On/off commands the "computer" has not applied yet; it applies each after a few seconds, like a real desktop. */
const queued = new Map<string, boolean>()
function queueFont(f: FontState, active: boolean) {
  queued.set(f.hash, active)
  f.updated_at = new Date().toISOString()
  setTimeout(() => { if (queued.get(f.hash) === active) { queued.delete(f.hash); f.active = active } }, 3000)
}

function route(method: string, p: string, sp: URLSearchParams, body: unknown): unknown {
  const m = (re: RegExp) => p.match(re)
  if (p === '/me') return { email: 'admin@example.test', role: 'owner' }
  if (method === 'DELETE') {
    const list: { id?: string; hash?: string }[] = p.startsWith('/installs/') ? installs : library
    const i = list.findIndex((x) => (x.id ?? x.hash) === decodeURIComponent(p.split('/')[2]))
    if (i >= 0) list.splice(i, 1)
    return undefined
  }
  if (p === '/library/bulk') {
    const b = body as { action: string; hashes?: string[]; q?: string; target?: LibraryFile['delivered_to'] }
    const hit = library.filter((f) => b.hashes ? b.hashes.includes(f.hash) : `${f.family} ${f.style} ${f.ps_name} ${f.filename}`.toLowerCase().includes((b.q ?? '').toLowerCase()))
    for (const f of hit) {
      if (b.action === 'delete') library.splice(library.indexOf(f), 1)
      else if (b.action === 'deliver') f.delivered_to = b.target ?? 'none'
      else f.blocked = b.action === 'block'
    }
    if (b.hashes && (b.action === 'block' || b.action === 'unblock')) for (const x of fonts) if (x.in_library && b.hashes.includes(x.hash)) { x.blocked = b.action === 'block'; if (x.blocked) x.active = false }
    return { changed: hit.length, skipped: 0, failed: 0 }
  }
  if (p === '/overview') return livePoll()
  if (p === '/installs' && method === 'GET') {
    return paged(installs, sp, (i) => [i.id, i.name, ...i.previous_names].join(' '), {
      name: (a, b) => a.name.localeCompare(b.name),
      last_seen: (a, b) => a.last_seen.localeCompare(b.last_seen),
      fonts: (a, b) => a.active_fonts - b.active_fonts,
    })
  }
  if (m(/^\/installs\/[^/]+\/fonts$/) && method === 'GET') {
    const fam = sp.get('family')?.toLowerCase()
    return paged(fonts.filter((f) => matches(f, sp) && (!fam || f.family.toLowerCase() === fam)), sp, fontText)
  }
  if (m(/^\/installs\/[^/]+\/families$/)) {
    const hits = fonts.filter((f) => matches(f, sp))
    const fams = new Map<string, FamilyRow>()
    for (const f of hits) {
      const k = f.family.toLowerCase()
      const r = fams.get(k) ?? { family: f.family, styles: 0, on: 0, sources: [], overridden: 0, in_library: 0, updated_at: f.updated_at }
      r.styles++; r.on += +f.active; r.overridden += +f.overridden; r.in_library += +f.in_library
      if (!r.sources.includes(f.source)) r.sources.push(f.source)
      if (f.updated_at > r.updated_at) r.updated_at = f.updated_at
      fams.set(k, r)
    }
    const rows = [...fams.values()].sort((a, b) => sp.get('sort') === 'recent' ? b.updated_at.localeCompare(a.updated_at) || a.family.localeCompare(b.family) : a.family.localeCompare(b.family, undefined, { sensitivity: 'base' }))
    const offset = Number(sp.get('offset') ?? 0)
    return { items: rows.slice(offset, offset + Math.min(200, Number(sp.get('limit') ?? 50))), total: rows.length, fonts: hits.length }
  }
  if (m(/^\/installs\/[^/]+\/fonts\/bulk$/)) {
    const b = body as { action: 'on' | 'off' | 'block'; families?: string[]; hashes?: string[]; all?: true } & Record<string, unknown>
    const fams = b.families?.map((x) => x.toLowerCase())
    const scope = new URLSearchParams(Object.entries(b).filter(([k, v]) => ['q', 'active', 'source', 'overridden'].includes(k) && v !== undefined).map(([k, v]) => [k, String(v)]))
    const hit = fonts.filter((f) => (b.hashes ? b.hashes.includes(f.hash) : matches(f, scope) && (!fams || fams.includes(f.family.toLowerCase()))))
    const changed: string[] = []
    let skipped = 0
    for (const f of hit) {
      if (b.action === 'block') {
        if (!f.in_library || f.blocked) { skipped++; continue }
        f.blocked = true; f.active = false
      } else {
        const active = queued.get(f.hash) ?? f.active
        if (b.action === 'on' && f.blocked && !active) { skipped++; continue }
        if (active === (b.action === 'on')) continue
        queueFont(f, b.action === 'on')
      }
      changed.push(f.hash)
    }
    return { changed, skipped }
  }
  if (m(/^\/installs\/[^/]+\/timeline$/)) return paged([
    { id: 1, at: at(5), by: 'user', kind: 'override', text: 'Turned off Fraunces' },
    { id: 2, at: at(day), by: 'admin', kind: 'font', text: 'Fraunces turned on remotely' },
    { id: 3, at: at(15 * day), by: 'user', kind: 'name', text: 'Changed name from “Budi S.” to “Budi Santoso”' },
  ], sp)
  if (m(/^\/installs\/[^/]+\/fonts$/)) {
    const b = body as { hash: string; active: boolean }
    const f = fonts.find((x) => x.hash === b.hash)
    if (f) queueFont(f, b.active)
    return { command_id: 1 }
  }
  if (m(/^\/installs\/[^/]+\/organize-request$/)) return { command_id: 1 }
  const one = m(/^\/installs\/([^/]+)$/)
  if (one) return detail(installs.find((i) => i.id === decodeURIComponent(one[1])) ?? installs[0])
  if (p === '/installs/merge') return detail(installs[0])
  if (p === '/name-changes') return paged([
    { at: at(1), install_ids: [installs[1].id], old_name: 'Sari W.', new_name: 'Sari Wulandari', by: 'user', next_allowed: at(-30 * day) },
    { at: at(3 * day), install_ids: ['A3C1-77E0-2B4D', '0F9E-1A2B-8C3D'], old_name: 'Rina (old laptop)', new_name: 'Rina Kusuma', by: 'merge', next_allowed: null },
    { at: at(26 * day), install_ids: [installs[0].id], old_name: null, new_name: 'Budi S.', by: 'first', next_allowed: null },
  ], sp, (r) => `${r.old_name ?? ''} ${r.new_name} ${r.install_ids.join(' ')}`)
  if (p === '/library' && method === 'GET') {
    return paged(library, sp, (f) => `${f.family} ${f.style} ${f.ps_name} ${f.filename}`, {
      family: (a, b) => a.family.localeCompare(b.family),
      uploaded: (a, b) => a.uploaded_at.localeCompare(b.uploaded_at),
      size: (a, b) => a.size - b.size,
    })
  }
  if (p === '/library') return []
  const lib = m(/^\/library\/([^/]+)(?:\/(block|deliver))?$/)
  if (lib && lib[1] !== 'organize') {
    const f = library.find((x) => x.hash === lib[1])!
    Object.assign(f, lib[2] === 'block' ? body : lib[2] === 'deliver' ? { delivered_to: (body as { target: string }).target } : body)
    return f
  }
  if (p === '/library/organize') return { changes: [
    { case: 'exact_copy', action: { op: 'trash', path: 'uploads/SpaceGrotesk-Bold.otf', kept: 'x' }, note: 'Removed · same file already in Space Grotesk' },
    { case: 'junk', action: { op: 'quarantine', from: '__MACOSX/._Fraunces-Regular.otf', to: 'q' }, note: 'Moved to quarantine' },
  ] }
  if (p === '/library/organize/apply') return { applied: 2 }
  if (p === '/requests') {
    const state = sp.get('state') ?? 'all'
    return paged(fontRequests.filter((r) => state === 'all' || (state === 'open') === !r.done_at), sp, undefined, {
      at: (a, b) => a.at.localeCompare(b.at),
      font: (a, b) => a.font_name.localeCompare(b.font_name),
    })
  }
  const req = m(/^\/requests\/(\d+)\/(done|reopen)$/)
  if (req) {
    const r = fontRequests.find((x) => x.id === Number(req[1]))!
    r.done_at = req[2] === 'done' ? new Date().toISOString() : null
    overview.attention.requests = fontRequests.filter((x) => !x.done_at).length
    return undefined
  }
  if (p === '/organize/reports') return paged([
    { install_id: installs[2].id, name: 'Rizky Pratama', duplicates: 41, version_conflicts: 6, name_clashes: 0, last_organized: null },
    { install_id: installs[1].id, name: 'Sari Wulandari', duplicates: 0, version_conflicts: 0, name_clashes: 0, last_organized: at(day) },
  ], sp)
  if (p === '/settings' && method === 'PUT') return (settings = { ...settings, ...(body as object) })
  if (p === '/settings') return settings
  if (p === '/settings/connection-password') {
    const pw = (body as { password: string | null }).password
    if (pw !== null && (pw.length < 8 || pw.length > 128)) throw { status: 400, error: 'invalid_password', message: 'The unlock password must be 8 to 128 characters.' }
    settings.connection_password_set = pw !== null
    return { connection_password_set: settings.connection_password_set }
  }
  if (p === '/settings/install-key') return { install_key: (settings.install_key = 'tsk_' + Math.random().toString(36).slice(2)) }
  if (p === '/admins' && method === 'GET') return [{ id: 1, email: 'admin@example.test', role: 'owner', last_login: at(10) }, { id: 2, email: 'second@example.test', role: 'admin', last_login: null }]
  if (p === '/audit') {
    const all = audit.filter((e) => (!sp.get('admin') || e.admin === sp.get('admin')) && (!sp.get('action') || e.action === sp.get('action')))
    return { ...paged(all, sp), admins: [...new Set(audit.map((e) => e.admin))].sort(), actions: [...new Set(audit.map((e) => e.action))].sort() }
  }
  if (p === '/update' && method === 'POST') return { version: '1.1.0', size: 14_680_064 }
  if (p === '/update') return { version: '1.0.0', size: 14_155_776 }
  return undefined
}

const realFetch = window.fetch.bind(window)
window.fetch = async (input, init) => {
  const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url
  const { pathname: path, searchParams } = new URL(url, location.origin)
  if (!path.startsWith('/api/admin')) return realFetch(input, init)
  await new Promise((r) => setTimeout(r, 200))
  const body = typeof init?.body === 'string' ? JSON.parse(init.body) : undefined
  let data: unknown
  try { data = route(init?.method ?? 'GET', path.slice('/api/admin'.length), searchParams, body) } catch (e) {
    const { status, ...err } = e as { status: number; error: string; message: string }
    return Response.json(err, { status })
  }
  return data === undefined ? new Response(null, { status: 204 }) : Response.json(data)
}
