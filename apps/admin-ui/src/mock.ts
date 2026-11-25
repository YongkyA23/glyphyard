// Dev-only fixtures (?mock=1). Patches fetch for /api/admin so pages render without a server.
import type { FontRequest, FontState, InstallDetail, InstallSummary, LibraryFile, Overview, Page, Settings } from './api'

const now = Date.now()
const t = (mins: number) => new Date(now - mins * 60000).toISOString()
const day = 1440

const installs: InstallSummary[] = [
  { id: '7F3A-2C9E-91C2', name: 'Budi Santoso', previous_names: ['Budi S.'], os: 'Windows 11', app_version: '1.0.0', active_fonts: 86, last_seen: t(1), online: true, next_name_change: t(-15 * day), flags: ['override'] },
  { id: '1B77-40DA-0E15', name: 'Sari Wulandari', previous_names: ['Sari W.'], os: 'Windows 11', app_version: '1.0.0', active_fonts: 54, last_seen: t(2), online: true, next_name_change: t(-30 * day), flags: [] },
  { id: 'C902-7E11-5A3B', name: 'Rizky Pratama', previous_names: [], os: 'Windows 10', app_version: '1.0.0', active_fonts: 112, last_seen: t(3), online: true, next_name_change: null, flags: ['failures'] },
  { id: 'E41D-03B7-7C6A', name: 'Budi S.', previous_names: [], os: 'macOS · Intel', app_version: '1.0.0', active_fonts: 0, last_seen: t(3 * day), online: false, next_name_change: null, flags: ['possible_duplicate'] },
  { id: '9AF0-6D25-3E8B', name: 'Putri Ananda', previous_names: [], os: 'Windows 11', app_version: '0.9.2', active_fonts: 61, last_seen: t(16 * day), online: false, next_name_change: null, flags: ['stale'] },
  // Filler so the pagers have several pages.
  ...Array.from({ length: 130 }, (_, i): InstallSummary => ({
    id: `F${String(i).padStart(3, '0')}-0000-0000`, name: `Designer ${i + 1}`, previous_names: [], os: i % 9 ? 'Windows 11' : 'macOS · Apple silicon',
    app_version: '1.0.0', active_fonts: (i * 7) % 90, last_seen: t(i * 37), online: i % 4 === 0, next_name_change: null, flags: i % 11 ? [] : ['stale'],
  })),
]
const fonts: FontState[] = [
  { hash: 'h1', ps_name: 'Fraunces-Regular', family: 'Fraunces', style: 'Regular', active: false, source: 'admin', overridden: true },
  { hash: 'h2', ps_name: 'SpaceGrotesk-Medium', family: 'Space Grotesk', style: 'Medium', active: true, source: 'admin', overridden: false },
  { hash: 'h3', ps_name: 'IBMPlexMono-Medium', family: 'IBM Plex Mono', style: 'Medium', active: true, source: 'user', overridden: false },
  { hash: 'h4', ps_name: 'Syne-SemiBold', family: 'Syne', style: 'SemiBold', active: true, source: 'project_set', overridden: false },
  ...Array.from({ length: 160 }, (_, i): FontState => ({
    hash: `hf${i}`, ps_name: `Sample${i}-Regular`, family: `Sample Sans ${i + 1}`, style: 'Regular', active: i % 3 !== 0, source: 'user', overridden: false,
  })),
]
const detail = (s: InstallSummary): InstallDetail => ({
  ...s, first_seen: t(26 * day), merged_from: [],
  name_history: [{ name: s.name, from: t(15 * day), to: null }, ...s.previous_names.map((n) => ({ name: n, from: t(26 * day), to: t(15 * day) }))],
  overrides: s.flags.includes('override') ? [{ font: { hash: 'h1', ps_name: 'Fraunces-Regular', family: 'Fraunces', style: 'Regular', active: false, source: 'admin', overridden: true }, admin_at: t(day), user_at: t(5) }] : [],
})
const library: LibraryFile[] = [
  { hash: 'h2', filename: 'SpaceGrotesk-Medium.otf', ps_name: 'SpaceGrotesk-Medium', family: 'Space Grotesk', style: 'Medium', version: '2.0', license: 'licensed', size: 84000, delivered_to: 'all', active_on: 41, blocked: false, uploaded_at: t(9 * day) },
  { hash: 'h4', filename: 'Syne-SemiBold.otf', ps_name: 'Syne-SemiBold', family: 'Syne', style: 'SemiBold', version: '2.0', license: 'client', size: 52000, delivered_to: ['7F3A-2C9E-91C2'], active_on: 12, blocked: false, uploaded_at: t(9 * day) },
  { hash: 'h9', filename: 'BebasNeue.otf', ps_name: 'BebasNeue', family: 'Bebas Neue', style: 'Regular', version: '1.0', license: 'unknown', size: 31000, delivered_to: 'none', active_on: 0, blocked: true, uploaded_at: t(2 * day) },
]
const overview: Overview = {
  installs: 48, online: 31, os: { windows: 46, macos: 2 }, name_changes_month: 6, overrides_week: 4, failures_week: 3,
  recent: [
    { id: 1, install_id: installs[0].id, install_name: 'Budi Santoso', kind: 'override', text: 'Budi Santoso turned off Fraunces, which you turned on', by: 'user', at: t(5) },
    { id: 2, install_id: installs[2].id, install_name: 'Rizky Pratama', kind: 'failure', text: 'Montserrat Bold failed to activate for Rizky Pratama (file missing)', by: 'system', at: t(60) },
    { id: 3, install_id: installs[1].id, install_name: 'Sari', kind: 'font', text: 'You turned on Space Grotesk for 12 users', by: 'admin', at: t(130) },
  ],
  attention: { overrides: 4, failures: 3, possible_duplicates: 2, stale: 5, requests: 2 },
  top_fonts: [{ family: 'Space Grotesk', users: 41 }, { family: 'Fraunces', users: 33 }, { family: 'Syne', users: 19 }],
}
const fontRequests: FontRequest[] = [
  { id: 3, install_id: installs[1].id, install_name: 'Sari Wulandari', font_name: 'Söhne Buch', note: 'For the Acme rebrand deck, due Friday.', at: t(40), done_at: null },
  { id: 2, install_id: installs[0].id, install_name: 'Budi Santoso', font_name: 'GT America Condensed', note: null, at: t(2 * day), done_at: null },
  { id: 1, install_id: installs[2].id, install_name: 'Rizky Pratama', font_name: 'Founders Grotesk', note: 'Client sent a PDF that uses it.', at: t(6 * day), done_at: t(5 * day) },
]
let settings: Settings = {
  name_change_days: 30, allow_connection_edit: true, deliver_library_to_new: true, activate_delivered: false,
  notify_admin_changes: true, timeline_retention_days: null, server_url: 'https://fonts.example.test/api', install_key: 'tsk_example_key_1234',
}

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
function paged<T>(list: T[], sp: URLSearchParams, text: (x: T) => string = () => ''): Page<T> {
  const q = (sp.get('q') ?? '').toLowerCase()
  const hits = q ? list.filter((x) => text(x).toLowerCase().includes(q)) : list
  const offset = Number(sp.get('offset') ?? 0)
  const limit = Math.min(200, Math.max(1, Number(sp.get('limit') ?? 50)))
  return { items: hits.slice(offset, offset + limit), total: hits.length }
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
    return { changed: hit.length, skipped: 0, failed: 0 }
  }
  if (p === '/overview') return livePoll()
  if (p === '/installs' && method === 'GET') return paged(installs, sp, (i) => [i.id, i.name, ...i.previous_names].join(' '))
  if (m(/^\/installs\/[^/]+\/fonts$/) && method === 'GET') return paged(fonts, sp, (f) => `${f.family} ${f.style} ${f.ps_name}`)
  if (m(/^\/installs\/[^/]+\/timeline$/)) return paged([
    { id: 1, at: t(5), by: 'user', kind: 'override', text: 'Turned off Fraunces' },
    { id: 2, at: t(day), by: 'admin', kind: 'font', text: 'Fraunces turned on remotely' },
    { id: 3, at: t(15 * day), by: 'user', kind: 'name', text: 'Changed name from “Budi S.” to “Budi Santoso”' },
  ], sp)
  if (m(/^\/installs\/[^/]+\/(fonts|organize-request)$/)) return { command_id: 1 }
  const one = m(/^\/installs\/([^/]+)$/)
  if (one) return detail(installs.find((i) => i.id === decodeURIComponent(one[1])) ?? installs[0])
  if (p === '/installs/merge') return detail(installs[0])
  if (p === '/name-changes') return paged([
    { at: t(1), install_ids: [installs[1].id], old_name: 'Sari W.', new_name: 'Sari Wulandari', by: 'user', next_allowed: t(-30 * day) },
    { at: t(3 * day), install_ids: ['A3C1-77E0-2B4D', '0F9E-1A2B-8C3D'], old_name: 'Rina (old laptop)', new_name: 'Rina Kusuma', by: 'merge', next_allowed: null },
    { at: t(26 * day), install_ids: [installs[0].id], old_name: null, new_name: 'Budi S.', by: 'first', next_allowed: null },
  ], sp, (r) => `${r.old_name ?? ''} ${r.new_name} ${r.install_ids.join(' ')}`)
  if (p === '/library' && method === 'GET') return paged(library, sp, (f) => `${f.family} ${f.style} ${f.ps_name} ${f.filename}`)
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
    return paged(fontRequests.filter((r) => state === 'all' || (state === 'open') === !r.done_at), sp)
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
    { install_id: installs[1].id, name: 'Sari Wulandari', duplicates: 0, version_conflicts: 0, name_clashes: 0, last_organized: t(day) },
  ], sp)
  if (p === '/settings' && method === 'PUT') return (settings = { ...settings, ...(body as object) })
  if (p === '/settings') return settings
  if (p === '/settings/install-key') return { install_key: (settings.install_key = 'tsk_' + Math.random().toString(36).slice(2)) }
  if (p === '/admins' && method === 'GET') return [{ email: 'admin@example.test', role: 'owner', last_login: t(10) }, { email: 'second@example.test', role: 'admin', last_login: null }]
  return undefined
}

const realFetch = window.fetch.bind(window)
window.fetch = async (input, init) => {
  const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url
  const { pathname: path, searchParams } = new URL(url, location.origin)
  if (!path.startsWith('/api/admin')) return realFetch(input, init)
  await new Promise((r) => setTimeout(r, 200))
  const body = typeof init?.body === 'string' ? JSON.parse(init.body) : undefined
  const data = route(init?.method ?? 'GET', path.slice('/api/admin'.length), searchParams, body)
  return data === undefined ? new Response(null, { status: 204 }) : Response.json(data)
}
