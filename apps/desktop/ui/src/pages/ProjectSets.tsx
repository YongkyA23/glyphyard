import { useEffect, useState, type FormEvent } from 'react'
import { Link } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, type Family, type ProjectSet } from '../api'
import { familyStyle } from '../fonts'
import { Hint } from '../Hint'
import { t } from '../i18n'
import { cssStagger, dur, easeOut } from '../motion'
import { useApp } from '../state'
import { ago, Badge, Dialog, errMsg, LicenseBadge, plural, SelectRing, Skel } from '../ui'

type Draft = { id?: number; name: string; note: string; familyKeys: string[] }

export function ProjectSets() {
  const { app, libVersion, activate, toast } = useApp()
  const [sets, setSets] = useState<ProjectSet[] | null>(null)
  const [families, setFamilies] = useState<Family[]>([])
  const [sel, setSel] = useState<number | null>(null)
  const [draft, setDraft] = useState<Draft | null>(null)
  const [draftError, setDraftError] = useState<string | null>(null)
  const [pick, setPick] = useState('')
  const [hits, setHits] = useState<Family[]>([])

  const loadSets = () => api.listProjectSets().then(setSets, (e) => toast(errMsg(e)))
  useEffect(() => {
    loadSets()
  }, [libVersion]) // eslint-disable-line react-hooks/exhaustive-deps
  // Only the families the sets use are loaded.
  const usedKeys = [...new Set(sets?.flatMap((s) => s.familyKeys) ?? [])].sort().join('\n')
  useEffect(() => {
    if (usedKeys) api.listFamilies({ keys: usedKeys.split('\n') }).then(setFamilies, () => {})
  }, [usedKeys, libVersion])
  // The editor's font list: search results, at most 40 at a time.
  useEffect(() => {
    if (!draft) return
    const h = setTimeout(() => api.familyPage({ search: pick.trim() || undefined, sort: 'name' }, 0, 40).then((p) => setHits(p.items), () => {}), 150)
    return () => clearTimeout(h)
  }, [pick, draft === null]) // eslint-disable-line react-hooks/exhaustive-deps

  const byKey = new Map(families.map((f) => [f.key, f]))
  const current = sets?.find((s) => s.id === sel) ?? sets?.[0]
  const meta = (s: ProjectSet) => [s.note, plural(s.familyKeys.length, 'font'), s.lastUsed ? t('last used {when}', { when: ago(s.lastUsed) }) : t('never used')].filter(Boolean).join(' · ')

  async function toggle(s: ProjectSet) {
    const res = await activate(() => api.toggleProjectSet(s.id, !s.on))
    if (res) loadSets()
  }
  async function save(e: FormEvent) {
    e.preventDefault()
    if (!draft) return
    if (!draft.name.trim()) return setDraftError(t('Give the set a name.'))
    try {
      const next = await api.saveProjectSet({ ...draft, name: draft.name.trim() })
      setSets(next)
      if (draft.id == null) setSel(next.at(-1)?.id ?? null)
      setDraft(null)
    } catch (err) {
      setDraftError(errMsg(err))
    }
  }
  async function remove() {
    if (draft?.id == null) return
    try {
      setSets(await api.deleteProjectSet(draft.id))
      setSel(null)
      setDraft(null)
    } catch (err) {
      setDraftError(errMsg(err))
    }
  }
  const openDraft = (d: Draft) => (setDraft(d), setDraftError(null), setPick(''))


  return (
    <div className="page" style={{ gap: 20 }}>
      <div className="row-between">
        <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
            <h1 className="h-page">{t('Project sets')}</h1>
            <Hint text={t('Turn a whole job’s fonts on or off in one click.')} />
          </div>
        </div>
        <button type="button" className="btn primary" data-tour="sets-new" onClick={() => openDraft({ name: '', note: '', familyKeys: [] })}>{t('New project set')}</button>
      </div>

      {sets === null && (
        <div aria-hidden="true" style={{ display: 'flex', flexWrap: 'wrap', gap: 24 }}>
          <div style={{ flex: '1 1 300px', display: 'flex', flexDirection: 'column', gap: 10 }}>{[0, 1, 2].map((i) => <Skel key={i} h={104} r={12} />)}</div>
          <div style={{ flex: '2 1 420px' }}><Skel h={320} r={12} /></div>
        </div>
      )}
      {sets?.length === 0 && <p className="muted">{t('No project sets yet. Create one for each job so you can switch its fonts on together.')}</p>}
      {current && (
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 24, alignItems: 'flex-start' }}>
          <div data-tour="sets-list" style={{ flex: '1 1 300px', display: 'flex', flexDirection: 'column', gap: 10 }}>
            {sets!.map((s, i) => (
              <button key={s.id} type="button" className="select-card enter" style={cssStagger(i)} aria-pressed={s.id === current.id} onClick={() => setSel(s.id)}>
                {s.id === current.id && <SelectRing group="set-ring" />}
                <span className="row-between" style={{ width: '100%', flexWrap: 'nowrap' }}>
                  <span style={{ fontWeight: 600, fontSize: 15 }}>{s.name}</span>
                  <Badge tone={s.on ? 'accent' : 'neutral'}>{s.on ? t('On') : t('Off')}</Badge>
                </span>
                <span className="faint" style={{ fontSize: 12 }}>{meta(s)}</span>
                <span aria-hidden="true" style={{ display: 'flex', flexWrap: 'wrap', gap: 10, fontSize: 20 }}>
                  {s.familyKeys.map((k) => <span key={k} style={familyStyle(byKey.get(k))}>Ag</span>)}
                </span>
              </button>
            ))}
          </div>

          <section aria-label={t('Selected project set')} className="card" style={{ flex: '2 1 420px', minWidth: 0, gap: 16, padding: 20 }}>
            <div className="row-between">
              <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
                <h2 style={{ margin: 0, fontSize: 18, fontWeight: 600 }}>{current.name}</h2>
                <span className="muted" style={{ fontSize: 13 }}>{meta(current)}</span>
              </div>
              <div style={{ display: 'flex', gap: 8 }}>
                <button type="button" className={current.on ? 'btn' : 'btn primary'} style={{ fontWeight: 500 }} onClick={() => toggle(current)}>
                  {current.on ? t('Turn all off') : t('Turn all on')}
                </button>
                <button type="button" className="btn" onClick={() => openDraft({ id: current.id, name: current.name, note: current.note, familyKeys: current.familyKeys })}>{t('Edit')}</button>
              </div>
            </div>
            {/* Keyed by set so switching sets re-staggers; a set turning on/off ripples down the status labels. */}
            <div key={current.id} style={{ display: 'flex', flexDirection: 'column' }}>
              {current.familyKeys.map((k, i) => {
                const f = byKey.get(k)
                const on = f ? f.activeState !== 'none' : false
                const label = f ? (on ? t('Active') : t('Inactive')) : t('Missing')
                return (
                  <div key={k} className="list-row enter" style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 14, padding: '12px 0', ...cssStagger(i) }}>
                    <Link to={`/family/${encodeURIComponent(k)}`} className="plain-link" style={{ flex: '1 1 220px', fontSize: 24, lineHeight: 1.2, ...familyStyle(f) }}>{f?.family ?? k}</Link>
                    {f && <LicenseBadge license={f.license} />}
                    <span style={{ width: 70, fontSize: 12, display: 'grid' }}>
                      <AnimatePresence initial={false}>
                        <motion.span
                          key={label} style={{ gridArea: '1 / 1', color: on ? 'var(--ok)' : 'var(--faint)' }}
                          initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -6 }}
                          transition={{ duration: dur.base, ease: easeOut, delay: i * 0.05 }}
                        >
                          {label}
                        </motion.span>
                      </AnimatePresence>
                    </span>
                  </div>
                )
              })}
            </div>
            <span className="faint" style={{ fontSize: 12 }}>
              {app.serverConfigured ? t('Turning a set off only turns off fonts that no other active set or admin rule still needs.') : t('Turning a set off only turns off fonts that no other active set still needs.')}
            </span>
          </section>
        </div>
      )}

      <Dialog open={!!draft} onClose={() => setDraft(null)} title={draft?.id != null ? t('Edit project set') : t('New project set')} wide>
        {draft && (
          <form onSubmit={save} style={{ display: 'flex', flexDirection: 'column', gap: 14 }} noValidate>
            <label className="field-label">
              {t('Name')}
              <input className="input" value={draft.name} autoFocus onChange={(e) => (setDraft({ ...draft, name: e.target.value }), setDraftError(null))} />
            </label>
            <label className="field-label">
              {t('Note')} <span className="faint">{t('(client, job number…)')}</span>
              <input className="input" value={draft.note} onChange={(e) => setDraft({ ...draft, note: e.target.value })} />
            </label>
            <fieldset className="fieldset" style={{ padding: 12, gap: 8 }}>
              <legend>{t('Fonts · {n} chosen', { n: draft.familyKeys.length })}</legend>
              <label htmlFor="set-filter" className="sr-only">{t('Filter fonts')}</label>
              <input id="set-filter" type="search" className="input" placeholder={t('Search fonts to add')} value={pick} onChange={(e) => setPick(e.target.value)} />
              <div className="check-list">
                {hits.map((f) => (
                  <label key={f.key} className="check-row">
                    <input
                      type="checkbox" checked={draft.familyKeys.includes(f.key)}
                      onChange={(e) => setDraft({ ...draft, familyKeys: e.target.checked ? [...draft.familyKeys, f.key] : draft.familyKeys.filter((k) => k !== f.key) })}
                    />
                    <span style={{ flex: 1, fontSize: 18, ...familyStyle(f) }}>{f.family}</span>
                    <LicenseBadge license={f.license} />
                  </label>
                ))}
              </div>
            </fieldset>
            {draftError && <p role="alert" style={{ margin: 0, color: 'var(--bad)', fontSize: 13 }}>{draftError}</p>}
            <div className="dialog-actions">
              {draft.id != null && <button type="button" className="btn danger" style={{ marginRight: 'auto' }} onClick={remove}>{t('Delete set')}</button>}
              <button type="button" className="btn" onClick={() => setDraft(null)}>{t('Cancel')}</button>
              <button type="submit" className="btn primary">{t('Save')}</button>
            </div>
          </form>
        )}
      </Dialog>
    </div>
  )
}
