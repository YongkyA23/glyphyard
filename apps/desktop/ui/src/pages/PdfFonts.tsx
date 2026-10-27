import { useEffect, useState } from 'react'
import { Link } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { api, inTauri, type Face, type PdfFont } from '../api'
import { faceStyle } from '../fonts'
import { cssStagger, dur, easeOut, rise } from '../motion'
import { useApp } from '../state'
import { Badge, basename, errMsg, plural, Spinner, type Tone } from '../ui'

const STATUS: Record<PdfFont['status'], [string, Tone]> = {
  active: ['Active', 'ok'], inactive: ['In library · off', 'accent'], version: ['Different version', 'warn'], missing: ['Not in library', 'bad'],
}

export function PdfFonts() {
  const { activate, toast } = useApp()
  const [path, setPath] = useState<string | null>(null)
  const [rows, setRows] = useState<PdfFont[] | null>(null)
  const [faces, setFaces] = useState<Map<string, Face>>(new Map())
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [dragging, setDragging] = useState(false)

  async function open(p: string) {
    setPath(p)
    setBusy(true)
    setError(null)
    try {
      const res = await api.pdfFonts(p)
      setRows(res)
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
    const p = await api.pickFile([{ name: 'PDF', extensions: ['pdf'] }]).catch((e) => (toast(errMsg(e)), null))
    if (p) open(p)
  }

  // Native drag and drop gives real file paths inside Tauri.
  useEffect(() => {
    if (!inTauri) return
    const un = getCurrentWebview().onDragDropEvent((e) => {
      const t = e.payload.type
      if (t === 'over' || t === 'enter') setDragging(true)
      else if (t === 'leave') setDragging(false)
      else if (t === 'drop') {
        setDragging(false)
        const pdf = e.payload.paths.find((p) => p.toLowerCase().endsWith('.pdf'))
        if (pdf) open(pdf)
        else toast('Drop a .pdf file.')
      }
    })
    return () => void un.then((f) => f())
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  async function turnOn(targets: PdfFont[]) {
    const hashes = targets.map((r) => r.hash!).filter(Boolean)
    if (!hashes.length) return
    const res = await activate((confirm) => api.setActive(hashes, true, confirm))
    if (!res) return
    const ok = new Set(res.filter((r) => r.ok).map((r) => r.hash))
    setRows((rs) => rs?.map((r) => (r.hash && ok.has(r.hash) ? { ...r, status: 'active' } : r)) ?? null)
  }

  const canActivate = (r: PdfFont) => (r.status === 'inactive' || r.status === 'version') && !!r.hash
  const pending = rows?.filter(canActivate) ?? []
  const n = (s: PdfFont['status']) => rows?.filter((r) => r.status === s).length ?? 0

  return (
    <div className="page" style={{ gap: 20 }}>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
        <h1 className="h-page">Fonts in a PDF</h1>
        <span className="muted">Find out which fonts a file uses and switch on the ones you have.</span>
      </div>
      <button type="button" className={dragging ? 'dropzone over' : 'dropzone'} onClick={choose} disabled={busy}>
        <span style={{ fontWeight: 500, fontSize: 15, display: 'inline-flex', alignItems: 'center', gap: 8 }}>
          {busy && <Spinner />}{busy ? 'Reading the PDF…' : 'Drop a PDF here, or choose a file'}
        </span>
        <span className="faint" style={{ fontSize: 13 }}>Coming later: .psd, .ai, .indd</span>
      </button>
      {error && <p role="alert" style={{ margin: 0, color: 'var(--bad)' }}>{error}</p>}

      {rows && path && (
        <motion.section key={path} aria-label="Results" style={{ display: 'flex', flexDirection: 'column', gap: 12 }} {...rise} transition={{ duration: dur.slow, ease: easeOut }}>
          <div className="row-between">
            <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
              <h2 style={{ margin: 0, fontSize: 16, fontWeight: 600 }}>{basename(path)}</h2>
              <span className="muted" style={{ fontSize: 13 }}>
                {plural(rows.length, 'font')} found · {n('active')} already active · {pending.length} can be turned on · {n('missing')} missing
              </span>
            </div>
            <button type="button" className="btn primary" onClick={() => turnOn(pending)} disabled={!pending.length}>
              {pending.length ? `Turn on ${pending.length} available ${pending.length === 1 ? 'font' : 'fonts'}` : 'All available fonts are on'}
            </button>
          </div>
          <div className="card" style={{ padding: 0, gap: 0 }}>
            {rows.map((r, i) => (
              <div key={r.psName} className="pdf-row enter" style={cssStagger(i)}>
                <span style={{ flex: '1 1 220px', display: 'flex', flexDirection: 'column', gap: 2 }}>
                  <span style={{ fontSize: 20, ...(r.hash ? faceStyle(faces.get(r.hash)) : {}) }}>{r.display}</span>
                  <span className="faint" style={{ fontSize: 12 }}>{r.detail}</span>
                </span>
                <AnimatePresence mode="popLayout" initial={false}>
                  <motion.span key={r.status} initial={{ opacity: 0, scale: 0.8 }} animate={{ opacity: 1, scale: 1 }} exit={{ opacity: 0, scale: 0.8 }} transition={{ duration: dur.base, ease: easeOut }}>
                    <Badge tone={STATUS[r.status][1]}>{STATUS[r.status][0]}</Badge>
                  </motion.span>
                </AnimatePresence>
                <span style={{ width: 170, display: 'flex', justifyContent: 'flex-end' }}>
                  {canActivate(r) && <button type="button" className="btn small outline-accent" onClick={() => turnOn([r])}>Turn on</button>}
                  {r.status === 'missing' && r.google && (
                    <Link to={`/google?q=${encodeURIComponent(r.display.split(' ').slice(0, -1).join(' ') || r.display)}`} style={{ fontSize: 13 }}>
                      Search Google Fonts
                    </Link>
                  )}
                </span>
              </div>
            ))}
          </div>
          <p className="faint" style={{ margin: 0, fontSize: 12, lineHeight: 1.5 }}>
            Fonts are matched by their internal name. When your library has a different version than the PDF used, both versions are shown so you can pick.
          </p>
        </motion.section>
      )}
    </div>
  )
}
