import { useEffect, useMemo, useRef, useState } from "react";
import { motion, useReducedMotion } from "motion/react";

import { SPRING } from "@/lib/motion";

export type DitherPoint = { date: string; value: number };

type Props = {
  points: DitherPoint[];
  axis?: "date" | "timeline";
  logScale?: boolean;
  height?: number;
  valueLabel?: string;
  valueFormatter?: (value: number) => string;
  interactive?: boolean;
  className?: string;
};

type Sample = { at: number; value: number; approximate: boolean };

const BAYER = [
  [0, 8, 2, 10],
  [12, 4, 14, 6],
  [3, 11, 1, 9],
  [15, 7, 13, 5],
].map((row) => row.map((value) => (value + 0.5) / 16));

const CELL = 3;
const PLOT = { left: 54, right: 16, top: 18, bottom: 32 };

function compact(value: number): string {
  return new Intl.NumberFormat("en", {
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(value);
}

function sampleAt(
  points: { at: number; value: number }[],
  fraction: number,
  axis: "date" | "timeline",
): Sample {
  const first = points[0];
  const last = points[points.length - 1];
  if (points.length === 1) return { ...first, approximate: false };

  let low = 0;
  let high = 1;
  let target = first.at;
  if (axis === "timeline") {
    const position = fraction * (points.length - 1);
    low = Math.floor(position);
    high = Math.min(points.length - 1, Math.ceil(position));
    const local = position - low;
    target = points[low].at + (points[high].at - points[low].at) * local;
  } else {
    target = first.at + (last.at - first.at) * fraction;
    let lo = 0;
    let hi = points.length - 1;
    while (lo < hi) {
      const mid = Math.floor((lo + hi) / 2);
      if (points[mid].at < target) lo = mid + 1;
      else hi = mid;
    }
    high = lo;
    low = Math.max(0, high - 1);
  }

  const before = points[low];
  const after = points[high];
  const span = Math.max(1, after.at - before.at);
  const local = high === low ? 0 : (target - before.at) / span;
  return {
    at: target,
    value: before.value + (after.value - before.value) * local,
    approximate: target !== before.at && target !== after.at,
  };
}

function valueFraction(value: number, max: number, logScale: boolean): number {
  if (logScale) return Math.log(Math.max(0, value) + 1) / Math.log(max + 1);
  return Math.max(0, value) / Math.max(1, max);
}

export function DitherAreaChart({
  points,
  axis = "date",
  logScale = false,
  height = 360,
  valueLabel = "stars",
  valueFormatter = (value) => Math.round(value).toLocaleString(),
  interactive = true,
  className = "",
}: Props) {
  const rootRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState({ width: 0, height });
  const [hover, setHover] = useState<(Sample & { fraction: number }) | null>(null);
  const hoverRef = useRef(false);
  const reducedMotion = useReducedMotion();

  const parsed = useMemo(
    () =>
      points
        .map((point) => ({ at: Date.parse(point.date), value: point.value }))
        .filter((point) => Number.isFinite(point.at) && Number.isFinite(point.value))
        .sort((a, b) => a.at - b.at),
    [points],
  );
  const max = Math.max(1, ...parsed.map((point) => point.value));

  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const resize = () =>
      setSize({ width: root.getBoundingClientRect().width, height });
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(root);
    return () => observer.disconnect();
  }, [height]);

  useEffect(() => {
    const canvas = canvasRef.current;
    const root = rootRef.current;
    if (!canvas || !root || parsed.length < 2 || size.width <= 0) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const cols = Math.max(8, Math.round(size.width / CELL));
    const rows = Math.max(8, Math.round(size.height / CELL));
    canvas.width = cols;
    canvas.height = rows;
    const left = Math.round(PLOT.left / CELL);
    const right = cols - Math.round(PLOT.right / CELL);
    const top = Math.round(PLOT.top / CELL);
    const bottom = rows - Math.round(PLOT.bottom / CELL);
    const plotWidth = Math.max(1, right - left);
    const plotHeight = Math.max(1, bottom - top);
    let intensity = reducedMotion ? (hoverRef.current ? 1 : 0) : 0;
    let reveal = reducedMotion ? 1 : 0;
    let phase = 0;
    let previous = performance.now();
    let raf = 0;

    const draw = (now = performance.now()) => {
      const elapsed = Math.min(48, Math.max(0, now - previous));
      previous = now;
      const target = hoverRef.current ? 1 : 0;
      intensity += (target - intensity) * (reducedMotion ? 1 : 0.17);
      reveal += (1 - reveal) * (reducedMotion ? 1 : Math.min(0.24, elapsed / 360));
      if (hoverRef.current && !reducedMotion) phase += elapsed * 0.0045;
      ctx.clearRect(0, 0, cols, rows);
      const styles = getComputedStyle(root);
      const wave = ctx.createLinearGradient(left, top, right, bottom);
      wave.addColorStop(
        0,
        styles.getPropertyValue("--dither-wave-1").trim() || styles.color,
      );
      wave.addColorStop(
        0.52,
        styles.getPropertyValue("--dither-wave-2").trim() || styles.color,
      );
      wave.addColorStop(
        1,
        styles.getPropertyValue("--dither-wave-3").trim() || styles.color,
      );
      const revealEdge = left + plotWidth * reveal;

      for (let x = left; x <= right; x += 1) {
        if (x > revealEdge) break;
        const fraction = (x - left) / plotWidth;
        const sample = sampleAt(parsed, fraction, axis);
        const lineY = Math.max(
          top,
          Math.min(
            bottom,
            Math.round(bottom - valueFraction(sample.value, max, logScale) * plotHeight),
          ),
        );
        const depth = Math.max(1, bottom - lineY);
        for (let y = lineY; y <= bottom; y += 1) {
          const depthFraction = (y - lineY) / depth;
          const ripple = Math.sin(x * 0.16 + y * 0.09 + phase) * 0.045 * intensity;
          const density = Math.min(
            0.985,
            0.34 + depthFraction * 0.6 + intensity * 0.14 + ripple,
          );
          const threshold = BAYER[y & 3][x & 3];
          if (density <= threshold) continue;
          const edgeFade = Math.min(1, Math.max(0, revealEdge - x));
          ctx.globalAlpha = (0.62 + density * 0.34) * edgeFade;
          ctx.fillStyle = wave;
          ctx.fillRect(x, y, 1, 1);
        }
        ctx.globalAlpha = 0.96;
        ctx.fillStyle = wave;
        ctx.fillRect(x, lineY, 1, 1);
      }
      ctx.globalAlpha = 1;
      if (
        reveal < 0.998 ||
        Math.abs(intensity - target) > 0.002 ||
        (hoverRef.current && !reducedMotion)
      ) {
        raf = requestAnimationFrame(draw);
      }
    };

    draw();
    const repaint = () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(draw);
    };
    root.addEventListener("gitdebt:chart-repaint", repaint);
    return () => {
      cancelAnimationFrame(raf);
      root.removeEventListener("gitdebt:chart-repaint", repaint);
    };
  }, [axis, logScale, max, parsed, reducedMotion, size]);

  function setPointer(clientX: number) {
    const root = rootRef.current;
    if (!root || parsed.length < 2) return;
    const bounds = root.getBoundingClientRect();
    const plotWidth = Math.max(1, bounds.width - PLOT.left - PLOT.right);
    const fraction = Math.max(
      0,
      Math.min(1, (clientX - bounds.left - PLOT.left) / plotWidth),
    );
    setHover({ ...sampleAt(parsed, fraction, axis), fraction });
  }

  function setHovered(value: boolean) {
    hoverRef.current = value;
    rootRef.current?.dispatchEvent(new Event("gitdebt:chart-repaint"));
    if (!value) setHover(null);
  }

  const ticks = parsed.length < 2
    ? []
    : [0, 1 / 3, 2 / 3, 1].map((fraction) => sampleAt(parsed, fraction, axis));
  const hoverY = hover
    ? PLOT.top +
      (1 - valueFraction(hover.value, max, logScale)) *
        Math.max(1, size.height - PLOT.top - PLOT.bottom)
    : 0;
  const hoverX = hover
    ? PLOT.left + hover.fraction * Math.max(1, size.width - PLOT.left - PLOT.right)
    : 0;

  return (
    <div
      ref={rootRef}
      className={`relative w-full overflow-hidden bg-transparent text-foreground ${className}`}
      style={{ height }}
      onPointerEnter={() => setHovered(true)}
      onPointerMove={interactive ? (event) => setPointer(event.clientX) : undefined}
      onPointerDown={interactive ? (event) => setPointer(event.clientX) : undefined}
      onPointerLeave={() => setHovered(false)}
      role="img"
      aria-label={`${valueLabel} over time`}
    >
      <svg className="pointer-events-none absolute inset-0 size-full" aria-hidden="true">
        {[0, 0.5, 1].map((fraction) => {
          const y = PLOT.top + fraction * (height - PLOT.top - PLOT.bottom);
          const value = max * (1 - fraction);
          return (
            <g key={fraction}>
              <line
                x1={PLOT.left}
                x2={Math.max(PLOT.left, size.width - PLOT.right)}
                y1={y}
                y2={y}
                className="stroke-border/65"
                vectorEffect="non-scaling-stroke"
              />
              <text x={PLOT.left - 9} y={y + 4} textAnchor="end" className="fill-muted-foreground font-mono text-[10px]">
                {compact(value)}
              </text>
            </g>
          );
        })}
        {ticks.map((tick, index) => (
          <text
            key={`${tick.at}-${index}`}
            x={PLOT.left + (index / Math.max(1, ticks.length - 1)) * Math.max(1, size.width - PLOT.left - PLOT.right)}
            y={height - 10}
            textAnchor={index === 0 ? "start" : index === ticks.length - 1 ? "end" : "middle"}
            className="fill-muted-foreground font-mono text-[10px]"
          >
            {new Date(tick.at).toLocaleDateString(undefined, {
              month: "short",
              year: "numeric",
              timeZone: "UTC",
            })}
          </text>
        ))}
      </svg>
      <canvas
        ref={canvasRef}
        className="pointer-events-none absolute inset-0 size-full [image-rendering:pixelated]"
        aria-hidden="true"
      />

      {hover && interactive && (
        <>
          <motion.div
            initial={false}
            animate={{ x: hoverX }}
            transition={reducedMotion ? { duration: 0 } : SPRING.snappy}
            className="pointer-events-none absolute top-[18px] bottom-[32px] left-0 w-px bg-foreground/45"
            aria-hidden="true"
          />
          <motion.span
            initial={false}
            animate={{ x: hoverX - 4, y: hoverY - 4 }}
            transition={reducedMotion ? { duration: 0 } : SPRING.snappy}
            className="pointer-events-none absolute top-0 left-0 size-2 border border-background bg-foreground"
            aria-hidden="true"
          />
          <motion.output
            initial={{ opacity: 0, y: -4 }}
            animate={{ opacity: 1, x: Math.min(Math.max(8, hoverX - 72), Math.max(8, size.width - 152)), y: 8 }}
            transition={reducedMotion ? { duration: 0 } : SPRING.snappy}
            className="pointer-events-none absolute top-0 left-0 z-20 w-36 border border-border bg-popover/90 px-3 py-2 text-popover-foreground shadow-sm backdrop-blur-xl"
            aria-live="polite"
          >
            <span className="block font-mono text-[10px] tracking-wide text-muted-foreground uppercase">
              {new Date(hover.at).toLocaleDateString(undefined, {
                year: "numeric",
                month: "short",
                day: "numeric",
                timeZone: "UTC",
              })}
            </span>
            <span className="mt-0.5 block text-sm font-semibold tabular-nums">
              {hover.approximate ? "≈ " : ""}{valueFormatter(hover.value)} {valueLabel}
            </span>
          </motion.output>
        </>
      )}
    </div>
  );
}
