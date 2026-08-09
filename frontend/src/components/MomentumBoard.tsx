"use client";

import { useEffect, useMemo, useRef } from "react";

import {
  BAYER4,
  BRAND,
  CELL,
  INK,
  OFF_TIER,
  RasterBuffer,
  WAVE_AMPLITUDE,
  clamp01,
  makeWaves,
  prefersReducedMotion,
  waveOffset,
  type RGB,
  type Wave,
} from "@/lib/dither";

export type MomentumRow = {
  rank: number;
  repo: string;
  stars: number;
  /** Stars gained across the board's window. The thing being ranked. */
  velocity: number;
};

export type MomentumBoardProps = {
  rows: MomentumRow[];
  /** Days the velocity covers, so the ripple can be normalised to a rate. */
  windowDays: number;
  /** Rows rendered before the list scrolls internally. */
  visibleRows?: number;
  /**
   * Which quantity the band's horizontal extent measures.
   *
   * The ripple always encodes rate, because rate is what "momentum" means. The
   * extent is whatever the list is ranked by — so a most-starred board shows
   * size as length and growth as speed, and a large repo that has stopped
   * moving is visibly still next to a smaller one that is climbing.
   */
  extentBy?: "velocity" | "stars";
};

/**
 * Rate at which the fastest repo's band ripples, in wave cycles per second.
 *
 * The whole page is about velocity, and velocity was previously a number in a
 * right-aligned column — the flattest possible way to show motion. Here the
 * band's ripple speed IS the repo's star rate, so the leader visibly churns
 * while a repo adding forty stars a week barely breathes. Removing the motion
 * removes a dimension of the data, which is the only justification for looping
 * animation on a page someone is trying to read.
 */
const MAX_RIPPLE_RATE = 0.55;
/** Slowest ripple a ranked repo gets, so "on the board" still reads as alive. */
const MIN_RIPPLE_RATE = 0.04;

/** Row height in CSS px. Generous on purpose: the list is the page. */
const ROW_HEIGHT = 56;

/**
 * Cell budget for one board's buffer.
 *
 * `gridFor` caps at 600 rows, which is right for the interactive panels it was
 * written for and wrong here: a fifty-row list is 2,800 CSS px tall, so that cap
 * would stretch each cell to nearly 5px and this board's texture would stop
 * matching every other dithered surface on the site. `CELL` is documented as
 * "identical for every interactive component", so the grid is computed at true
 * cell density and bounded by area instead — a wide full-width board is about
 * 700x1400 cells, or 3.9 MB of RGBA, and only the boards actually on screen are
 * being painted.
 */
const MAX_CELLS = 1_100_000;

/** Fill for the leader's band. Every other row is ink. */
const LEADER_FILL: RGB = BRAND;

type Band = {
  /** 0..1 share of the leader's rate — the horizontal extent of the band. */
  share: number;
  /** Cycles per second for this row's ripple. */
  rate: number;
  waves: Wave[];
};

/**
 * Ranked repositories as dithered momentum bands.
 *
 * One canvas paints every row. Fifty canvases would be fifty compositor layers
 * and fifty rAF callbacks; `RasterBuffer` is built for exactly this instead —
 * one buffer, one `putImageData` per frame, whatever the row count.
 *
 * The list itself is ordinary semantic markup sitting above the canvas, so the
 * ranking is complete and readable with the canvas absent, scripts disabled, or
 * motion reduced. The canvas only ever adds a reading of *rate* that the
 * numbers alone do not carry.
 */
export function MomentumBoard({
  rows,
  windowDays,
  visibleRows = 12,
  extentBy = "velocity",
}: MomentumBoardProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const hostRef = useRef<HTMLDivElement>(null);

  const bands = useMemo<Band[]>(() => {
    const days = Math.max(1, windowDays);
    const rates = rows.map((row) => Math.max(0, row.velocity) / days);
    const fastest = Math.max(1, ...rates);
    const extents = rows.map((row, index) =>
      extentBy === "stars" ? Math.max(0, row.stars) : rates[index],
    );
    const widest = Math.max(1, ...extents);
    return rows.map((row, index) => {
      // Square-root, not linear: both star counts and velocities are
      // heavy-tailed, and a linear map leaves everything below the leader as an
      // indistinguishable stub.
      const share = clamp01(Math.sqrt(extents[index] / widest));
      const rateShare = clamp01(Math.sqrt(rates[index] / fastest));
      return {
        share,
        rate: MIN_RIPPLE_RATE + rateShare * (MAX_RIPPLE_RATE - MIN_RIPPLE_RATE),
        // Seeded by slug, matching how the comparison chart gives each series a
        // stable texture: the same repo ripples the same way on every render.
        waves: makeWaves(row.repo),
      };
    });
  }, [rows, windowDays, extentBy]);

  useEffect(() => {
    const canvas = canvasRef.current;
    const host = hostRef.current;
    if (!canvas || !host || bands.length === 0) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    host.classList.remove("dither-fallback");

    let buffer: RasterBuffer | null = null;
    let raf = 0;
    let running = false;
    let onScreen = false;
    // Accumulated animation seconds, not wall-clock since mount. A paused loop
    // must resume where it stopped: timing from the first frame would jump the
    // ripple forward by however long the tab was hidden, which reads as a
    // glitch precisely when the reader looks back at it.
    let elapsed = 0;
    let last: number | undefined;

    const paint = (seconds: number) => {
      const box = host.getBoundingClientRect();
      if (box.width < 4 || box.height < 4) return;
      // True cell density, so the texture matches the rest of the system.
      let cols = Math.max(4, Math.round(box.width / CELL));
      let gridRows = Math.max(4, Math.round(box.height / CELL));
      if (cols * gridRows > MAX_CELLS) {
        const scale = Math.sqrt(MAX_CELLS / (cols * gridRows));
        cols = Math.max(4, Math.floor(cols * scale));
        gridRows = Math.max(4, Math.floor(gridRows * scale));
      }
      if (!buffer || buffer.cols !== cols || buffer.rows !== gridRows) {
        buffer = new RasterBuffer(cols, gridRows);
        canvas.width = cols;
        canvas.height = gridRows;
      }
      buffer.clear();

      const cellsPerRow = gridRows / bands.length;
      for (let y = 0; y < gridRows; y++) {
        const index = Math.min(bands.length - 1, Math.floor(y / cellsPerRow));
        const band = bands[index];
        // Distance from the band's vertical centre, 0 at the middle and 1 at
        // its edge: the ripple fades out rather than butting against its
        // neighbour, so fifty bands read as fifty rows, not one field.
        const withinBand = (y % cellsPerRow) / cellsPerRow;
        const edge = 1 - Math.abs(withinBand - 0.5) * 2;
        const falloff = clamp01(edge * 1.6);
        const reach = band.share * cols;
        const fill = index === 0 ? LEADER_FILL : INK;

        for (let x = 0; x < cols; x++) {
          if (x > reach) break;
          const u = x / Math.max(1, cols);
          // The wave set is the repo's own; `rate` scales time, so a faster
          // repo advances further per second through the same waveform.
          const ripple = waveOffset(band.waves, u, seconds * band.rate);
          // Density falls toward the band's leading edge so the bar reads as a
          // measured extent, not a rectangle with texture on it.
          const head = clamp01(1 - x / Math.max(1, reach));
          const density = (0.24 + head * 0.52 + ripple / WAVE_AMPLITUDE * 0.09) * falloff;
          if (density <= 0) continue;
          const lit = density > BAYER4[y & 3][x & 3];
          const alpha = (0.26 + density * 0.62) * (lit ? 1 : OFF_TIER);
          if (alpha <= 0.004) continue;
          buffer.set(x, y, fill, alpha);
        }
      }
      ctx.putImageData(buffer.image, 0, 0);
    };

    const step = (now: number) => {
      // Clamped so a long pause the observers did not catch still resumes
      // smoothly instead of leaping.
      elapsed += Math.min(0.05, last === undefined ? 0 : (now - last) / 1000);
      last = now;
      paint(elapsed);
      if (running) raf = requestAnimationFrame(step);
    };

    // Reduced motion gets the same information as a single still frame: the
    // extents still encode rank and share, only the rate is dropped.
    const reduced = prefersReducedMotion();
    const startLoop = () => {
      if (running || reduced) return;
      running = true;
      raf = requestAnimationFrame(step);
    };
    const stopLoop = () => {
      running = false;
      if (raf) cancelAnimationFrame(raf);
      raf = 0;
      // Dropping the timestamp is what makes the next frame a resume rather
      // than a jump.
      last = undefined;
    };

    paint(0);

    // A loop nobody can see is pure battery cost, and this one can be long.
    const observer = new IntersectionObserver(
      ([entry]) => {
        onScreen = entry?.isIntersecting ?? false;
        if (onScreen && !document.hidden) startLoop();
        else stopLoop();
      },
      { rootMargin: "128px" },
    );
    observer.observe(host);
    // Both directions. Stopping on hide without restarting on show left the
    // board frozen for the rest of the session after a single tab switch: the
    // observer does not fire again, because the intersection never changed.
    const onVisibility = () => {
      if (document.hidden) stopLoop();
      else if (onScreen) startLoop();
    };
    document.addEventListener("visibilitychange", onVisibility);
    const resize = new ResizeObserver(() => paint(0));
    resize.observe(host);

    return () => {
      stopLoop();
      observer.disconnect();
      resize.disconnect();
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [bands]);

  const maxHeight = visibleRows * ROW_HEIGHT;
  return (
    <div
      ref={hostRef}
      className="dither-fallback relative isolate overflow-hidden rounded-lg border border-border/60"
      style={{ maxHeight: rows.length > visibleRows ? `${maxHeight}px` : undefined }}
    >
      <canvas
        ref={canvasRef}
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 -z-10 h-full w-full [image-rendering:pixelated]"
      />
      <ol className="relative divide-y divide-border/30">
        {rows.map((row, index) => (
          <li key={row.repo}>
            <a
              href={`/${row.repo}`}
              className="group grid grid-cols-[3.5rem_1fr_auto] items-center gap-4 px-4 outline-none transition-colors duration-150 hover:bg-card/40 focus-visible:ring-2 focus-visible:ring-accent/30 sm:grid-cols-[4.5rem_1fr_auto_auto] sm:gap-6 sm:px-6"
              style={{ height: `${ROW_HEIGHT}px` }}
            >
              <span
                className={`font-mono tabular-nums ${
                  index === 0
                    ? "text-[26px] leading-none text-foreground"
                    : "text-[15px] leading-none text-muted-foreground"
                }`}
              >
                {row.rank}
              </span>
              <span className="min-w-0 truncate text-[15px] text-foreground sm:text-[17px]">
                {row.repo}
              </span>
              <span className="text-right font-mono text-[13px] tabular-nums text-foreground sm:text-[15px]">
                {extentBy === "stars"
                  ? row.stars.toLocaleString()
                  : `+${row.velocity.toLocaleString()}`}
              </span>
              <span className="hidden text-right font-mono text-[12px] tabular-nums text-muted-foreground sm:block">
                {extentBy === "stars"
                  ? `+${row.velocity.toLocaleString()}`
                  : row.stars.toLocaleString()}
              </span>
            </a>
          </li>
        ))}
      </ol>
    </div>
  );
}
