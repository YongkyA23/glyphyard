// Top-of-window banner when an update is downloaded and waiting for a restart.
import { useEffect, useState } from 'react'
import { api, on } from './api'
import { t } from './i18n'
import { useApp } from './state'
import { errMsg } from './ui'

export function UpdateBanner() {
  const [v, setV] = useState<string | null>(null)
  const [hidden, setHidden] = useState(false)
  const { toast } = useApp()
  useEffect(() => {
    api.getSettings().then((s) => s.updateReady && setV(s.updateReady), () => {}) // fire-and-forget; startup never waits on this
    return on('update-ready', (e) => (setV(e.version), setHidden(false)))
  }, [])
  if (!v || hidden) return null
  return (
    <div role="status" className="update-banner">
      <span>{t('Glyphyard {v} is ready to install', { v })}</span>
      <button type="button" className="btn small primary" onClick={() => api.restartToUpdate().catch((e) => toast(errMsg(e)))}>{t('Restart now')}</button>
      <button type="button" className="btn small" aria-label={t('Dismiss')} onClick={() => setHidden(true)}>✕</button>
    </div>
  )
}
