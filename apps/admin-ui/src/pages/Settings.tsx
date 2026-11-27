import { useState, type FormEvent, type ReactNode } from 'react'
import { AnimatePresence, motion } from 'motion/react'
import { api, ApiError, type Admin, type Me, type Role, type Settings } from '../api'
import { base, fadeRise, fast, row } from '../motion'
import { ago, Badge, Empty, ErrorBox, errMsg, Loaded, Skel, Spin, Swap, Toggle, useConfirm, useLoad } from '../ui'
import { toast } from '../toast'
import { Hint } from '../Hint'
import { t, tk } from '../i18n'

const skeleton = (
  <div className="stack-18">
    <div className="card pad-20 stack-12"><Skel w={140} h={16} /><Skel w="60%" h={38} r={8} /><Skel w="85%" /></div>
    <div className="card pad-20 stack-12"><Skel w={200} h={16} /><Skel h={40} r={8} /><Skel h={40} r={8} /></div>
    <div className="card pad-20 stack-14"><Skel w={120} h={16} />{[0, 1, 2, 3].map((i) => <span key={i} className="row-between"><Skel w="55%" /><Skel w={40} h={22} r={11} /></span>)}</div>
  </div>
)

type Flags = 'allow_connection_edit' | 'deliver_library_to_new' | 'activate_delivered' | 'notify_admin_changes'
const TOGGLES: [Flags, string, string][] = [
  ['allow_connection_edit', tk('Let users change the server address and key'), tk('Off locks Settings › Connection in the desktop app')],
  ['deliver_library_to_new', tk('Deliver the shared library to new installs'), tk('New computers get every “Everyone” font automatically')],
  ['activate_delivered', tk('Also turn those fonts on'), tk('Otherwise they arrive turned off')],
  ['notify_admin_changes', tk('Tell users when an admin changes their fonts'), tk('Shows a short banner in the app')],
]
const LOGO_TYPES = ['image/png', 'image/jpeg', 'image/webp', 'image/svg+xml']
const LOGO_MAX = 200 * 1024
const RETENTION: [string, string][] = [['', tk('Forever')], ['1095', tk('3 years')], ['365', tk('1 year')]]

export function SettingsPage() {
  const state = useLoad(() => api.settings(), [])
  const me = useLoad(() => api.me(), [])
  // undefined while loading, so owner-only controls neither flash on nor show their reason early.
  const owner = me.data && me.data.role === 'owner'
  return (
    <div className="page">
      <div className="narrow stack-18">
        <h1>{t('Settings')}</h1>
        <Loaded state={state} skeleton={skeleton}>{(s) => <Form initial={s} onSaved={(n) => state.setData(() => n)} owner={owner}
          admins={<><Admins me={me.data} /><OwnPassword /></>} />}</Loaded>
      </div>
    </div>
  )
}

const OWNER_ONLY = tk('Only an owner can add or remove admins, change roles, reset passwords and regenerate the install key.')

// Admins has its own form, so the settings form is an empty <form> that the fields join via `form=`.
/** Same rule as the server's `valid_install_key`: travels unescaped in a header and a URL. */
const INSTALL_KEY = /^[A-Za-z0-9._~-]{8,128}$/

function Form({ initial, onSaved, admins, owner }: { initial: Settings; onSaved: (s: Settings) => void; admins: ReactNode; owner?: boolean }) {
  const [s, setS] = useState(initial)
  const [days, setDays] = useState(String(initial.name_change_days))
  const [showKey, setShowKey] = useState(false)
  const [status, setStatus] = useState<'' | 'saving' | 'saved' | 'regen'>('')
  const [error, setError] = useState('')
  const [confirmDialog, confirm] = useConfirm()
  const daysNum = Number(days)
  const daysValid = Number.isInteger(daysNum) && daysNum >= 1 && daysNum <= 365
  const dirty = daysNum !== initial.name_change_days || TOGGLES.some(([k]) => s[k] !== initial[k])
    || s.timeline_retention_days !== initial.timeline_retention_days
    || s.brand_name.trim() !== initial.brand_name || s.brand_color !== initial.brand_color || s.brand_logo !== initial.brand_logo

  const save = async (e: FormEvent) => {
    e.preventDefault()
    if (!daysValid) return
    setStatus('saving')
    setError('')
    try {
      const { install_key: _, server_url: __, connection_password_set: ___, ...rest } = s
      const saved = await api.saveSettings({ ...rest, name_change_days: daysNum })
      setS(saved)
      onSaved(saved)
      window.dispatchEvent(new Event('brand-changed'))
      setStatus('saved')
    } catch (err) {
      setError(errMsg(err))
      setStatus('')
    }
  }

  const regenerate = async () => {
    if (!(await confirm(t('Regenerate the install key? Apps installed with the old key disconnect until they’re updated.'), t('Regenerate'), true))) return
    setStatus('regen')
    setError('')
    try {
      const { install_key } = await api.regenerateKey()
      setS((x) => ({ ...x, install_key }))
      onSaved({ ...initial, install_key })
      setShowKey(true)
    } catch (err) {
      setError(errMsg(err))
    } finally {
      setStatus('')
    }
  }

  const [customKey, setCustomKey] = useState<string | null>(null)
  const [customError, setCustomError] = useState('')
  const changeKey = async (e: FormEvent) => {
    e.preventDefault()
    const key = (customKey ?? '').trim()
    if (!INSTALL_KEY.test(key)) return setCustomError(t('Use 8 to 128 characters: letters, digits, - _ . or ~, no spaces.'))
    if (key === s.install_key) return setCustomError(t('That is already the install key.'))
    if (!(await confirm(t('Change the install key to your own? Apps installed with the old key disconnect until they’re updated.'), t('Change key'), true))) return
    setStatus('regen')
    setCustomError('')
    try {
      const { install_key } = await api.regenerateKey(key)
      setS((x) => ({ ...x, install_key }))
      onSaved({ ...initial, install_key })
      setShowKey(true)
      setCustomKey(null)
      toast(t('Install key changed.'))
    } catch (err) {
      setCustomError(errMsg(err))
    } finally {
      setStatus('')
    }
  }

  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => { setS((x) => ({ ...x, [k]: v })); setStatus('') }

  const pickLogo = (file: File | undefined) => {
    if (!file) return
    setError('')
    if (!LOGO_TYPES.includes(file.type)) return setError(t('Choose a PNG, JPEG, WebP or SVG image for the logo.'))
    const reader = new FileReader()
    reader.onload = () => {
      const url = String(reader.result)
      // The server caps the data URL (base64 is a third bigger than the file).
      if (url.length > LOGO_MAX) return setError(t('The logo is too big: keep it under 150 KB.'))
      set('brand_logo', url)
    }
    reader.readAsDataURL(file)
  }

  return (
    <div className="stack-18">
      <form id="settings-form" onSubmit={save} noValidate />
      <motion.section aria-labelledby="pol" className="card pad-20 stack-12" variants={fadeRise}>
        <div className="title-row"><h2 id="pol" className="h2">{t('Name changes')}</h2><Hint text={t('The limit is checked on the server, so it can’t be bypassed by editing files on a computer. You can reset it for one person from their page.')} /></div>
        <label className="inline-field">
          <span>{t('Users can change their name once every')}</span>
          <input type="number" form="settings-form" min={1} max={365} step={1} required className="input input-days" value={days}
            aria-invalid={!daysValid} aria-describedby={daysValid ? undefined : 'days-err'}
            onChange={(e) => { setDays(e.target.value); setStatus('') }} />
          <span>{t('days')}</span>
        </label>
        {!daysValid && <span id="days-err" role="alert" className="form-error">{t('Enter a whole number of days from 1 to 365.')}</span>}
      </motion.section>

      <motion.section aria-labelledby="brd" className="card pad-20 stack-12" variants={fadeRise}>
        <div className="title-row"><h2 id="brd" className="h2">{t('Branding')}</h2><Hint text={t('The logo replaces the Glyphyard mark at the top of the sidebar (without a logo, the name sits beside the mark), and buttons, links and highlights take the colour. A wide logo on a transparent background works best (PNG, JPEG, WebP or SVG, under 150 KB). Leave all three empty for plain Glyphyard.')} /></div>
        <label className="field"><span>{t('Name shown in the desktop app')}</span>
          <input type="text" form="settings-form" className="input" maxLength={40} placeholder={t('e.g. your studio’s name')} value={s.brand_name}
            onChange={(e) => set('brand_name', e.target.value)} />
        </label>
        <div className="row-8">
          <span className="logo-box">{s.brand_logo ? <img src={s.brand_logo} alt={t('Current logo')} /> : <span className="meta">{t('No logo')}</span>}</span>
          <label className="btn btn-sm">
            {s.brand_logo ? t('Replace logo…') : t('Upload logo…')}
            <input type="file" className="sr-only" accept={LOGO_TYPES.join(',')} onChange={(e) => { pickLogo(e.target.files?.[0]); e.target.value = '' }} />
          </label>
          {s.brand_logo && <button type="button" className="btn btn-sm" onClick={() => set('brand_logo', '')}>{t('Remove logo')}</button>}
        </div>
        <div className="row-8">
          <label className="inline-field">
            <span>{t('Accent colour')}</span>
            {/* A colour input can't be empty: it shows Glyphyard's blue until the admin picks one. */}
            <input type="color" form="settings-form" className="input input-color" value={s.brand_color || '#2547d0'}
              onChange={(e) => set('brand_color', e.target.value)} />
          </label>
          <span className="small mute">{s.brand_color || t('Glyphyard’s own')}</span>
          {s.brand_color && <button type="button" className="btn btn-sm" onClick={() => set('brand_color', '')}>{t('Use Glyphyard’s colour')}</button>}
        </div>
      </motion.section>

      <motion.section aria-labelledby="con" className="card pad-20 stack-12" variants={fadeRise}>
        <div className="title-row"><h2 id="con" className="h2">{t('Desktop app connection')}</h2><Hint text={t('Both are built into the installer, so onboarding still asks only for a name. Users can change them later in the app under Settings › Connection, unless you lock that below. Regenerating disconnects apps installed with the old key until they’re updated.')} /></div>
        <div className="stack-6">
          <span className="small mute">{t('Server address built into the installer')}</span>
          <span className="code-box">{s.server_url || '—'}</span>
        </div>
        <div className="key-row">
          <div className="stack-6 grow">
            <span className="small mute" id="key-label">{t('Install key (stops strangers from connecting)')}</span>
            <span className="code-box" aria-labelledby="key-label"><Swap>{showKey ? s.install_key : '••••••••••••••••••••'}</Swap></span>
          </div>
          <button type="button" className="btn" aria-pressed={showKey} onClick={() => setShowKey(!showKey)}><Swap>{showKey ? t('Hide') : t('Show')}</Swap></button>
          <button type="button" className="btn btn-danger" disabled={status === 'regen' || !owner} onClick={regenerate}
            aria-describedby={owner === false ? 'key-owner' : undefined}>
            {status === 'regen' && <Spin />}{status === 'regen' ? t('Regenerating…') : t('Regenerate')}
          </button>
        </div>
        {owner === false && <span id="key-owner" className="meta">{t('Only an owner can regenerate the install key.')}</span>}
        {owner && customKey === null && (
          <button type="button" className="btn btn-sm" style={{ alignSelf: 'flex-start' }} onClick={() => { setCustomKey(''); setCustomError('') }}>{t('Use your own key…')}</button>
        )}
        {owner && customKey !== null && (
          <form className="stack-6" onSubmit={changeKey} noValidate>
            <div className="title-row">
              <label htmlFor="custom-key" className="small mute">{t('Your own install key')}</label>
              <Hint text={t('8 to 128 characters: letters, digits, - _ . or ~. Build new installers with the same key, or give it to users for Settings › Connection.')} />
            </div>
            <input id="custom-key" type="text" className="input mono" autoComplete="off" spellCheck={false} maxLength={128} autoFocus value={customKey}
              aria-invalid={!!customError} aria-describedby={customError ? 'custom-key-err' : undefined}
              onChange={(e) => { setCustomKey(e.target.value); setCustomError('') }} />
            {customError && <span id="custom-key-err" role="alert" className="form-error">{customError}</span>}
            <div className="row-8">
              <button type="submit" className="btn btn-danger btn-sm" disabled={status === 'regen' || !customKey.trim()}>
                {status === 'regen' && <Spin />}{t('Change key')}
              </button>
              <button type="button" className="btn btn-sm" onClick={() => { setCustomKey(null); setCustomError('') }}>{t('Cancel')}</button>
            </div>
          </form>
        )}
      </motion.section>

      <motion.section aria-labelledby="new" className="card pad-20 stack-4" variants={fadeRise}>
        <h2 id="new" className="h2 mb-8">{t('New installs')}</h2>
        {TOGGLES.map(([k, label, hint]) => (
          <div key={k} className="setting-row">
            <span className="title-row"><span>{t(label /* i18n-dynamic */)}</span><Hint text={t(hint /* i18n-dynamic */)} /></span>
            <Toggle on={s[k]} label={t(label /* i18n-dynamic */)} onChange={() => set(k, !s[k])} />
          </div>
        ))}
      </motion.section>

      <UnlockPassword allowEdit={s.allow_connection_edit} isSet={s.connection_password_set} owner={owner}
        onChanged={(v) => { setS((x) => ({ ...x, connection_password_set: v })); onSaved({ ...initial, connection_password_set: v }) }} />

      {admins}

      <Updates owner={owner} />

      <motion.section aria-labelledby="dat" className="card pad-20 stack-10" variants={fadeRise}>
        <div className="title-row"><h2 id="dat" className="h2">{t('History and data')}</h2><Hint text={t('Name history is always kept. Retired installs from merges are kept with their history.')} /></div>
        <label className="inline-field">
          <span>{t('Keep timeline events for')}</span>
          <select className="input" value={s.timeline_retention_days === null ? '' : String(s.timeline_retention_days)}
            onChange={(e) => set('timeline_retention_days', e.target.value ? Number(e.target.value) : null)}>
            {RETENTION.map(([v, label]) => <option key={v} value={v}>{t(label /* i18n-dynamic */)}</option>)}
            {s.timeline_retention_days !== null && !RETENTION.some(([v]) => v === String(s.timeline_retention_days)) &&
              <option value={String(s.timeline_retention_days)}>{t('{n} days', { n: s.timeline_retention_days })}</option>}
          </select>
        </label>
        <span className="row-8"><a className="btn btn-38" href={api.nameChangesCsvUrl()} download="name-history.csv">{t('Export name history (CSV)')}</a></span>
      </motion.section>

      {error && <ErrorBox message={error} />}
      <motion.div className="save-bar" variants={fadeRise}>
        <span className="small mute" role="status"><Swap>{status === 'saved' && !dirty ? t('Saved. Desktop apps get the new policy right away.') : dirty ? t('You have unsaved changes.') : ''}</Swap></span>
        <button type="submit" form="settings-form" className="btn btn-primary" disabled={!dirty || !daysValid || status === 'saving'}>
          {status === 'saving' && <Spin />}{status === 'saving' ? t('Saving…') : t('Save changes')}
        </button>
      </motion.div>
      {confirmDialog}
    </div>
  )
}

function Admins({ me }: { me?: Me }) {
  const state = useLoad(() => api.admins(), [])
  const [adding, setAdding] = useState(false)
  const [resetting, setResetting] = useState<number>()
  const [error, setError] = useState('')
  const [confirmDialog, confirm] = useConfirm()
  const owner = me?.role === 'owner'
  const act = async (f: () => Promise<void>) => {
    setError('')
    try { await f(); await state.reload(true) } catch (err) { setError(errMsg(err)) }
  }
  const setRole = async (a: Admin, role: Role) => {
    const text = role === 'owner'
      ? t('Make {email} an owner? Owners can add and remove admins, change roles, reset passwords and regenerate the install key.', { email: a.email })
      : t('Make {email} a plain admin? They can no longer manage admins or the install key.', { email: a.email })
    if (await confirm(text, role === 'owner' ? t('Make owner') : t('Make admin'), true)) await act(() => api.setRole(a.id, role))
  }
  const remove = async (a: Admin) => {
    if (await confirm(t('Remove {email}? They are signed out at once and can no longer sign in.', { email: a.email }), t('Remove'), true)) await act(() => api.removeAdmin(a.id))
  }
  return (
    <motion.section aria-labelledby="adm" className="card pad-20 stack-10" variants={fadeRise}>
      <div className="row-between">
        <h2 id="adm" className="h2">{t('Admins')}</h2>
        {owner && <button type="button" className="btn btn-36" aria-expanded={adding} onClick={() => setAdding(!adding)}><Swap>{adding ? t('Cancel') : t('Add admin')}</Swap></button>}
      </div>
      {me && !owner && <span className="meta">{t(OWNER_ONLY /* i18n-dynamic */)}</span>}
      <AnimatePresence initial={false}>
        {adding && <AddAdmin onAdded={() => { setAdding(false); void state.reload(true) }} />}
      </AnimatePresence>
      {error && <ErrorBox message={error} />}
      <Loaded state={state} skeleton={<div className="stack-14">{[0, 1].map((i) => <span key={i} className="stack-6"><Skel w="45%" /><Skel w="30%" h={10} /></span>)}</div>}>{(admins) => (
        <div>
          {admins.length === 0 && <Empty>{t('No admins.')}</Empty>}
          {admins.map((a: Admin, i) => {
            const self = a.email === me?.email
            return (
              <motion.div key={a.email} className="stack-6" {...row(i, admins.length)}>
                <div className="admin-row">
                  <span className="stack-2">
                    <span>{a.email}</span>
                    <span className="meta">{[self && t('You'), a.last_login ? t('last sign-in {when}', { when: ago(a.last_login).toLowerCase() }) : t('Never signed in')].filter(Boolean).join(' · ')}</span>
                  </span>
                  {owner && !self ? (
                    <span className="row-8">
                      <select className="input" aria-label={t('Role for {email}', { email: a.email })} value={a.role} onChange={(e) => void setRole(a, e.target.value as Role)}>
                        <option value="owner">{t('Owner')}</option>
                        <option value="admin">{t('Admin')}</option>
                      </select>
                      <button type="button" className="btn btn-sm" aria-expanded={resetting === a.id} onClick={() => setResetting(resetting === a.id ? undefined : a.id)}>{t('Reset password')}</button>
                      <button type="button" className="btn btn-sm btn-danger" onClick={() => void remove(a)}>{t('Remove')}</button>
                    </span>
                  ) : <span className="small mute">{a.role === 'owner' ? t('Owner') : t('Admin')}</span>}
                </div>
                <AnimatePresence initial={false}>
                  {resetting === a.id && <ResetPassword admin={a} onDone={() => setResetting(undefined)} />}
                </AnimatePresence>
              </motion.div>
            )
          })}
        </div>
      )}</Loaded>
      {confirmDialog}
    </motion.section>
  )
}

function ResetPassword({ admin, onDone }: { admin: Admin; onDone: () => void }) {
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const submit = async (e: FormEvent) => {
    e.preventDefault()
    if (password.length < 8) return setError(t('Use a password of at least 8 characters.'))
    setBusy(true)
    setError('')
    try {
      await api.resetPassword(admin.id, password)
      onDone()
    } catch (err) {
      setError(errMsg(err))
      setBusy(false)
    }
  }
  return (
    <motion.form className="add-admin" onSubmit={submit} noValidate
      initial={{ opacity: 0, y: -6 }} animate={{ opacity: 1, y: 0, transition: base }} exit={{ opacity: 0, y: -6, transition: fast }}>
      <label className="field grow"><span>{t('New password for {email}', { email: admin.email })}</span>
        <input type="password" className="input" autoComplete="new-password" value={password} onChange={(e) => { setPassword(e.target.value); setError('') }} />
      </label>
      <button type="submit" className="btn btn-primary" disabled={busy}>{busy && <Spin />}{busy ? t('Saving…') : t('Set password')}</button>
      <span className="meta full">{t('They are signed out everywhere. Share the new password with them privately.')}</span>
      {error && <span role="alert" className="form-error full">{error}</span>}
    </motion.form>
  )
}

function OwnPassword() {
  const [current, setCurrent] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [status, setStatus] = useState<'' | 'saving' | 'saved'>('')
  const submit = async (e: FormEvent) => {
    e.preventDefault()
    if (!current) return setError(t('Enter your current password.'))
    if (password.length < 8) return setError(t('Use a new password of at least 8 characters.'))
    setStatus('saving')
    setError('')
    try {
      await api.changePassword(current, password)
      setCurrent('')
      setPassword('')
      setStatus('saved')
    } catch (err) {
      setError(errMsg(err))
      setStatus('')
    }
  }
  return (
    <motion.section aria-labelledby="pw" className="card pad-20 stack-10" variants={fadeRise}>
      <h2 id="pw" className="h2">{t('Your password')}</h2>
      <form className="add-admin" onSubmit={submit} noValidate>
        <label className="field grow"><span>{t('Current password')}</span>
          <input type="password" className="input" autoComplete="current-password" value={current} onChange={(e) => { setCurrent(e.target.value); setError(''); setStatus('') }} />
        </label>
        <label className="field grow"><span>{t('New password')}</span>
          <input type="password" className="input" autoComplete="new-password" value={password} onChange={(e) => { setPassword(e.target.value); setError(''); setStatus('') }} />
        </label>
        <button type="submit" className="btn btn-primary" disabled={status === 'saving'}>{status === 'saving' && <Spin />}{status === 'saving' ? t('Changing…') : t('Change password')}</button>
        <span className="meta full">{t('Changing it signs you out on every other browser.')}</span>
        {error && <span role="alert" className="form-error full">{error}</span>}
        <span className="meta full" role="status">{status === 'saved' ? t('Password changed. You were signed out everywhere else.') : ''}</span>
      </form>
    </motion.section>
  )
}

const UPDATE_FIELDS: [string, string, string][] = [
  ['file', tk('Setup program'), '.exe'],
  ['manifest', tk('Manifest'), '.manifest'],
  ['signature', tk('Signature'), '.sig'],
]

function Updates({ owner }: { owner?: boolean }) {
  const state = useLoad(() => api.updateRelease(), [])
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [done, setDone] = useState('')
  const submit = async (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault()
    const formEl = e.currentTarget
    const form = new FormData(formEl)
    if (UPDATE_FIELDS.some(([name]) => !(form.get(name) instanceof File) || (form.get(name) as File).size === 0)) {
      return setError(t('Choose the setup program, its manifest and its signature.'))
    }
    setBusy(true)
    setError('')
    setDone('')
    try {
      const r = await api.uploadUpdate(form)
      state.setData(() => r)
      setDone(t('Version {version} is now offered to every computer.', { version: r.version }))
      formEl.reset()
    } catch (err) {
      setError(err instanceof ApiError && err.status === 413 ? t('The upload is larger than 100 MB, the most the server accepts. Check that you chose the setup program.') : errMsg(err))
    } finally {
      setBusy(false)
    }
  }
  return (
    <motion.section aria-labelledby="upd" className="card pad-20 stack-10" variants={fadeRise}>
      <div className="title-row"><h2 id="upd" className="h2">{t('App updates')}</h2><Hint text={t('Sign the setup program on the computer that keeps the private key, with typeshelf-server update-sign. Apps built with the matching public key check once a day, check the signature and the file, and install the new version when the user quits Glyphyard. The server refuses a setup program that doesn’t match its manifest.')} /></div>
      <Loaded state={state} skeleton={<Skel w="50%" />}>{(r) => (
        <span className="small">{r ? t('Current release: version {version} ({size} MB)', { version: r.version, size: (r.size / 1_048_576).toFixed(1) }) : t('No release uploaded. Desktop apps keep the version they have.')}</span>
      )}</Loaded>
      <form className="stack-10" onSubmit={submit} noValidate>
        <div className="file-grid">
          {UPDATE_FIELDS.map(([name, label, accept]) => (
            <label key={name} className="field"><span>{t(label /* i18n-dynamic */)} ({accept})</span>
              <input type="file" name={name} accept={accept} disabled={!owner} onChange={() => { setError(''); setDone('') }} />
            </label>
          ))}
        </div>
        <span className="row-8"><button type="submit" className="btn btn-primary" disabled={busy || !owner} aria-describedby={owner === false ? 'upd-owner' : undefined}>{busy && <Spin />}{busy ? t('Uploading…') : t('Upload release')}</button></span>
        {owner === false && <span id="upd-owner" className="meta">{t('Only an owner can upload a release.')}</span>}
        {error && <span role="alert" className="form-error">{error}</span>}
        <span className="small mute" role="status">{done}</span>
      </form>
    </motion.section>
  )
}

function UnlockPassword({ allowEdit, isSet, owner, onChanged }: { allowEdit: boolean; isSet: boolean; owner?: boolean; onChanged: (set: boolean) => void }) {
  const [editing, setEditing] = useState(false)
  const [password, setPassword] = useState('')
  const [again, setAgain] = useState('')
  const [shown, setShown] = useState(false)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const [confirmDialog, confirm] = useConfirm()
  const close = () => { setEditing(false); setPassword(''); setAgain(''); setShown(false); setError('') }
  const save = async (e: FormEvent) => {
    e.preventDefault()
    if (password.length < 8) return setError(t('Use a password of at least 8 characters.'))
    if (password !== again) return setError(t('The two passwords don’t match.'))
    setBusy(true)
    setError('')
    try {
      const r = await api.setConnectionPassword(password)
      onChanged(r.connection_password_set)
      toast(t('Unlock password saved.'))
      close()
    } catch (err) {
      setError(errMsg(err))
    } finally {
      setBusy(false)
    }
  }
  const remove = async () => {
    if (!(await confirm(t('Remove the unlock password? On a locked computer, nobody will be able to unlock the connection settings any more.'), t('Remove'), true))) return
    try {
      const r = await api.setConnectionPassword(null)
      onChanged(r.connection_password_set)
      toast(t('Unlock password removed.'))
    } catch (err) {
      toast(errMsg(err), { tone: 'bad' })
    }
  }
  return (
    <motion.section aria-labelledby="unl" className="card pad-20 stack-12" variants={fadeRise}>
      <div className="row-between">
        <div className="title-row"><h2 id="unl" className="h2">{t('Unlock password')}</h2><Hint text={t('Stored as a one-way hash on every computer so you can unlock offline. Use a long passphrase.')} /></div>
        <Badge tone={isSet ? 'ok' : 'outline'}>{isSet ? t('Password set') : t('No password')}</Badge>
      </div>
      {!allowEdit && !isSet && <p className="note warn">{t('Connection changes are locked and no unlock password is set, so nobody can change the connection on a locked computer.')}</p>}
      {owner === false && <span className="meta">{t('Only an owner can set or remove the unlock password.')}</span>}
      {owner && !editing && (
        <span className="row-8">
          <button type="button" className="btn" onClick={() => setEditing(true)}>{isSet ? t('Change password') : t('Set password')}</button>
          {isSet && <button type="button" className="btn btn-danger" onClick={() => void remove()}>{t('Remove password')}</button>}
        </span>
      )}
      {owner && editing && (
        <form className="stack-10" onSubmit={save} noValidate>
          <label className="field"><span>{t('Unlock password (at least 8 characters)')}</span>
            <span className="row-8">
              <input type={shown ? 'text' : 'password'} className="input grow" autoFocus autoComplete="new-password" maxLength={128} value={password}
                onChange={(e) => { setPassword(e.target.value); setError('') }} />
              <button type="button" className="btn" aria-pressed={shown} aria-label={shown ? t('Hide password') : t('Show password')} onClick={() => setShown(!shown)}>{shown ? t('Hide') : t('Show')}</button>
            </span>
          </label>
          <label className="field"><span>{t('Confirm unlock password')}</span>
            <input type={shown ? 'text' : 'password'} className="input" autoComplete="new-password" maxLength={128} value={again}
              onChange={(e) => { setAgain(e.target.value); setError('') }} />
          </label>
          {error && <span role="alert" className="form-error">{error}</span>}
          <span className="row-8">
            <button type="submit" className="btn btn-primary" disabled={busy}>{busy && <Spin />}{isSet ? t('Change password') : t('Set password')}</button>
            <button type="button" className="btn" onClick={close}>{t('Cancel')}</button>
          </span>
        </form>
      )}
      {confirmDialog}
    </motion.section>
  )
}

function AddAdmin({ onAdded }: { onAdded: () => void }) {
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const submit = async (e: FormEvent) => {
    e.preventDefault()
    if (!email.includes('@')) return setError(t('Enter the new admin’s email.'))
    if (password.length < 8) return setError(t('Use a password of at least 8 characters.'))
    setBusy(true)
    setError('')
    try {
      await api.addAdmin(email.trim(), password)
      onAdded()
    } catch (err) {
      setError(errMsg(err))
      setBusy(false)
    }
  }
  return (
    <motion.form className="add-admin" onSubmit={submit} noValidate
      initial={{ opacity: 0, y: -6 }} animate={{ opacity: 1, y: 0, transition: base }} exit={{ opacity: 0, y: -6, transition: fast }}>
      <label className="field grow"><span>{t('Email')}</span>
        <input type="email" className="input" autoComplete="off" value={email} onChange={(e) => { setEmail(e.target.value); setError('') }} />
      </label>
      <label className="field grow"><span>{t('Password')}</span>
        <input type="password" className="input" autoComplete="new-password" value={password} onChange={(e) => { setPassword(e.target.value); setError('') }} />
      </label>
      <span className="title-row"><button type="submit" className="btn btn-primary" disabled={busy}>{busy && <Spin />}{busy ? t('Adding…') : t('Add admin')}</button><Hint text={t('Share the password with them privately. They sign in at this page’s address.')} /></span>
      {error && <span role="alert" className="form-error full">{error}</span>}
    </motion.form>
  )
}
