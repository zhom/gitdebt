import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { motion, useReducedMotion } from "motion/react";

import { EmbedSnippet } from "@/components/EmbedSnippet";
import { EYEBROW, PANEL } from "@/components/style-tokens";
import { Button } from "@/components/ui/button";
import { DitherSegmented } from "@/components/ui/dither-segmented";
import { CONTROL } from "@/components/ui/dither-surface";
import {
  BAYER4,
  CELL,
  OFF_TIER,
  RasterBuffer,
  SWATCH,
  clamp01,
  hashSeed,
  makeSurfaceMotion,
  makeWaves,
  waveOffset,
  type RGB,
  type SurfaceController,
  type SurfaceMotion,
} from "@/lib/dither";
import { SPRING } from "@/lib/motion";
import { cn } from "@/lib/utils";

export type ComparisonChartPoint = { date: string; stars: number };
export type ComparisonChartSeries = {
  slug: string;
  points: ComparisonChartPoint[];
};

type Axis = "date" | "timeline";

type Props = {
  apiBase: string;
  path: string;
  caption: string;
  embedLink: string;
  label: string;
  series: ComparisonChartSeries[];
  height?: number;
};

type ParsedPoint = { at: number; value: number };
type ParsedSeries = {
  slug: string;
  color: RGB;
  cssColor: string;
  phaseX: number;
  phaseY: number;
  points: ParsedPoint[];
};
/**
 * `approximate` here means ONE thing: the reading was interpolated between two
 * samples rather than landing on one. It is a geometry fact about the hover
 * position and it is NOT source provenance — it says nothing about whether the
 * series came from GitHub's stargazer list or from public GH Archive star
 * events. That distinction belongs to `SeriesProvenance`, which reads
 * `history_kind` / `history_status` off the analyze payload. Do not wire this
 * flag to a provenance marker; the two words collide and mean different things.
 */
type Sample = { at: number; value: number; approximate: boolean };
type Hover = {
  fraction: number;
  at: number;
  values: { slug: string; color: string; value: number; approximate: boolean }[];
};

const PLOT = { left: 56, right: 18, top: 22, bottom: 46 };
const DATE_RE = /^\d{4}-\d{2}-\d{2}$/;
const DATE_FIELD = "w-[8.5rem] tabular-nums scheme-dark";
const AXIS_OPTIONS = [
  { value: "date" as const, label: "Date" },
  { value: "timeline" as const, label: "Timeline" },
];

const SERIES_COLORS: RGB[] = [
  SWATCH.blue,
  SWATCH.pink,
  SWATCH.green,
  SWATCH.orange,
  SWATCH.purple,
  SWATCH.red,
  [40, 205, 210],
  [240, 205, 70],
];

function rgb(color: RGB): string {
  return `rgb(${color[0]} ${color[1]} ${color[2]})`;
}

/** Stable within a comparison and collision-free until the palette is full. */
export function comparisonColors(slugs: string[]): Map<string, RGB> {
  const colors = new Map<string, RGB>();
  const occupied = new Set<number>();
  for (const slug of [...new Set(slugs)].sort()) {
    const preferred = hashSeed(slug.toLowerCase()) % SERIES_COLORS.length;
    let index = preferred;
    for (let attempt = 0; attempt < SERIES_COLORS.length; attempt += 1) {
      index = (preferred + attempt) % SERIES_COLORS.length;
      if (!occupied.has(index)) break;
    }
    occupied.add(index);
    colors.set(slug, SERIES_COLORS[index]);
  }
  return colors;
}

function compact(value: number): string {
  return new Intl.NumberFormat("en", {
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(value);
}

function valueFraction(value: number, max: number, logScale: boolean): number {
  if (logScale) return Math.log(Math.max(0, value) + 1) / Math.log(max + 1);
  return Math.max(0, value) / Math.max(1, max);
}

function sampleTimeline(points: ParsedPoint[], fraction: number): Sample {
  const position = clamp01(fraction) * (points.length - 1);
  const low = Math.floor(position);
  const high = Math.min(points.length - 1, Math.ceil(position));
  const local = position - low;
  const before = points[low];
  const after = points[high];
  return {
    at: before.at + (after.at - before.at) * local,
    value: before.value + (after.value - before.value) * local,
    approximate: low !== high,
  };
}

function sampleDate(points: ParsedPoint[], target: number): Sample | null {
  if (target < points[0].at) return null;
  if (target >= points.at(-1)!.at) {
    return { ...points.at(-1)!, approximate: target !== points.at(-1)!.at };
  }
  let lo = 0;
  let hi = points.length - 1;
  while (lo < hi) {
    const mid = Math.floor((lo + hi) / 2);
    if (points[mid].at < target) lo = mid + 1;
    else hi = mid;
  }
  const high = lo;
  const low = Math.max(0, high - 1);
  const before = points[low];
  const after = points[high];
  const span = Math.max(1, after.at - before.at);
  const local = (target - before.at) / span;
  return {
    at: target,
    value: before.value + (after.value - before.value) * local,
    approximate: target !== before.at && target !== after.at,
  };
}

function sample(
  series: ParsedSeries,
  fraction: number,
  axis: Axis,
  minDate: number,
  maxDate: number,
): Sample | null {
  if (axis === "timeline") return sampleTimeline(series.points, fraction);
  return sampleDate(
    series.points,
    minDate + clamp01(fraction) * Math.max(1, maxDate - minDate),
  );
}

export function DitherComparisonChart({
  apiBase,
  path,
  caption,
  embedLink,
  label,
  series,
  height = 500,
}: Props) {
  const [axis, setAxis] = useState<Axis>("date");
  const [logScale, setLogScale] = useState(false);
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const id = useId();
  const validFrom = DATE_RE.test(from) ? from : "";
  const validTo = DATE_RE.test(to) ? to : "";
  const inverted = Boolean(validFrom && validTo && validFrom > validTo);
  const appliedFrom = inverted ? "" : validFrom;
  const appliedTo = inverted ? "" : validTo;

  const visibleSeries = useMemo(() => {
    const fromMs = appliedFrom
      ? Date.parse(`${appliedFrom}T00:00:00Z`)
      : -Infinity;
    const toMs = appliedTo ? Date.parse(`${appliedTo}T23:59:59Z`) : Infinity;
    return series
      .map((item) => ({
        ...item,
        points: item.points.filter((point) => {
          const at = Date.parse(point.date);
          return Number.isFinite(at) && at >= fromMs && at <= toMs;
        }),
      }))
      .filter((item) => item.points.length >= 2);
  }, [appliedFrom, appliedTo, series]);

  return (
    <figure
      className={cn(PANEL, "relative min-w-0 max-w-full overflow-hidden")}
    >
      <figcaption className="flex min-w-0 flex-wrap items-center justify-between gap-3 border-b border-border/40 px-4 py-3">
        <div className={EYEBROW}>{caption}</div>
        <div className="flex min-w-0 w-full flex-wrap items-center gap-2 sm:w-auto">
          <label htmlFor={`${id}-from`} className={EYEBROW}>
            From
            <span className="sr-only"> date (YYYY-MM-DD)</span>
          </label>
          <input
            id={`${id}-from`}
            name="from"
            type="date"
            value={from}
            onChange={(event) => setFrom(event.target.value)}
            className={cn(CONTROL, DATE_FIELD)}
          />
          <label htmlFor={`${id}-to`} className={EYEBROW}>
            To
            <span className="sr-only"> date (YYYY-MM-DD)</span>
          </label>
          <input
            id={`${id}-to`}
            name="to"
            type="date"
            value={to}
            onChange={(event) => setTo(event.target.value)}
            className={cn(CONTROL, DATE_FIELD)}
          />
          {(from || to) && (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                setFrom("");
                setTo("");
              }}
            >
              Reset
            </Button>
          )}
          <Button
            variant={logScale ? "primary" : "outline"}
            size="sm"
            pulse={false}
            aria-pressed={logScale}
            onClick={() => setLogScale((value) => !value)}
          >
            Log
          </Button>
          <DitherSegmented
            role="radiogroup"
            aria-label="Chart axis"
            value={axis}
            options={AXIS_OPTIONS}
            onValueChange={setAxis}
          />
          <div className="basis-full sm:basis-auto">
            <EmbedSnippet
              apiBase={apiBase}
              chartPath={path}
              linkHref={embedLink}
              label={label}
              state={{
                type: axis,
                log: logScale,
                from: appliedFrom,
                to: appliedTo,
              }}
              variant="menu"
            />
          </div>
        </div>
      </figcaption>
      {inverted && (
        <p
          role="alert"
          className="border-b border-border/40 px-4 py-2 text-[0.6875rem] text-[var(--swatch-red)]"
        >
          The end date is before the start date. Showing the full range.
        </p>
      )}
      {visibleSeries.length >= 2 ? (
        <MultiSeriesCanvas
          series={visibleSeries}
          axis={axis}
          logScale={logScale}
          height={height}
        />
      ) : (
        <div
          className="grid place-items-center px-6 text-center text-base text-muted-foreground sm:text-sm"
          style={{ height }}
          aria-live="polite"
        >
          This range needs at least two repositories with recorded star history.
        </div>
      )}
    </figure>
  );
}

function MultiSeriesCanvas({
  series,
  axis,
  logScale,
  height,
}: {
  series: ComparisonChartSeries[];
  axis: Axis;
  logScale: boolean;
  height: number;
}) {
  const rootRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const surfaceRef = useRef<SurfaceController | null>(null);
  const pointerRef = useRef({ inside: false, px: 0.5, py: 0.5 });
  const revealRef = useRef(1);
  const [size, setSize] = useState({ width: 0, height });
  const [hover, setHover] = useState<Hover | null>(null);
  const reducedMotion = useReducedMotion();

  const parsed = useMemo(() => {
    const colors = comparisonColors(series.map((item) => item.slug));
    return series
      .map((item): ParsedSeries => {
        const color = colors.get(item.slug) ?? SWATCH.blue;
        return {
          slug: item.slug,
          color,
          cssColor: rgb(color),
          phaseX: hashSeed(item.slug.slice(0, 5)) & 3,
          phaseY: hashSeed(item.slug) & 3,
          points: item.points
            .map((point) => ({
              at: Date.parse(point.date),
              value: point.stars,
            }))
            .filter(
              (point) =>
                Number.isFinite(point.at) && Number.isFinite(point.value),
            )
            .sort((a, b) => a.at - b.at),
        };
      })
      .filter((item) => item.points.length >= 2);
  }, [series]);
  const minDate = Math.min(...parsed.map((item) => item.points[0].at));
  const maxDate = Math.max(...parsed.map((item) => item.points.at(-1)!.at));
  const maxValue = Math.max(
    1,
    ...parsed.flatMap((item) => item.points.map((point) => point.value)),
  );
  const waves = useMemo(
    () => new Map(parsed.map((item) => [item.slug, makeWaves(item.slug)])),
    [parsed],
  );

  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const resize = () => {
      const width = root.getBoundingClientRect().width;
      setSize((previous) =>
        previous.width === width && previous.height === height
          ? previous
          : { width, height },
      );
    };
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(root);
    return () => observer.disconnect();
  }, [height]);

  useEffect(() => {
    const canvas = canvasRef.current;
    const root = rootRef.current;
    if (!canvas || !root || size.width <= 0 || parsed.length < 2) return;
    const context = canvas.getContext("2d");
    if (!context) return;
    const cols = Math.max(8, Math.round(size.width / CELL));
    const rows = Math.max(8, Math.round(size.height / CELL));
    canvas.width = cols;
    canvas.height = rows;
    const buffer = new RasterBuffer(cols, rows);
    const left = Math.round(PLOT.left / CELL);
    const right = cols - Math.round(PLOT.right / CELL);
    const top = Math.round(PLOT.top / CELL);
    const bottom = rows - Math.round(PLOT.bottom / CELL);
    const width = Math.max(1, right - left);
    const plotHeight = Math.max(1, bottom - top);

    const paint = (motion: SurfaceMotion) => {
      buffer.clear();
      const revealX = left + Math.round(width * revealRef.current);
      for (const item of parsed) {
        const itemWaves = waves.get(item.slug) ?? [];
        for (let x = left; x <= Math.min(right, revealX); x += 1) {
          const fraction = (x - left) / width;
          const current = sample(item, fraction, axis, minDate, maxDate);
          if (!current) continue;
          const lineY = Math.max(
            top,
            Math.min(
              bottom,
              Math.round(
                bottom -
                  valueFraction(current.value, maxValue, logScale) * plotHeight,
              ),
            ),
          );
          const ripple =
            motion.intensity *
            waveOffset(itemWaves, fraction, motion.time) *
            32;
          for (let offset = -4; offset <= 4; offset += 1) {
            const y = lineY + offset;
            if (y < top || y > bottom) continue;
            const density = clamp01(
              0.86 - Math.abs(offset - ripple) * 0.2,
            );
            const lit =
              density >
              BAYER4[(y + item.phaseY) & 3][(x + item.phaseX) & 3];
            if (!lit) continue;
            buffer.set(
              x,
              y,
              item.color,
              (0.18 + density * 0.35) *
                (1 + motion.intensity * 0.18) *
                (offset === 0 ? 1 : OFF_TIER),
            );
          }
          // Exact contour remains legible while the dither field moves around it.
          buffer.set(x, lineY, item.color, 0.98);
          if (lineY + 1 <= bottom) {
            buffer.set(x, lineY + 1, item.color, 0.5);
          }
        }
      }
      context.putImageData(buffer.image, 0, 0);
    };

    const surface = makeSurfaceMotion(paint, { continuous: true });
    surfaceRef.current = surface;
    const pointer = pointerRef.current;
    if (pointer.inside) surface.enter(pointer.px, pointer.py);
    revealRef.current = reducedMotion ? 1 : 0;
    surface.repaint();

    let revealFrame = 0;
    const revealStart = performance.now();
    const drawReveal = (now: number) => {
      const elapsed = (now - revealStart) / 1_100;
      revealRef.current = clamp01(1 - Math.pow(1 - clamp01(elapsed), 3));
      surface.repaint();
      if (revealRef.current < 1) revealFrame = requestAnimationFrame(drawReveal);
    };
    if (!reducedMotion) revealFrame = requestAnimationFrame(drawReveal);

    const observer =
      typeof IntersectionObserver === "function"
        ? new IntersectionObserver((entries) => {
            for (const entry of entries) {
              surface.setVisible(entry.isIntersecting);
            }
          })
        : null;
    observer?.observe(root);
    return () => {
      if (revealFrame) cancelAnimationFrame(revealFrame);
      observer?.disconnect();
      surface.stop();
      surfaceRef.current = null;
    };
  }, [
    axis,
    logScale,
    maxDate,
    maxValue,
    minDate,
    parsed,
    reducedMotion,
    size,
    waves,
  ]);

  function location(clientX: number, clientY: number) {
    const bounds = rootRef.current?.getBoundingClientRect();
    if (!bounds) return { fraction: 0, px: 0.5, py: 0.5 };
    const plotWidth = Math.max(1, bounds.width - PLOT.left - PLOT.right);
    return {
      fraction: clamp01((clientX - bounds.left - PLOT.left) / plotWidth),
      px: clamp01((clientX - bounds.left) / Math.max(1, bounds.width)),
      py: clamp01((clientY - bounds.top) / Math.max(1, bounds.height)),
    };
  }

  function track(clientX: number, clientY: number) {
    const { fraction, px, py } = location(clientX, clientY);
    pointerRef.current = { inside: true, px, py };
    surfaceRef.current?.move(px, py);
    const values = parsed.flatMap((item) => {
      const current = sample(item, fraction, axis, minDate, maxDate);
      return current
        ? [
            {
              slug: item.slug,
              color: item.cssColor,
              value: current.value,
              approximate: current.approximate,
            },
          ]
        : [];
    });
    const at =
      axis === "date"
        ? minDate + fraction * Math.max(1, maxDate - minDate)
        : Math.max(...parsed.map((item) => sampleTimeline(item.points, fraction).at));
    setHover({ fraction, at, values });
  }

  const xTicks = [0, 1 / 3, 2 / 3, 1];
  const hoverX = hover
    ? PLOT.left +
      hover.fraction * Math.max(1, size.width - PLOT.left - PLOT.right)
    : 0;

  return (
    <div
      ref={rootRef}
      className="relative min-w-0 max-w-full overflow-hidden bg-transparent text-foreground"
      style={{ height }}
      onPointerEnter={(event) => {
        const { px, py } = location(event.clientX, event.clientY);
        pointerRef.current = { inside: true, px, py };
        surfaceRef.current?.enter(px, py);
      }}
      onPointerMove={(event) => track(event.clientX, event.clientY)}
      onPointerDown={(event) => track(event.clientX, event.clientY)}
      onPointerLeave={() => {
        pointerRef.current.inside = false;
        surfaceRef.current?.leave();
        setHover(null);
      }}
      role="img"
      aria-label={`Animated star history comparison of ${parsed
        .map((item) => item.slug)
        .join(", ")}`}
    >
      <svg
        className="pointer-events-none absolute inset-0 size-full"
        aria-hidden="true"
      >
        {[0, 0.5, 1].map((fraction) => {
          const y = PLOT.top + fraction * (height - PLOT.top - PLOT.bottom);
          const raw = maxValue * (1 - fraction);
          const value = logScale
            ? Math.exp(Math.log(maxValue + 1) * (1 - fraction)) - 1
            : raw;
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
              <text
                x={PLOT.left - 9}
                y={y + 4}
                textAnchor="end"
                className="fill-muted-foreground font-mono text-[10px]"
              >
                {compact(value)}
              </text>
            </g>
          );
        })}
        {xTicks.map((fraction, index) => {
          const date = minDate + fraction * Math.max(1, maxDate - minDate);
          return (
            <text
              key={fraction}
              x={
                PLOT.left +
                fraction * Math.max(1, size.width - PLOT.left - PLOT.right)
              }
              y={height - 12}
              textAnchor={
                index === 0
                  ? "start"
                  : index === xTicks.length - 1
                    ? "end"
                    : "middle"
              }
              className="fill-muted-foreground font-mono text-[10px]"
            >
              {axis === "date"
                ? new Date(date).toLocaleDateString(undefined, {
                    month: "short",
                    year: "numeric",
                    timeZone: "UTC",
                  })
                : index === 0
                  ? "start"
                  : index === xTicks.length - 1
                    ? "latest"
                    : `${Math.round(fraction * 100)}%`}
            </text>
          );
        })}
      </svg>
      <canvas
        ref={canvasRef}
        className="pointer-events-none absolute inset-0 size-full [image-rendering:pixelated]"
        aria-hidden="true"
      />

      <ul
        role="list"
        className="pointer-events-none absolute top-3 left-16 z-10 flex max-w-[calc(100%-5rem)] flex-wrap gap-x-4 gap-y-1 font-mono text-[0.6875rem]"
      >
        {parsed.map((item) => (
          <li
            key={item.slug}
            className="flex items-center gap-1.5"
            style={{ "--series-color": item.cssColor } as CSSProperties}
          >
            <span
              className="size-2 bg-(--series-color) [clip-path:polygon(50%_0,100%_50%,50%_100%,0_50%)]"
              aria-hidden="true"
            />
            <span>{item.slug}</span>
          </li>
        ))}
      </ul>

      {hover && (
        <>
          <motion.div
            initial={false}
            animate={{ x: hoverX }}
            transition={reducedMotion ? { duration: 0 } : SPRING.snappy}
            className="pointer-events-none absolute top-[22px] bottom-[46px] left-0 w-px bg-foreground/45"
            aria-hidden="true"
          />
          <motion.output
            initial={{ opacity: 0, y: -4 }}
            animate={{
              opacity: 1,
              x: Math.min(
                Math.max(8, hoverX - 104),
                Math.max(8, size.width - 224),
              ),
              y: 42,
            }}
            transition={reducedMotion ? { duration: 0 } : SPRING.snappy}
            className="pointer-events-none absolute top-0 left-0 z-20 w-52 border border-border bg-popover/90 px-3 py-2 text-popover-foreground shadow-sm backdrop-blur-xl"
            aria-live="polite"
          >
            <span className="font-mono text-[0.625rem] tracking-wide text-muted-foreground uppercase">
              {axis === "date"
                ? new Date(hover.at).toLocaleDateString(undefined, {
                    year: "numeric",
                    month: "short",
                    day: "numeric",
                    timeZone: "UTC",
                  })
                : "Same point in each lifetime"}
            </span>
            <span className="mt-2 grid gap-1.5">
              {[...hover.values]
                .sort((a, b) => b.value - a.value)
                .map((value) => (
                  <span
                    key={value.slug}
                    className="grid grid-cols-[0.5rem_1fr_auto] items-center gap-2"
                    style={
                      { "--series-color": value.color } as CSSProperties
                    }
                  >
                    <span
                      className="size-2 bg-(--series-color)"
                      aria-hidden="true"
                    />
                    <span className="truncate font-mono text-[0.6875rem] text-muted-foreground">
                      {value.slug}
                    </span>
                    <span className="text-sm font-semibold tabular-nums">
                      {value.approximate ? "≈" : ""}
                      {compact(value.value)}
                    </span>
                  </span>
                ))}
            </span>
          </motion.output>
        </>
      )}
    </div>
  );
}
