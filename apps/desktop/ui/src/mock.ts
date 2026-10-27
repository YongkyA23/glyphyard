// In-memory backend used when the UI runs in a normal browser (no Tauri). Sample data comes from the design mockups.
// Dev helpers on window.typeshelfMock: emit(event, payload), setOffline(bool).
import type {
  ActivationResult, AdobeRow, AppState, Collection, Connection, Face, Family, FamilyDetail, FileRow, Folder,
  GoogleFont, HistoryEntry, License, ListFamiliesArgs, OrganizeChange, PdfFont, ProjectSet, Settings,
} from './api'

const day = 86_400_000
const iso = (offsetMs = 0) => new Date(Date.now() + offsetMs).toISOString()
const clone = <T,>(v: T): T => structuredClone(v)
const err = (code: string, message: string, extra: Record<string, unknown> = {}) => ({ code, message, ...extra })

const WEIGHT_NAMES: Record<number, string> = { 100: 'Thin', 200: 'ExtraLight', 300: 'Light', 400: 'Regular', 500: 'Medium', 600: 'SemiBold', 700: 'Bold', 800: 'ExtraBold', 900: 'Black' }

type Seed = {
  family: string; foundry: string; designer: string; license: License; folder: string; weights: number[]; italics?: boolean
  variable?: boolean; admin?: boolean; dupe?: boolean; on: boolean; addedDaysAgo: number; axes?: FamilyDetail['axes']; features: string[]
}
const SEEDS: Seed[] = [
  { family: 'Fraunces', foundry: 'Undercase Type', designer: 'Phaedra Charles, Flavia Zimbardi', license: 'licensed', folder: 'D:\\Fonts\\Licensed', weights: [400], italics: true, variable: true, admin: true, on: true, addedDaysAgo: 2,
    axes: [{ tag: 'wght', min: 100, default: 600, max: 900 }, { tag: 'opsz', min: 9, default: 72, max: 144 }, { tag: 'SOFT', min: 0, default: 50, max: 100 }, { tag: 'WONK', min: 0, default: 0, max: 1 }],
    features: ['liga', 'onum', 'frac', 'smcp'] },
  { family: 'Space Grotesk', foundry: 'Florian Karsten', designer: 'Florian Karsten', license: 'licensed', folder: 'D:\\Fonts\\Licensed', weights: [300, 400, 500, 600, 700], admin: true, on: true, addedDaysAgo: 2, features: ['liga', 'ss01', 'ss02', 'tnum'] },
  { family: 'Instrument Serif', foundry: 'Instrument', designer: 'Rodrigo Fuenzalida', license: 'personal', folder: 'D:\\Fonts\\Client work', weights: [400], italics: true, on: false, addedDaysAgo: 30, features: ['liga', 'onum'] },
  { family: 'DM Serif Display', foundry: 'Colophon', designer: 'Colophon Foundry', license: 'client', folder: 'D:\\Fonts\\Client work', weights: [400], italics: true, on: true, addedDaysAgo: 12, features: ['liga', 'dlig'] },
  { family: 'Archivo Black', foundry: 'Omnibus-Type', designer: 'Héctor Gatti', license: 'unknown', folder: 'E:\\Archive', weights: [400], on: false, addedDaysAgo: 120, features: ['liga'] },
  { family: 'IBM Plex Mono', foundry: 'IBM', designer: 'Mike Abbink, Bold Monday', license: 'licensed', folder: 'D:\\Fonts\\Licensed', weights: [100, 200, 300, 400, 500, 600, 700], italics: true, on: true, addedDaysAgo: 60, features: ['zero', 'ss01', 'frac'] },
  { family: 'Bebas Neue', foundry: 'Dharma Type', designer: 'Ryoichi Tsunekawa', license: 'unknown', folder: 'D:\\Fonts\\Client work', weights: [400], dupe: true, on: false, addedDaysAgo: 8, features: ['kern'] },
  { family: 'Syne', foundry: 'Bonjour Monde', designer: 'Lucas Descroix', license: 'licensed', folder: 'D:\\Fonts\\Licensed', weights: [400, 500, 600, 700, 800], admin: true, on: true, addedDaysAgo: 2, features: ['liga', 'ss01'] },
  { family: 'Bricolage Grotesque', foundry: 'Mathieu Triay', designer: 'Mathieu Triay', license: 'licensed', folder: 'D:\\Fonts\\Licensed', weights: [400], variable: true, on: false, addedDaysAgo: 40,
    axes: [{ tag: 'wght', min: 200, default: 400, max: 800 }, { tag: 'opsz', min: 12, default: 14, max: 96 }], features: ['liga', 'ss01'] },
  { family: 'Familjen Grotesk', foundry: 'Familjen STHLM', designer: 'Familjen STHLM', license: 'licensed', folder: 'D:\\Fonts\\Licensed', weights: [400, 500, 600, 700], on: false, addedDaysAgo: 50, features: ['liga'] },
  { family: 'Schibsted Grotesk', foundry: 'Bakken & Bæck', designer: 'Bakken & Bæck', license: 'personal', folder: 'D:\\Fonts\\Client work', weights: [400], variable: true, on: false, addedDaysAgo: 70,
    axes: [{ tag: 'wght', min: 400, default: 400, max: 900 }], features: ['liga', 'tnum'] },
  { family: 'Hanken Grotesk', foundry: 'Hanken Design Co.', designer: 'Alfredo Marco Pradil', license: 'unknown', folder: 'D:\\Fonts\\Client work', weights: [400], variable: true, on: false, addedDaysAgo: 90,
    axes: [{ tag: 'wght', min: 100, default: 400, max: 900 }], features: ['liga'] },
]

let hashN = 0
const nextHash = () => (++hashN).toString(16).padStart(8, '0') + 'b3a1c9e2'
const keyOf = (family: string) => family.toLowerCase().replace(/\s+/g, '-')

type Fam = Family & Omit<FamilyDetail, keyof Family> & { addedAt: number; tags: string[] }

const families: Fam[] = SEEDS.map((s) => {
  const dir = `${s.folder}\\${s.foundry}\\${s.family}`
  const base = s.family.replace(/\s+/g, '')
  const faces: Face[] = []
  for (const italic of s.italics ? [false, true] : [false]) {
    for (const w of s.weights) {
      const style = s.variable ? (italic ? 'Italic' : 'Regular') : `${w === 400 && italic ? '' : WEIGHT_NAMES[w]}${italic ? 'Italic' : ''}` || 'Regular'
      const file = s.variable ? `${base}${italic ? '-Italic' : ''}[${(s.axes ?? []).map((a) => a.tag).join(',')}].ttf` : `${base}-${style}.otf`
      faces.push({
        hash: nextHash(), psName: `${base}-${style}`, family: s.family, style, fullName: `${s.family} ${style}`, version: s.dupe ? '2.000' : '1.000',
        path: `${dir}\\${file}`, active: s.on, source: s.on ? (s.admin ? 'admin' : 'user') : null, weight: w, italic, isVariable: !!s.variable,
      })
    }
  }
  const files: FileRow[] = faces.map((f) => ({ path: f.path, version: f.version, status: s.on ? 'active' : 'inactive' }))
  if (s.family === 'Fraunces') {
    files.push({ path: 'D:\\Fonts\\Client work\\Old\\Fraunces v0.900.ttf', version: '0.900', status: 'older_version' })
    files.push({ path: `${dir}\\static\\Fraunces-Regular.otf`, version: '1.000', status: 'grouped' })
  }
  if (s.dupe) files.push({ path: 'E:\\Archive\\Bebas Neue\\BebasNeue-Regular.otf', version: '1.400', status: 'older_version' })
  if (s.family === 'Space Grotesk') files.push({ path: 'D:\\Fonts\\Client work\\SpaceGrotesk-Medium.ttf', version: '1.000', status: 'alternate' })
  return {
    key: keyOf(s.family), family: s.family, foundry: s.foundry, styles: faces.length, isVariable: !!s.variable,
    license: s.license, activeState: s.on ? 'all' : 'none', adminSet: !!s.admin, hasDuplicates: !!s.dupe, faces,
    axes: s.axes ?? [], features: s.features, glyphCount: s.family === 'Fraunces' ? 612 : 380 + s.family.length * 11, designer: s.designer, files,
    licenseInfo: {
      label: s.license,
      file: s.license === 'licensed' ? `${dir}\\OFL.txt` : null,
      note: s.license === 'licensed' ? 'Free for commercial use, including logos.' : '',
    },
    addedAt: Date.now() - s.addedDaysAgo * day, tags: [],
  }
})
// Fraunces ships 9 static styles grouped with the variable one; the mockup counts them as "9 styles".
families[0].styles = 9

const fam = (key: string) => families.find((f) => f.key === key)
const famOfHash = (hash: string) => families.find((f) => f.faces.some((x) => x.hash === hash))
const refreshState = (f: Fam) => {
  const n = f.faces.filter((x) => x.active).length
  f.activeState = n === 0 ? 'none' : n === f.faces.length ? 'all' : 'some'
  f.files.forEach((r) => {
    const face = f.faces.find((x) => x.path === r.path)
    if (face) r.status = face.active ? 'active' : 'inactive'
  })
}
const toFamily = (f: Fam): Family => {
  const { axes: _a, features: _f, glyphCount: _g, designer: _d, files: _fi, licenseInfo: _l, addedAt: _ad, tags: _t, ...rest } = f
  return clone(rest)
}

let folders: Folder[] = [
  { id: 1, path: 'D:\\Fonts\\Licensed', status: 'watching', fontCount: 812, offlineSince: null },
  { id: 2, path: 'D:\\Fonts\\Client work', status: 'watching', fontCount: 340, offlineSince: null },
  { id: 3, path: 'E:\\Archive', status: 'offline', fontCount: 132, offlineSince: new Date(Date.now() - 3 * day).toISOString() },
]
let collections: Collection[] = [
  { id: 1, name: 'Favorites', parentId: null, count: 0, builtin: 'favorites' },
  { id: 2, name: 'Recently used', parentId: null, count: 0, builtin: 'recent' },
  { id: 3, name: 'Headlines', parentId: null, count: 0, builtin: null },
]
const collItems: Record<number, Set<string>> = {
  1: new Set(['fraunces', 'space-grotesk', 'syne', 'ibm-plex-mono']),
  2: new Set(['fraunces', 'dm-serif-display', 'space-grotesk']),
  3: new Set(['fraunces', 'dm-serif-display', 'bebas-neue', 'archivo-black', 'instrument-serif']),
}
const collList = () => collections.map((c) => ({ ...c, count: collItems[c.id]?.size ?? 0 }))

let sets: ProjectSet[] = [
  { id: 1, name: 'Northwind rebrand', note: 'Client', familyKeys: ['dm-serif-display', 'space-grotesk', 'ibm-plex-mono', 'archivo-black', 'syne'], on: true, lastUsed: iso(-2 * 3600_000) },
  { id: 2, name: 'Café Lumen menu', note: 'Client', familyKeys: ['fraunces', 'instrument-serif', 'space-grotesk'], on: false, lastUsed: iso(-4 * day) },
  { id: 3, name: 'Wedding invitations', note: 'Personal', familyKeys: ['instrument-serif', 'bebas-neue'], on: false, lastUsed: iso(-21 * day) },
]

const DEFAULT_CONN = { serverUrl: 'https://fonts.northwind-studio.com', installKey: 'tsk_live_4f8a2c91e7d3' }
let connection: Connection = {
  status: 'connected', ...DEFAULT_CONN, editable: true, lastSync: iso(-60_000), installId: '7F3A-2C9E-91C2', error: null,
}
// The onboarding name survives reloads within the tab so `bun run dev` doesn't ask every time.
const savedName = sessionStorage.getItem('typeshelf-mock-name') ?? ''
let app: AppState = {
  onboarded: !!savedName, name: savedName, nextNameChange: null, nameChangeDays: 30, connection, platform: 'windows', appVersion: '1.0.0',
}
let settings: Settings = {
  startAtLogin: true, startHidden: true, autoUpdate: true, deactivateOnQuit: false,
  quickShortcut: 'CmdOrCtrl+Alt+Space', theme: 'light', sampleText: 'Sphinx of black quartz, judge my vow',
}
let history: HistoryEntry[] = [
  { id: 2, kind: 'organize', summary: 'Auto-organize · 89 changes applied', at: '2026-10-21T14:05:00', undone: false },
  { id: 1, kind: 'restore', summary: 'Restored backup from 20 Oct 2026', at: '2026-10-20T09:40:00', undone: false },
]
let health = { lastCheck: new Date(new Date().setHours(9, 12, 0, 0)).toISOString(), problems: [] as { hash: string; message: string }[], removed: 0, reregistered: 0 }
const adobe: AdobeRow[] = [
  { app: 'photoshop', label: 'Photoshop', versions: ['2026'], installed: true, paired: true },
  { app: 'illustrator', label: 'Illustrator', versions: ['2026'], installed: false, paired: false },
  { app: 'indesign', label: 'InDesign', versions: ['2026'], installed: false, paired: false },
]
// Extra rows so the Google Fonts pager has several pages in the browser mock.
const mockFiller: GoogleFont[] = Array.from({ length: 80 }, (_, i) => ({
  family: `Mock Family ${String(i + 1).padStart(2, '0')}`, category: ['Serif', 'Sans Serif', 'Display'][i % 3], styles: 1 + (i % 9), variable: i % 4 === 0, installed: false,
}))

const google: GoogleFont[] = [
  { family: 'Lora', category: 'Serif', styles: 8, variable: true, installed: false },
  { family: 'Bricolage Grotesque', category: 'Sans Serif', styles: 1, variable: true, installed: false },
  { family: 'Caveat', category: 'Handwriting', styles: 1, variable: true, installed: false },
  { family: 'Playfair Display', category: 'Display', styles: 12, variable: false, installed: false },
  { family: 'Work Sans', category: 'Sans Serif', styles: 18, variable: false, installed: false },
  { family: 'Space Mono', category: 'Monospace', styles: 4, variable: false, installed: false },
  { family: 'Syne', category: 'Display', styles: 5, variable: false, installed: true },
  { family: 'Instrument Serif', category: 'Serif', styles: 2, variable: false, installed: true },
  { family: 'DM Serif Display', category: 'Display', styles: 2, variable: false, installed: false },
  { family: 'Archivo Black', category: 'Sans Serif', styles: 1, variable: false, installed: false },
  { family: 'Inter', category: 'Sans Serif', styles: 18, variable: true, installed: false },
  { family: 'Fira Code', category: 'Monospace', styles: 5, variable: true, installed: false },
]

const PLAN: [OrganizeChange['case'], OrganizeChange['action'], string][] = [
  ['exact_copy', { op: 'trash', path: 'D:\\Fonts\\Client work\\Montserrat-Bold.otf', kept: 'D:\\Fonts\\Licensed\\Montserrat\\Montserrat-Bold.otf' }, ''],
  ['exact_copy', { op: 'trash', path: 'C:\\Users\\budi\\Downloads\\Montserrat-Bold (1).otf', kept: 'D:\\Fonts\\Licensed\\Montserrat\\Montserrat-Bold.otf' }, 'same file as above'],
  ['different_version', { op: 'rename', from: 'D:\\Fonts\\Licensed\\Montserrat-Bold.otf', to: 'D:\\Fonts\\Licensed\\Montserrat-Bold v8.000.otf' }, 'newest, used by default'],
  ['different_version', { op: 'rename', from: 'E:\\Archive\\Montserrat-Bold.otf', to: 'E:\\Archive\\Montserrat-Bold v7.200.otf' }, 'never active at the same time'],
  ['other_format', { op: 'keep_as_alternate', path: 'D:\\Fonts\\Client work\\Syne-Bold.ttf', preferred: 'D:\\Fonts\\Licensed\\Syne-Bold.otf' }, ''],
  ['name_clash', { op: 'rename', from: 'E:\\Archive\\Unnamed-Regular.otf', to: 'E:\\Archive\\Unnamed-Regular (Studio A).otf' }, 'never active with the other one'],
  ['variable_static', { op: 'group', family: 'Fraunces', paths: ['D:\\Fonts\\Licensed\\Fraunces[SOFT,WONK,opsz,wght].ttf', ...Array.from({ length: 9 }, (_, i) => `D:\\Fonts\\Licensed\\static\\Fraunces-${i}.otf`)] }, 'variable version preferred'],
  ['in_system', { op: 'skip', path: 'D:\\Fonts\\Client work\\Arial.ttf' }, 'Windows already has Arial'],
  ['junk', { op: 'quarantine', from: 'D:\\Fonts\\Client work\\__MACOSX\\._Lora-Regular.ttf', to: 'C:\\Users\\budi\\AppData\\Roaming\\Typeshelf\\Quarantine\\._Lora-Regular.ttf' }, 'not a real font file'],
  ['junk', { op: 'quarantine', from: 'D:\\Fonts\\Client work\\Thumbs.db', to: 'C:\\Users\\budi\\AppData\\Roaming\\Typeshelf\\Quarantine\\Thumbs.db' }, ''],
]
const TIDY: OrganizeChange['action'][] = [
  { op: 'move', from: 'D:\\Fonts\\Client work\\SpaceGrotesk-Medium.ttf', to: 'D:\\Fonts\\Client work\\Florian Karsten\\Space Grotesk\\SpaceGrotesk-Medium v1.000.ttf' },
  { op: 'move', from: 'D:\\Fonts\\Client work\\BebasNeue-Regular.otf', to: 'D:\\Fonts\\Client work\\Dharma Type\\Bebas Neue\\BebasNeue-Regular v2.000.otf' },
]
const plan = (tidy: boolean): OrganizeChange[] => [
  ...PLAN.map(([c, action, note], i) => ({ id: i + 1, case: c, action, note })),
  ...(tidy ? TIDY.map((action, i) => ({ id: 100 + i, case: 'tidy' as const, action, note: '' })) : []),
]

// ---- events ----
const listeners = new Map<string, Set<(p: unknown) => void>>()
export function mockListen(event: string, cb: (p: unknown) => void) {
  if (!listeners.has(event)) listeners.set(event, new Set())
  listeners.get(event)!.add(cb)
  return () => listeners.get(event)?.delete(cb)
}
function emit(event: string, payload: unknown) {
  setTimeout(() => listeners.get(event)?.forEach((cb) => cb(clone(payload))), 0)
}
function setConnection(c: Partial<Connection>) {
  connection = { ...connection, ...c }
  app = { ...app, connection }
  emit('connection-changed', connection)
}
Object.assign(window, {
  typeshelfMock: {
    emit,
    setOffline: (off: boolean) => setConnection(off ? { status: 'offline', error: null } : { status: 'connected', lastSync: iso(), error: null }),
  },
})

// ---- activation ----
function activate(hashes: string[], active: boolean, confirm?: boolean): ActivationResult[] {
  if (active && !confirm) {
    const unknown = [...new Set(hashes.map(famOfHash).filter((f) => f && f.license === 'unknown').map((f) => f!.family))]
    if (unknown.length) throw err('unknown_license', `${unknown.join(', ')} has no license label.`, { families: unknown })
  }
  const results = hashes.map((hash): ActivationResult => {
    const f = famOfHash(hash)
    const face = f?.faces.find((x) => x.hash === hash)
    if (!f || !face) return { hash, ok: false, error: 'file_missing', message: 'This font is no longer in your library.' }
    const folder = folders.find((d) => face.path.startsWith(d.path))
    if (active && folder?.status === 'offline')
      return { hash, ok: false, error: 'file_missing', message: `${face.fullName}: the file is on ${folder.path}, which is offline.` }
    face.active = active
    face.source = active ? 'user' : null
    refreshState(f)
    return { hash, ok: true, error: null, message: null }
  })
  emit('library-changed', { reason: 'activation' })
  return results
}

function listFamilies(a: ListFamiliesArgs): Family[] {
  const q = a.search?.trim().toLowerCase()
  let out = families.filter((f) => {
    if (q && !`${f.family} ${f.foundry}`.toLowerCase().includes(q)) return false
    switch (a.filter) {
      case 'active': if (f.activeState === 'none') return false; break
      case 'inactive': if (f.activeState !== 'none') return false; break
      case 'admin': if (!f.adminSet) return false; break
      case 'unknown_license': if (f.license !== 'unknown') return false; break
      case 'duplicates': if (!f.hasDuplicates) return false; break
    }
    if (a.collectionId != null && !collItems[a.collectionId]?.has(f.key)) return false
    if (a.folderId != null) {
      const p = folders.find((d) => d.id === a.folderId)?.path
      if (!p || !f.faces.some((x) => x.path.startsWith(p))) return false
    }
    if (a.license && f.license !== a.license) return false
    if (a.tag && !f.tags.includes(a.tag)) return false
    return true
  })
  const by = { name: (x: Fam, y: Fam) => x.family.localeCompare(y.family), name_desc: (x: Fam, y: Fam) => y.family.localeCompare(x.family),
    date: (x: Fam, y: Fam) => y.addedAt - x.addedAt, foundry: (x: Fam, y: Fam) => x.foundry.localeCompare(y.foundry) }
  out = [...out].sort(by[a.sort ?? 'name'])
  return out.map(toFamily)
}

const delay = () => new Promise((r) => setTimeout(r, 60 + Math.random() * 140))

type Args = Record<string, any> // eslint-disable-line @typescript-eslint/no-explicit-any

const handlers: Record<string, (a: Args) => unknown> = {
  get_app_state: () => app,
  complete_onboarding: ({ name }) => {
    if (!name?.trim()) throw err('invalid_name', 'Enter your name to continue.')
    if (connection.status !== 'connected') throw err('offline', 'Typeshelf can’t reach your server. Check your connection and try again.')
    app = { ...app, onboarded: true, name: name.trim(), nextNameChange: null }
    sessionStorage.setItem('typeshelf-mock-name', app.name)
    setTimeout(() => emit('admin-change', { action: 'activated', fonts: [{ family: 'Fraunces', style: 'Regular' }, { family: 'Space Grotesk', style: 'Medium' }, { family: 'Syne', style: 'SemiBold' }] }), 1500)
    return app
  },
  change_name: ({ name }) => {
    if (connection.status !== 'connected') throw err('offline', 'You need a connection to your server to change your name.')
    if (app.nextNameChange && new Date(app.nextNameChange).getTime() > Date.now())
      throw err('name_locked', 'You can’t change your name yet.', { nextNameChange: app.nextNameChange })
    app = { ...app, name: name.trim(), nextNameChange: iso(app.nameChangeDays * day) }
    emit('name-changed', { name: app.name, nextNameChange: app.nextNameChange })
    return { name: app.name, nextNameChange: app.nextNameChange }
  },
  list_families: (a) => listFamilies(a),
  get_family: ({ key }) => {
    const f = fam(key)
    if (!f) throw err('not_found', 'This font family is no longer in your library.')
    const { addedAt: _a, tags: _t, ...rest } = f
    return rest
  },
  set_active: ({ hashes, active, confirmUnknownLicense }) => activate(hashes, active, confirmUnknownLicense),
  list_folders: () => folders,
  add_folder: ({ path }) => {
    folders = [...folders, { id: Math.max(0, ...folders.map((f) => f.id)) + 1, path, status: 'scanning', fontCount: 0, offlineSince: null }]
    const id = folders.at(-1)!.id
    setTimeout(() => {
      folders = folders.map((f) => (f.id === id ? { ...f, status: 'watching', fontCount: 47 } : f))
      emit('folder-status', folders.find((f) => f.id === id))
    }, 1500)
    return folders
  },
  remove_folder: ({ id }) => (folders = folders.filter((f) => f.id !== id)),
  locate_folder: ({ id, path }) => (folders = folders.map((f) => (f.id === id ? { ...f, path, status: 'watching', offlineSince: null } : f))),
  list_collections: () => collList(),
  create_collection: ({ name, parentId }) => {
    const id = Math.max(0, ...collections.map((c) => c.id)) + 1
    collections = [...collections, { id, name, parentId: parentId ?? null, count: 0, builtin: null }]
    collItems[id] = new Set()
    return collList()
  },
  rename_collection: ({ id, name }) => ((collections = collections.map((c) => (c.id === id ? { ...c, name } : c))), collList()),
  delete_collection: ({ id }) => ((collections = collections.filter((c) => c.id !== id || c.builtin)), collList()),
  add_to_collection: ({ id, familyKeys }) => (familyKeys.forEach((k: string) => collItems[id]?.add(k)), collList()),
  remove_from_collection: ({ id, familyKeys }) => (familyKeys.forEach((k: string) => collItems[id]?.delete(k)), collList()),
  list_tags: () => {
    const counts = new Map<string, number>()
    families.forEach((f) => f.tags.forEach((t) => counts.set(t, (counts.get(t) ?? 0) + 1)))
    return [...counts].map(([name, count]) => ({ name, count }))
  },
  set_tags: ({ familyKey, tags }) => ((fam(familyKey)!.tags = tags), tags),
  set_license: ({ familyKey, label, file, note }) => {
    const f = fam(familyKey)!
    f.license = label
    f.licenseInfo = { label, file: file === undefined ? f.licenseInfo.file : file, note: note === undefined ? f.licenseInfo.note : note }
    emit('library-changed', { reason: 'scan' })
    return f.licenseInfo
  },
  list_project_sets: () => sets,
  save_project_set: ({ id, name, note, familyKeys }) => {
    if (id != null) sets = sets.map((s) => (s.id === id ? { ...s, name, note, familyKeys } : s))
    else sets = [...sets, { id: Math.max(0, ...sets.map((s) => s.id)) + 1, name, note, familyKeys, on: false, lastUsed: null }]
    return sets
  },
  delete_project_set: ({ id }) => (sets = sets.filter((s) => s.id !== id)),
  toggle_project_set: ({ id, on, confirmUnknownLicense }) => {
    const set = sets.find((s) => s.id === id)!
    let keys = set.familyKeys
    if (!on) {
      // Keep fonts another active set or an admin rule still needs.
      const needed = new Set(sets.filter((s) => s.on && s.id !== id).flatMap((s) => s.familyKeys))
      keys = keys.filter((k) => !needed.has(k) && !fam(k)?.adminSet)
    }
    const hashes = keys.flatMap((k) => fam(k)?.faces.map((x) => x.hash) ?? [])
    const res = activate(hashes, on, confirmUnknownLicense)
    sets = sets.map((s) => (s.id === id ? { ...s, on, lastUsed: iso() } : s))
    return res
  },
  organize_plan: ({ tidy }) => ({ changes: plan(tidy), scanned: 1284, scannedAt: iso(-2 * 60_000) }),
  organize_apply: ({ tidy, excludeIds }) => {
    const applied = plan(tidy).filter((c) => !excludeIds.includes(c.id)).length
    const id = Math.max(0, ...history.map((h) => h.id)) + 1
    history = [{ id, kind: 'organize', summary: `Auto-organize · ${applied} changes applied`, at: iso(), undone: false }, ...history]
    emit('library-changed', { reason: 'organize' })
    return { historyId: id, applied }
  },
  list_history: () => history,
  undo_history: ({ id }) => {
    history = history.map((h) => (h.id === id ? { ...h, undone: true } : h))
    emit('library-changed', { reason: 'organize' })
    return history
  },
  health: () => ({ ...health, active: families.flatMap((f) => f.faces).filter((x) => x.active).length }),
  repair_now: () => {
    health = { lastCheck: iso(), problems: [], removed: 3, reregistered: 1 }
    return { ...health, active: families.flatMap((f) => f.faces).filter((x) => x.active).length }
  },
  pdf_fonts: () => {
    const h = (family: string, style: string) => fam(keyOf(family))?.faces.find((x) => x.style === style)?.hash ?? fam(keyOf(family))?.faces[0].hash ?? null
    const st = (family: string): PdfFont['status'] => (fam(keyOf(family))?.activeState === 'none' ? 'inactive' : 'active')
    return [
      { psName: 'Fraunces-SemiBold', display: 'Fraunces SemiBold', pages: 12, status: st('Fraunces'), detail: 'Used on 12 pages', hash: h('Fraunces', 'Regular'), google: true },
      { psName: 'SpaceGrotesk-Medium', display: 'Space Grotesk Medium', pages: 14, status: st('Space Grotesk'), detail: 'Used on 14 pages', hash: h('Space Grotesk', 'Medium'), google: true },
      { psName: 'BricolageGrotesque-Regular', display: 'Bricolage Grotesque', pages: 3, status: st('Bricolage Grotesque'), detail: 'Used on 3 pages', hash: h('Bricolage Grotesque', 'Regular'), google: true },
      { psName: 'BebasNeue-Regular', display: 'Bebas Neue', pages: 2, status: 'version', detail: 'PDF used v1.400 · your library has v2.000', hash: h('Bebas Neue', 'Regular'), google: true },
      { psName: 'Lora-SemiBold', display: 'Lora SemiBold', pages: 1, status: 'missing', detail: 'Used on 1 page', hash: null, google: true },
      { psName: 'BrandonGrotesque-Bold', display: 'Brandon Grotesque Bold', pages: 4, status: 'missing', detail: 'Only a subset is embedded in the PDF', hash: null, google: false },
    ] satisfies PdfFont[]
  },
  google_catalog: ({ search, category, offset = 0, limit = 30 }) => {
    const q = search?.trim().toLowerCase()
    const all = [...google, ...mockFiller]
      .filter((g) => (!q || g.family.toLowerCase().includes(q)) && (!category || g.category === category))
      .sort((a, b) => a.family.toLowerCase().localeCompare(b.family.toLowerCase()))
    return { items: all.slice(offset, offset + limit), total: all.length }
  },
  google_install: ({ family }) => {
    const g = google.find((x) => x.family === family)!
    g.installed = true
    emit('library-changed', { reason: 'scan' })
    return { family, installed: true }
  },
  google_remove: ({ family }) => {
    const g = google.find((x) => x.family === family)!
    g.installed = false
    emit('library-changed', { reason: 'scan' })
    return { family, installed: false }
  },
  backup_create: ({ path }) => ({ path, at: iso() }),
  backup_restore: ({ path }) => {
    history = [{ id: Math.max(0, ...history.map((h) => h.id)) + 1, kind: 'restore', summary: `Restored backup ${path.split(/[\\/]/).pop()}`, at: iso(), undone: false }, ...history]
    return { path, at: iso() }
  },
  get_settings: () => settings,
  update_settings: ({ patch }) => (settings = { ...settings, ...patch }),
  get_connection: () => connection,
  test_connection: ({ serverUrl, installKey }) =>
    /^https:\/\/\S+$/.test(serverUrl) && installKey.trim()
      ? { ok: true, error: null }
      : { ok: false, error: 'The server didn’t accept this address and install key.' },
  save_connection: ({ serverUrl, installKey }) => {
    if (!connection.editable) throw err('locked', 'Your admin has locked the connection settings.')
    const ok = /^https:\/\/\S+$/.test(serverUrl) && installKey.trim()
    setConnection({ serverUrl, installKey, status: ok ? 'connected' : 'error', lastSync: ok ? iso() : connection.lastSync, error: ok ? null : 'Can’t reach the server.' })
    return connection
  },
  reset_connection: () => (setConnection({ ...DEFAULT_CONN, status: 'connected', lastSync: iso(), error: null }), connection),
  quick_search: ({ query }) => {
    const q = query.trim().toLowerCase()
    return families.flatMap((f) => f.faces).filter((x) => !q || x.fullName.toLowerCase().includes(q)).slice(0, 20)
  },
  show_in_folder: ({ path }) => console.info('[mock] show_in_folder', path),
  adobe_status: () => adobe,
  adobe_install: ({ app: a }) => Object.assign(adobe.find((r) => r.app === a)!, { installed: true }),
  adobe_remove: ({ app: a }) => Object.assign(adobe.find((r) => r.app === a)!, { installed: false, paired: false }),
  adobe_pair_respond: ({ requestId, allow }) => {
    console.info('[mock] adobe_pair_respond', requestId, allow)
  },
  pick_folder: () => 'D:\\Fonts\\New folder',
  pick_file: ({ filters }) => {
    const ext = filters?.[0]?.extensions?.[0] ?? 'txt'
    return { pdf: 'C:\\Users\\budi\\Documents\\brochure-final.pdf', typeshelf: 'C:\\Users\\budi\\Documents\\typeshelf-backup-2026-10-20.typeshelf' }[ext as string]
      ?? `C:\\Users\\budi\\Documents\\license.${ext}`
  },
}

export async function mockInvoke<T>(cmd: string, args: Args): Promise<T> {
  await delay()
  const h = handlers[cmd]
  if (!h) throw err('unknown_command', `Mock has no handler for ${cmd}`)
  return clone(h(args)) as T
}
