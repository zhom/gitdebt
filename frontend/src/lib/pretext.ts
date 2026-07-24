/**
 * Client-only text metrics, powered by `@chenglou/pretext`.
 *
 * pretext measures and wraps text with the browser's own font engine as
 * ground truth, entirely off the DOM — no `getBoundingClientRect`, no reflow.
 * That buys two things this app uses:
 *   1. seamless, constant-speed marquees (measure N items, zero layout thrash)
 *   2. pixel-balanced, zero-CLS headings (reserve the exact wrapped height)
 *
 * SSR safety: pretext only touches a canvas the first time a measurement runs
 * (`getMeasureContext()` is lazy), so importing this module is safe during
 * Astro's static build. Every function here still assumes a browser — call
 * them from effects, never from `.astro` frontmatter or a module top level.
 * `isBrowser()` guards the paranoid paths.
 */

import {
  measureLineStats,
  measureNaturalWidth,
  prepareWithSegments,
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
