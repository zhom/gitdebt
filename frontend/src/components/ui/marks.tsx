import type { SVGProps } from "react";

/**
 * The house marks.
 *
 * Every glyph here is drawn from the drawing's own vocabulary: a terminator, a
 * leader, an extension tick, a section cut. They are not an icon pack's shapes
 * redrawn at a different stroke weight — a redrawn document-with-a-checkmark is
 * still the generic outline set. These are marks a draughtsman already uses,
 * and there are only five because a product this size needs five.
 *
 * All of them are monoline at 1.5, square-ended where a drafting mark is
 * square-ended and round where it is round, and all of them inherit
 * `currentColor` so they take the ink of whatever they sit in.
 */

type MarkProps = SVGProps<SVGSVGElement> & { size?: number };

function Mark({ size = 16, children, ...props }: MarkProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
      {...props}
    >
      {children}
    </svg>
  );
}

/**
 * The leader arrow: up and to the right, because this action leaves the
 * surface it sits on. The horizontal right-arrow is the stock component's
 * arrow and says only "next"; a leader says "out to there".
 */
export function Leader(props: MarkProps) {
  return (
    <Mark {...props}>
      <path d="M4.5 11.5 11.5 4.5" />
      <path d="M6 4.5h5.5V10" />
    </Mark>
  );
}

/**
 * A dimension terminator: the filled arrowhead that lands on a datum, with its
 * extension tick. Used wherever the interface points at a measured thing.
 */
export function Terminator({ size = 16, ...props }: MarkProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      aria-hidden="true"
      focusable="false"
      {...props}
    >
      <path d="M13 3v10" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      <path d="M2 8h9" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      <path d="M13 8 8.5 5.4v5.2z" fill="currentColor" />
    </svg>
  );
}

/**
 * The sheet index: three rules of unequal length, the way a title block stacks
 * its fields. It is the menu control, and it is not three identical bars.
 */
export function Index(props: MarkProps) {
  return (
    <Mark {...props}>
      <path d="M2.5 4.5h11" />
      <path d="M2.5 8h7.5" />
      <path d="M2.5 11.5h11" />
    </Mark>
  );
}

/**
 * The section cut: the mark a drawing puts where something is removed. It
 * closes things.
 */
export function Cut(props: MarkProps) {
  return (
    <Mark {...props}>
      <path d="M3.5 3.5 12.5 12.5" />
      <path d="M12.5 3.5 3.5 12.5" />
    </Mark>
  );
}

/**
 * The check: a draughtsman's tick, struck at the angle a hand strikes it
 * rather than as a symmetrical V.
 */
export function Tick(props: MarkProps) {
  return (
    <Mark {...props}>
      <path d="M3 8.5 6.5 12 13 4.5" />
    </Mark>
  );
}
