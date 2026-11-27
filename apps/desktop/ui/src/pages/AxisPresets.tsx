import { useEffect, useState } from 'react'
import { api, type AxisPreset } from '../api'
import { t } from '../i18n'
import { errMsg, PromptDialog } from '../ui'
import { useApp } from '../state'
import type { AxisState } from './Playground'

/** Named variable-axis settings for one family: apply, delete, or save the current ones. */
export function AxisPresets({ s, set }: { s: AxisState | null; set: (s: AxisState) => void }) {
  const { toast } = useApp()
  const key = s?.axes.length ? s.key : ''
  const [list, setList] = useState<{ key: string; items: AxisPreset[] } | null>(null)
  const [naming, setNaming] = useState(false)
  useEffect(() => {
    if (!key) return
    let live = true
    api.listAxisPresets(key).then((items) => live && setList({ key, items }), () => {})
    return () => void (live = false)
  }, [key])
  if (!s || !key) return null
  const items = list?.key === key ? list.items : []
  const run = (p: Promise<AxisPreset[]>, done: string) =>
    p.then((next) => (setList({ key, items: next }), toast(done)), (e) => toast(errMsg(e)))
  const apply = (p: AxisPreset) => set({
    ...s,
    v: { ...s.v, ...Object.fromEntries(s.axes.filter((a) => a.tag in p.values).map((a) => [a.tag, Math.min(a.max, Math.max(a.min, p.values[a.tag]))])) },
  })
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 4, fontSize: 13 }}>
      {items.map((p) => (
        <div key={p.name} className="row-between">
          <button type="button" className="btn small link" style={{ paddingLeft: 0 }} aria-label={t('Apply preset {name}', { name: p.name })} onClick={() => apply(p)}>{p.name}</button>
          <button type="button" className="btn small link" aria-label={t('Delete preset {name}', { name: p.name })} onClick={() => run(api.deleteAxisPreset(key, p.name), t('Preset “{name}” deleted', { name: p.name }))}>{t('Delete')}</button>
        </div>
      ))}
      <button type="button" className="btn small" style={{ alignSelf: 'flex-start' }} onClick={() => setNaming(true)}>{t('Save axes as preset…')}</button>
      <PromptDialog
        open={naming} title={t('Save axis preset')} label={t('Preset name')} confirm={t('Save')} onClose={() => setNaming(false)}
        onSubmit={(name) => (setNaming(false), run(api.saveAxisPreset(key, name, s.v), t('Preset “{name}” saved', { name: name.trim() })))}
      />
    </div>
  )
}
