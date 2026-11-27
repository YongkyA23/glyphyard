// Lists t('…') / tk('…') strings with no Indonesian entry. Run with bun from an app folder:
//   bun <path-to>/scripts/i18n-check.ts src [files…]
// Scans every .ts/.tsx under src (minus src/i18n), or only the listed files. The dictionary is always every
// src/i18n/id/*.ts default export. Also fails on t()/tk() calls whose first argument isn't a string literal
// (mark a deliberate one with an `i18n-dynamic` comment on the same line) and on keys two files translate differently.
// Parses with the app's own TypeScript, so comments, JSX text and escapes can't fool it.
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { createRequire } from 'node:module'
import { join, relative, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import type * as TS from 'typescript'

const ts: typeof TS = createRequire(join(process.cwd(), 'package.json'))('typescript')

const [srcArg, ...only] = process.argv.slice(2)
if (!srcArg) {
  console.error('usage: bun scripts/i18n-check.ts <srcDir> [files…]')
  process.exit(2)
}
const src = resolve(srcArg)
const dictDir = join(src, 'i18n', 'id')
const rel = (f: string) => relative(process.cwd(), f).replaceAll('\\', '/')

// ---- dictionary ----
const dict = new Map<string, { id: string; file: string }>()
const problems: string[] = []
for (const name of readdirSync(dictDir).filter((n) => /\.ts$/.test(n) && n !== 'index.ts').sort()) {
  const file = join(dictDir, name)
  const entries: Record<string, string> = (await import(pathToFileURL(file).href)).default
  for (const [en, id] of Object.entries(entries)) {
    const had = dict.get(en)
    if (had && had.id !== id) problems.push(`${rel(file)}: '${en}' is '${id}' here but '${had.id}' in ${rel(had.file)}`)
    else dict.set(en, { id, file })
  }
}

// ---- sources ----
const walk = (dir: string): string[] =>
  readdirSync(dir).flatMap((n) => {
    const p = join(dir, n)
    if (statSync(p).isDirectory()) return p === join(src, 'i18n') ? [] : walk(p)
    return /\.tsx?$/.test(n) && !n.endsWith('.d.ts') ? [p] : []
  })
const files = only.length ? only.map((f) => resolve(f)) : walk(src)

let missing = 0
let calls = 0
for (const file of files) {
  const text = readFileSync(file, 'utf8')
  const lines = text.split('\n')
  const sf = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS)
  const visit = (node: TS.Node) => {
    if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && (node.expression.text === 't' || node.expression.text === 'tk')) {
      calls++
      const arg = node.arguments[0]
      const line = sf.getLineAndCharacterOfPosition(node.getStart(sf)).line
      const where = `${rel(file)}:${line + 1}`
      if (arg && ts.isStringLiteral(arg)) {
        if (!dict.has(arg.text)) (missing++, console.log(`${where}  ${JSON.stringify(arg.text)}`))
      } else if (!lines[line].includes('i18n-dynamic')) {
        problems.push(`${where}: ${node.expression.text}() needs a string literal as its first argument`)
      }
    }
    ts.forEachChild(node, visit)
  }
  visit(sf)
}

for (const p of problems) console.log(p)
console.log(`${files.length} files, ${calls} calls, ${dict.size} dictionary entries: ${missing} missing, ${problems.length} other problems`)
process.exit(missing || problems.length ? 1 : 0)
