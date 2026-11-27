// Merges every area dictionary in this folder (one file per area, so parallel work never edits a shared file).
// Outside Vite (bun scripts) import.meta.glob doesn't exist: the dictionary is empty and t() returns English.
let mods: Record<string, Record<string, string>> = {}
try {
  mods = import.meta.glob<Record<string, string>>(['./*.ts', '!./index.ts'], { eager: true, import: 'default' })
} catch {
  // not running under Vite
}

const ID = new Map<string, string>()
for (const dict of Object.values(mods)) for (const [en, id] of Object.entries(dict)) ID.set(en, id)
export default ID
