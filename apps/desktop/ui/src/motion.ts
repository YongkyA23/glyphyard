// Shared motion presets. CSS mirrors the durations/easing as --dur-* / --ease-* in app.css.
// <MotionConfig reducedMotion="user"> (main.tsx) turns transform/layout animations into instant jumps for
// people who ask for reduced motion; opacity fades stay, they don't move anything.
import type { CSSProperties } from 'react'
import type { Transition } from 'motion/react'

export const dur = { fast: 0.12, base: 0.2, slow: 0.32, count: 0.6 }
export const easeOut = [0.22, 1, 0.36, 1] as const

export const spring = {
  /** Glides: nav highlight, tab underline, selection rings, layout reflow. */
  glide: { type: 'spring', stiffness: 520, damping: 42, mass: 0.9 },
  /** Small things that should feel springy: switch knobs, chips, toasts. */
  pop: { type: 'spring', stiffness: 640, damping: 30 },
  /** Panels: dialogs, popups. */
  panel: { type: 'spring', stiffness: 420, damping: 32 },
} satisfies Record<string, Transition>

/** Stagger only the first N items; the rest render instantly. */
export const STAGGER_CAP = 20
export const staggerDelay = (i: number, step = 0.03) => (i < STAGGER_CAP ? i * step : 0)
/** For CSS `.enter` stagger (delay = min(--i, 20) * 30ms). */
export const cssStagger = (i: number) => ({ '--i': i }) as CSSProperties

/** Fade + rise for content entering (route changes, list rows, waterfall lines). */
export const rise = {
  initial: { opacity: 0, y: 8 },
  animate: { opacity: 1, y: 0 },
  exit: { opacity: 0, y: -4, transition: { duration: dur.fast } },
}

/** Fade + slight scale for panels (dialogs, quick window, toasts). */
export const pop = {
  initial: { opacity: 0, scale: 0.96, y: 6 },
  animate: { opacity: 1, scale: 1, y: 0 },
  exit: { opacity: 0, scale: 0.97, y: 4, transition: { duration: dur.fast } },
}

/** Route cross-fade for the main content. */
export const route = {
  initial: { opacity: 0, y: 8, filter: 'blur(4px)' },
  // filter: none afterwards so the page doesn't keep a containing block/stacking context.
  animate: { opacity: 1, y: 0, filter: 'blur(0px)', transitionEnd: { filter: 'none' }, transition: { duration: dur.base, ease: easeOut } },
  exit: { opacity: 0, filter: 'blur(2px)', transition: { duration: dur.fast } },
}

/** Fade-rise once when scrolled into view (Settings sections). */
export const inView = {
  initial: { opacity: 0, y: 12 },
  whileInView: { opacity: 1, y: 0 },
  viewport: { once: true, amount: 0.15 },
  transition: { duration: dur.slow, ease: easeOut },
}

/** Height collapse for banners/notes that appear and get dismissed. */
export const collapse = {
  initial: { opacity: 0, height: 0 },
  animate: { opacity: 1, height: 'auto' },
  exit: { opacity: 0, height: 0 },
  transition: { duration: dur.base, ease: easeOut },
}

/** Light horizontal shake for validation errors. */
export const shake = { x: [0, -6, 6, -4, 4, 0], transition: { duration: 0.36 } }
