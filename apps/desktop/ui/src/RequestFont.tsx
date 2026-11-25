// "Ask for a font": a button that opens a dialog sending a font request to the admin.
import { useState } from 'react'
import { api } from './api'
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
      <button type="button" className={className} onClick={() => setOpen(true)} aria-label={name ? `Ask your admin for ${name}` : undefined}>
        Ask for a font
      </button>
      <Dialog open={open} onClose={() => setOpen(false)} title="Ask your admin for a font">
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
      toast(online ? `Asked your admin for ${name.trim()}.` : `Your request for ${name.trim()} will be sent when this computer is next connected.`)
      onDone()
    } catch (e) {
      setError(errMsg(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <form style={{ display: 'contents' }} onSubmit={(e) => (e.preventDefault(), name.trim() && submit())}>
      <p className="muted" style={{ margin: 0 }}>
        Your request goes to your Typeshelf admin, who can add the font to the shared library for you.
        {!online && ' You’re offline right now: it will be sent when this computer is next connected.'}
      </p>
      <label className="field-label">
        Font name
        <input className="input" value={name} maxLength={NAME_MAX} onChange={(e) => setName(e.target.value)} autoFocus placeholder="For example Söhne Buch" />
      </label>
      <label className="field-label">
        Note (optional)
        <textarea className="input" rows={3} value={note} maxLength={NOTE_MAX} onChange={(e) => setNote(e.target.value)} placeholder="What you need it for, a link to the foundry…" />
      </label>
      {error && <p role="alert" style={{ margin: 0, color: 'var(--bad)' }}>{error}</p>}
      <div className="dialog-actions">
        <button type="button" className="btn" onClick={onDone}>Cancel</button>
        <button type="submit" className="btn primary" disabled={!name.trim() || busy}>Send request</button>
      </div>
    </form>
  )
}
