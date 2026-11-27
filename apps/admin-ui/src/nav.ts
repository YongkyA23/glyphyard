// Navigation list and keyboard-shortcut helpers shared by the sidebar and the command palette.
// Labels are marked with tk(); render them with t().
import { tk } from './i18n'

export const OPEN_PALETTE = 'open-palette'
export const isMac = typeof navigator !== 'undefined' && /mac|iphone|ipad/i.test(navigator.platform || navigator.userAgent)

export const NAV: [to: string, label: string][] = [
  ['/', tk('Overview')], ['/users', tk('Users')], ['/names', tk('Name history')], ['/library', tk('Shared library')],
  ['/organize', tk('Auto-organize')], ['/requests', tk('Font requests')], ['/settings', tk('Settings')], ['/audit', tk('Audit log')],
]

/** True when a key press belongs to a text control or an open dialog, so global shortcuts stay out of the way. */
export function isTyping(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false
  return t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName) || !!t.closest('dialog[open]')
}
