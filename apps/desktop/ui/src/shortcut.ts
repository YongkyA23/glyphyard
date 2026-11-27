// Turns a key press into the shell's quick-activate shortcut string: the global-shortcut plug-in's format,
// modifiers then one key joined by "+" (e.g. "CmdOrCtrl+Alt+Space"). CmdOrCtrl is Ctrl on Windows and Cmd on macOS.
import { t } from './i18n'
import { formatShortcut } from './ui'

type Platform = 'windows' | 'macos'
type KeyPress = Pick<KeyboardEvent, 'key' | 'code' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey'>

const MODIFIER_KEYS = ['Control', 'Alt', 'AltGraph', 'Shift', 'Meta', 'OS']
const NAMED_KEYS = [
  'Space', 'Enter', 'Backspace', 'Delete', 'Insert', 'Home', 'End', 'PageUp', 'PageDown', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight',
  'Minus', 'Equal', 'BracketLeft', 'BracketRight', 'Backslash', 'Semicolon', 'Quote', 'Backquote', 'Comma', 'Period', 'Slash',
]
// Taken by the system even with two modifiers. ponytail: the common ones only; anything else the OS refuses
// shows up as a failed registration, and the old shortcut stays.
const RESERVED: Record<Platform, string[]> = {
  windows: ['CmdOrCtrl+Alt+Delete'],
  macos: ['CmdOrCtrl+Shift+3', 'CmdOrCtrl+Shift+4', 'CmdOrCtrl+Shift+5', 'CmdOrCtrl+Shift+Q', 'CmdOrCtrl+Ctrl+Q', 'CmdOrCtrl+Ctrl+F', 'CmdOrCtrl+Ctrl+Space', 'CmdOrCtrl+Alt+D'],
}

function keyName(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3)
  if (/^Digit\d$/.test(code)) return code.slice(5)
  if (/^F([1-9]|1\d|2[0-4])$/.test(code)) return code
  return NAMED_KEYS.includes(code) ? code : null
}

/** null while only modifiers are held; otherwise the shortcut, or a sentence saying why it can't be one. */
export function shortcutFromKey(e: KeyPress, platform: Platform): { shortcut: string } | { error: string } | null {
  if (MODIFIER_KEYS.includes(e.key)) return null
  const mac = platform === 'macos'
  // `code` names the physical key (layout-independent); some synthetic or remote-desktop events leave it empty.
  const code = e.code && e.code !== 'Unidentified' ? e.code : /^[a-z]$/i.test(e.key) ? `Key${e.key.toUpperCase()}` : /^\d$/.test(e.key) ? `Digit${e.key}` : e.key === ' ' ? 'Space' : e.key
  const key = keyName(code)
  if (!key) return { error: t('{key} can’t be used in a shortcut. Use a letter, a number, a function key or Space.', { key: e.key === ' ' ? 'Space' : e.key }) }
  if (!mac && e.metaKey) return { error: t('Windows keeps shortcuts with the Windows key for itself. Use Ctrl, Alt or Shift.') }
  const mods = [
    ...((mac ? e.metaKey : e.ctrlKey) ? ['CmdOrCtrl'] : []),
    ...(mac && e.ctrlKey ? ['Ctrl'] : []),
    ...(e.altKey ? ['Alt'] : []),
    ...(e.shiftKey ? ['Shift'] : []),
  ]
  // One modifier is not enough: Ctrl/Cmd + key belongs to the app in front, Alt/Option + key types characters or
  // opens menus, and Shift + key types a capital.
  if (mods.length < 2)
    return { error: t('Hold two modifier keys with it, for example {example}.', { example: `${formatShortcut('CmdOrCtrl+Alt', platform)} + ${key}` }) }
  const shortcut = [...mods, key].join('+')
  if (RESERVED[platform].includes(shortcut)) return { error: t('{shortcut} is used by {os}. Pick another combination.', { shortcut: formatShortcut(shortcut, platform), os: mac ? 'macOS' : 'Windows' }) }
  return { shortcut }
}
