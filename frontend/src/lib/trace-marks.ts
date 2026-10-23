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
  },
): TraceMilestone[] {
  if (points.length < 3) return [];
  const { baselineY, fontSize, avoid = [], duration } = options;
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
      delay: drawDelayAt(fractions[index], duration),
      labelX,
      labelY,
    });
  }
  return kept;
}
