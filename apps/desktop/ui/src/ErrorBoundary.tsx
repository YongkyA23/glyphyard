// A render error in one page shows a plain message instead of a blank window; the sidebar stays usable.
// `quiet` renders nothing instead (the easter-egg layer must never take the app down with it).
// `resetKey` (the location key) clears the error on navigation, so a crash in one Library scope doesn't stick to the next.
// `sidebar`: the window has a sidebar to go elsewhere (not the quick window or the last-resort boundary).
import { t } from './i18n'
import { Component, useEffect, useRef, useState, type ErrorInfo, type ReactNode } from 'react'

type Props = { children: ReactNode; quiet?: boolean; resetKey?: string; sidebar?: boolean }
type State = { error: Error | null; stack: string; resetKey?: string }

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null, stack: '', resetKey: this.props.resetKey }
  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error }
  }
  static getDerivedStateFromProps(props: Props, state: State): Partial<State> | null {
    return props.resetKey !== state.resetKey ? { resetKey: props.resetKey, error: null, stack: '' } : null
  }
  componentDidCatch(error: Error, info: ErrorInfo) {
    this.setState({ stack: info.componentStack ?? '' })
    console.error(error)
  }
  render() {
    const { error, stack } = this.state
    if (!error) return this.props.children
    if (this.props.quiet) return null
    // Message and component stack only: nothing from app state, settings or the server goes into the details.
    return <Fallback details={`${error.message}\n${stack.trim()}`} sidebar={!!this.props.sidebar} />
  }
}

function Fallback({ details, sidebar }: { details: string; sidebar: boolean }) {
  const [copied, setCopied] = useState<'yes' | 'no' | null>(null)
  const heading = useRef<HTMLHeadingElement>(null)
  useEffect(() => heading.current?.focus(), [])
  const copy = () => navigator.clipboard.writeText(details).then(() => setCopied('yes'), () => setCopied('no'))
  return (
    <div className="page" role="alert" style={{ gap: 12, maxWidth: 560 }}>
      <h1 className="h-page" ref={heading} tabIndex={-1}>{t('This page ran into a problem')}</h1>
      <p className="muted">{sidebar ? t('Your fonts and settings are fine. Reload to try again, or open another page from the sidebar.') : t('Your fonts and settings are fine. Reload to try again.')}</p>
      <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
        <button type="button" className="btn primary" onClick={() => location.reload()}>{t('Reload')}</button>
        <button type="button" className="btn" onClick={copy}>{t('Copy details')}</button>
        <span className="faint" role="status" style={{ fontSize: 13 }}>{copied === 'yes' ? t('Copied.') : copied === 'no' ? t('Couldn’t copy.') : ''}</span>
      </div>
    </div>
  )
}
