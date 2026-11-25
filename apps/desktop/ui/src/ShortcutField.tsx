// Settings row control for the quick-activate shortcut: "Change", press the new combination, it is checked and saved.
import { useState, type KeyboardEvent } from 'react'
import { shortcutFromKey } from './shortcut'
import { errMsg, formatShortcut } from './ui'

export function ShortcutField({ value, platform, onSave }: { value: string; platform: 'windows' | 'macos'; onSave: (s: string) => Promise<void> }) {
  const [recording, setRecording] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const stop = () => setRecording(false)
  async function onKey(e: KeyboardEvent) {
    if (!recording || busy) return
    // Plain Tab still moves focus (and blur cancels); plain Escape cancels.
    if ((e.key === 'Tab' || e.key === 'Escape') && !e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey) {
      if (e.key === 'Escape') {
        e.preventDefault()
        setError(null)
        stop()
      }
      return
    }
    e.preventDefault()
    const r = shortcutFromKey(e.nativeEvent, platform)
    if (!r) return
    if ('error' in r) return setError(r.error)
    setError(null)
    if (r.shortcut === value) return stop()
    setBusy(true)
    try {
      await onSave(r.shortcut)
      stop()
    } catch (err) {
      setError(`${errMsg(err)} Your shortcut is still ${formatShortcut(value, platform)}.`)
    } finally {
      setBusy(false)
    }
  }

  return (
    <span style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-end', gap: 6, maxWidth: 360 }}>
      <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <kbd className="kbd">{formatShortcut(value, platform)}</kbd>
        <button
          type="button" className={recording ? 'btn small outline-accent' : 'btn small'} aria-label={recording || busy ? undefined : 'Change quick activate shortcut'} aria-describedby={error ? 'shortcut-error' : undefined}
          onClick={() => (setError(null), setRecording(!recording))} onKeyDown={onKey} onBlur={() => !busy && stop()}
        >
          {busy ? 'Saving…' : recording ? 'Press the new shortcut…' : 'Change'}
        </button>
      </span>
      {recording && !error && <span className="faint" style={{ fontSize: 12 }}>Hold two modifier keys and press a key. Esc cancels.</span>}
      {error && <span id="shortcut-error" role="alert" style={{ fontSize: 12, color: 'var(--bad)', textAlign: 'right' }}>{error}</span>}
    </span>
  )
}
