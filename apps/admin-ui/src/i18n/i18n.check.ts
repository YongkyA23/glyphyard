// Run: bun src/i18n/i18n.check.ts
import { setLangSetting, t } from '.'

const eq = (a: string, b: string) => { if (a !== b) throw new Error(`expected ${JSON.stringify(b)}, got ${JSON.stringify(a)}`) }

setLangSetting('en')
eq(t('Hello {name}', { name: 'A' }), 'Hello A')
eq(t('{n} fonts', { n: 1234.5 }), '1,234.5 fonts')

setLangSetting('id')
eq(t('Not in any dictionary'), 'Not in any dictionary')
eq(t('{n} fonts', { n: 1234.5 }), '1.234,5 fonts')
console.log('i18n.check ok')
