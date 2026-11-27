import { useState, type FormEvent, type MouseEvent } from 'react'
import { useNavigate } from 'react-router'
import { motion } from 'motion/react'
import { api } from '../api'
import { cssStagger, shake, spring } from '../motion'
import { useApp } from '../state'
import { errMsg } from '../ui'
import { t } from '../i18n'
import { useTour } from '../tour/Tour'
import { isDone } from '../tour/progress'

export function Onboarding() {
  const { app, setApp } = useApp()
  const [name, setName] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const navigate = useNavigate()
  const { start } = useTour()
  const conn = app.connection
  const local = !app.serverConfigured
  // A server is set up but not answering (or refused us): the name can be kept here and sent later.
  const canGoOffline = !local && conn.status !== 'connected'

  async function submit(e: FormEvent | MouseEvent, offline = false) {
    e.preventDefault()
    if (!name.trim()) return setError(t('Enter your name to continue.'))
    setBusy(true)
    try {
      const next = await api.completeOnboarding(name.trim(), offline)
      setApp(() => next)
      navigate('/', { replace: true })
      // The welcome tour, once: after the library has mounted.
      if (!isDone('welcome')) setTimeout(() => start('welcome'), 400)
    } catch (err) {
      setError(errMsg(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="onboarding">
      <form className="onboarding-card" onSubmit={submit} noValidate>
        <motion.span
          aria-hidden="true" className="gy-mark onboarding-mark"
          initial={{ opacity: 0, y: 28, filter: 'blur(6px)' }} animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
          transition={{ ...spring.panel, delay: 0.08, filter: { duration: 0.3, delay: 0.08 } }}
        />
        <div className="enter" style={{ display: 'flex', flexDirection: 'column', gap: 10, ...cssStagger(3) }}>
          <h1>{t('Welcome. What should we call you?')}</h1>
          <p className="muted" style={{ margin: 0, lineHeight: 1.55 }}>
            {local ? t('Your name stays on this computer.') : t('Your name is shown to your admin next to the fonts you use.')} {t('That’s the only thing we need to get started.')}
          </p>
        </div>
        <div className="enter" style={{ display: 'flex', flexDirection: 'column', gap: 8, ...cssStagger(4) }}>
          <label htmlFor="name" style={{ fontWeight: 500 }}>{t('Your name')}</label>
          <input
            id="name" type="text" className="input big" value={name} placeholder={t('e.g. Budi Santoso')} autoComplete="name" autoFocus
            aria-invalid={!!error} aria-describedby={error ? 'name-error' : undefined}
            onChange={(e) => (setName(e.target.value), setError(null))}
          />
          {error && (
            <motion.span key={error} id="name-error" role="alert" style={{ color: 'var(--bad)', fontSize: 13 }} initial={{ opacity: 0 }} animate={{ opacity: 1, ...shake }}>
              {error}
            </motion.span>
          )}
          <span className="faint" style={{ fontSize: 13, lineHeight: 1.5 }}>
            {local ? t('You can change it later in Settings.') : t('You can change it later in Settings, once every {n} days.', { n: app.nameChangeDays })}
          </span>
        </div>
        <button type="submit" className="btn primary big enter" style={cssStagger(5)} disabled={busy}>{busy ? (local ? t('Saving…') : t('Connecting…')) : t('Continue')}</button>
        {canGoOffline && (
          <button type="button" className="btn big enter" style={cssStagger(5)} disabled={busy} onClick={(e) => submit(e, true)}>{t('Continue offline')}</button>
        )}
        <div className="faint enter" style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 12, ...cssStagger(6) }}>
          <span aria-hidden="true" className="dot" style={{ width: 8, height: 8, background: local || conn.status === 'connected' ? 'var(--ok-dot)' : 'var(--amber)' }} />
          <span aria-live="polite">
            {local
              ? t('Local library: no server is set up. You can add one later in Settings › Connection.')
              : conn.status === 'connected'
                ? t('Connected to {url}', { url: conn.serverUrl })
                : conn.status === 'error' && conn.error
                  ? t(conn.error) // i18n-dynamic: engine text
                  : t('Can’t reach {url}. Continue offline and Glyphyard connects by itself when the server answers.', { url: conn.serverUrl })}
          </span>
        </div>
      </form>
    </div>
  )
}
