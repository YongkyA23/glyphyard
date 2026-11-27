import { useEffect, useState } from 'react'
import { Link } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { api, inTauri, type Face, type PdfFont } from '../api'
import { faceStyle } from '../fonts'
import { Hint } from '../Hint'
import { t, tk } from '../i18n'
import { cssStagger, dur, easeOut, rise } from '../motion'
import { useApp } from '../state'
import { RequestFontButton } from '../RequestFont'
import { Badge, basename, errMsg, plural, Spinner, type Tone } from '../ui'

const DOC_TYPES = ['pdf', 'psd', 'ai', 'indd']
const STATUS: Record<PdfFont['status'], [string, Tone]> = {
  active: [tk('Active'), 'ok'], inactive: [tk('In library · off'), 'accent'], version: [tk('Different version'), 'warn'], missing: [tk('Not in library'), 'bad'],
}

export function PdfFonts() {
  const { app, activate, toast } = useApp()
  const [path, setPath] = useState<string | null>(null)
  const [rows, setRows] = useState<PdfFont[] | null>(null)
  const [partial, setPartial] = useState<string | null>(null)
  const [faces, setFaces] = useState<Map<string, Face>>(new Map())
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [dragging, setDragging] = useState(false)

  async function open(p: string) {
    setPath(p)
    setBusy(true)
    setError(null)
    try {
      const { fonts: res, partial } = await api.documentFonts(p)
      setRows(res)
      setPartial(partial)
      if (res.some((r) => r.hash)) {
        const faces = await api.getFaces(res.flatMap((r) => (r.hash ? [r.hash] : [])))
        setFaces(new Map(faces.map((f) => [f.hash, f])))
      }
    } catch (e) {
      setRows(null)
      setError(errMsg(e))
    } finally {
      setBusy(false)
    }
  }
  const choose = async () => {
    const p = await api.pickFile([{ name: t('Documents'), extensions: DOC_TYPES }]).catch((e) => (toast(errMsg(e)), null))
    if (p) open(p)
  }

  // Native drag and drop gives real file paths inside Tauri.
  useEffect(() => {
    if (!inTauri) return
    const un = getCurrentWebview().onDragDropEvent((e) => {
      const kind = e.payload.type
      if (kind === 'over' || kind === 'enter') setDragging(true)
      else if (kind === 'leave') setDragging(false)
      else if (kind === 'drop') {
        setDragging(false)
        const doc = e.payload.paths.find((p) => DOC_TYPES.some((ext) => p.toLowerCase().endsWith('.' + ext)))
        if (doc) open(doc)
        else toast(t('Drop a .pdf, .psd, .ai or .indd file.'))
      }
    })
    return () => void un.then((f) => f())
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  async function turnOn(targets: PdfFont[]) {
    const hashes = targets.map((r) => r.hash!).filter(Boolean)
    if (!hashes.length) return
    const res = await activate(() => api.setActive(hashes, true))
    if (!res) return
    const ok = new Set(res.filter((r) => r.ok).map((r) => r.hash))
    setRows((rs) => rs?.map((r) => (r.hash && ok.has(r.hash) ? { ...r, status: 'active' } : r)) ?? null)
  }

  const canActivate = (r: PdfFont) => (r.status === 'inactive' || r.status === 'version') && !!r.hash
  const pending = rows?.filter(canActivate) ?? []
  const n = (s: PdfFont['status']) => rows?.filter((r) => r.status === s).length ?? 0

  return (
    <div className="page" style={{ gap: 20 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
        <h1 className="h-page">{t('Fonts in a document')}</h1>
        <Hint text={t('Find out which fonts a PDF, Photoshop, Illustrator or InDesign file uses and switch on the ones you have.')} />
      </div>
      <button type="button" className={dragging ? 'dropzone over' : 'dropzone'} onClick={choose} disabled={busy}>
        <span style={{ fontWeight: 500, fontSize: 15, display: 'inline-flex', alignItems: 'center', gap: 8 }}>
          {busy && <Spinner />}{busy ? t('Reading the file…') : t('Drop a file here, or choose one')}
        </span>
        <span className="faint" style={{ fontSize: 13 }}>{t('PDF, .psd, .ai and .indd files')}</span>
      </button>
      {error && <p role="alert" style={{ margin: 0, color: 'var(--bad)' }}>{error}</p>}

      {rows && path && (
        <motion.section key={path} aria-label={t('Results')} style={{ display: 'flex', flexDirection: 'column', gap: 12 }} {...rise} transition={{ duration: dur.slow, ease: easeOut }}>
          <div className="row-between">
            <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
              <h2 style={{ margin: 0, fontSize: 16, fontWeight: 600 }}>{basename(path)}</h2>
              <span className="muted" style={{ fontSize: 13 }}>
                {t('{fonts} found · {active} already active · {pending} can be turned on · {missing} missing', { fonts: plural(rows.length, 'font'), active: n('active'), pending: pending.length, missing: n('missing') })}
              </span>
            </div>
            <button type="button" className="btn primary" onClick={() => turnOn(pending)} disabled={!pending.length}>
              {pending.length === 0 ? t('All available fonts are on') : pending.length === 1 ? t('Turn on 1 available font') : t('Turn on {n} available fonts', { n: pending.length })}
            </button>
          </div>
          {partial && <p role="status" className="muted" style={{ margin: 0 }}>{partial} {t('The list below may be incomplete.')}</p>}
          {!rows.length && <p className="muted" style={{ margin: 0 }}>{partial ? t('No fonts were found in the part that was read.') : t('This file doesn’t list its fonts.')}</p>}
          <div className="card" style={{ padding: 0, gap: 0 }}>
            {rows.map((r, i) => (
              <div key={r.psName} className="pdf-row enter" style={cssStagger(i)}>
                <span style={{ flex: '1 1 220px', display: 'flex', flexDirection: 'column', gap: 2 }}>
                  <span style={{ fontSize: 20, ...(r.hash ? faceStyle(faces.get(r.hash)) : {}) }}>{r.display}</span>
                  <span className="faint" style={{ fontSize: 12 }}>{r.detail}</span>
                </span>
                <AnimatePresence mode="popLayout" initial={false}>
                  <motion.span key={r.status} initial={{ opacity: 0, scale: 0.8 }} animate={{ opacity: 1, scale: 1 }} exit={{ opacity: 0, scale: 0.8 }} transition={{ duration: dur.base, ease: easeOut }}>
                    <Badge tone={STATUS[r.status][1]}>{t(STATUS[r.status][0] /* i18n-dynamic */)}</Badge>
                  </motion.span>
                </AnimatePresence>
                <span style={{ width: 170, display: 'flex', justifyContent: 'flex-end' }}>
                  {canActivate(r) && <button type="button" className="btn small outline-accent" onClick={() => turnOn([r])}>{t('Turn on')}</button>}
                  {r.status === 'missing' && r.google && (
                    <Link to={`/google?q=${encodeURIComponent(r.display.split(' ').slice(0, -1).join(' ') || r.display)}`} style={{ fontSize: 13 }}>
                      {t('Search Google Fonts')}
                    </Link>
                  )}
                  {r.status === 'missing' && app.serverConfigured && <RequestFontButton name={r.display} />}
                </span>
              </div>
            ))}
          </div>
          <span className="faint" style={{ fontSize: 12 }}>
            {t('How fonts are matched')} <Hint text={t('Fonts are matched by their internal name. Only PDFs record which version of a font they used, so “Different version” appears for PDFs only.')} />
          </span>
        </motion.section>
      )}
    </div>
  )
}
