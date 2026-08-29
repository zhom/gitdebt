import {
  useEffect,
  useId,
  useState,
  type ReactNode,
  type RefObject,
} from "react";

import { FIELD, PANEL } from "@/components/style-tokens";
import { Button } from "@/components/ui/button";
import { CONTROL, Segmented, Separator } from "@/components/ui/controls";
import { Terminator } from "@/components/ui/marks";
import { cn } from "@/lib/utils";

/**
 * The sheet a plotted chart is drawn on, and the pure geometry every chart on
 * this site shares.
 *
 * `ChartViewer` and `ComparisonSheet` used to carry two copies of the same
 * caption row — the same two date fields, the same log toggle, the same axis
 * switch, the same embed menu, and the same inverted-range sentence, word for
 * word. Two copies of one control row is not two components, it is one
 * component written twice, and the second copy is where the drift starts. It
 * lives here once.
 *
 * The measuring helpers sit here too, because both charts measure the same way
 * and neither of them may guess: `usePlotWidth` reads the real drawn width so
 * the drawing is lettered at true size instead of being stretched by a
 * `preserveAspectRatio="none"` viewBox, and `polylineLength` sums a path's
 * segments rather than asking the DOM for them after the fact.
 */

export type ChartAxis = "date" | "timeline";

/** A calendar day, the only shape the range fields accept. */
export const DATE_RE = /^\d{4}-\d{2}-\d{2}$/;

const AXIS_OPTIONS = [
  { value: "date" as const, label: "Date" },
  { value: "timeline" as const, label: "Timeline" },
];

const SCALE_OPTIONS = [
  { value: "linear" as const, label: "Linear" },
  { value: "log" as const, label: "Log" },
];

const DATE_FIELD =
  "min-h-11 w-[8.75rem] px-2 font-mono text-[0.8125rem] tabular-nums";

/* ── Interpolation ───────────────────────────────────────────────────────── */

/**
 * `approximate` means ONE thing: the reading was interpolated between two
 * samples rather than landing on one. It is a geometry fact about where the
 * pointer is and it is NOT source provenance — it says nothing about whether
 * the series came from GitHub's stargazer list or from public historical star
 * events. That distinction belongs to `SeriesProvenance`. Do not wire this flag
 * to a provenance marker; the two words collide and mean different things.
 */
export type Sample = { at: number; value: number; approximate: boolean };

export function sampleAt(
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

export function valueFraction(
  value: number,
  max: number,
  logScale: boolean,
): number {
  if (logScale) return Math.log(Math.max(0, value) + 1) / Math.log(max + 1);
  return Math.max(0, value) / Math.max(1, max);
}

export const clamp01 = (t: number) => (t < 0 ? 0 : t > 1 ? 1 : t);

/* ── Geometry ────────────────────────────────────────────────────────────── */

export type Vertex = { x: number; y: number };

/**
 * Polyline length, summed here rather than measured from the DOM.
 *
 * `getTotalLength()` would need an effect, which means the first paint would
 * carry a wrong dash length and the stroke would animate to somewhere short of
 * its own end — the half-drawn line that reads as broken. Summing the segments
 * is exact for a polyline and identical on the server and the client.
 */
export function polylineLength(points: Vertex[]): number {
  let total = 0;
  for (let i = 1; i < points.length; i += 1) {
    total += Math.hypot(
      points[i].x - points[i - 1].x,
      points[i].y - points[i - 1].y,
    );
  }
  return Math.ceil(total);
}

export function pathData(points: Vertex[]): string {
  return points
    .map(
      (p, i) => `${i === 0 ? "M" : "L"}${p.x.toFixed(2)} ${p.y.toFixed(2)}`,
    )
    .join(" ");
}

/** Even-stride sample that always keeps the first and last real readings. */
export function stride<T>(items: T[], limit: number): T[] {
  if (items.length <= limit) return items;
  const step = (items.length - 1) / (limit - 1);
  const out: T[] = [];
  for (let i = 0; i < limit; i += 1) out.push(items[Math.round(i * step)]);
  return out;
}

/** Whole months between two epoch times, as a drawing states a span. */
export function spanLabel(from: number, to: number): string {
  if (!Number.isFinite(from) || !Number.isFinite(to) || to <= from) return "";
  const months = Math.max(1, Math.round((to - from) / 2_629_800_000));
  const years = Math.floor(months / 12);
  const rest = months % 12;
  if (years === 0) return `${months} MO`;
  if (rest === 0) return `${years} YR`;
  return `${years} YR ${rest} MO`;
}

export function shortDate(at: number): string {
  return new Date(at).toLocaleDateString("en-US", {
    month: "short",
    year: "numeric",
    timeZone: "UTC",
  });
}

export function fullDate(at: number): string {
  return new Date(at).toLocaleDateString("en-US", {
    month: "long",
    day: "numeric",
    year: "numeric",
    timeZone: "UTC",
  });
}

/**
 * The drawn width of the plot, in real CSS pixels.
 *
 * A chart laid out in a fixed viewBox and stretched to fit letters its own type
 * at whatever horizontal scale the container happens to impose — condensed to a
 * third of its width on a phone, widened on a desktop. So the drawing is
 * lettered at 1:1 instead: the viewBox width IS the measured width.
 *
 * Before the measurement lands the chart draws at `fallback`, stretched. That
 * is a complete, readable drawing with every line, value and label already in
 * the markup — nothing here is invisible until this runs, it only becomes
 * exactly proportioned.
 */
export function usePlotWidth(
  ref: RefObject<HTMLElement | null>,
  fallback = 1000,
): number {
  const [width, setWidth] = useState(fallback);

  useEffect(() => {
    const node = ref.current;
    if (!node) return;
    const measure = () => {
      const next = Math.round(node.getBoundingClientRect().width);
      if (next > 0) setWidth((previous) => (previous === next ? previous : next));
    };
    measure();
    if (typeof ResizeObserver !== "function") return;
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, [ref]);

  return width;
}

/* ── The sheet ───────────────────────────────────────────────────────────── */

export type ChartFrameProps = {
  /** Names what is plotted. A field label on the drawing, not a caption. */
  title?: string;
  axis: ChartAxis;
  onAxisChange: (axis: ChartAxis) => void;
  logScale: boolean;
  onLogScaleChange: (logScale: boolean) => void;
  from: string;
  to: string;
  onFromChange: (value: string) => void;
  onToChange: (value: string) => void;
  onReset: () => void;
  /** True when `to` precedes `from`. Reported, never silently applied. */
  inverted: boolean;
  /** The embed menu, built by the caller because only it knows the asset URL. */
  embed?: ReactNode;
  className?: string;
  children: ReactNode;
};

export function ChartFrame({
  title,
  axis,
  onAxisChange,
  logScale,
  onLogScaleChange,
  from,
  to,
  onFromChange,
  onToChange,
  onReset,
  inverted,
  embed,
  className,
  children,
}: ChartFrameProps) {
  const id = useId();
  const hasRange = Boolean(from || to);

  return (
    <figure className={cn(PANEL, "min-w-0 max-w-full", className)}>
      <figcaption className="flex min-w-0 flex-wrap items-center justify-between gap-x-6 gap-y-3">
        {title && <span className={FIELD}>{title}</span>}

        <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-2">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-2">
            <label htmlFor={`${id}-from`} className={FIELD}>
              From
              <span className="sr-only"> date (YYYY-MM-DD)</span>
            </label>
            <input
              id={`${id}-from`}
              name="from"
              type="date"
              value={from}
              onChange={(event) => onFromChange(event.target.value)}
              className={cn(CONTROL, DATE_FIELD)}
            />
            <label htmlFor={`${id}-to`} className={FIELD}>
              To
              <span className="sr-only"> date (YYYY-MM-DD)</span>
            </label>
            <input
              id={`${id}-to`}
              name="to"
              type="date"
              value={to}
              onChange={(event) => onToChange(event.target.value)}
              className={cn(CONTROL, DATE_FIELD)}
            />
            {hasRange && (
              <Button variant="quiet" onClick={onReset}>
                Reset
              </Button>
            )}
          </div>

          {/* Two settings, two identical controls. The log switch used to be a
              filled button beside an outlined one, which is a preset pair and
              says nothing about what it selects; a scale is a choice between
              two named things, so it is drawn as one. */}
          <Segmented
            role="radiogroup"
            aria-label="Value scale"
            value={logScale ? "log" : "linear"}
            options={SCALE_OPTIONS}
            onValueChange={(value) => onLogScaleChange(value === "log")}
            itemClassName="min-h-11"
          />
          <Segmented
            role="radiogroup"
            aria-label="Chart axis"
            value={axis}
            options={AXIS_OPTIONS}
            onValueChange={onAxisChange}
            itemClassName="min-h-11"
          />
          {embed}
        </div>
      </figcaption>

      {inverted && (
        <p
          role="alert"
          className="mt-3 flex items-start gap-2 text-[0.8125rem] leading-[1.5] text-ink"
        >
          <Terminator
            size={14}
            className="mt-[0.2rem] shrink-0 text-signal"
          />
          The end date is before the start date. Showing the full range.
        </p>
      )}

      <Separator className="my-4" />

      {children}
    </figure>
  );
}
