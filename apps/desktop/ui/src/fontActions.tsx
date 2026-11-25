// What you can do to one or more families: the right-click menu, the selection bar's actions and their dialogs,
// plus dragging families onto a sidebar collection.
import { useCallback, useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import { createPortal } from 'react-dom'
import { useNavigate } from 'react-router'
import { AnimatePresence, motion, useMotionValue } from 'motion/react'
import { api, tagsOf, type Collection, type Family, type ListFamiliesArgs, type Until } from './api'
import type { MenuItem } from './menu'
import { spring } from './motion'
import { useApp } from './state'
import { turnsOff } from './temporary'
import { andList, collectionTree, Dialog, errMsg, plural, PromptDialog, Spinner } from './ui'

/** "Select all N results": every family matching a Library query. The engine resolves the query when an action runs. */
export type AllResults = { query: ListFamiliesArgs; count: number }
/** What a bulk action applies to: the selected families, or all results. */
export type Targets = Family[] | AllResults
const isAll = (t: Targets): t is AllResults => !Array.isArray(t)
const editable = (fams: Family[]) => fams.flatMap((f) => f.faces.filter((x) => !x.system).map((x) => x.hash))
const allHashes = (fams: Family[]) => fams.flatMap((f) => f.faces.map((x) => x.hash))
/** Family keys and switchable font hashes of the targets. */
const resolve = (t: Targets) => (isAll(t) ? api.queryTargets(t.query) : Promise.resolve({ keys: t.map((f) => f.key), hashes: editable(t) }))
export const famNames = (t: Targets) => (isAll(t) ? plural(t.count, 'family', 'families') : t.length === 1 ? t[0].family : plural(t.length, 'family', 'families'))
/** Names for a toast after the action: for all results, the number the engine actually resolved. */
const doneNames = (t: Targets, keys: string[]) => (isAll(t) ? plural(keys.length, 'family', 'families') : famNames(t))
/** Why an action is off for all results: it makes no sense for thousands of families at once. */
export const NOT_ALL = 'Not for all results'

/** `onChanged(clear)`: refetch; `clear` = the selection no longer matches (fonts deleted or regrouped). */
export function useFontActions(onChanged: (clear: boolean) => void) {
  const { app, activate, toast, bump, libVersion } = useApp()
  const navigate = useNavigate()
  const [collections, setCollections] = useState<Collection[]>([])
  const [tagging, setTagging] = useState<Targets | null>(null)
  const [renaming, setRenaming] = useState<Family[] | null>(null)
  const [deleting, setDeleting] = useState<Family[] | null>(null)
  const [newColl, setNewColl] = useState<Targets | null>(null)
  const [busy, setBusy] = useState(false)
  useEffect(() => void api.listCollections().then(setCollections, () => {}), [libVersion])
  const bin = app.platform === 'macos' ? 'Trash' : 'Recycle Bin'
  const mod = app.platform === 'macos' ? '⌘' : 'Ctrl+'

  async function setActive(fams: Targets, on: boolean) {
    if (!isAll(fams) && !editable(fams).length) return
    let keys: string[] = []
    const res = await activate(async () => {
      const t = await resolve(fams)
      keys = t.keys
      return t.hashes.length ? api.setActive(t.hashes, on) : []
    })
    if (res?.some((r) => r.ok)) toast(`${on ? 'Turned on' : 'Turned off'} ${doneNames(fams, keys)}`)
    onChanged(false)
  }
  async function setActiveFor(fams: Targets, until: Until) {
    if (!isAll(fams) && !editable(fams).length) return
    let keys: string[] = []
    const res = await activate(async () => {
      const t = await resolve(fams)
      keys = t.keys
      return t.hashes.length ? api.setActiveFor(t.hashes, until) : []
    })
    const when = until === 'restart' ? 'until you restart Typeshelf' : 'for 2 hours'
    // Fonts already on for good stay on for good (the engine skips them), so the toast names no count then.
    const mixed = isAll(fams) || fams.some((f) => f.activeState !== 'none' && !turnsOff(f))
    if (res?.some((r) => r.ok)) toast(mixed ? `Turned on ${when}; fonts that were already on stay on` : `Turned on ${doneNames(fams, keys)} ${when}`)
    onChanged(false)
  }
  async function setPinned(fams: Family[], pinned: boolean) {
    try {
      await api.setPinned(fams.map((f) => f.key), pinned)
      toast(pinned ? `Pinned ${famNames(fams)} to the top` : `Unpinned ${famNames(fams)}`)
      onChanged(false)
    } catch (e) {
      toast(errMsg(e))
    }
  }
  async function addTo(c: { id: number; name: string }, fams: Targets) {
    try {
      const { keys } = await resolve(fams)
      await api.addToCollection(c.id, keys)
      toast(`Added ${doneNames(fams, keys)} to ${c.name}`)
      bump()
    } catch (e) {
      toast(errMsg(e))
    }
  }
  async function createAndAdd(name: string, fams: Targets) {
    try {
      const all = await api.createCollection(name, null)
      const made = all.filter((c) => c.name === name).sort((a, b) => b.id - a.id)[0]
      if (made) await addTo(made, fams)
    } catch (e) {
      toast(errMsg(e))
    }
  }

  const collectionItems = (fams: Targets): MenuItem[] => [
    ...collectionTree(collections).filter(({ c }) => c.builtin !== 'recent')
      .map(({ c, depth }): MenuItem => ({ label: `${' '.repeat(depth)}${c.name}`, onSelect: () => addTo(c, fams) })),
    'sep',
    { label: 'New collection…', onSelect: () => setNewColl(fams) },
  ]

  /** The right-click menu for these families. */
  const menuItems = (targets: Targets): MenuItem[] => {
    // All results: on/off, collections and tags reach every match; the rest stay with a page-sized selection.
    const all = isAll(targets)
    const fams = all ? [] : targets
    const one = fams.length === 1 ? fams[0] : null
    const locked = !all && !editable(fams).length
    return [
      // A temporary font is on but not for good: activating keeps it on (the engine drops its timer).
      { label: fams.some(turnsOff) ? 'Keep on' : 'Activate', hint: `${mod}Shift+A`, disabled: locked || (!all && fams.every((f) => f.activeState === 'all' && !turnsOff(f))), onSelect: () => setActive(targets, true) },
      { label: 'Deactivate', hint: `${mod}Shift+D`, disabled: locked || (!all && fams.every((f) => f.activeState === 'none')), onSelect: () => setActive(targets, false) },
      {
        label: 'Turn on for…', disabled: locked, items: [
          { label: '2 hours', onSelect: () => setActiveFor(targets, { minutes: 120 }) },
          { label: 'Until I restart Typeshelf', onSelect: () => setActiveFor(targets, 'restart') },
        ],
      },
      { label: 'Show in folder', disabled: !fams[0]?.faces[0], onSelect: () => api.showInFolder(fams[0].faces[0].path).catch((e) => toast(errMsg(e))) },
      'sep',
      all
        ? { label: 'Pin to top', disabled: true, hint: NOT_ALL, onSelect: () => {} }
        : fams.every((f) => f.pinned)
          ? { label: 'Unpin', onSelect: () => setPinned(fams, false) }
          : { label: 'Pin to top', onSelect: () => setPinned(fams, true) },
      { label: 'Add to collection', items: collectionItems(targets) },
      { label: 'Tags…', onSelect: () => setTagging(targets) },
      { label: one ? 'Rename family…' : 'Show as one family…', disabled: all, hint: all ? NOT_ALL : undefined, onSelect: () => setRenaming(fams) },
      'sep',
      { label: 'Use as Playground font A', disabled: !one, onSelect: () => navigate(`/playground?a=${encodeURIComponent(fams[0].key)}`) },
      { label: 'Use as Playground font B', disabled: !one, onSelect: () => navigate(`/playground?b=${encodeURIComponent(fams[0].key)}`) },
      'sep',
      { label: 'Delete from disk…', danger: true, disabled: locked || all, hint: all ? NOT_ALL : undefined, onSelect: () => setDeleting(fams) },
    ]
  }

  async function rename(name: string | null) {
    if (!renaming) return
    try {
      await api.setFamilyName(allHashes(renaming), name)
      toast(name ? `Now shown as “${name}”` : 'Back to the font’s own family name')
      setRenaming(null)
      onChanged(true)
    } catch (e) {
      toast(errMsg(e))
    }
  }
  async function remove() {
    if (!deleting) return
    setBusy(true)
    try {
      const res = await api.deleteFonts(editable(deleting))
      toast(`Moved ${plural(res.trashed, 'file')} to the ${bin}${res.failed.length ? ` · ${plural(res.failed.length, 'file')} couldn’t be moved` : ''}`)
      setDeleting(null)
      onChanged(true)
    } catch (e) {
      toast(errMsg(e))
    } finally {
      setBusy(false)
    }
  }

  const delFiles = deleting ? editable(deleting).length : 0
  const dialogs = (
    <>
      <TagsDialog fams={tagging} onClose={() => setTagging(null)} />
      <PromptDialog
        open={!!renaming} onClose={() => setRenaming(null)} onSubmit={rename} confirm="Rename"
        title={renaming && renaming.length > 1 ? `Show ${famNames(renaming)} as one family` : 'Rename family'}
        label="Family name in Typeshelf (the font files don’t change)" initial={renaming?.[0]?.family ?? ''}
      >
        <button type="button" className="btn link" style={{ marginRight: 'auto', paddingLeft: 0 }} onClick={() => rename(null)}>Reset family name</button>
      </PromptDialog>
      <PromptDialog
        open={!!newColl} onClose={() => setNewColl(null)} title="New collection" label="Name" confirm="Create and add"
        onSubmit={(name) => (newColl && createAndAdd(name, newColl), setNewColl(null))}
      />
      <Dialog open={!!deleting} onClose={() => !busy && setDeleting(null)} title={`Delete ${plural(delFiles, 'font file')} from disk?`}>
        <p>
          {andList((deleting ?? []).slice(0, 3).map((f) => f.family))}{(deleting?.length ?? 0) > 3 && ` and ${(deleting?.length ?? 0) - 3} more`}:
          {' '}{delFiles === 1 ? 'the file is' : `all ${plural(delFiles, 'file')} are`} turned off and moved to the {bin}, including copies in other folders.
          Fonts in system font folders aren’t touched.
        </p>
        <div className="dialog-actions">
          <button type="button" className="btn" onClick={() => setDeleting(null)} disabled={busy}>Cancel</button>
          <button type="button" className="btn primary danger-fill" onClick={remove} disabled={busy} aria-busy={busy}>
            {busy && <Spinner />}Move to {bin}
          </button>
        </div>
      </Dialog>
    </>
  )
  return { menuItems, collectionItems, setActive, setPinned, addTo, openTags: setTagging, collections, dialogs }
}

const FONT_FILES = [{ name: 'Fonts', extensions: ['ttf', 'otf', 'ttc', 'otc', 'woff', 'woff2'] }]

/** Adding fonts: copy files/folders into "Imported fonts" (dialogs or drops), or watch a folder where it is. */
export function useAddFonts() {
  const { toast, bump } = useApp()
  const navigate = useNavigate()
  const fail = (e: unknown) => toast(errMsg(e))
  const showPath = (path: string) => ({ label: 'Show', run: () => navigate(`/?path=${encodeURIComponent(path)}`) })

  async function importPaths(paths: string[]) {
    if (!paths.length) return
    try {
      const r = await api.importPaths(paths)
      bump()
      const skipped = r.skipped ? ` · ${plural(r.skipped, 'font')} already in your library` : ''
      if (r.imported) toast(`Imported ${plural(r.imported, 'font')}${skipped}`, showPath(r.folder))
      else toast(`No new fonts to import${skipped}`)
    } catch (e) {
      fail(e)
    }
  }
  return {
    importPaths,
    addFonts: () => api.pickFiles(FONT_FILES).then((paths) => void (paths && importPaths(paths)), fail),
    addFolder: () => api.pickFolder().then((path) => void (path && importPaths([path])), fail),
    watchFolder: async () => {
      try {
        const path = await api.pickFolder()
        if (!path) return
        await api.addFolder(path)
        bump()
        toast(`Watching ${path}`, showPath(path))
      } catch (e) {
        fail(e)
      }
    },
  }
}

/**
 * Tags for one or more families. Tags only some of them have show as mixed and stay as they are unless clicked.
 * For all results the dialog doesn't load every family: each tag starts as "leave as is" and a click adds or removes it.
 * Saving is one engine call (`edit_tags`) either way.
 */
function TagsDialog({ fams, onClose }: { fams: Targets | null; onClose: () => void }) {
  const { toast, bump } = useApp()
  const [cur, setCur] = useState<Map<string, string[]> | null>(null)
  const [names, setNames] = useState<string[]>([])
  /** true = every family gets it, false = none, null = leave as is (mixed). */
  const [want, setWant] = useState<Record<string, boolean | null>>({})
  const [draft, setDraft] = useState('')
  const [busy, setBusy] = useState(false)
  const list = fams && !isAll(fams) ? fams : null
  useEffect(() => {
    if (!fams) return
    setCur(null)
    setDraft('')
    Promise.all([list ? tagsOf(list.map((f) => f.key)) : new Map<string, string[]>(), api.listTags()]).then(([m, all]) => {
      setCur(m)
      setNames(all.map((t) => t.name))
      setWant(Object.fromEntries(all.map((t) => {
        if (!list) return [t.name, null]
        const has = list.filter((f) => m.get(f.key)?.includes(t.name)).length
        return [t.name, has === list.length ? true : has === 0 ? false : null]
      })))
    }, (e) => (toast(errMsg(e)), onClose()))
  }, [fams]) // eslint-disable-line react-hooks/exhaustive-deps

  const add = () => {
    const t = draft.trim()
    if (!t) return
    if (!names.includes(t)) setNames((ns) => [...ns, t])
    setWant((w) => ({ ...w, [t]: true }))
    setDraft('')
  }
  async function save() {
    if (!fams || !cur) return
    setBusy(true)
    try {
      const { keys } = await resolve(fams)
      const changed = await api.editTags(keys, names.filter((t) => want[t] === true), names.filter((t) => want[t] === false))
      toast(changed ? `Updated tags on ${changed === 1 && list?.length === 1 ? list[0].family : plural(changed, 'family', 'families')}` : 'No tags changed')
      bump()
      onClose()
    } catch (e) {
      toast(errMsg(e))
    } finally {
      setBusy(false)
    }
  }

  const count = (t: string) => list?.filter((f) => cur?.get(f.key)?.includes(t)).length ?? 0
  return (
    <Dialog open={!!fams} onClose={onClose} title={fams ? `Tags for ${famNames(fams)}` : 'Tags'}>
      {!cur ? (
        <p className="muted" style={{ display: 'flex', gap: 8, alignItems: 'center' }}><Spinner /> Loading tags…</p>
      ) : (
        <>
          {names.length > 0 ? (
            <div className="check-list" role="group" aria-label="Tags">
              {names.map((t) => (
                <label key={t} className="check-row">
                  <input
                    type="checkbox" checked={want[t] === true} ref={(el) => void (el && (el.indeterminate = want[t] === null))}
                    // All results: add → remove → leave as is. Otherwise on / off (a mixed tag can't go back to mixed).
                    onChange={() => setWant((w) => ({ ...w, [t]: list ? w[t] !== true : w[t] === null ? true : w[t] ? false : null }))}
                  />
                  <span style={{ flex: 1 }}>{t}</span>
                  {want[t] === null && <span className="faint" style={{ fontSize: 12 }}>{list ? `${count(t)} of ${list.length}` : 'Left as is'}</span>}
                  {!list && want[t] !== null && <span className="faint" style={{ fontSize: 12 }}>{want[t] ? 'Add to all' : 'Remove from all'}</span>}
                </label>
              ))}
            </div>
          ) : <p className="muted">No tags yet. Add the first one below.</p>}
          <form style={{ display: 'flex', gap: 8 }} onSubmit={(e) => (e.preventDefault(), add())}>
            <label htmlFor="new-tag" className="sr-only">New tag</label>
            <input id="new-tag" className="input" style={{ flex: 1 }} placeholder="New tag" value={draft} onChange={(e) => setDraft(e.target.value)} autoFocus />
            <button type="submit" className="btn" disabled={!draft.trim()}>Add</button>
          </form>
        </>
      )}
      <div className="dialog-actions">
        <button type="button" className="btn" onClick={onClose}>Cancel</button>
        <button type="button" className="btn primary" onClick={save} disabled={!cur || busy}>Save tags</button>
      </div>
    </Dialog>
  )
}

/**
 * Drag families onto a sidebar collection (`[data-drop-collection]`). Pointer events rather than HTML5 drag and drop:
 * Tauri's native file drop (used for importing) disables HTML5 drag and drop inside the window on Windows.
 * With `onPair`, one family dropped on another family's row (`[data-drop-family]`) calls it with (target key, dragged).
 */
export function useFamilyDrag(onDrop: (collection: { id: number; name: string }, fams: Family[]) => void, onPair?: (targetKey: string, dragged: Family) => void) {
  const [label, setLabel] = useState<string | null>(null)
  const x = useMotionValue(0), y = useMotionValue(0)
  const cb = useRef({ onDrop, onPair })
  cb.current = { onDrop, onPair }

  const start = useCallback((e: ReactPointerEvent, getFams: () => Family[]) => {
    if (e.button !== 0 || e.pointerType === 'touch' || (e.target as Element).closest('button, input, select, textarea, label')) return
    const sx = e.clientX, sy = e.clientY
    let fams: Family[] | null = null
    let over: HTMLElement | null = null
    const setOver = (el: HTMLElement | null) => {
      if (el === over) return
      over?.classList.remove('drop-over')
      el?.classList.add('drop-over')
      over = el
    }
    const move = (ev: PointerEvent) => {
      if (!fams) {
        if (Math.hypot(ev.clientX - sx, ev.clientY - sy) < 6) return
        fams = getFams()
        setLabel(fams.length === 1 ? fams[0].family : `${fams.length} families`)
        document.body.classList.add('dragging-fonts')
        getSelection()?.removeAllRanges()
      }
      x.set(ev.clientX + 14)
      y.set(ev.clientY + 10)
      const el = document.elementFromPoint(ev.clientX, ev.clientY)
      const row = cb.current.onPair && fams.length === 1 ? el?.closest<HTMLElement>('[data-drop-family]') : null
      setOver(el?.closest<HTMLElement>('[data-drop-collection]') ?? (row && row.dataset.dropFamily !== fams[0].key ? row : null))
    }
    const end = (ev: Event) => {
      removeEventListener('pointermove', move)
      removeEventListener('pointerup', end)
      removeEventListener('pointercancel', end)
      removeEventListener('keydown', esc, true)
      if (!fams) return
      const target = ev.type === 'pointerup' ? over : null
      setOver(null)
      setLabel(null)
      document.body.classList.remove('dragging-fonts')
      // The pointerup can still become a click on the row or its link; swallow that one.
      const stop = (c: Event) => (c.preventDefault(), c.stopPropagation())
      addEventListener('click', stop, true)
      setTimeout(() => removeEventListener('click', stop, true), 0)
      if (target?.dataset.dropFamily) cb.current.onPair?.(target.dataset.dropFamily, fams[0])
      else if (target) cb.current.onDrop({ id: Number(target.dataset.dropCollection), name: target.dataset.dropName ?? '' }, fams)
    }
    const esc = (k: KeyboardEvent) => {
      if (k.key !== 'Escape' || !fams) return
      k.stopPropagation()
      end(k)
    }
    addEventListener('pointermove', move)
    addEventListener('pointerup', end)
    addEventListener('pointercancel', end)
    addEventListener('keydown', esc, true)
  }, [x, y])

  const ghost = createPortal(
    <AnimatePresence>
      {label && (
        <motion.div
          className="drag-ghost" style={{ x, y }} aria-hidden="true"
          initial={{ opacity: 0, scale: 0.8 }} animate={{ opacity: 1, scale: 1 }} exit={{ opacity: 0, scale: 0.8 }} transition={spring.pop}
        >
          {label}
        </motion.div>
      )}
    </AnimatePresence>,
    document.body,
  )
  return { start, ghost }
}
