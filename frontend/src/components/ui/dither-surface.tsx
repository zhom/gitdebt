"use client";

import { useEffect, useRef, type PointerEvent as ReactPointerEvent } from "react";

import {
  RasterBuffer,
  gridFor,
  makeSurfaceMotion,
  paintPanel,
  prefersReducedMotion,
  stampPulse,
  type RGB,
  type SurfaceController,
  type SurfaceMotion,
  type Variant,
} from "@/lib/dither";

/** One ring lives long enough for the next to begin just before it disappears. */
const PULSE_LIFETIME = 0.92;
const PULSE_INTERVAL = 0.7;

export type DitherSurfaceOptions = {
  /** Stable module-level tuple: the paint effect keys off its identity. */
  fill: RGB;
  variant?: Variant;
  /** `null` drops the 1-cell frame; a number pins it; omit for the hover ramp. */
  edge?: number | null;
  animated?: boolean;
  cell?: number;
  className?: string;
  /** Global alpha multiplier. Quiet beds sit near 0.15; controls stay at 1. */
  alpha?: number;
  /** Emits overlapping one-shot dither rings at the live pointer position. */
  pulse?: boolean;
};

const CANVAS_CLASS =
  "pointer-events-none absolute inset-0 -z-10 h-full w-full [image-rendering:pixelated]";

/**
 * Paints the parent element's box as a dithered canvas.
 *
 * The host must be `relative isolate overflow-hidden`, and its text content
 * must sit in a `relative` wrapper so it stays above the `-z-10` canvas.
 */
export function useDitherSurface(opts: DitherSurfaceOptions) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const ctrl = useRef<SurfaceController | null>(null);
  const hovering = useRef(false);
  const { fill, variant, edge, cell, className, alpha, pulse } = opts;

  useEffect(() => {
    const canvas = canvasRef.current;
    const host = canvas?.parentElement;
    if (!canvas || !host) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    // The no-JS checkerboard tint is only a stand-in for this canvas.
    host.classList.remove("dither-fallback");

    let buf: RasterBuffer | null = null;
    let lastEmission = -Infinity;
    const trail: { x: number; y: number; born: number }[] = [];
    const paint = (m: SurfaceMotion) => {
      const box = host.getBoundingClientRect();
      const { cols, rows } = gridFor(box.width, box.height, cell);
      if (!buf || buf.cols !== cols || buf.rows !== rows) {
        buf = new RasterBuffer(cols, rows);
        canvas.width = cols;
        canvas.height = rows;
      }
      paintPanel(buf, fill, variant ?? "gradient", m.intensity, { edge, alpha });
      if (
        pulse &&
        hovering.current &&
        (trail.length === 0 || m.time - lastEmission >= PULSE_INTERVAL)
      ) {
        trail.push({ x: m.px, y: m.py, born: m.time });
        lastEmission = m.time;
      }
      if (pulse && trail.length > 0) {
        while (trail.length > 0 && m.time - trail[0].born >= PULSE_LIFETIME) {
          trail.shift();
        }
        const base = Math.max(5, Math.min(cols, rows) * 0.8);
        for (const ring of trail) {
          const progress = Math.max(
            0,
            Math.min(1, (m.time - ring.born) / PULSE_LIFETIME),
          );
          const eased = 1 - (1 - progress) ** 3;
          // Fast rise, long dissolve. Adjacent one-shot rings overlap for
          // ~220ms and retain their own cursor positions.
          const envelope =
            Math.min(1, progress / 0.14) * (1 - progress) ** 0.72;
          stampPulse(buf, fill, {
            x: ring.x * cols,
            y: ring.y * rows,
            radius: base * (0.24 + 0.96 * eased),
            energy: envelope * Math.max(0.35, m.pulse),
          });
        }
      }
      ctx.putImageData(buf.image, 0, 0);
    };

    const c = makeSurfaceMotion(paint, { continuous: pulse });
    ctrl.current = c;
    // rAF runs after layout, so the box read below is free.
    const raf = requestAnimationFrame(() => c.repaint());
    const ro = new ResizeObserver(() => c.repaint());
    ro.observe(host);
    const io =
      pulse && typeof IntersectionObserver === "function"
        ? new IntersectionObserver((entries) => {
            for (const entry of entries) c.setVisible(entry.isIntersecting);
          })
        : null;
    io?.observe(host);
    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
      io?.disconnect();
      c.stop();
      ctrl.current = null;
    };
  }, [fill, variant, edge, cell, alpha, pulse]);

  const animated = (opts.animated ?? true) && !prefersReducedMotion();
  const local = (event: ReactPointerEvent<HTMLElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    return {
      x: box.width > 0 ? (event.clientX - box.left) / box.width : 0.5,
      y: box.height > 0 ? (event.clientY - box.top) / box.height : 0.5,
    };
  };
  return {
    canvasRef,
    surface: (
      <canvas
        ref={canvasRef}
        aria-hidden="true"
        className={className ?? CANVAS_CLASS}
      />
    ),
    handlers: animated
      ? {
          onPointerEnter: (event: ReactPointerEvent<HTMLElement>) => {
            const { x, y } = local(event);
            hovering.current = true;
            ctrl.current?.enter(x, y);
          },
          onPointerMove: (event: ReactPointerEvent<HTMLElement>) => {
            const { x, y } = local(event);
            ctrl.current?.move(x, y);
          },
          onPointerLeave: () => {
            hovering.current = false;
            ctrl.current?.leave();
          },
          onPointerDown: () => ctrl.current?.down(),
          onPointerUp: () => ctrl.current?.up(),
          onPointerCancel: () => ctrl.current?.up(),
        }
      : {},
  };
}

/**
 * Static dithered bed for a host that owns no pointer state. Render it as the
 * first child of a `relative isolate overflow-hidden` element.
 */
export function DitherSurface(props: DitherSurfaceOptions) {
  const { surface } = useDitherSurface({ ...props, animated: false });
  return surface;
}

/**
 * Canvas whose cell grid is either fixed or measured from the parent box, with
 * `draw` re-run on resize and on every render that changes the closure. Used by
 * the controls, whose paints are state-driven rather than hover-driven.
 */
export function useRasterCanvas(
  draw: (buf: RasterBuffer) => void,
  opts: { cols?: number; rows?: number; cell?: number } = {},
) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const drawRef = useRef(draw);
  drawRef.current = draw;
  const paintRef = useRef<(() => void) | null>(null);
  const { cols: fixedCols, rows: fixedRows, cell } = opts;

  useEffect(() => {
    const canvas = canvasRef.current;
    const host = canvas?.parentElement;
    if (!canvas || !host) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    host.classList.remove("dither-fallback");

    let buf: RasterBuffer | null = null;
    const paint = () => {
      let cols = fixedCols;
      let rows = fixedRows;
      if (cols === undefined || rows === undefined) {
        const box = host.getBoundingClientRect();
        const grid = gridFor(box.width, box.height, cell);
        cols = cols ?? grid.cols;
        rows = rows ?? grid.rows;
      }
      if (!buf || buf.cols !== cols || buf.rows !== rows) {
        buf = new RasterBuffer(cols, rows);
        canvas.width = cols;
        canvas.height = rows;
      }
      drawRef.current(buf);
      ctx.putImageData(buf.image, 0, 0);
    };

    // First paint lands in rAF, after layout, so the box read is free.
    const raf = requestAnimationFrame(() => {
      paintRef.current = paint;
      paint();
    });
    const measured = fixedCols === undefined || fixedRows === undefined;
    const ro = measured ? new ResizeObserver(() => paint()) : null;
    ro?.observe(host);
    return () => {
      cancelAnimationFrame(raf);
      ro?.disconnect();
      paintRef.current = null;
    };
  }, [fixedCols, fixedRows, cell]);

  // State changes repaint instantly; these canvases are tiny.
  useEffect(() => {
    paintRef.current?.();
  });

  return canvasRef;
}

/** Shared focus ring for every control that owns its own outline. */
export const CONTROL_FOCUS =
  "outline-none focus-visible:ring-2 focus-visible:ring-accent/30 focus-visible:ring-offset-2 focus-visible:ring-offset-background";

/** Shared field treatment for selects, inputs and popover triggers. */
export const CONTROL =
  "min-h-10 w-full rounded-md border border-border/60 bg-background/60 px-3 py-2 font-mono text-[13px] outline-none transition-[border-color,box-shadow,background-color] duration-150 hover:border-foreground/25 focus-visible:border-accent/70 focus-visible:ring-2 focus-visible:ring-accent/20 disabled:opacity-40";

/** Shared floating-layer treatment. */
export const POPOVER =
  "rounded-lg border border-border/80 bg-card shadow-[0_8px_24px_rgba(0,0,0,0.32)]";
