"use client";

import { useEffect, useMemo, useRef, useState } from "react";

import {
  BAYER4,
  INK,
  OFF_TIER,
  RasterBuffer,
  SWATCH,
  type RGB,
} from "@/lib/dither";
import {
  AGE_LABEL,
  ageRingAtPoint,
  layoutAgeRings,
  type FileAgeBand,
} from "@/lib/repo-signal-visuals";
import { cn } from "@/lib/utils";

export type FileAgeRingsProps = {
  bands: readonly FileAgeBand[];
  className?: string;
};

const FRAME_MS = 50;

const compact = (value: number) =>
  new Intl.NumberFormat("en", {
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(value);

function mixColor(a: RGB, b: RGB, t: number): RGB {
  const amount = Math.max(0, Math.min(1, t));
  return [
    Math.round(a[0] + (b[0] - a[0]) * amount),
    Math.round(a[1] + (b[1] - a[1]) * amount),
    Math.round(a[2] + (b[2] - a[2]) * amount),
  ];
}

function angleFraction(dx: number, dy: number): number {
  const angle = Math.atan2(dy, dx) + Math.PI / 2;
  return ((angle % (Math.PI * 2)) + Math.PI * 2) % (Math.PI * 2) /
    (Math.PI * 2);
}

export function FileAgeRings({
  bands,
  className,
}: FileAgeRingsProps) {
  const rings = useMemo(() => layoutAgeRings(bands), [bands]);
  const hottest = useMemo(() => {
    let index = 0;
    for (let next = 1; next < rings.length; next += 1) {
      if (rings[next].changeIntensity > rings[index].changeIntensity) {
        index = next;
      }
    }
    return index;
  }, [rings]);
  const [activeIndex, setActiveIndex] = useState(hottest);
  const activeIndexRef = useRef(activeIndex);
  activeIndexRef.current = activeIndex;
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const totalFiles = rings.reduce((sum, ring) => sum + ring.files, 0);
  const active = rings[activeIndex] ?? rings[0];

  useEffect(() => {
    setActiveIndex(hottest);
  }, [hottest]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const context = canvas.getContext("2d");
    if (!context) return;

    let buffer: RasterBuffer | null = null;
    let frame = 0;
    let visible = true;
    let reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    let lastPaint = 0;
    const started = performance.now();
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");

    const paint = (now: number) => {
      const box = canvas.getBoundingClientRect();
      const cols = Math.max(80, Math.min(190, Math.round(box.width / 2.4)));
      const rows = Math.max(80, Math.min(190, Math.round(box.height / 2.4)));
      if (!buffer || buffer.cols !== cols || buffer.rows !== rows) {
        buffer = new RasterBuffer(cols, rows);
        canvas.width = cols;
        canvas.height = rows;
      }
      buffer.clear();
      const cx = cols / 2;
      const cy = rows / 2;
      const radiusScale = Math.min(cols, rows) * 0.5;
      const phase = reduced ? 0 : (now - started) / 1_700;
      const phaseStep = reduced ? 0 : Math.floor(phase * 2) & 3;

      for (let y = 0; y < rows; y += 1) {
        const dy = y + 0.5 - cy;
        for (let x = 0; x < cols; x += 1) {
          const dx = x + 0.5 - cx;
          const radius = Math.hypot(dx, dy) / radiusScale;
          const index = rings.findIndex(
            (ring) =>
              radius >= ring.innerRadius && radius <= ring.outerRadius,
          );
          if (index < 0) continue;
          const ring = rings[index];
          const fraction = angleFraction(dx, dy);
          const inShare = fraction <= ring.fileShare;
          const selected = index === activeIndexRef.current;
          const wave =
            0.5 +
            0.5 *
              Math.sin(fraction * Math.PI * 8 - phase * 1.6 + index * 0.9);
          const density = inShare
            ? Math.min(
                0.98,
                0.28 +
                  ring.changeIntensity * 0.56 +
                  (selected ? wave * 0.12 : 0),
              )
            : selected
              ? 0.16 + wave * 0.05
              : 0.08;
          const threshold =
            BAYER4[(y + phaseStep) & 3][(x + index) & 3];
          const lit = density > threshold;
          const color = inShare
            ? mixColor(SWATCH.blue, SWATCH.orange, ring.changeIntensity)
            : INK;
          const alpha = inShare
            ? (0.3 + density * 0.7) * (lit ? 1 : OFF_TIER)
            : lit
              ? 0.14
              : 0.035;
          buffer.set(x, y, color, alpha);
        }
      }
      context.putImageData(buffer.image, 0, 0);
    };

    const tick = (now: number) => {
      frame = 0;
      if (!visible) return;
      if (now - lastPaint >= FRAME_MS) {
        lastPaint = now;
        paint(now);
      }
      if (!reduced) frame = requestAnimationFrame(tick);
    };
    const start = () => {
      if (reduced) paint(performance.now());
      else if (!frame && visible) frame = requestAnimationFrame(tick);
    };
    const stop = () => {
      if (frame) cancelAnimationFrame(frame);
      frame = 0;
    };
    const intersection = new IntersectionObserver(([entry]) => {
      visible = entry?.isIntersecting ?? true;
      if (visible) start();
      else stop();
    });
    intersection.observe(canvas);
    const resize = new ResizeObserver(() => paint(performance.now()));
    resize.observe(canvas);
    const updateMotion = () => {
      reduced = motion.matches;
      stop();
      start();
    };
    motion.addEventListener("change", updateMotion);
    start();

    return () => {
      stop();
      intersection.disconnect();
      resize.disconnect();
      motion.removeEventListener("change", updateMotion);
    };
  }, [rings]);

  const selectPointerRing = (clientX: number, clientY: number) => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const box = canvas.getBoundingClientRect();
    const index = ageRingAtPoint(
      clientX - box.left,
      clientY - box.top,
      box.width,
      box.height,
      rings,
    );
    if (index !== null) setActiveIndex(index);
  };

  return (
    <figure className={cn("min-w-0 p-3.5", className)}>
      <div className="relative mx-auto aspect-square w-full max-w-72 overflow-hidden">
        <canvas
          ref={canvasRef}
          aria-hidden="true"
          className="size-full touch-pan-y [image-rendering:pixelated]"
          onPointerMove={(event) =>
            selectPointerRing(event.clientX, event.clientY)
          }
          onPointerDown={(event) =>
            selectPointerRing(event.clientX, event.clientY)
          }
          onPointerLeave={() => setActiveIndex(hottest)}
        />
        <div className="pointer-events-none absolute inset-[37%] grid place-items-center rounded-full bg-background/85 text-center">
          <div>
            <p className="font-mono text-lg font-medium tabular-nums">
              {compact(totalFiles)}
            </p>
            <p className="font-mono text-[0.625rem] tracking-wide text-muted-foreground uppercase">
              files
            </p>
          </div>
        </div>
      </div>

      <figcaption className="grid gap-2 sm:grid-cols-2">
        {rings.map((ring, index) => (
          <button
            key={ring.range}
            type="button"
            aria-pressed={index === activeIndex}
            onPointerEnter={() => setActiveIndex(index)}
            onFocus={() => setActiveIndex(index)}
            className="min-w-0 rounded-md border border-border/60 p-2.5 text-left outline-none transition-transform duration-200 hover:-translate-y-0.5 focus-visible:ring-2 focus-visible:ring-accent/30 aria-pressed:border-foreground/30 aria-pressed:bg-card/70 motion-reduce:transition-none"
          >
            <div className="flex items-center justify-between gap-2">
              <p className="min-w-0 truncate font-mono text-base text-muted-foreground sm:text-[0.6875rem]">
                {AGE_LABEL[ring.range]}
              </p>
              <p className="font-mono text-base tabular-nums sm:text-[0.6875rem]">
                {compact(ring.files)}
              </p>
            </div>
            <div className="mt-1 flex items-center justify-between gap-2 font-mono text-sm text-muted-foreground tabular-nums sm:text-[0.625rem]">
              <p>{Math.round(ring.fileShare * 100)}% of files</p>
              <p>{ring.changeRate.toFixed(1)} changes/file</p>
            </div>
          </button>
        ))}
      </figcaption>
      <p
        className="mt-3 text-pretty text-base text-muted-foreground sm:text-sm"
        aria-live="polite"
      >
        {active
          ? `${AGE_LABEL[active.range]}: ${active.files.toLocaleString()} files and ${active.changes.toLocaleString()} recorded changes. Hotter color means more changes per file.`
          : "File ages are unavailable."}
      </p>
    </figure>
  );
}
