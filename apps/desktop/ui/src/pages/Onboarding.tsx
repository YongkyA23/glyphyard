import { useState, type FormEvent } from 'react'
import { useNavigate } from 'react-router'
import { motion } from 'motion/react'
import { api } from '../api'
import { cssStagger, shake, spring } from '../motion'
import { useApp } from '../state'
import { errMsg } from '../ui'

/** The mark's letters rise in one after another; the italic g swings in last. */
const MARK: [string, string | undefined][] = [['A', undefined], ['a', undefined], ['g', 'g']]

export function Onboarding() {
  const { app, setApp } = useApp()
  const [name, setName] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const navigate = useNavigate()
  const conn = app.connection

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!name.trim()) return setError('Enter your name to continue.')
    setBusy(true)
    try {
      const next = await api.completeOnboarding(name.trim())
      setApp(() => next)
      navigate('/', { replace: true })
    } catch (err) {
      setError(errMsg(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="onboarding">
      <form className="onboarding-card" onSubmit={submit} noValidate>
        <div aria-hidden="true" className="onboarding-mark">
          {MARK.map(([ch, cls], i) => (
            <motion.span
              key={ch} className={cls}
              initial={{ opacity: 0, y: 28, rotate: cls ? -12 : 0, filter: 'blur(6px)' }} animate={{ opacity: 1, y: 0, rotate: 0, filter: 'blur(0px)' }}
              transition={{ ...spring.panel, delay: 0.08 + i * 0.09, filter: { duration: 0.3, delay: 0.08 + i * 0.09 } }}
            >
              {ch}
            </motion.span>
          ))}
        </div>
        <div className="enter" style={{ display: 'flex', flexDirection: 'column', gap: 10, ...cssStagger(3) }}>
          <h1>Welcome. What should we call you?</h1>
          <p className="muted" style={{ margin: 0, lineHeight: 1.55 }}>
            Your name is shown to your admin next to the fonts you use. That’s the only thing we need to get started.
          </p>
        </div>
        <div className="enter" style={{ display: 'flex', flexDirection: 'column', gap: 8, ...cssStagger(4) }}>
          <label htmlFor="name" style={{ fontWeight: 500 }}>Your name</label>
          <input
            id="name" type="text" className="input big" value={name} placeholder="e.g. Budi Santoso" autoComplete="name" autoFocus
            aria-invalid={!!error} aria-describedby={error ? 'name-error' : undefined}
            onChange={(e) => (setName(e.target.value), setError(null))}
          />
          {error && (
            <motion.span key={error} id="name-error" role="alert" style={{ color: 'var(--bad)', fontSize: 13 }} initial={{ opacity: 0 }} animate={{ opacity: 1, ...shake }}>
              {error}
            </motion.span>
          )}
          <span className="faint" style={{ fontSize: 13, lineHeight: 1.5 }}>
            You can change it later in Settings, once every {app.nameChangeDays} days.
          </span>
        </div>
        <button type="submit" className="btn primary big enter" style={cssStagger(5)} disabled={busy}>{busy ? 'Connecting…' : 'Continue'}</button>
        <div className="faint enter" style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 12, ...cssStagger(6) }}>
          <span aria-hidden="true" className="dot" style={{ width: 8, height: 8, background: conn.status === 'connected' ? 'var(--ok-dot)' : 'var(--amber)' }} />
          <span>{conn.status === 'connected' ? `Connected to ${conn.serverUrl}` : `Not connected to ${conn.serverUrl}. You need a connection to finish setting up.`}</span>
        </div>
      </form>
    </div>
  )
}
