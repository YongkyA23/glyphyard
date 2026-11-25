import { useState, type FormEvent } from 'react'
import { useNavigate } from 'react-router'
import { AnimatePresence, motion } from 'motion/react'
import { api, ApiError, errorText } from '../api'
import { fadeRise, press, shakeOpts, shakeX, spring, stagger } from '../motion'
import { Spin } from '../ui'

const word = { hidden: { opacity: 0, y: '0.45em' }, show: { opacity: 1, y: 0, transition: spring } }
const Words = ({ text }: { text: string }) =>
  <>{text.split(' ').map((w, i) => <span key={i}>{i > 0 && ' '}<motion.span className="swap" variants={word}>{w}</motion.span></span>)}</>

export function Login() {
  const navigate = useNavigate()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [attempt, setAttempt] = useState(0)
  const [busy, setBusy] = useState(false)
  const [help, setHelp] = useState(false)

  const fail = (text: string) => { setError(text); setAttempt((n) => n + 1) }
  const submit = async (e: FormEvent) => {
    e.preventDefault()
    if (!email.includes('@') || !password) return fail('Enter your email and password.')
    setBusy(true)
    setError('')
    try {
      await api.login(email.trim(), password)
      navigate('/', { replace: true })
    } catch (err) {
      fail(err instanceof ApiError && err.status === 401 ? 'That email and password don’t match an admin.' : errorText(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="signin">
      <motion.div className="signin-art" initial="hidden" animate="show" variants={stagger(0.06, 0.1)}>
        <motion.span className="brand-name" variants={fadeRise}>Typeshelf <span className="brand-sub-inline">Admin</span></motion.span>
        <motion.div className="signin-hero" variants={stagger(0.07)}>
          <Words text="Every font," /><br /><em><Words text="on every desk." /></em>
        </motion.div>
        <motion.span className="signin-note" variants={fadeRise}>Only admins sign in here. Desktop users never need an account.</motion.span>
      </motion.div>
      <div className="signin-side">
        <motion.form className="signin-form" onSubmit={submit} noValidate initial="hidden" animate="show" variants={stagger(0.05, 0.2)}>
          <motion.h1 variants={fadeRise}>Sign in</motion.h1>
          <motion.label className="field" variants={fadeRise}>
            <span>Email</span>
            <input type="email" value={email} autoComplete="username" required
              onChange={(e) => { setEmail(e.target.value); setError('') }} />
          </motion.label>
          <motion.label className="field" variants={fadeRise}>
            <span>Password</span>
            <input type="password" value={password} autoComplete="current-password" required
              onChange={(e) => { setPassword(e.target.value); setError('') }} />
          </motion.label>
          <AnimatePresence initial={false}>
            {error && (
              <motion.span key={attempt} role="alert" className="form-error" style={{ animation: 'none' }}
                initial={{ opacity: 0 }} animate={{ opacity: 1, x: shakeX, transition: shakeOpts }} exit={{ opacity: 0, transition: { duration: 0.1 } }}>
                {error}
              </motion.span>
            )}
          </AnimatePresence>
          <motion.button type="submit" className="btn btn-primary btn-lg" disabled={busy} variants={fadeRise} {...(busy ? {} : press)}>
            {busy && <Spin />}{busy ? 'Signing in…' : 'Sign in'}
          </motion.button>
          <motion.button type="button" className="link-btn small" aria-expanded={help} onClick={() => setHelp(!help)} variants={fadeRise}>
            Forgot password?
          </motion.button>
          <AnimatePresence>
            {help && (
              <motion.span className="hint" initial={{ opacity: 0, y: -4 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -4 }}>
                Ask an owner to reset your password from Settings, or run <code>typeshelf-server create-admin</code> with your email on the server.
              </motion.span>
            )}
          </AnimatePresence>
        </motion.form>
      </div>
    </div>
  )
}
