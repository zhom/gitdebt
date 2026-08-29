/**
 * Motion, as a pen moves.
 *
 * One gesture governs this site: the drawing draws itself. A trace inks in
 * along its own length, a dimension line extends from its datum, and an arrow
 * terminator lands with a half-pixel overshoot. Everything else is a colour
 * step measured in a single frame budget.
 *
 * The rule that outranks all of these values: **nothing on this site is
 * invisible until an animation runs.** Content is in the markup and painted at
 * first paint; the motion happens over it. There is no `opacity: 0` initial
 * state anywhere, so a throttled tab, a stalled hydration, a screenshot pass,
 * or a browser with no animation support still renders a complete page.
 */

/** A pen accelerating off a datum and decelerating onto its terminator. */
export const EASE_DRAW = [0.65, 0, 0.35, 1] as const;

/** The landing: fast in, settling with a slight overshoot. */
export const EASE_LAND = [0.16, 1, 0.3, 1] as const;

export const DURATION = {
  /** A full trace inking in across the sheet. */
  draw: 0.9,
  /** A dimension line extending to its terminator. */
  measure: 0.26,
  /** An arrow terminator arriving. */
  land: 0.13,
  /** Any ordinary state change: colour, ground, position. */
  ui: 0.14,
} as const;

/** Reduced motion keeps the mark and removes the travel. */
export const REDUCED_DURATION = 0.001;

/**
 * Physical settle for a control the pointer is driving directly — a value
 * following the cursor along a curve, a handle being dragged. Springs belong
 * to things a hand is moving; timed easing belongs to things the page draws.
 */
export const SPRING = {
  follow: { type: "spring", stiffness: 520, damping: 40, mass: 0.6 },
} as const;

/**
 * The dash-offset pair that makes an SVG path draw itself along its own
 * length. Pass the path's measured length; the caller reads it once from
 * `getTotalLength()` and never guesses it, because a wrong length is exactly
 * how a stroke animation ends up filling only part of its track.
 */
export function drawPath(length: number, delay = 0) {
  return {
    strokeDasharray: length,
    strokeDashoffset: length,
    animate: { strokeDashoffset: 0 },
    transition: { duration: DURATION.draw, ease: EASE_DRAW, delay },
  } as const;
}
