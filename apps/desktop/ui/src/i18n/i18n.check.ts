// Self-check for t(): `bun src/i18n/i18n.check.ts` from apps/desktop/ui. Outside Vite the dictionary starts empty.
import { getLang, numLocale, setLang, t, tk } from './index'
import ID from './id'
import common from './id/common'

const eq = (got: unknown, want: unknown, what: string) => {
  if (got !== want) throw new Error(`${what}: got ${JSON.stringify(got)}, want ${JSON.stringify(want)}`)
}
for (const [en, id] of Object.entries(common)) ID.set(en, id)

setLang('en')
eq(getLang(), 'en', 'setLang')
eq(t('Hello {name}', { name: 'A' }), 'Hello A', 'placeholder in en')
eq(t('{n} fonts', { n: 1234567 }), '1,234,567 fonts', 'en number format')
eq(t('Back'), 'Back', 'en ignores the dictionary')
eq(t('{missing} stays'), '{missing} stays', 'unknown placeholder')

setLang('id')
eq(t('Not in any dictionary {x}', { x: 'y' }), 'Not in any dictionary y', 'unknown key falls back to English')
eq(t('Back'), 'Kembali', 'known key in id')
eq(numLocale(), 'id-ID', 'numLocale in id')
eq(t('{n} font', { n: 1234567 }), `${(1234567).toLocaleString('id-ID')} font`, '{n} uses numLocale')
eq(tk('Raw'), 'Raw', 'tk is identity')

console.log('i18n check passed')
