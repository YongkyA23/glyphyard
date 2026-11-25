// Printable specimen of a collection: every family's name and sample in its own font. Opens the print dialog
// (which also offers Save as PDF) once the fonts have loaded.
import { useEffect, useState } from 'react'
import { Link, useParams } from 'react-router'
import { api, type Family } from '../api'
import { familyStyle } from '../fonts'
import { cssStagger } from '../motion'
import { paintTheme } from '../theme'
import { CATEGORY_LABEL, errMsg, fmtDate, plural, Skel } from '../ui'

const ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz 0123456789 &?!'

async function printWhenReady(cancelled: () => boolean) {
  // Let the previews render and their font files start loading, then wait for them.
  await new Promise((r) => setTimeout(r, 500))
  await document.fonts.ready
  if (!cancelled()) window.print()
}

export function PrintCollection() {
  const { id = '' } = useParams()
  const [name, setName] = useState('Collection')
  const [fams, setFams] = useState<Family[] | null>(null)
  const [sample, setSample] = useState('Sphinx of black quartz, judge my vow')
  const [error, setError] = useState<string | null>(null)
  // A specimen is on paper: light while this page is open, the chosen theme again after.
  useEffect(() => {
    paintTheme()
    return paintTheme
  }, [])

  useEffect(() => {
    Promise.all([api.listCollections(), api.listFamilies({ collectionId: Number(id), sort: 'name', includeHidden: true }), api.getSettings()]).then(
      ([cols, list, settings]) => {
        setName(cols.find((c) => String(c.id) === id)?.name ?? 'Collection')
        if (settings.sampleText) setSample(settings.sampleText)
        setFams(list)
      },
      (e) => setError(errMsg(e)),
    )
  }, [id])
  useEffect(() => {
    if (!fams?.length) return
    let gone = false
    printWhenReady(() => gone)
    return () => void (gone = true)
  }, [fams])

  return (
    <div className="print-page">
      <div className="print-bar no-print">
        <Link to={`/?collection=${id}`} className="back">← Back to the library</Link>
        <span className="faint" style={{ fontSize: 13, flex: 1 }}>To get a PDF, pick “Save as PDF” (or “Microsoft Print to PDF”) as the printer.</span>
        <button type="button" className="btn primary" onClick={() => window.print()} disabled={!fams?.length}>Print…</button>
      </div>
      <header className="print-head">
        <h1 style={{ fontSize: 32, fontWeight: 600, letterSpacing: -0.4 }}>{name}</h1>
        <span className="muted">{fams ? plural(fams.length, 'font family', 'font families') : '…'} · {fmtDate(new Date().toISOString())}</span>
      </header>
      {error && <p role="alert">{error}</p>}
      {!fams && !error && [0, 1, 2].map((i) => <div key={i} className="print-item"><Skel w="30%" h={14} /><Skel w="80%" h={36} /></div>)}
      {fams?.length === 0 && <p className="muted">This collection is empty.</p>}
      {fams?.map((f, i) => (
        <section key={f.key} className="print-item enter" style={cssStagger(i)}>
          <div className="row-between" style={{ alignItems: 'baseline' }}>
            <strong>{f.family}</strong>
            <span className="faint" style={{ fontSize: 12 }}>
              {plural(f.styles, 'style')} · {f.foundry}{f.category && ` · ${CATEGORY_LABEL[f.category]}`}
            </span>
          </div>
          <div style={{ ...familyStyle(f), fontSize: 40, lineHeight: 1.15, overflowWrap: 'anywhere' }}>{sample}</div>
          <div style={{ ...familyStyle(f), fontSize: 15, color: 'var(--ink-2)', overflowWrap: 'anywhere' }}>{ALPHABET}</div>
        </section>
      ))}
    </div>
  )
}
