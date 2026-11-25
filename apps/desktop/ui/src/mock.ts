// In-memory backend used when the UI runs in a normal browser (no Tauri). Sample data comes from the design mockups.
// Dev helpers on window.typeshelfMock: emit(event, payload), setOffline(bool).
import type {
  ActivationResult, AdobeRow, AppState, Collection, Connection, Face, Family, FamilyDetail, FileRow, Folder, FolderNode, FontCategory,
  GlyphBlock, GoogleFont, HistoryEntry, License, ListFamiliesArgs, OrganizeChange, PdfFont, ProjectSet, Settings,
} from './api'

const day = 86_400_000
const iso = (offsetMs = 0) => new Date(Date.now() + offsetMs).toISOString()
const clone = <T,>(v: T): T => structuredClone(v)
const err = (code: string, message: string, extra: Record<string, unknown> = {}) => ({ code, message, ...extra })

const WEIGHT_NAMES: Record<number, string> = { 100: 'Thin', 200: 'ExtraLight', 300: 'Light', 400: 'Regular', 500: 'Medium', 600: 'SemiBold', 700: 'Bold', 800: 'ExtraBold', 900: 'Black' }

type Seed = {
  family: string; foundry: string; designer: string; license: License; folder: string; weights: number[]; italics?: boolean
  variable?: boolean; admin?: boolean; dupe?: boolean; on: boolean; addedDaysAgo: number; axes?: FamilyDetail['axes']; features: string[]
  category: FontCategory
}
const SEEDS: Seed[] = [
  { family: 'Fraunces', category: 'serif', foundry: 'Undercase Type', designer: 'Phaedra Charles, Flavia Zimbardi', license: 'licensed', folder: 'D:\\Fonts\\Licensed', weights: [400], italics: true, variable: true, admin: true, on: true, addedDaysAgo: 2,
    axes: [{ tag: 'wght', min: 100, default: 600, max: 900 }, { tag: 'opsz', min: 9, default: 72, max: 144 }, { tag: 'SOFT', min: 0, default: 50, max: 100 }, { tag: 'WONK', min: 0, default: 0, max: 1 }],
    features: ['liga', 'onum', 'frac', 'smcp'] },
  { family: 'Space Grotesk', category: 'sans', foundry: 'Florian Karsten', designer: 'Florian Karsten', license: 'licensed', folder: 'D:\\Fonts\\Licensed', weights: [300, 400, 500, 600, 700], admin: true, on: true, addedDaysAgo: 2, features: ['liga', 'ss01', 'ss02', 'tnum'] },
  { family: 'Instrument Serif', category: 'serif', foundry: 'Instrument', designer: 'Rodrigo Fuenzalida', license: 'personal', folder: 'D:\\Fonts\\Client work', weights: [400], italics: true, on: false, addedDaysAgo: 30, features: ['liga', 'onum'] },
  { family: 'DM Serif Display', category: 'display', foundry: 'Colophon', designer: 'Colophon Foundry', license: 'client', folder: 'D:\\Fonts\\Client work', weights: [400], italics: true, on: true, addedDaysAgo: 12, features: ['liga', 'dlig'] },
  { family: 'Archivo Black', category: 'display', foundry: 'Omnibus-Type', designer: 'Héctor Gatti', license: 'unknown', folder: 'E:\\Archive', weights: [400], on: false, addedDaysAgo: 120, features: ['liga'] },
  { family: 'IBM Plex Mono', category: 'mono', foundry: 'IBM', designer: 'Mike Abbink, Bold Monday', license: 'licensed', folder: 'D:\\Fonts\\Licensed', weights: [100, 200, 300, 400, 500, 600, 700], italics: true, on: true, addedDaysAgo: 60, features: ['zero', 'ss01', 'frac'] },
  { family: 'Bebas Neue', category: 'display', foundry: 'Dharma Type', designer: 'Ryoichi Tsunekawa', license: 'unknown', folder: 'D:\\Fonts\\Client work', weights: [400], dupe: true, on: false, addedDaysAgo: 8, features: ['kern'] },
  { family: 'Syne', category: 'display', foundry: 'Bonjour Monde', designer: 'Lucas Descroix', license: 'licensed', folder: 'D:\\Fonts\\Licensed', weights: [400, 500, 600, 700, 800], admin: true, on: true, addedDaysAgo: 2, features: ['liga', 'ss01'] },
  { family: 'Bricolage Grotesque', category: 'sans', foundry: 'Mathieu Triay', designer: 'Mathieu Triay', license: 'licensed', folder: 'D:\\Fonts\\Licensed', weights: [400], variable: true, on: false, addedDaysAgo: 40,
    axes: [{ tag: 'wght', min: 200, default: 400, max: 800 }, { tag: 'opsz', min: 12, default: 14, max: 96 }], features: ['liga', 'ss01'] },
  { family: 'Familjen Grotesk', category: 'sans', foundry: 'Familjen STHLM', designer: 'Familjen STHLM', license: 'licensed', folder: 'D:\\Fonts\\Licensed', weights: [400, 500, 600, 700], on: false, addedDaysAgo: 50, features: ['liga'] },
  { family: 'Schibsted Grotesk', category: 'sans', foundry: 'Bakken & Bæck', designer: 'Bakken & Bæck', license: 'personal', folder: 'D:\\Fonts\\Client work', weights: [400], variable: true, on: false, addedDaysAgo: 70,
    axes: [{ tag: 'wght', min: 400, default: 400, max: 900 }], features: ['liga', 'tnum'] },
  { family: 'Hanken Grotesk', category: 'sans', foundry: 'Hanken Design Co.', designer: 'Alfredo Marco Pradil', license: 'unknown', folder: 'D:\\Fonts\\Client work', weights: [400], variable: true, on: false, addedDaysAgo: 90,
    axes: [{ tag: 'wght', min: 100, default: 400, max: 900 }], features: ['liga'] },
  { family: 'Caveat', category: 'handwriting', foundry: 'Impallari Type', designer: 'Pablo Impallari', license: 'licensed', folder: 'D:\\Fonts\\Client work', weights: [400, 700], on: false, addedDaysAgo: 15, features: ['liga', 'calt'] },
]

// ---- glyph coverage (face_glyphs, glyph search) ----
const span = (a: number, b: number) => Array.from({ length: b - a + 1 }, (_, i) => a + i)
const UNI_BLOCKS: [string, number, number][] = [
  ['Basic Latin', 0x0000, 0x007f], ['Latin-1 Supplement', 0x0080, 0x00ff], ['Latin Extended-A', 0x0100, 0x017f],
  ['Greek and Coptic', 0x0370, 0x03ff], ['Cyrillic', 0x0400, 0x04ff], ['Latin Extended Additional', 0x1e00, 0x1eff],
  ['General Punctuation', 0x2000, 0x206f], ['Currency Symbols', 0x20a0, 0x20cf], ['Arrows', 0x2190, 0x21ff], ['Box Drawing', 0x2500, 0x257f],
  ['CJK Unified Ideographs', 0x4e00, 0x9fff],
]
const COMMON = [...span(0x20, 0x7e), ...span(0xa0, 0xff).filter((c) => c !== 0xad), 0x2013, 0x2014, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2026, 0x20ac]
const EXTRA: Record<string, number[]> = {
  fraunces: [...span(0x100, 0x17f), 0x1a0, 0x1a1, 0x1af, 0x1b0, ...span(0x1e00, 0x1eff)],
  // Over 512 CJK codepoints so the Glyphs tab shows its "Show all" button.
  'ibm-plex-mono': [...span(0x100, 0x17f), ...span(0x391, 0x3a1), ...span(0x3a3, 0x3a9), ...span(0x3b1, 0x3c9), ...span(0x410, 0x44f), ...span(0x2190, 0x2199), ...span(0x2500, 0x257f), ...span(0x4e00, 0x50ff)],
  'space-grotesk': span(0x100, 0x17f), 'bricolage-grotesque': span(0x100, 0x17f), syne: [...span(0x100, 0x17f), ...span(0x410, 0x44f)],
}
const glyphsOf = (key: string) => [...COMMON, ...(EXTRA[key] ?? [])].sort((a, b) => a - b)
const covers = (key: string, chars: string) => {
  const have = new Set(glyphsOf(key))
  return [...chars].every((c) => have.has(c.codePointAt(0)!))
}
// Shortened sets: the real list (crates/core/src/languages.rs) has every required letter.
const LATIN = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz'
const LANGS: [id: string, name: string, chars: string][] = [
  ['english', 'English', LATIN], ['indonesian', 'Indonesian', LATIN], ['malay', 'Malay', LATIN],
  ['vietnamese', 'Vietnamese', `${LATIN}ĂÂĐÊÔƠƯăâđêôơưẠạẤấẦầẬậỆệỘộỢợỰự`], ['french', 'French', `${LATIN}ÀÂÇÉÈÊËÎÏÔÙÛÜàâçéèêëîïôùûü`],
  ['german', 'German', `${LATIN}ÄÖÜäöüß`], ['spanish', 'Spanish', `${LATIN}ÁÉÍÓÚÜÑáéíóúüñ¿¡`], ['portuguese', 'Portuguese', `${LATIN}ÁÀÂÃÇÉÊÍÓÔÕÚáàâãçéêíóôõú`],
  ['polish', 'Polish', `${LATIN}ĄĆĘŁŃÓŚŹŻąćęłńóśźż`], ['turkish', 'Turkish', `${LATIN}ÇĞİÖŞÜçğıöşü`],
  ['russian', 'Russian', 'АБВГДЕЁЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯабвгдеёжзийклмнопрстуфхцчшщъыьэюя'],
  ['greek', 'Greek', 'ΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡΣΤΥΦΧΨΩαβγδεζηθικλμνξοπρστυφχψω'], ['arabic', 'Arabic', 'ءآأإابتثجحخدذرزسشصضطظعغفقكلمنهوي'],
  ['hebrew', 'Hebrew', 'אבגדהוזחטיכלמנסעפצקרשת'], ['thai', 'Thai', 'กขคงจฉชซญดตทนบปผฝพฟภมยรลวศษสหอฮ'], ['hindi', 'Hindi', 'अआइईउऊएऐओऔकखगघचछजझटठडढणतथदधनपफबभमयरलवशषसह'],
  ['japanese', 'Japanese', 'あいうえおかきくけこアイウエオカキクケコ日本語人大中小'], ['korean', 'Korean', '가나다라마바사아자차카타파하한국어'], ['chinese', 'Chinese', '的一是不了人我在有他这中大来上国个到说们'],
]

let hashN = 0
const nextHash = () => (++hashN).toString(16).padStart(8, '0') + 'b3a1c9e2'
const keyOf = (family: string) => family.toLowerCase().replace(/\s+/g, '-')

type Fam = Family & Omit<FamilyDetail, keyof Family> & { addedAt: number; original: string }

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
        path: `${dir}\\${file}`, active: s.on, source: s.on ? (s.admin ? 'admin' : 'user') : null, weight: w, italic, isVariable: !!s.variable, system: false,
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
    license: s.license, activeState: s.on ? 'all' : 'none', adminSet: !!s.admin, hasDuplicates: !!s.dupe, system: false, faces, category: s.category,
    axes: s.axes ?? [], features: s.features, glyphCount: glyphsOf(keyOf(s.family)).length + 24, designer: s.designer, files,
    licenseInfo: {
      label: s.license,
      file: s.license === 'licensed' ? `${dir}\\OFL.txt` : null,
      note: s.license === 'licensed' ? 'Free for commercial use, including logos.' : '',
    },
    addedAt: Date.now() - s.addedDaysAgo * day, tags: s.category === 'display' ? ['headline'] : [], original: s.family, pinned: false,
    details: {
      copyright: `Copyright 2026 The ${s.family} Project Authors`,
      trademark: s.license === 'licensed' ? null : `${s.family} is a trademark of ${s.foundry}.`,
      manufacturer: s.foundry, designer: s.designer,
      description: s.category === 'display' ? `${s.family} is a display face for headlines and posters, with tight spacing and high contrast.` : null,
      license: s.license === 'licensed' ? 'This Font Software is licensed under the SIL Open Font License, Version 1.1.'
        : s.license === 'personal' ? 'Free for personal use. Contact the foundry for a commercial license.' : null,
      licenseUrl: s.license === 'licensed' ? 'https://openfontlicense.org' : null,
      vendorUrl: `https://${s.foundry.toLowerCase().replace(/[^a-z0-9]+/g, '')}.com`,
      designerUrl: s.family === 'Fraunces' ? 'https://undercase.xyz' : null,
      version: `Version ${s.dupe ? '2.000' : '1.000'}`, uniqueId: `${s.dupe ? '2.000' : '1.000'};${s.foundry.slice(0, 4).toUpperCase()};${base}-Regular`,
      format: s.variable ? 'Variable TrueType' : 'OpenType (CFF)', upm: 1000, glyphCount: glyphsOf(keyOf(s.family)).length + 24,
    },
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
  const { axes: _a, features: _f, glyphCount: _g, designer: _d, files: _fi, licenseInfo: _l, addedAt: _ad, original: _o, details: _de, ...rest } = f
  return clone(rest)
}

let folders: Folder[] = [
  { id: 1, path: 'D:\\Fonts\\Licensed', status: 'watching', fontCount: 812, offlineSince: null },
  { id: 2, path: 'D:\\Fonts\\Client work', status: 'watching', fontCount: 340, offlineSince: null },
  { id: 3, path: 'E:\\Archive', status: 'offline', fontCount: 132, offlineSince: new Date(Date.now() - 3 * day).toISOString() },
]
let collections: Omit<Collection, 'active'>[] = [
  { id: 1, name: 'Favorites', parentId: null, count: 0, builtin: 'favorites' },
  { id: 2, name: 'Recently used', parentId: null, count: 0, builtin: 'recent' },
  { id: 3, name: 'Headlines', parentId: null, count: 0, builtin: null },
]
const collItems: Record<number, Set<string>> = {
  1: new Set(['fraunces', 'space-grotesk', 'syne', 'ibm-plex-mono']),
  2: new Set(['fraunces', 'dm-serif-display', 'space-grotesk']),
  3: new Set(['fraunces', 'dm-serif-display', 'bebas-neue', 'archivo-black', 'instrument-serif']),
}
const collList = (): Collection[] => collections.map((c) => ({
  ...c,
  count: collItems[c.id]?.size ?? 0,
  active: [...(collItems[c.id] ?? [])].filter((k) => (fam(k)?.activeState ?? 'none') !== 'none').length,
}))

let sets: ProjectSet[] = [
  { id: 1, name: 'Northwind rebrand', note: 'Client', familyKeys: ['dm-serif-display', 'space-grotesk', 'ibm-plex-mono', 'archivo-black', 'syne'], on: true, lastUsed: iso(-2 * 3600_000) },
  { id: 2, name: 'Café Lumen menu', note: 'Client', familyKeys: ['fraunces', 'instrument-serif', 'space-grotesk'], on: false, lastUsed: iso(-4 * day) },
  { id: 3, name: 'Wedding invitations', note: 'Personal', familyKeys: ['instrument-serif', 'bebas-neue'], on: false, lastUsed: iso(-21 * day) },
]

const DEFAULT_CONN = { serverUrl: 'https://fonts.northwind-studio.com', installKey: 'tsk_live_4f8a2c91e7d3' }
let connection: Connection = {
  status: 'connected', ...DEFAULT_CONN, editable: true, lastSync: iso(-60_000), installId: '7F3A-2C9E-91C2', error: null,
}
// The onboarding name survives reloads within the tab so `bun run dev` doesn't ask every time;
// `?mockName=Ayu` skips onboarding (used for README screenshots with a headless browser).
const savedName = new URLSearchParams(location.search).get('mockName') ?? sessionStorage.getItem('typeshelf-mock-name') ?? ''
let app: AppState = {
  onboarded: !!savedName, name: savedName, nextNameChange: null, nameChangeDays: 30, connection, platform: 'windows', appVersion: '1.0.0',
}
let settings: Settings = {
  startAtLogin: true, startHidden: true, autoUpdate: true, deactivateOnQuit: false,
  quickShortcut: 'CmdOrCtrl+Alt+Space', theme: 'light', sampleText: 'Sphinx of black quartz, judge my vow',
  sampleTexts: [
    'Sphinx of black quartz, judge my vow', 'ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz', '0123456789 !?&@#%(){}[]', 'Hamburgefonstiv',
    'Typography is the craft of endowing human language with a durable visual form.',
  ],
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
function activate(hashes: string[], active: boolean): ActivationResult[] {
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

const hiddenPaths = new Set<string>()
const autoPaths = new Set<string>()
const inside = (path: string, dir: string) => path === dir || path.startsWith(dir + '\\')
const isHidden = (path: string) => [...hiddenPaths].some((h) => inside(path, h))
/** One level of subfolders (the foundry folders) under each watched folder. */
function folderTree(): FolderNode[] {
  const node = (path: string, name: string, faces: Face[], fontCount: number, children: FolderNode[]): FolderNode => ({
    path, name, fontCount, activeCount: faces.filter((x) => x.active).length, autoActivate: autoPaths.has(path), hidden: hiddenPaths.has(path), children,
  })
  return folders.map((d) => {
    const subs = new Map<string, Face[]>()
    families.flatMap((f) => f.faces).filter((x) => x.path.startsWith(d.path + '\\')).forEach((x) => {
      const p = d.path + '\\' + x.path.slice(d.path.length + 1).split('\\')[0]
      if (p !== x.path) subs.set(p, [...(subs.get(p) ?? []), x])
    })
    return node(d.path, d.path, [...subs.values()].flat(), d.fontCount, [...subs].map(([p, fs]) => node(p, p.split('\\').pop()!, fs, fs.length, [])))
  })
}
/** Deterministic shuffle key: a hash of family key + seed. */
const seeded = (key: string, seed: number) => {
  let h = seed | 0
  for (const c of key) h = Math.imul(h ^ c.charCodeAt(0), 2654435761)
  return (h ^ (h >>> 15)) >>> 0
}

/** SuperSearch: key:value tokens (quotes for spaces) AND together; other words match the family or foundry name. */
function superMatch(f: Fam, search: string) {
  for (const m of search.matchAll(/(\w+):(?:"([^"]*)"|(\S+))|"([^"]*)"|(\S+)/g)) {
    const key = m[1]?.toLowerCase(), raw = m[2] ?? m[3] ?? '', val = raw.toLowerCase()
    const word = (key ? `${m[1]}:${raw}` : (m[4] ?? m[5])).toLowerCase()
    let ok: boolean
    switch (key) {
      case 'tag': ok = f.tags.some((t) => t.toLowerCase() === val); break
      case 'foundry': ok = f.foundry.toLowerCase().includes(val); break
      case 'designer': ok = f.designer.toLowerCase().includes(val); break
      case 'category': ok = f.category === val; break
      case 'license': ok = f.license === val; break
      case 'char': case 'glyph': ok = covers(f.key, raw); break
      case 'lang': case 'language': { const l = LANGS.find(([id]) => id === val); ok = !!l && covers(f.key, l[2]); break }
      case 'weight': {
        const r = /^([<>])?(\d+)(?:-(\d+))?$/.exec(val)
        ok = !!r && f.faces.some(({ weight: w }) => (r[1] === '>' ? w > +r[2] : r[1] === '<' ? w < +r[2] : r[3] ? w >= +r[2] && w <= +r[3] : w === +r[2]))
        break
      }
      default: {
        const flags: Record<string, boolean> = {
          italic: f.faces.some((x) => x.italic), variable: f.isVariable, active: f.activeState !== 'none', inactive: f.activeState === 'none', pinned: f.pinned, system: f.system,
        }
        ok = flags[word] ?? `${f.family} ${f.foundry}`.toLowerCase().includes(word)
      }
    }
    if (!ok) return false
  }
  return true
}

function listFamilies(a: ListFamiliesArgs): Family[] {
  const showHidden = a.includeHidden || (a.path != null && isHidden(a.path)) || a.keys != null
  let out = families.filter((f) => {
    if (!showHidden && f.faces.every((x) => isHidden(x.path))) return false
    if (a.category && f.category !== a.category) return false
    if (a.search?.trim() && !superMatch(f, a.search)) return false
    if (a.chars && !covers(f.key, a.chars)) return false
    if (a.lang && !covers(f.key, LANGS.find(([id]) => id === a.lang)?.[2] ?? ' ')) return false
    switch (a.filter) {
      case 'active': if (f.activeState === 'none') return false; break
      case 'inactive': if (f.activeState !== 'none') return false; break
      case 'admin': if (!f.adminSet) return false; break
      case 'unknown_license': if (f.license !== 'unknown') return false; break
      case 'duplicates': if (!f.hasDuplicates) return false; break
      case 'system': return false
    }
    if (a.collectionId != null && !collItems[a.collectionId]?.has(f.key)) return false
    if (a.folderId != null) {
      const p = folders.find((d) => d.id === a.folderId)?.path
      if (!p || !f.faces.some((x) => x.path.startsWith(p))) return false
    }
    if (a.license && f.license !== a.license) return false
    if (a.path && !f.faces.some((x) => x.path.startsWith(a.path!))) return false
    if (a.keys && !a.keys.includes(f.key)) return false
    if (a.tag && !f.tags.includes(a.tag)) return false
    return true
  })
  const by = { name: (x: Fam, y: Fam) => x.family.localeCompare(y.family), name_desc: (x: Fam, y: Fam) => y.family.localeCompare(x.family),
    date: (x: Fam, y: Fam) => y.addedAt - x.addedAt, foundry: (x: Fam, y: Fam) => x.foundry.localeCompare(y.foundry),
    random: (x: Fam, y: Fam) => seeded(x.key, a.seed ?? 0) - seeded(y.key, a.seed ?? 0) }
  out = [...out].sort((x, y) => Number(y.pinned) - Number(x.pinned) || by[a.sort ?? 'name'](x, y))
  return out.map(toFamily)
}

/** Drops matching faces (and families left empty); returns how many went. */
function removeFaces(match: (x: Face) => boolean) {
  let n = 0
  for (let i = families.length - 1; i >= 0; i--) {
    const f = families[i]
    const keep = f.faces.filter((x) => !match(x))
    if (keep.length === f.faces.length) continue
    n += f.faces.length - keep.length
    f.faces = keep
    f.styles = keep.length
    f.files = f.files.filter((r) => keep.some((x) => x.path === r.path))
    if (keep.length) refreshState(f)
    else {
      families.splice(i, 1)
      Object.values(collItems).forEach((set) => set.delete(f.key))
    }
  }
  return n
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
  list_families: ({ query }) => listFamilies(query ?? {}),
  family_page: ({ query, offset, limit }) => {
    const all = listFamilies(query ?? {})
    return { items: all.slice(offset, offset + limit), total: all.length }
  },
  library_counts: () => {
    const all = listFamilies({})
    const licenses: Record<string, number> = {}
    for (const f of all) licenses[f.license] = (licenses[f.license] ?? 0) + 1
    return { library: all.length, active: all.filter((f) => f.activeState !== 'none').length, system: 0, licenses }
  },
  language_counts: () => {
    const all = listFamilies({})
    return LANGS.map(([id, name, chars]) => ({ id, name, families: all.filter((f) => covers(f.key, chars)).length }))
  },
  get_faces: ({ hashes }) => families.flatMap((f) => f.faces).filter((x) => hashes.includes(x.hash)),
  folder_tree: () => folderTree(),
  set_folder_active: ({ path, active }) =>
    activate(families.flatMap((f) => f.faces).filter((x) => x.path.startsWith(path)).map((x) => x.hash), active),
  set_folder_auto: ({ path, on: auto }) => {
    const res = auto ? activate(families.flatMap((f) => f.faces).filter((x) => inside(x.path, path)).map((x) => x.hash), true) : []
    if (auto) autoPaths.add(path)
    else autoPaths.delete(path)
    emit('library-changed', { reason: 'scan' })
    return res
  },
  set_folder_hidden: ({ path, hidden }) => {
    if (hidden) hiddenPaths.add(path)
    else hiddenPaths.delete(path)
    emit('library-changed', { reason: 'scan' })
    return folderTree()
  },
  delete_folder: ({ path }) => {
    const trashed = removeFaces((x) => inside(x.path, path))
    folders = folders.filter((d) => !inside(d.path, path))
    emit('library-changed', { reason: 'scan' })
    return { trashed, failed: [] }
  },
  similar_families: ({ key, limit }) => {
    const me = fam(key)
    if (!me) throw err('not_found', 'This font family is no longer in your library.')
    const score = (f: Fam) => (f.category === me.category ? 2 : 0) + (f.isVariable === me.isVariable ? 1 : 0)
    return families.filter((f) => f.key !== key).sort((x, y) => score(y) - score(x) || x.family.localeCompare(y.family)).slice(0, limit).map(toFamily)
  },
  set_collection_active: ({ id, active }) => {
    const ids: number[] = [id], keys = new Set<string>()
    for (let i = 0; i < ids.length; i++) collections.filter((c) => c.parentId === ids[i]).forEach((c) => ids.push(c.id))
    ids.forEach((i) => collItems[i]?.forEach((k) => keys.add(k)))
    return activate([...keys].flatMap((k) => fam(k)?.faces.map((x) => x.hash) ?? []), active)
  },
  export_collection: ({ id, dest }) => ({ files: [...(collItems[id] ?? [])].reduce((n, k) => n + (fam(k)?.faces.length ?? 0), 0), path: dest }),
  pick_save_file: ({ defaultName }) => `C:\\Users\\budi\\Documents\\${defaultName}`,
  import_paths: ({ paths }) => {
    const folder = 'C:\\Users\\budi\\AppData\\Roaming\\Typeshelf\\Imported fonts'
    if (!folders.some((d) => d.path === folder))
      folders = [...folders, { id: Math.max(0, ...folders.map((f) => f.id)) + 1, path: folder, status: 'watching', fontCount: 0, offlineSince: null }]
    // ponytail: the mock copies nothing; a font file counts as one font, anything else (a folder) as six.
    const imported = (paths as string[]).reduce((n, p) => n + (/\.(otf|ttf|ttc|woff2?)$/i.test(p) ? 1 : 6), 0)
    folders = folders.map((d) => (d.path === folder ? { ...d, fontCount: d.fontCount + imported } : d))
    emit('library-changed', { reason: 'scan' })
    return { imported, skipped: 0, folder }
  },
  set_family_name: ({ hashes, name }) => {
    // ponytail: renames whole families; the real backend can split some of a family's files off.
    new Set((hashes as string[]).map(famOfHash)).forEach((f) => {
      if (!f) return
      f.family = name ?? f.original
      f.faces.forEach((x) => (x.family = f.family))
    })
    emit('library-changed', { reason: 'scan' })
  },
  delete_fonts: ({ hashes }) => {
    const trashed = removeFaces((x) => hashes.includes(x.hash) && !x.system)
    emit('library-changed', { reason: 'scan' })
    return { trashed, failed: [] }
  },
  get_family: ({ key }) => {
    const f = fam(key)
    if (!f) throw err('not_found', 'This font family is no longer in your library.')
    const { addedAt: _a, original: _o, ...rest } = f
    return rest
  },
  set_active: ({ hashes, active }) => activate(hashes, active),
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
  set_tags: ({ familyKey, tags }) => {
    const clean = [...new Set((tags as string[]).map((t) => t.trim()).filter(Boolean))].sort()
    fam(familyKey)!.tags = clean
    return clean
  },
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
  toggle_project_set: ({ id, on }) => {
    const set = sets.find((s) => s.id === id)!
    let keys = set.familyKeys
    if (!on) {
      // Keep fonts another active set or an admin rule still needs.
      const needed = new Set(sets.filter((s) => s.on && s.id !== id).flatMap((s) => s.familyKeys))
      keys = keys.filter((k) => !needed.has(k) && !fam(k)?.adminSet)
    }
    const hashes = keys.flatMap((k) => fam(k)?.faces.map((x) => x.hash) ?? [])
    const res = activate(hashes, on)
    sets = sets.map((s) => (s.id === id ? { ...s, on, lastUsed: iso() } : s))
    return res
  },
  organize_plan: ({ tidy }) => {
    const all = plan(tidy), info = ['keep_as_alternate', 'group', 'skip']
    return { changes: all.filter((c) => !info.includes(c.action.op)), notes: all.filter((c) => info.includes(c.action.op)), scanned: 1284, scannedAt: iso(-2 * 60_000) }
  },
  organize_apply: ({ tidy, excludeIds }) => {
    const applied = plan(tidy).filter((c) => !excludeIds.includes(c.id)).length
    const id = Math.max(0, ...history.map((h) => h.id)) + 1
    history = [{ id, kind: 'organize', summary: `Auto-organize · ${applied} changes applied`, at: iso(), undone: false }, ...history]
    emit('library-changed', { reason: 'organize' })
    return { historyId: id, applied, failed: [] }
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
  open_url: ({ url }) => void window.open(url, '_blank', 'noopener'),
  adobe_status: () => adobe,
  indesign_running: () => false,
  adobe_install: ({ app: a }) => Object.assign(adobe.find((r) => r.app === a)!, { installed: true }),
  adobe_remove: ({ app: a }) => Object.assign(adobe.find((r) => r.app === a)!, { installed: false, paired: false }),
  adobe_pair_respond: ({ requestId, allow }) => {
    console.info('[mock] adobe_pair_respond', requestId, allow)
  },
  pick_folder: () => 'D:\\Fonts\\New folder',
  pick_file: ({ filters }) => {
    const ext = filters?.[0]?.extensions?.[0] ?? 'txt'
    return { json: 'C:\\Users\\budi\\Documents\\typeshelf-collections.json', pdf: 'C:\\Users\\budi\\Documents\\brochure-final.pdf', typeshelf: 'C:\\Users\\budi\\Documents\\typeshelf-backup-2026-10-20.typeshelf' }[ext as string]
      ?? `C:\\Users\\budi\\Documents\\license.${ext}`
  },
  pick_files: () => ['C:\\Users\\budi\\Downloads\\Inter-Regular.otf', 'C:\\Users\\budi\\Downloads\\Inter-Bold.otf'],
  face_glyphs: ({ hash }) => {
    const f = famOfHash(hash)
    if (!f) throw err('not_found', 'This font is no longer in your library.')
    const cps = glyphsOf(f.key)
    const blocks: GlyphBlock[] = UNI_BLOCKS.map(([name, start, end]) => ({ name, start, end, chars: cps.filter((c) => c >= start && c <= end) })).filter((b) => b.chars.length)
    return { total: cps.length, blocks }
  },
  set_pinned: ({ keys, pinned }) => {
    families.forEach((f) => keys.includes(f.key) && (f.pinned = pinned))
    emit('library-changed', { reason: 'scan' })
  },
  export_collections: () => ({ collections: collections.filter((c) => !c.builtin).length }),
  import_collections: () => {
    // ponytail: the mock reads no file; it always merges the same small "Brand kit" collection.
    const name = 'Brand kit'
    let c = collections.find((x) => x.name === name && x.parentId === null)
    if (!c) {
      c = { id: Math.max(0, ...collections.map((x) => x.id)) + 1, name, parentId: null, count: 0, builtin: null }
      collections = [...collections, c]
      collItems[c.id] = new Set()
    }
    for (const k of ['syne', 'caveat', 'familjen-grotesk']) collItems[c.id].add(k)
    emit('library-changed', { reason: 'scan' })
    return { collections: 1, families: 3, missing: 1 }
  },
}

export async function mockInvoke<T>(cmd: string, args: Args): Promise<T> {
  await delay()
  const h = handlers[cmd]
  if (!h) throw err('unknown_command', `Mock has no handler for ${cmd}`)
  return clone(h(args)) as T
}
