// Shared motion presets. Transform/opacity only; springs for anything that moves in space.
// Reduced motion: <MotionConfig reducedMotion="user"> in main.tsx drops transforms, app.css kills CSS animations.
import type { Transition, Variants } from 'motion/react'

export const easeOut = [0.22, 1, 0.36, 1] as const
export const fast: Transition = { duration: 0.14, ease: easeOut }
export const base: Transition = { duration: 0.24, ease: easeOut }
/** Snappy spring for things that travel (pills, toggles, buttons popping in). */
export const spring: Transition = { type: 'spring', stiffness: 520, damping: 36, mass: 0.8 }
/** Softer spring for larger surfaces (dialogs, cards). */
export const softSpring: Transition = { type: 'spring', stiffness: 380, damping: 32 }

/** Parent that staggers its variant children. Children inherit "hidden" → "show". */
export const stagger = (step = 0.04, delay = 0): Variants => ({
  hidden: {}, show: { transition: { staggerChildren: step, delayChildren: delay } },
})

/** The default entrance: fade + rise. */
export const fadeRise: Variants = {
  hidden: { opacity: 0, y: 10 },
  // staggerChildren only affects nested variant children (e.g. rows inside a card).
  show: { opacity: 1, y: 0, transition: { ...base, staggerChildren: 0.04, delayChildren: 0.06 } },
  exit: { opacity: 0, transition: fast },
}

// ponytail: stagger capped at 12 rows so long tables don't take seconds to appear.
const ROW_STAGGER_CAP = 12
const LAYOUT_ROW_LIMIT = 150
const rowDelay = (i: number) => Math.min(i, ROW_STAGGER_CAP) * 0.03

/** For lists where new items arrive at the top (activity feeds, timelines). */
export const dropIn: Variants = {
  hidden: { opacity: 0, y: -10 },
  show: (i = 0) => ({ opacity: 1, y: 0, transition: { ...spring, delay: rowDelay(i) } }),
  exit: { opacity: 0, transition: fast },
}

/** Table rows and list rows. */
export const rowIn: Variants = {
  hidden: { opacity: 0, y: 6 },
  show: (i = 0) => ({ opacity: 1, y: 0, transition: { ...base, delay: rowDelay(i) } }),
  exit: { opacity: 0, transition: fast },
}

/**
 * Props for item `i` of a keyed list inside <AnimatePresence>: cascades in, fades out, and slides into place
 * (layout skipped above LAYOUT_ROW_LIMIT items). Items set their own initial/animate because rows that mount
 * after the parent's first render (filtering, polling) don't inherit the parent's "hidden" start.
 */
export const row = (i: number, count: number, variants = rowIn) => ({
  variants, custom: i, initial: 'hidden', animate: 'show', exit: 'exit',
  layout: count <= LAYOUT_ROW_LIMIT ? ('position' as const) : false,
})

/** Button-ish hover/press for motion elements (CSS :hover/:active transforms can't beat motion's inline transform). */
export const press = { whileHover: { y: -1 }, whileTap: { scale: 0.97 } }
/** Card hover lift for motion elements. */
export const lift = { whileHover: { y: -2 }, whileTap: { scale: 0.99 } }

/** Pop in/out for elements swapped by state (merge button, success badges). */
export const pop = {
  initial: { opacity: 0, scale: 0.92 },
  animate: { opacity: 1, scale: 1, transition: spring },
  exit: { opacity: 0, scale: 0.96, transition: fast },
}

/** Light horizontal shake for errors: animate={{ x: shakeX }} with shakeOpts. */
export const shakeX = [0, -6, 6, -4, 4, -2, 0]
export const shakeOpts: Transition = { duration: 0.32, ease: 'easeOut' }

/** Page transition used by the route outlet. */
export const page = {
  initial: { opacity: 0, y: 8 },
  animate: { opacity: 1, y: 0, transition: base },
  exit: { opacity: 0, transition: { duration: 0.1 } },
}
