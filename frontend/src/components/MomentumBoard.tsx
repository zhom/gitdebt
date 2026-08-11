"use client";

import { useEffect, useMemo, useRef } from "react";

import {
  BAYER4,
  CELL,
  INK,
  OFF_TIER,
  RasterBuffer,
  SWATCH,
  WAVE_AMPLITUDE,
  clamp01,
  makeWaves,
  prefersReducedMotion,
  waveOffset,
  type RGB,
  type Wave,
} from "@/lib/dither";
import { rates, trendOf, type MomentumRow, type Trend } from "@/lib/momentum";

export type { MomentumRow, Trend };

export type MomentumBoardProps = {
  rows: MomentumRow[];
};

/** Cycles per second for the fastest repository's ripple. */
const MAX_RIPPLE_RATE = 0.6;
/** Slowest ripple a ranked repository gets, so "on the board" reads as alive. */
const MIN_RIPPLE_RATE = 0.05;

/**
 * Row height in CSS px.
 *
 * Deliberately tight. An earlier pass spent 56px per repository to carry four
 * numbers — less information per screen than the plain table it replaced. A
 * leaderboard's job is comparison, and comparison needs rows close enough to
 * scan against one another.
 */
const ROW_HEIGHT = 30;

/** Width of the momentum strip, in CSS px. Matches the `8.25rem` grid column. */
const STRIP_WIDTH = 132;

/** Area ceiling for one buffer, at true `CELL` density. */
const MAX_CELLS = 900_000;

const TREND_FILL: Record<Trend, RGB> = {
  rising: SWATCH.green,
  steady: INK,
  fading: SWATCH.orange,
  unknown: SWATCH.grey,
};

const TREND_LABEL: Record<Trend, string> = {
  rising: "climbing, faster today than its monthly pace",
  steady: "steady, today matches its monthly pace",
  fading: "cooling, slower today than its monthly pace",
  unknown: "ranked in only one window, so there is nothing to compare",
};

const TREND_MARK: Record<Trend, string> = {
  rising: "▲",
  steady: "–",
  fading: "▼",
  unknown: "·",
};

type Band = {
  /** 0..1 extent from the weekly rate — the dimension the board ranks by. */
  share: number;
  /** 0..1 extent of today's rate, on the same per-day scale. */
  today: number;
  rate: number;
  fill: RGB;
  waves: Wave[];
};

const nf = new Intl.NumberFormat("en-US");
const fmt = (value: number | null) => (value === null ? "—" : nf.format(value));

/**
 * The ranking as one dense list carrying every window at once.
 *
 * The page used to render three velocity boards and a stars board: the same
 * repositories four times, one number each, and no way to answer what a
 * trending board is for. Rates make the windows comparable, so a row now shows
 * level (weekly), momentum (today), direction (today against the month) and
 * size together, in less vertical space than any single old board occupied.
 *
 * One canvas paints every strip. Fifty canvases would be fifty compositor
 * layers and fifty rAF callbacks; `RasterBuffer` exists so a single
 * `putImageData` per frame serves any row count.
 */
export function MomentumBoard({ rows }: MomentumBoardProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const hostRef = useRef<HTMLDivElement>(null);

  const bands = useMemo<Band[]>(() => {
    const all = rows.map(rates);
    const fastestWeek = Math.max(1, ...all.map((r) => r.r7 ?? 0));
    const fastestDay = Math.max(1, ...all.map((r) => r.r1 ?? 0));
    return rows.map((row, index) => {
      const { r1, r7 } = all[index];
      // Square root: star rates are heavy-tailed, and a linear map leaves
      // everything below the leader an indistinguishable stub.
      const share = clamp01(Math.sqrt((r7 ?? 0) / fastestWeek));
      const today = clamp01(Math.sqrt((r1 ?? 0) / fastestDay));
      return {
        share,
        today,
        rate: MIN_RIPPLE_RATE + today * (MAX_RIPPLE_RATE - MIN_RIPPLE_RATE),
        fill: TREND_FILL[trendOf(row)],
        // Seeded by slug, as the comparison chart seeds each series: the same
        // repository keeps the same grain across renders.
        waves: makeWaves(row.repo),
      };
    });
  }, [rows]);

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
    // Accumulated seconds, not wall-clock since mount: a paused loop resumes
    // where it stopped instead of leaping forward by the hidden duration.
    let elapsed = 0;
    let last: number | undefined;

    const paint = (seconds: number) => {
      const box = canvas.getBoundingClientRect();
      if (box.width < 4 || box.height < 4) return;
      // True `CELL` density — documented as identical for every interactive
      // component, so a stretched grid here would stop matching the site.
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
        // Fade toward each band's edge so rows read as rows, not one field.
        const within = (y % cellsPerRow) / cellsPerRow;
        const falloff = clamp01((1 - Math.abs(within - 0.5) * 2) * 2.2);
        if (falloff <= 0) continue;
        const reach = band.share * cols;
        const head = band.today * cols;
        const span = Math.max(1, Math.max(reach, head));

        for (let x = 0; x < cols; x++) {
          if (x > reach && x > head) break;
          const u = x / Math.max(1, cols);
          const ripple = waveOffset(band.waves, u, seconds * band.rate);
          // Two readings in one mark: the body is the weekly level, the
          // brighter leading segment is today's rate on the same per-day
          // scale. A spike therefore shows as a bright tip past a short body.
          const base = x <= reach ? 0.3 + clamp01(1 - x / span) * 0.45 : 0;
          const surge = x <= head ? 0.34 : 0;
          const density =
            (Math.max(base, surge) + (ripple / WAVE_AMPLITUDE) * 0.1) * falloff;
          if (density <= 0) continue;
          const lit = density > BAYER4[y & 3][x & 3];
          const alpha = (0.24 + density * 0.6) * (lit ? 1 : OFF_TIER);
          if (alpha <= 0.004) continue;
          buffer.set(x, y, band.fill, alpha);
        }
      }
      ctx.putImageData(buffer.image, 0, 0);
    };

    const step = (now: number) => {
      elapsed += Math.min(0.05, last === undefined ? 0 : (now - last) / 1000);
      last = now;
      paint(elapsed);
      if (running) raf = requestAnimationFrame(step);
    };

    // Reduced motion keeps every reading: extents still encode level and
    // today's rate, colour still encodes direction. Only speed is dropped.
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
      last = undefined;
    };

    paint(0);

    const observer = new IntersectionObserver(
      ([entry]) => {
        onScreen = entry?.isIntersecting ?? false;
        if (onScreen && !document.hidden) startLoop();
        else stopLoop();
      },
      { rootMargin: "128px" },
    );
    observer.observe(host);
    // Both directions: stopping on hide without restarting on show froze the
    // board for the rest of the session, because the intersection never
    // changed and so the observer never fired again.
    const onVisibility = () => {
      if (document.hidden) stopLoop();
      else if (onScreen) startLoop();
    };
    document.addEventListener("visibilitychange", onVisibility);
    const resize = new ResizeObserver(() => paint(elapsed));
    resize.observe(host);

    return () => {
      stopLoop();
      observer.disconnect();
      resize.disconnect();
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [bands]);

  // The strip sits immediately after the rank so its offset is a constant the
  // canvas can be positioned against; a variable `1fr` column before it would
  // put the painted band and its column in different places at every width.
  const grid =
    "grid grid-cols-[2.25rem_minmax(0,1fr)_3.25rem_3.25rem_3.25rem_3.75rem] sm:grid-cols-[2.75rem_8.25rem_minmax(0,1fr)_3.75rem_3.75rem_3.75rem_4.5rem]";
  const head =
    "px-2 py-1.5 text-right font-mono text-[10px] tracking-[0.12em] text-muted-foreground/70 uppercase";
  return (
    <div className="relative">
      <div className={`${grid} items-center border-b border-border`}>
        <span className={`${head} text-left`}>#</span>
        <span className={`${head} hidden text-left sm:block`}>Momentum</span>
        <span className={`${head} text-left`}>Repository</span>
        <span className={head}>+1d</span>
        <span className={head}>+7d</span>
        <span className={head}>+30d</span>
        <span className={head}>Stars</span>
      </div>
      <div ref={hostRef} className="dither-fallback relative isolate">
        <canvas
          ref={canvasRef}
          aria-hidden="true"
          className="pointer-events-none absolute top-0 bottom-0 -z-10 hidden [image-rendering:pixelated] sm:block"
          style={{ left: "2.75rem", width: `${STRIP_WIDTH}px` }}
        />
        <ol className="relative">
          {rows.map((row, index) => {
            const trend = trendOf(row);
            return (
              <li key={row.repo}>
                <a
                  href={`/${row.repo}`}
                  className={`${grid} items-center border-b border-border/25 outline-none transition-colors duration-150 hover:bg-card/50 focus-visible:ring-2 focus-visible:ring-accent/30 focus-visible:ring-inset`}
                  style={{ height: `${ROW_HEIGHT}px` }}
                >
                  <span className="px-2 font-mono text-[11px] tabular-nums text-muted-foreground">
                    {index + 1}
                  </span>
                  <span className="hidden sm:block" aria-hidden="true" />
                  <span className="flex min-w-0 items-center gap-1.5 px-2">
                    <span
                      aria-hidden="true"
                      className="font-mono text-[9px] leading-none"
                      style={{ color: `rgb(${TREND_FILL[trend].join(" ")})` }}
                    >
                      {TREND_MARK[trend]}
                    </span>
                    <span className="truncate text-[13px] text-foreground">{row.repo}</span>
                    <span className="sr-only">, {TREND_LABEL[trend]}</span>
                  </span>
                  <span className="px-2 text-right font-mono text-[11px] tabular-nums text-foreground">
                    {fmt(row.d1)}
                  </span>
                  <span className="px-2 text-right font-mono text-[11px] tabular-nums text-foreground">
                    {fmt(row.d7)}
                  </span>
                  <span className="px-2 text-right font-mono text-[11px] tabular-nums text-muted-foreground">
                    {fmt(row.d30)}
                  </span>
                  <span className="px-2 text-right font-mono text-[11px] tabular-nums text-muted-foreground">
                    {nf.format(row.stars)}
                  </span>
                </a>
              </li>
            );
          })}
        </ol>
      </div>
    </div>
  );
}
