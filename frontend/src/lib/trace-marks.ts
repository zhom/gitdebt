import { DURATION, EASE_DRAW } from "./motion.ts";

/**
 * Marks that wait for the pen.
 *
 * A trace inks in along its own length on `EASE_DRAW`. Anything drawn ON the
 * trace — a milestone, a datum — has to arrive when the pen reaches it, not on
 * a guessed delay, or it is a mark standing on paper the object has not yet
 * crossed. These helpers compute that moment and decide which marks the sheet
 * has room to letter. They are pure, so the server and the browser place every
 * mark identically.
 */

export type TracePoint = { x: number; y: number; value: number };

export type Box = { x0: number; y0: number; x1: number; y1: number };

export type TraceMilestone = {
  threshold: number;
  label: string;
  x: number;
  y: number;
  /** Milliseconds after the draw starts at which the pen reaches this point. */
  delay: number;
  /** Where the label is lettered: up and to the left of the datum. */
  labelX: number;
  labelY: number;
};

const THRESHOLDS = [100, 1_000, 10_000, 100_000, 1_000_000];

function cubic(a: number, b: number, t: number): number {
  const u = 1 - t;
  return 3 * u * u * t * a + 3 * u * t * t * b + t * t * t;
}

/**
 * The moment the pen reaches a fraction of its own length.
 *
 * The dash offset is eased, so the pen is at arc fraction `p` when the eased
 * progress equals `p`. The curve is solved for that progress by bisection
 * (it is monotonic in both axes) and its time coordinate is the answer.
 */
export function drawDelayAt(
  fraction: number,
  duration = DURATION.draw * 1000,
): number {
  const p = Math.min(1, Math.max(0, fraction));
  const [x1, y1, x2, y2] = EASE_DRAW;
  let lo = 0;
  let hi = 1;
  for (let i = 0; i < 32; i += 1) {
    const mid = (lo + hi) / 2;
    if (cubic(y1, y2, mid) < p) lo = mid;
    else hi = mid;
  }
  return Math.round(cubic(x1, x2, (lo + hi) / 2) * duration);
}

/** Cumulative length along a polyline, as fractions of the whole. */
export function arcFractions(points: { x: number; y: number }[]): number[] {
  const out = [0];
  let total = 0;
  for (let i = 1; i < points.length; i += 1) {
    total += Math.hypot(
      points[i].x - points[i - 1].x,
      points[i].y - points[i - 1].y,
    );
    out.push(total);
  }
  return total > 0 ? out.map((length) => length / total) : out.map(() => 0);
}

/**
 * Where a polyline is at `x`: the segment that spans it and how far along that
 * segment `x` falls. A time series never runs backwards in x, so this is the
 * one point of the trace above that station.
 */
export function segmentAtX(
  points: { x: number }[],
  x: number,
): { index: number; fraction: number } {
  const last = points.length - 1;
  if (last < 1 || x <= points[0].x) return { index: 0, fraction: 0 };
  if (x >= points[last].x) return { index: Math.max(0, last - 1), fraction: 1 };
  let lo = 0;
  let hi = last;
  while (hi - lo > 1) {
    const mid = (lo + hi) >> 1;
    if (points[mid].x <= x) lo = mid;
    else hi = mid;
  }
  const span = points[hi].x - points[lo].x;
  return { index: lo, fraction: span > 0 ? (x - points[lo].x) / span : 0 };
}

/**
 * The easing that draws a trace in calendar time.
 *
 * `inks-in` advances the dash along the trace's LENGTH, so a steep climb — the
 * very moment a repository went viral — took most of the draw and a flat decade
 * flew past. Sampling the arc length at equal steps in x and handing them to a
 * CSS `linear()` makes the pen cross the sheet at an even pace through time
 * instead, so its climb rate IS the star rate: a launch or a spike is a visible
 * leap. `x` is time on every sheet that uses this, including the timeline axis
 * (time since the first star).
 */
export function calendarEasing(
  points: { x: number; y: number }[],
  steps = 24,
): string {
  if (points.length < 2) return "linear";
  const cumulative = arcFractions(points);
  const x0 = points[0].x;
  const x1 = points[points.length - 1].x;
  if (x1 <= x0) return "linear";
  const stops: string[] = [];
  for (let k = 0; k <= steps; k += 1) {
    const { index, fraction } = segmentAtX(points, x0 + ((x1 - x0) * k) / steps);
    const next = Math.min(points.length - 1, index + 1);
    const at = cumulative[index] + (cumulative[next] - cumulative[index]) * fraction;
    stops.push((k === 0 ? 0 : k === steps ? 1 : at).toFixed(4));
  }
  return `linear(${stops.join(", ")})`;
}

export function thresholdLabel(threshold: number): string {
  if (threshold >= 1_000_000) return `${threshold / 1_000_000}M`;
  if (threshold >= 1_000) return `${threshold / 1_000}K`;
  return String(threshold);
}

const overlaps = (a: Box, b: Box, pad = 0) =>
  a.x0 - pad < b.x1 && b.x0 - pad < a.x1 && a.y0 - pad < b.y1 && b.y0 - pad < a.y1;

/**
 * Round-number crossings of a cumulative trace that the sheet can letter.
 *
 * A milestone is the first plotted point at or above 100, 1K, 10K, 100K or 1M
 * stars, taken only where the trace starts below that level, so the mark is a
 * crossing the drawing actually shows. Its label sits up and to the left of
 * the datum: a cumulative series never falls, so every earlier point is at or
 * below the datum and that corner of the sheet is always clear of the trace.
 *
 * A mark is dropped rather than squeezed when its label or drop line would
 * touch another label, a box the caller reserves (the origin stamp, the value
 * column), or the last point, which already carries the measured value.
 */
export function traceMilestones(
  points: TracePoint[],
  options: {
    baselineY: number;
    fontSize: number;
    avoid?: Box[];
    duration?: number;
    /** The trace draws in calendar time (`calendarEasing`), so the pen
     *  reaches a point when the draw has covered its share of x. */
    calendar?: boolean;
  },
): TraceMilestone[] {
  if (points.length < 3) return [];
  const { baselineY, fontSize, avoid = [], duration, calendar = false } = options;
  const x0 = points[0].x;
  const xSpan = Math.max(1, points[points.length - 1].x - x0);
  const draw = duration ?? DURATION.draw * 1000;
  const fractions = arcFractions(points);
  const last = points[points.length - 1];
  const kept: TraceMilestone[] = [];
  const taken: Box[] = [...avoid];

  let index = 0;
  for (const threshold of THRESHOLDS) {
    if (points[0].value >= threshold) continue;
    while (index < points.length && points[index].value < threshold) index += 1;
    if (index >= points.length - 1) break;
    const point = points[index];
    if (last.x - point.x < fontSize * 4) continue;

    const label = thresholdLabel(threshold);
    // Format 1452 is condensed: a figure is about 0.6em with the tracking.
    const width = label.length * fontSize * 0.62 + 2;
    const labelX = point.x - 5;
    const labelY = point.y - 6;
    const labelBox: Box = {
      x0: labelX - width,
      y0: labelY - fontSize,
      x1: labelX,
      y1: labelY + 2,
    };
    const dropBox: Box = {
      x0: point.x - 1,
      y0: point.y,
      x1: point.x + 1,
      y1: baselineY,
    };
    if (labelBox.x0 < 0) continue;
    if (taken.some((box) => overlaps(box, labelBox, 4) || overlaps(box, dropBox, 2))) {
      continue;
    }

    taken.push(labelBox);
    kept.push({
      threshold,
      label,
      x: point.x,
      y: point.y,
      delay: calendar
        ? Math.round(((point.x - x0) / xSpan) * draw)
        : drawDelayAt(fractions[index], duration),
      labelX,
      labelY,
    });
  }
  return kept;
}

export type TraceCrossing = {
  x: number;
  y: number;
  /** Indices of the two traces, in the caller's order. */
  a: number;
  b: number;
};

function yAtX(points: { x: number; y: number }[], x: number): number {
  const { index, fraction } = segmentAtX(points, x);
  const next = points[Math.min(points.length - 1, index + 1)];
  return points[index].y + (next.y - points[index].y) * fraction;
}

/**
 * Where one trace overtakes another, worth marking on a comparison.
 *
 * A crossing counts only where it means a change of order: both lines are
 * clear of the baseline tangle (above `minHeight` of the plot), and the new
 * order holds for `hold` px on both sides rather than flickering where two
 * lines run together. The highest crossings win, at most `limit`, no two
 * closer than `spacing` px so their stamps never collide.
 */
export function traceCrossings(
  traces: { x: number; y: number }[][],
  options: {
    baselineY: number;
    plotH: number;
    xMax: number;
    limit?: number;
    spacing?: number;
    hold?: number;
    minHeight?: number;
    step?: number;
  },
): TraceCrossing[] {
  const {
    baselineY,
    plotH,
    xMax,
    limit = 3,
    spacing = 90,
    hold = 24,
    minHeight = 0.08,
    step = 3,
  } = options;
  const found: TraceCrossing[] = [];
  for (let a = 0; a < traces.length; a += 1) {
    for (let b = a + 1; b < traces.length; b += 1) {
      const A = traces[a];
      const B = traces[b];
      if (A.length < 2 || B.length < 2) continue;
      const from = Math.max(A[0].x, B[0].x);
      const to = Math.min(A[A.length - 1].x, B[B.length - 1].x, xMax - hold);
      if (to - from < hold * 2) continue;
      const flips: number[] = [];
      let sign = 0;
      let prev = from;
      let prevGap = 0;
      for (let x = from; x <= to; x += step) {
        const gap = yAtX(A, x) - yAtX(B, x);
        const now = Math.abs(gap) < 0.5 ? 0 : Math.sign(gap);
        if (now !== 0 && sign !== 0 && now !== sign) {
          // Linear interpolation of the zero between the two samples.
          flips.push(prev + (x - prev) * (prevGap / (prevGap - gap)));
        }
        if (now !== 0) {
          sign = now;
          prev = x;
          prevGap = gap;
        }
      }
      for (let i = 0; i < flips.length; i += 1) {
        const x = flips[i];
        const isolated =
          (i === 0 || x - flips[i - 1] >= hold) &&
          (i === flips.length - 1 || flips[i + 1] - x >= hold) &&
          x - from >= hold;
        if (!isolated) continue;
        const y = yAtX(A, x);
        if (baselineY - y < plotH * minHeight) continue;
        found.push({ x, y, a, b });
      }
    }
  }
  const kept: TraceCrossing[] = [];
  for (const crossing of found.sort((p, q) => p.y - q.y)) {
    if (kept.length >= limit) break;
    if (kept.some((other) => Math.abs(other.x - crossing.x) < spacing)) continue;
    kept.push(crossing);
  }
  return kept.sort((p, q) => p.x - q.x);
}

type Reading = { at: number; value: number };

function readingAt(points: Reading[], at: number): number | null {
  if (points.length === 0 || at < points[0].at || at > points[points.length - 1].at) {
    return null;
  }
  let lo = 0;
  let hi = points.length - 1;
  while (hi - lo > 1) {
    const mid = (lo + hi) >> 1;
    if (points[mid].at <= at) lo = mid;
    else hi = mid;
  }
  const span = points[hi].at - points[lo].at;
  const f = span > 0 ? (at - points[lo].at) / span : 0;
  return points[lo].value + (points[hi].value - points[lo].value) * f;
}

/**
 * The moment two series cross, read from their full readings.
 *
 * A crossing found on a drawn path is only as precise as that path's thinning,
 * which depends on the sheet's width and on how the series reached the page,
 * so the same overtake could be lettered in two different months. This scans
 * the readings themselves, a day at a time within `window` of the estimate,
 * and returns the sign change nearest to it (null when there is none).
 */
export function crossingTime(
  a: Reading[],
  b: Reading[],
  estimate: number,
  window: number,
  step = 86_400_000,
): number | null {
  let best: number | null = null;
  let prev: { at: number; gap: number } | null = null;
  for (let at = estimate - window; at <= estimate + window; at += step) {
    const va = readingAt(a, at);
    const vb = readingAt(b, at);
    if (va === null || vb === null) {
      prev = null;
      continue;
    }
    const gap = va - vb;
    if (prev && gap !== 0 && prev.gap !== 0 && Math.sign(gap) !== Math.sign(prev.gap)) {
      const cross = prev.at + (at - prev.at) * (prev.gap / (prev.gap - gap));
      if (best === null || Math.abs(cross - estimate) < Math.abs(best - estimate)) {
        best = cross;
      }
    }
    if (gap !== 0) prev = { at, gap };
  }
  return best;
}
