"use client";

import { useEffect, useRef } from "react";

import { BRAND } from "@/lib/dither";
import { useDitherSurface } from "@/components/ui/dither-surface";

/**
 * Session-scoped decision, shared by every ticker on the page.
 *
 * The count-up is an entrance flourish, so it belongs to a genuine arrival: a
 * hard load or the first page of a session. Replaying it on every internal
 * navigation would animate the same unchanged number at someone reading, which
 * reads as a glitch rather than as life.
 */
const SESSION_FLAG = "gitdebt:navStarTicker";
const WINDOW_KEY = "__gitdebtNavStarTicker";

function shouldAnimateOnce(): boolean {
  if (typeof window === "undefined") return false;
  const scope = window as Window & { [WINDOW_KEY]?: boolean };
  if (scope[WINDOW_KEY] !== undefined) return scope[WINDOW_KEY];
  try {
    const seen = sessionStorage.getItem(SESSION_FLAG);
    const entry = performance.getEntriesByType("navigation")[0] as
      | PerformanceNavigationTiming
      | undefined;
    scope[WINDOW_KEY] = (entry?.type ?? "navigate") === "reload" || !seen;
    sessionStorage.setItem(SESSION_FLAG, "1");
  } catch {
    // Private-mode sessionStorage throws on write. A flourish is not worth a
    // failed render, and not animating is the safe side of that choice.
    scope[WINDOW_KEY] = false;
  }
  return scope[WINDOW_KEY];
}

const format = (value: number) => new Intl.NumberFormat("en-US").format(Math.round(value));

/** "1 star", not "1 stars" — the badge shows a real count, including one. */
const starLabel = (value: number) =>
  `gitdebt on GitHub \u2014 ${format(value)} star${Math.round(value) === 1 ? "" : "s"}`;

/**
 * `cubic-bezier(0.16, 1, 0.3, 1)` evaluated directly. The curve lands almost
 * all of its distance early, so the number is legible for most of the run
 * instead of blurring to the last frame.
 */
function easeOut(p: number): number {
  if (p <= 0) return 0;
  if (p >= 1) return 1;
  return 1 - Math.pow(1 - p, 3);
}

const DURATION_MS = 1100;

export type NavStarCountProps = {
  /**
   * Resolved at build time, so the served HTML already carries the number.
   * `null` when the build could not reach the API: the badge then ships
   * hidden and the live refresh below is what reveals it.
   */
  stars: number | null;
  href: string;
  /** Origin of gitdebt's own API, for the post-mount refresh. */
  apiBase: string;
  /** `owner/repo` to count. */
  repo: string;
};

/**
 * gitdebt's own star count, in the nav, on the product's dithered surface.
 *
 * The number is server-rendered at its final value and the animation only ever
 * writes through a ref, so the markup a crawler (or a reader with JS disabled)
 * receives is the true count rather than a `0` waiting to be corrected. Width
 * is reserved in `ch` against `tabular-nums`, so counting up cannot reflow the
 * header around it.
 */
export function NavStarCount({ stars, href, apiBase, repo }: NavStarCountProps) {
  const ref = useRef<HTMLSpanElement>(null);
  const hostRef = useRef<HTMLAnchorElement>(null);
  // The value currently painted, so a refresh counts up from what the reader
  // is already looking at rather than restarting from zero.
  const shown = useRef<number | null>(stars);
  const { surface, handlers } = useDitherSurface({
    fill: BRAND,
    variant: "gradient",
    alpha: 0.32,
    edge: 0.45,
    animated: true,
    pulse: true,
  });

  useEffect(() => {
    const node = ref.current;
    const host = hostRef.current;
    if (!node || !host) return;

    let raf = 0;
    let cancelled = false;

    const paint = (to: number, from: number | null) => {
      const label = starLabel(to);
      host.setAttribute("aria-label", label);
      host.setAttribute("title", label);
      // Reserve the final width before the first frame, so counting cannot
      // reflow the header around the badge.
      node.style.minWidth = `${format(to).length}ch`;
      host.hidden = false;
      shown.current = to;

      const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
      const start = from ?? 0;
      if (reduced || start === to || (from !== null && !shouldAnimateOnce())) {
        node.textContent = format(to);
        return;
      }
      let t0: number | undefined;
      const step = (now: number) => {
        if (cancelled) return;
        t0 ??= now;
        const p = Math.min((now - t0) / DURATION_MS, 1);
        node.textContent = format(start + (to - start) * easeOut(p));
        if (p < 1) raf = requestAnimationFrame(step);
      };
      raf = requestAnimationFrame(step);
    };

    if (stars !== null) paint(stars, shouldAnimateOnce() ? 0 : stars);

    // A static build bakes the count at deploy time. Re-reading it here is
    // what keeps the badge honest between releases, and it is also the only
    // thing that can reveal the badge at all when the build could not reach
    // the API. Failure is silent on purpose: a nav ornament must never
    // surface an error, and a build-time number is still a correct number.
    const controller = new AbortController();
    fetch(`${apiBase}/api/repos/${repo}/analyze?enqueue=0`, {
      headers: { accept: "application/json" },
      signal: controller.signal,
    })
      .then((response) => (response.ok ? response.json() : null))
      .then((data: { total_stars?: number; not_found?: boolean } | null) => {
        if (cancelled || !data || data.not_found) return;
        const live = data.total_stars;
        if (typeof live !== "number" || !Number.isFinite(live)) return;
        if (live === shown.current) return;
        cancelAnimationFrame(raf);
        paint(live, shown.current);
      })
      .catch(() => undefined);

    return () => {
      cancelled = true;
      controller.abort();
      cancelAnimationFrame(raf);
    };
  }, [stars, apiBase, repo]);

  const initial = stars === null ? "" : format(stars);
  const label = stars === null ? "gitdebt on GitHub" : starLabel(stars);
  return (
    <a
      ref={hostRef}
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      title={label}
      aria-label={label}
      data-press="off"
      // Hidden only when the build had no number at all. The refresh above
      // reveals it; if that fails too, no badge is better than an empty one.
      hidden={stars === null}
      className="dither-fallback relative isolate inline-flex min-h-10 items-center gap-1.5 overflow-hidden rounded-md border border-border/60 px-2.5 font-mono text-[12px] whitespace-nowrap text-foreground outline-none transition-[opacity,scale] duration-150 active:scale-[0.96] motion-reduce:transition-none focus-visible:ring-2 focus-visible:ring-accent/30 focus-visible:ring-offset-2 focus-visible:ring-offset-background"
      {...handlers}
    >
      {surface}
      <span className="relative inline-flex items-center gap-1.5">
        <span aria-hidden="true" className="text-[13px] leading-none">
          ★
        </span>
        <span
          ref={ref}
          // `1ch` is one digit under tabular-nums, so the box is already the
          // width of the final number before the first frame is written.
          style={{ minWidth: `${initial.length || 1}ch` }}
          className="inline-block text-right tabular-nums"
        >
          {initial}
        </span>
      </span>
    </a>
  );
}
