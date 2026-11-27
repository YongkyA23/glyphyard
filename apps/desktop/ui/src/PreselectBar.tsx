// The Library opened from Stats › Never used shows only those families (`location.state.select`); this bar says
// so and leads back to the whole library, since that state survives a reload and Back / Forward.
import { useNavigate } from 'react-router'
import { t } from './i18n'
import { plural } from './ui'

export function PreselectBar({ count }: { count: number }) {
  const navigate = useNavigate()
  return (
    <div role="status" className="banner" style={{ margin: '4px 24px 8px' }}>
      <span style={{ flex: '1 1 200px', color: 'var(--banner-ink)' }}>{t('Showing {what} from Stats.', { what: plural(count, 'family', 'families') })}</span>
      <button type="button" className="btn small" onClick={() => navigate('/', { replace: true })}>{t('Show all')}</button>
    </div>
  )
}
