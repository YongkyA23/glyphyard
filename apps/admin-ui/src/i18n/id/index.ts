import { setDict } from '..'

const mods = import.meta.glob<{ default: Record<string, string> }>('./*.ts', { eager: true })
const merged: Record<string, string> = {}
for (const [path, m] of Object.entries(mods)) if (path !== './index.ts') Object.assign(merged, m.default)
setDict(merged)
