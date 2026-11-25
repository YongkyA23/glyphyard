// A render error in one page shows a plain message instead of a blank page (the sidebar stays usable), and a promise
// nobody handled shows a short error note once per distinct message.
import { Component, useEffect, useState, type ErrorInfo, type ReactNode } from 'react'
import { errorText } from './api'

type State = { error: Error | null; stack: string }

export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null, stack: '' }
  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error }
  }
  componentDidCatch(error: Error, info: ErrorInfo) {
    this.setState({ stack: info.componentStack ?? '' })
    console.error(error)
  }
  render() {
    const { error, stack } = this.state
    if (!error) return this.props.children
    // Message and component stack only: no session, user or server data goes into the details.
    return <Fallback details={`${error.message}\n${stack.trim()}`} />
  }
}

function Fallback({ details }: { details: string }) {
  const [copied, setCopied] = useState<'yes' | 'no' | null>(null)
  const copy = () => navigator.clipboard.writeText(details).then(() => setCopied('yes'), () => setCopied('no'))
  return (
    <div className="stack-8" role="alert" style={{ maxWidth: 560 }}>
      <h1>This page ran into a problem</h1>
      <p className="mute">Nothing was changed. Reload to try again, or open another page from the sidebar.</p>
      <div className="toolbar">
        <button type="button" className="btn btn-primary" onClick={() => location.reload()}>Reload</button>
        <button type="button" className="btn" onClick={copy}>Copy details</button>
        <span className="small mute" role="status">{copied === 'yes' ? 'Copied.' : copied === 'no' ? 'Couldn’t copy.' : ''}</span>
      </div>
    </div>
  )
}

/** Unhandled promise rejections, each distinct message once; the admin UI has no toast system, so this is its own. */
export function RejectionToast() {
  const [msg, setMsg] = useState<string | null>(null)
  useEffect(() => {
    const seen = new Set<string>()
    let hide = 0
    const onRejection = (e: PromiseRejectionEvent) => {
      const m = errorText(e.reason)
      if (seen.has(m)) return
      seen.add(m)
      setMsg(m)
      clearTimeout(hide)
      hide = window.setTimeout(() => setMsg(null), 8000)
    }
    addEventListener('unhandledrejection', onRejection)
    return () => (removeEventListener('unhandledrejection', onRejection), clearTimeout(hide))
  }, [])
  if (!msg) return null
  return (
    <div className="error-box" role="alert" style={{ position: 'fixed', left: '50%', bottom: 20, transform: 'translateX(-50%)', zIndex: 20, maxWidth: 'calc(100vw - 32px)' }}>
      <span>Something didn’t work: {msg}</span>
      <button type="button" className="btn btn-sm" onClick={() => setMsg(null)}>Dismiss</button>
    </div>
  )
}
