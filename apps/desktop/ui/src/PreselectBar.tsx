// The Library opened from Stats › Never used shows only those families (`location.state.select`); this bar says
// so and leads back to the whole library, since that state survives a reload and Back / Forward.
import { useNavigate } from 'react-router'
import { plural } from './ui'

export function PreselectBar({ count }: { count: number }) {
  const navigate = useNavigate()
  return (
    <div role="status" className="banner" style={{ margin: '4px 24px 8px' }}>
      <span style={{ flex: '1 1 200px', color: 'var(--banner-ink)' }}>Showing {plural(count, 'family', 'families')} from Stats.</span>
      <button type="button" className="btn small" onClick={() => navigate('/', { replace: true })}>Show all</button>
    </div>
  )
}
