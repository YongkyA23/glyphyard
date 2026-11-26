// A render error in one page shows a plain message instead of a blank page (the sidebar stays usable).
import { Component, useEffect, useRef, useState, type ErrorInfo, type ReactNode } from 'react'

/** `resetKey` (the location key) clears the error on navigation; `sidebar`: there is a sidebar to go elsewhere. */
type Props = { children: ReactNode; resetKey?: string; sidebar?: boolean }
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
    // Message and component stack only: no session, user or server data goes into the details.
    return <Fallback details={`${error.message}\n${stack.trim()}`} sidebar={!!this.props.sidebar} />
  }
}

function Fallback({ details, sidebar }: { details: string; sidebar: boolean }) {
  const [copied, setCopied] = useState<'yes' | 'no' | null>(null)
  const heading = useRef<HTMLHeadingElement>(null)
  useEffect(() => heading.current?.focus(), [])
  const copy = () => navigator.clipboard.writeText(details).then(() => setCopied('yes'), () => setCopied('no'))
  return (
    <div className="stack-8" role="alert" style={{ maxWidth: 560 }}>
      <h1 ref={heading} tabIndex={-1}>This page ran into a problem</h1>
      <p className="mute">Nothing was changed. Reload to try again{sidebar ? ', or open another page from the sidebar' : ''}.</p>
      <div className="toolbar">
        <button type="button" className="btn btn-primary" onClick={() => location.reload()}>Reload</button>
        <button type="button" className="btn" onClick={copy}>Copy details</button>
        <span className="small mute" role="status">{copied === 'yes' ? 'Copied.' : copied === 'no' ? 'Couldn’t copy.' : ''}</span>
      </div>
    </div>
  )
}

