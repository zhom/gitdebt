"use client";

import { useEffect, useRef } from "react";

import { BAYER4, RasterBuffer, clamp01, type RGB } from "@/lib/dither";
import { cn } from "@/lib/utils";

/**
 * Ambient background field.
 *
 * The whole surface *is* the ordered dither: the Bayer threshold test happens
 * inside the field's own math, so a cell whose signal is below threshold is
 * fully transparent rather than tinted. That is the difference between a
 * generative field and a uniform dot lattice composited on top — the latter
 * reads as noise at every zoom level and is the exact failure mode this
 * replaces.
 *
 * The backing store is tiny (240x150 at most) and stretched with
 * `image-rendering: pixelated`, so cells stay crisp squares at any size.
 */

const CELL = 4;
const MAX_COLS = 240;
const MAX_ROWS = 150;
/** ~30fps. Anything faster is invisible at speed 0.15 and costs battery. */
const FRAME_MS = 33;
/** The single frame a static or reduced-motion field settles on. */
const STATIC_CLOCK = 4;

/**
 * Near-neutral by design. The field covers about half its cells, so a
 * saturated top stop would paint a full-bleed hue across the hero and break
 * the rule that chroma is reserved for data series, state and focus. These
 * three steps land between `--background` and `--card`, so the hero reads as
 * depth rather than as a colour wash.
 */
const DEFAULT_COLORS = ["#0b0c10", "#111419", "#1b2230"] as const;

function parseHex(value: string): RGB {
  const hex = value.trim().replace("#", "");
  const full =
    hex.length === 3
      ? hex
          .split("")
          .map((c) => c + c)
          .join("")
      : hex;
  const n = Number.parseInt(full.slice(0, 6), 16);
  if (!Number.isFinite(n)) return [0, 0, 0];
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** Linear sample across N stops. Only the generative fields interpolate hue. */
function sampleRgbGradient(stops: readonly RGB[], t: number): RGB {
  if (stops.length === 0) return [0, 0, 0];
  if (stops.length === 1) return stops[0];
  const span = (stops.length - 1) * clamp01(t);
  const index = Math.min(stops.length - 2, Math.floor(span));
  const f = span - index;
  const a = stops[index];
  const b = stops[index + 1];
  return [
    a[0] + (b[0] - a[0]) * f,
    a[1] + (b[1] - a[1]) * f,
    a[2] + (b[2] - a[2]) * f,
  ];
}

/** Integer hash: deterministic on every engine, unlike a `Math.sin` hash. */
function hash2(x: number, y: number): number {
  let h = Math.imul(x | 0, 374761393) + Math.imul(y | 0, 668265263);
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
}

const smoothstep = (t: number) => t * t * (3 - 2 * t);

function valueNoise(x: number, y: number): number {
  const xi = Math.floor(x);
  const yi = Math.floor(y);
  const u = smoothstep(x - xi);
  const v = smoothstep(y - yi);
  const a = hash2(xi, yi);
  const b = hash2(xi + 1, yi);
  const c = hash2(xi, yi + 1);
  const d = hash2(xi + 1, yi + 1);
  return a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v;
}

/** Four octaves is enough structure at 240 columns; more is invisible. */
function fbm(x: number, y: number, t: number): number {
  let sum = 0;
  let amp = 0.5;
  let freq = 1;
  for (let octave = 0; octave < 4; octave += 1) {
    sum +=
      amp * valueNoise(x * freq + t * (octave + 1) * 0.35, y * freq - t * 0.2);
    freq *= 2;
    amp *= 0.5;
  }
  return sum;
}

export function paintField(
  buf: RasterBuffer,
  stops: readonly RGB[],
  clock: number,
  opts: { scale: number; vignette: number; opacity: number },
  matrix: number[][] = BAYER4,
) {
  buf.clear();
  const { cols, rows } = buf;
  for (let y = 0; y < rows; y++) {
    const v0 = (y + 0.5) / rows;
    const dy = (v0 - 0.5) * 2;
    for (let x = 0; x < cols; x++) {
      const u = (x + 0.5) / cols;
      const dx = (u - 0.5) * 2;
      const falloff = clamp01(Math.sqrt(dx * dx + dy * dy) / Math.SQRT2);
      const shade = 1 - opts.vignette * falloff * falloff;
      const v = fbm(u * opts.scale, v0 * opts.scale, clock) * 1.4 * shade;
      if (v <= matrix[y & 3][x & 3]) continue;
      const level = clamp01(v);
      buf.set(x, y, sampleRgbGradient(stops, level), level * opts.opacity);
    }
  }
}

export type DitherFieldProps = {
  /** Gradient stops sampled by the field's own signal, dark to light. */
  colors?: readonly string[];
  /** Ambient washes live at 0.07-0.2; a hero field owns its box and runs at 1. */
  opacity?: number;
  scale?: number;
  /** Drift rate. 0.15 is deliberately below the threshold of notice. */
  speed?: number;
  vignette?: number;
  cell?: number;
  /** `static` paints exactly one frame and never schedules a rAF. */
  render?: "animated" | "static";
  className?: string;
};

const FIELD_CLASS =
  "pointer-events-none absolute inset-0 -z-10 h-full w-full [image-rendering:pixelated]";

export function DitherField({
  colors = DEFAULT_COLORS,
  opacity = 1,
  scale = 2.6,
  speed = 0.15,
  vignette = 0.9,
  cell = CELL,
  render = "animated",
  className,
}: DitherFieldProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const key = colors.join(",");

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    // The canvas is measured, never its parent: an Astro island wraps it in an
    // inline `<astro-island>` whose box is zero, while the canvas itself is
    // stretched to `inset-0` of the section it sits behind.
    const host = canvas;

    const stops = key.split(",").map(parseHex);
    const reduce =
      typeof matchMedia === "function" &&
      matchMedia("(prefers-reduced-motion: reduce)").matches;
    const animated = render === "animated" && !reduce;

    let buf: RasterBuffer | null = null;
    let raf = 0;
    let lastPaint = 0;
    let visible = true;
    const started =
      typeof performance === "object" ? performance.now() : Date.now();

    const paint = (clock: number) => {
      const box = host.getBoundingClientRect();
      const size = cell > 0 ? cell : CELL;
      const cols = Math.max(4, Math.min(MAX_COLS, Math.round(box.width / size)));
      const rows = Math.max(
        4,
        Math.min(MAX_ROWS, Math.round(box.height / size)),
      );
      if (!buf || buf.cols !== cols || buf.rows !== rows) {
        buf = new RasterBuffer(cols, rows);
        canvas.width = cols;
        canvas.height = rows;
      }
      paintField(buf, stops, clock, { scale, vignette, opacity });
      ctx.putImageData(buf.image, 0, 0);
    };

    const frame = (now: number) => {
      raf = 0;
      if (!visible) return;
      if (now - lastPaint >= FRAME_MS) {
        lastPaint = now;
        paint(((now - started) / 1000) * speed);
      }
      raf = requestAnimationFrame(frame);
    };

    const start = () => {
      if (!animated || raf) return;
      raf = requestAnimationFrame(frame);
    };
    const stop = () => {
      if (raf) cancelAnimationFrame(raf);
      raf = 0;
    };

    // The first paint lands in rAF, after layout, so the box read below is free.
    const first = requestAnimationFrame(() => {
      paint(animated ? 0 : STATIC_CLOCK);
      start();
    });

    const observer = new IntersectionObserver((entries) => {
      visible = entries.some((entry) => entry.isIntersecting);
      if (visible) start();
      else stop();
    });
    observer.observe(host);

    const resize = new ResizeObserver(() => {
      if (!animated) paint(STATIC_CLOCK);
    });
    resize.observe(host);

    return () => {
      cancelAnimationFrame(first);
      stop();
      observer.disconnect();
      resize.disconnect();
    };
  }, [key, opacity, scale, speed, vignette, cell, render]);

  return (
    <canvas
      ref={canvasRef}
      aria-hidden="true"
      className={cn(FIELD_CLASS, className)}
    />
  );
}
