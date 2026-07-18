/**
 * Pure helpers for deriving crawlable SEO text (narratives, FAQs,
 * milestones, tables) from the analyze endpoints' cumulative star
 * history. Everything here is deterministic given its inputs — the only
 * time anchor is the data's own last point, never the wall clock — so
 * SSR output stays stable within an edge-cache window.
 */

export type HistoryPoint = { date: string; stars: number };

const DAY_MS = 86_400_000;

/** Parse an ISO date defensively; NaN-safe. */
function ts(date: string): number {
  const t = Date.parse(date);
  return Number.isNaN(t) ? 0 : t;
}

/** Compact human number: 950, 1.2k, 34k, 1.2M. */
export function formatCompact(n: number): string {
  if (!Number.isFinite(n)) return "0";
  const abs = Math.abs(n);
  if (abs >= 1_000_000) {
    const v = n / 1_000_000;
    return `${v >= 10 ? Math.round(v) : Math.round(v * 10) / 10}M`;
  }
  if (abs >= 1_000) {
    const v = n / 1_000;
    return `${v >= 10 ? Math.round(v) : Math.round(v * 10) / 10}k`;
  }
  return String(Math.round(n));
}

/** "Mar 2021" (UTC, en-US) from an ISO date; null when unparseable. */
export function formatMonthYear(date: string | null | undefined): string | null {
  if (!date) return null;
  const t = Date.parse(date);
  if (Number.isNaN(t)) return null;
  return new Date(t).toLocaleDateString("en-US", {
    month: "short",
    year: "numeric",
    timeZone: "UTC",
  });
}

/** Full "March 12, 2021" (UTC, en-US); null when unparseable. */
export function formatFullDate(date: string | null | undefined): string | null {
  if (!date) return null;
  const t = Date.parse(date);
  if (Number.isNaN(t)) return null;
  return new Date(t).toLocaleDateString("en-US", {
    month: "long",
    day: "numeric",
    year: "numeric",
    timeZone: "UTC",
  });
}

/** UTC year of the first history point, or null. */
export function firstStarYear(history: HistoryPoint[]): number | null {
  const first = history[0]?.date;
  if (!first) return null;
  const t = Date.parse(first);
  return Number.isNaN(t) ? null : new Date(t).getUTCFullYear();
}

/**
 * Cumulative star total at-or-before a timestamp. History is cumulative
 * and date-ascending (the backend contract); returns 0 before the first
 * point.
 */
function totalAtOrBefore(history: HistoryPoint[], atMs: number): number {
  let total = 0;
  for (const p of history) {
    if (ts(p.date) > atMs) break;
    total = p.stars;
  }
  return total;
}

/**
 * Stars gained in the trailing `days` window, anchored on the LAST data
 * point (not the wall clock — deterministic per dataset). Returns null
 * when the history is empty or shorter than the window would need.
 */
export function gainedInTrailingDays(
  history: HistoryPoint[],
  days: number,
): number | null {
  const last = history[history.length - 1];
  if (!last) return null;
  const lastMs = ts(last.date);
  const cutoff = lastMs - days * DAY_MS;
  const before = totalAtOrBefore(history, cutoff);
  return Math.max(0, last.stars - before);
}

/** Lifetime average stars/day (first→last point); null when degenerate. */
export function lifetimePacePerDay(history: HistoryPoint[]): number | null {
  const first = history[0];
  const last = history[history.length - 1];
  if (!first || !last) return null;
  const spanDays = (ts(last.date) - ts(first.date)) / DAY_MS;
  if (spanDays < 1) return null;
  return last.stars / spanDays;
}

export type Milestone = { threshold: number; date: string };

const MILESTONE_THRESHOLDS = [100, 1_000, 10_000, 100_000, 1_000_000];

/**
 * Dates the cumulative series first crossed each round-number threshold.
 * Quotable, page-unique facts ("react crossed 100k stars in Feb 2019").
 */
export function starMilestones(history: HistoryPoint[]): Milestone[] {
  const out: Milestone[] = [];
  let i = 0;
  for (const threshold of MILESTONE_THRESHOLDS) {
    while (i < history.length && history[i].stars < threshold) i++;
    if (i >= history.length) break;
    out.push({ threshold, date: history[i].date });
  }
  return out;
}

/**
 * Growth-trend verdict comparing the recent pace (trailing 90 days) to
 * the lifetime average. Null when there isn't enough history to say
 * anything honest (under ~180 days of data).
 */
export function growthTrend(
  history: HistoryPoint[],
): "accelerating" | "steady" | "slowing" | null {
  const first = history[0];
  const last = history[history.length - 1];
  if (!first || !last) return null;
  const spanDays = (ts(last.date) - ts(first.date)) / DAY_MS;
  if (spanDays < 180) return null;
  const lifetime = lifetimePacePerDay(history);
  const recent90 = gainedInTrailingDays(history, 90);
  if (lifetime === null || recent90 === null || lifetime <= 0) return null;
  const recentPace = recent90 / 90;
  if (recentPace > lifetime * 1.25) return "accelerating";
  if (recentPace < lifetime * 0.75) return "slowing";
  return "steady";
}
