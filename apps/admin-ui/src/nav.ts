// Navigation list and keyboard-shortcut helpers shared by the sidebar and the command palette.
export const OPEN_PALETTE = 'open-palette'
export const isMac = typeof navigator !== 'undefined' && /mac|iphone|ipad/i.test(navigator.platform || navigator.userAgent)

export const NAV: [to: string, label: string][] = [
  ['/', 'Overview'], ['/users', 'Users'], ['/names', 'Name history'], ['/library', 'Shared library'],
  ['/organize', 'Auto-organize'], ['/requests', 'Font requests'], ['/settings', 'Settings'], ['/audit', 'Audit log'],
]

/** True when a key press belongs to a text control or an open dialog, so global shortcuts stay out of the way. */
export function isTyping(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false
  return t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName) || !!t.closest('dialog[open]')
}
