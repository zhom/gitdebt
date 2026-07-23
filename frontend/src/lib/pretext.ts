/**
 * Client-only text metrics, powered by `@chenglou/pretext`.
 *
 * pretext measures and wraps text with the browser's own font engine as
 * ground truth, entirely off the DOM — no `getBoundingClientRect`, no reflow.
 * That buys three things this app uses:
 *   1. seamless, constant-speed marquees (measure N items, zero layout thrash)
 *   2. pixel-balanced, zero-CLS headings (reserve the exact wrapped height)
 *   3. text that flows around an arbitrary data curve (variable width per line)
 *
 * SSR safety: pretext only touches a canvas the first time a measurement runs
 * (`getMeasureContext()` is lazy), so importing this module is safe during
 * Astro's static build. Every function here still assumes a browser — call
 * them from effects, never from `.astro` frontmatter or a module top level.
 * `isBrowser()` guards the paranoid paths.
 */

import {
  layoutNextLineRange,
  materializeLineRange,
  measureLineStats,
  measureNaturalWidth,
  prepareWithSegments,
  type LayoutCursor,
} from "@chenglou/pretext";

export const isBrowser = (): boolean =>
  typeof window !== "undefined" && typeof document !== "undefined";

/** The canvas-font inputs that decide how a specific element's text measures. */
export type TextMetrics = {
  /** Canvas `ctx.font` shorthand: `"<style> <weight> <size>px <family>"`. */
  font: string;
  /** Resolved `line-height` in px (CSS `normal` → 1.2·size). */
  lineHeight: number;
  /** Resolved `letter-spacing` in px (CSS `normal` → 0). */
  letterSpacing: number;
  /** Font size in px, kept for callers that reserve height themselves. */
  fontSize: number;
};

/**
 * Read the canvas-font inputs off an element's *computed* style, so a
 * measurement always matches what that element actually renders — no
 * hard-coded font stacks that could drift from the CSS. Canvas `measureText`
 * honours style/weight/size/family; `letter-spacing` is fed to pretext
 * separately because canvas does not apply it.
 */
export function metricsFor(el: Element): TextMetrics {
  const cs = getComputedStyle(el);
  const fontSize = parseFloat(cs.fontSize) || 16;
  const lineHeight =
    cs.lineHeight === "normal"
      ? fontSize * 1.2
      : parseFloat(cs.lineHeight) || fontSize * 1.2;
  const letterSpacing =
    cs.letterSpacing === "normal" ? 0 : parseFloat(cs.letterSpacing) || 0;
  const weight = cs.fontWeight || "400";
  const style = cs.fontStyle && cs.fontStyle !== "normal" ? cs.fontStyle : "";
  const font = `${style} ${weight} ${fontSize}px ${cs.fontFamily}`.trim();
  return { font, lineHeight, letterSpacing, fontSize };
}

/** Natural (unwrapped) width of a single-line string, in px. */
export function naturalWidth(
  text: string,
  font: string,
  letterSpacing = 0,
): number {
  if (!text) return 0;
  return measureNaturalWidth(prepareWithSegments(text, font, { letterSpacing }));
}

/** How many lines `text` wraps to at `maxWidth`, and its widest line. */
export function lineStatsAt(
  text: string,
  font: string,
  maxWidth: number,
  letterSpacing = 0,
): { lineCount: number; maxLineWidth: number } {
  return measureLineStats(
    prepareWithSegments(text, font, { letterSpacing }),
    Math.max(1, maxWidth),
  );
}

/**
 * The tightest `max-width` (px) that keeps `text` on the same number of lines
 * it takes at `maxWidth` — CSS `text-wrap: balance` with pixel control and no
 * dependency on browser support. Also returns the reserved height so a caller
 * can pin it and never shift when async text swaps in.
 */
export function balancedLayout(
  text: string,
  font: string,
  maxWidth: number,
  lineHeight: number,
  letterSpacing = 0,
): { width: number; lineCount: number; height: number } {
  const cap = Math.max(1, Math.floor(maxWidth));
  const prepared = prepareWithSegments(text, font, { letterSpacing });
  const target = measureLineStats(prepared, cap).lineCount;
  if (target <= 1) {
    const w = Math.min(cap, Math.ceil(measureNaturalWidth(prepared)));
    return { width: w, lineCount: target, height: target * lineHeight };
  }
  // Smallest width in (cap/target, cap] that still fits in `target` lines.
  let lo = Math.max(1, Math.floor(cap / target)); // fewer lines impossible below here
  let hi = cap;
  for (let i = 0; i < 22 && hi - lo > 1; i++) {
    const mid = (lo + hi) / 2;
    if (measureLineStats(prepared, mid).lineCount <= target) hi = mid;
    else lo = mid;
  }
  const width = Math.ceil(hi);
  return { width, lineCount: target, height: target * lineHeight };
}

/** One laid-out line produced by {@link flowAroundBoundary}. */
export type FlowedLine = {
  text: string;
  /** Baseline-independent top offset of the line box, in px. */
  y: number;
  /** Left inset applied to this line, in px. */
  x: number;
  /** Measured width of the line, in px. */
  width: number;
};

/**
 * Flow a paragraph into a region whose usable width changes with vertical
 * position — e.g. the empty space above a rising data curve. `widthAt(yBottom)`
 * returns the px width available to a line whose box bottom sits at `yBottom`;
 * return `0` to skip a band entirely.
 *
 * This is pretext's variable-width path (`layoutNextLineRange`): each line is
 * routed at its own width without ever building intermediate strings for lines
 * we discard, and without a single DOM measurement.
 */
export function flowAroundBoundary(
  text: string,
  font: string,
  opts: {
    top: number;
    bottom: number;
    lineHeight: number;
    letterSpacing?: number;
    left?: number;
    minWidth?: number;
    /** px width available to a line ending at `yBottom`. */
    widthAt: (yBottom: number) => number;
  },
): FlowedLine[] {
  const {
    top,
    bottom,
    lineHeight,
    letterSpacing = 0,
    left = 0,
    minWidth = 24,
    widthAt,
  } = opts;
  const prepared = prepareWithSegments(text, font, { letterSpacing });
  const lines: FlowedLine[] = [];
  let cursor: LayoutCursor = { segmentIndex: 0, graphemeIndex: 0 };
  let y = top;
  // A generous cap: never loop past the region even if `widthAt` misbehaves.
  const maxLines = Math.ceil((bottom - top) / lineHeight) + 2;
  for (let i = 0; i < maxLines; i++) {
    if (y + lineHeight > bottom) break;
    const avail = Math.max(0, widthAt(y + lineHeight) - left);
    if (avail < minWidth) {
      // Too narrow here; drop down a row and try again (flows around a bulge).
      y += lineHeight;
      continue;
    }
    const range = layoutNextLineRange(prepared, cursor, avail);
    if (range === null) break;
    const line = materializeLineRange(prepared, range);
    lines.push({ text: line.text, y, x: left, width: line.width });
    cursor = range.end;
    y += lineHeight;
  }
  return lines;
}
