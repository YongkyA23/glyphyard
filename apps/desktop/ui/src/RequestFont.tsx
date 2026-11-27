// "Ask for a font": a button that opens a dialog sending a font request to the admin.
import { useState } from 'react'
import { api } from './api'
import { Hint } from './Hint'
import { t } from './i18n'
import { useApp } from './state'
import { Dialog, errMsg } from './ui'

// Same limits as the engine (typeshelf_protocol::FONT_REQUEST_*_MAX).
const NAME_MAX = 200
const NOTE_MAX = 1000

/** `name` prefills the font name, e.g. a font missing from a document. */
export function RequestFontButton({ name = '', className = 'btn small' }: { name?: string; className?: string }) {
  const [open, setOpen] = useState(false)
  return (
    <>
      <button type="button" className={className} onClick={() => setOpen(true)} aria-label={name ? t('Ask your admin for {name}', { name }) : undefined}>
        {t('Ask for a font')}
      </button>
      <Dialog open={open} onClose={() => setOpen(false)} title={t('Ask your admin for a font')}>
        {open && <RequestForm initial={name} onDone={() => setOpen(false)} />}
      </Dialog>
    </>
  )
}

function RequestForm({ initial, onDone }: { initial: string; onDone: () => void }) {
  const { app, toast } = useApp()
  const [name, setName] = useState(initial)
  const [note, setNote] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const online = app.connection.status === 'connected'

  async function submit() {
    setBusy(true)
    setError(null)
    try {
      await api.requestFont(name.trim(), note.trim() || null)
      toast(online ? t('Request for {name} queued for your admin.', { name: name.trim() }) : t('Request for {name} queued: it will be sent when this computer is next connected.', { name: name.trim() }))
      onDone()
    } catch (e) {
      setError(errMsg(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <form style={{ display: 'contents' }} onSubmit={(e) => (e.preventDefault(), name.trim() && submit())}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
        <span className="muted" style={{ fontSize: 12 }}>{t('Who gets this?')}</span>
        <Hint text={t('Your request goes to your Glyphyard admin, who can add the font to the shared library for you.')} />
      </div>
      {!online && <p className="muted" style={{ margin: 0 }}>{t('You’re offline right now: it will be sent when this computer is next connected.')}</p>}
      <label className="field-label">
        {t('Font name')}
        <input className="input" value={name} maxLength={NAME_MAX} onChange={(e) => setName(e.target.value)} autoFocus placeholder={t('For example Söhne Buch')} />
      </label>
      <label className="field-label">
        {t('Note (optional)')}
        <textarea className="input" rows={3} value={note} maxLength={NOTE_MAX} onChange={(e) => setNote(e.target.value)} placeholder={t('What you need it for, a link to the foundry…')} />
      </label>
      {error && <p role="alert" style={{ margin: 0, color: 'var(--bad)' }}>{error}</p>}
      <div className="dialog-actions">
        <button type="button" className="btn" onClick={onDone}>{t('Cancel')}</button>
        <button type="submit" className="btn primary" disabled={!name.trim() || busy}>{t('Send request')}</button>
      </div>
    </form>
  )
}
