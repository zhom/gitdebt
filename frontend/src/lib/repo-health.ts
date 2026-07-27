/**
 * Plain-English readings derived from `/api/repos/{owner}/{repo}/health.json`.
 *
 * Star counts say a repository is popular. These say whether the code
 * underneath is still being looked after — the question a maintainer or an
 * adopter actually has. Everything here is pure and deterministic given its
 * input (the trailing windows are already resolved server-side, so no wall
 * clock is read), which keeps the thresholds testable and the same verdict
 * reachable from any surface that holds the payload.
 */

// Relative, with its extension: this module is covered by the Node test
// runner in `scripts/`, which resolves neither the `@/` alias nor an
// extensionless specifier.
import { formatCompact } from "./star-insights.ts";

export type RepoHealthHotspot = {
  path: string;
  commits: number;
  fix_commits: number;
};

export type RepoHealthMonth = { month: string; commits: number };

/** The `ready: true` shape of the health endpoint. */
export type RepoHealth = {
  ready: boolean;
  repo: string;
  stars: number;
  archived: boolean;
  analyzed_at: string | null;
  /** Length of every trailing window in the payload, in days. */
  window_days: number;
  total_commits: number;
  attributed_commits: number;
  analysis_truncated: boolean;
  bus_factor: number;
  contributors: number;
  top_author_commits: number;
  commits_window: number;
  commits_previous_window: number;
  last_commit_day: string | null;
  commit_months: RepoHealthMonth[];
  tracked_files: number;
  file_changes: number;
  fix_changes: number;
  fresh_files: number;
  hotspot: RepoHealthHotspot | null;
  todo_delta_window: number;
  todo_outstanding: number;
};

/**
 * Reading severity. `steady` is the deliberate neutral: plenty of honest
 * answers ("balanced", "not measured yet") are neither good news nor a
 * warning, and colouring them either way would be editorialising.
 */
export type HealthTone = "good" | "steady" | "watch" | "risk";

export type HealthReadingKey =
  | "maintenance"
  | "ownership"
  | "repair"
  | "debt";

export type HealthReading = {
  key: HealthReadingKey;
  /** Signal name. */
  label: string;
  /** The question the signal answers, in the visitor's words. */
  question: string;
  /** Two or three words a reader can repeat back. */
  verdict: string;
  /** The numbers behind the verdict. */
  detail: string;
  tone: HealthTone;
  /** Meter fill, 0..1. */
  ratio: number;
};

export type HealthFact = {
  key: string;
  label: string;
  value: string;
  detail: string;
};

const clamp01 = (value: number): number =>
  Number.isFinite(value) ? Math.min(1, Math.max(0, value)) : 0;

/** "18%", and "<1%" rather than a misleading "0%". */
function percent(share: number): string {
  if (!Number.isFinite(share) || share <= 0) return "0%";
  const rounded = Math.round(share * 100);
  return rounded === 0 ? "<1%" : `${rounded}%`;
}

function plural(count: number, one: string, many: string): string {
  return count === 1 ? one : many;
}

/** "Mar 2021" from an ISO day; null when unparseable. */
function monthYear(day: string | null): string | null {
  if (!day) return null;
  const at = Date.parse(day);
  if (Number.isNaN(at)) return null;
  return new Date(at).toLocaleDateString("en-US", {
    month: "short",
    year: "numeric",
    timeZone: "UTC",
  });
}

function maintenance(health: RepoHealth): HealthReading {
  const days = health.window_days;
  const now = Math.max(0, health.commits_window);
  const before = Math.max(0, health.commits_previous_window);
  const base = {
    key: "maintenance" as const,
    label: "Maintenance",
    question: "Is anyone still shipping?",
  };

  if (now === 0) {
    const since = monthYear(health.last_commit_day);
    return {
      ...base,
      verdict: before > 0 ? "Went quiet" : "Dormant",
      detail: since
        ? `No commits in ${days} days · last one ${since}`
        : `No commits recorded in the last ${days} days`,
      tone: "risk",
      ratio: 0,
    };
  }

  // A momentum share, not a raw count: 0.5 is a repository holding its
  // pace, and the bar reads the same way for a 20-commit project and a
  // 20,000-commit one.
  const ratio = clamp01(now / (now + before));
  const detail =
    before > 0
      ? `${formatCompact(now)} commits in ${days} days vs ${formatCompact(before)} in the ${days} before`
      : `${formatCompact(now)} commits in ${days} days, after a quiet ${days} before`;

  if (before === 0) {
    return { ...base, verdict: "Newly active", detail, tone: "good", ratio };
  }
  if (now >= before * 1.25) {
    return { ...base, verdict: "Speeding up", detail, tone: "good", ratio };
  }
  if (now <= before * 0.75) {
    return { ...base, verdict: "Slowing down", detail, tone: "watch", ratio };
  }
  return { ...base, verdict: "Steady", detail, tone: "steady", ratio };
}

function ownership(health: RepoHealth): HealthReading {
  const bus = Math.max(0, health.bus_factor);
  const people = Math.max(0, health.contributors);
  const base = {
    key: "ownership" as const,
    label: "Ownership",
    question: "How many people could walk away?",
  };

  if (bus === 0 || people === 0) {
    return {
      ...base,
      verdict: "Not attributed",
      detail: "No commit authorship has been attributed yet",
      tone: "steady",
      ratio: 0,
    };
  }

  const detail = `${bus} of ${formatCompact(people)} ${plural(people, "contributor", "contributors")} ${plural(bus, "writes", "write")} half the commits`;
  // Ten is where the count stops carrying information: past it the answer
  // is simply "many", so the meter saturates rather than implying precision.
  const ratio = clamp01(bus / 10);

  if (bus <= 1) {
    return { ...base, verdict: "One person carries it", detail, tone: "risk", ratio };
  }
  if (bus <= 3) {
    return { ...base, verdict: "A few hands", detail, tone: "watch", ratio };
  }
  if (bus <= 9) {
    return { ...base, verdict: "Shared", detail, tone: "good", ratio };
  }
  return { ...base, verdict: "Broadly shared", detail, tone: "good", ratio };
}

function repair(health: RepoHealth): HealthReading {
  const changes = Math.max(0, health.file_changes);
  const fixes = Math.max(0, health.fix_changes);
  const base = {
    key: "repair" as const,
    label: "Repair load",
    question: "How much work is fixing rather than building?",
  };

  if (changes === 0) {
    return {
      ...base,
      verdict: "Not measured",
      detail: "No file-level changes have been recorded yet",
      tone: "steady",
      ratio: 0,
    };
  }

  const share = fixes / changes;
  const detail = `${percent(share)} of file changes came from fix-labelled commits`;
  // 40% fix work is the top of the scale; beyond that the bar is pinned and
  // the verdict carries the rest.
  const ratio = clamp01(share / 0.4);

  if (share < 0.08) {
    return { ...base, verdict: "Mostly building", detail, tone: "good", ratio };
  }
  if (share < 0.2) {
    return { ...base, verdict: "Balanced", detail, tone: "steady", ratio };
  }
  if (share < 0.35) {
    return { ...base, verdict: "Repair-heavy", detail, tone: "watch", ratio };
  }
  return { ...base, verdict: "Mostly firefighting", detail, tone: "risk", ratio };
}

function debt(health: RepoHealth): HealthReading {
  const days = health.window_days;
  const delta = health.todo_delta_window;
  const outstanding = Math.max(0, health.todo_outstanding);
  const base = {
    key: "debt" as const,
    label: "Debt markers",
    question: "Is known debt piling up?",
  };

  if (outstanding === 0 && delta === 0) {
    return {
      ...base,
      verdict: "None tracked",
      detail: "No TODO or FIXME markers found in the analysed history",
      tone: "steady",
      ratio: 0,
    };
  }

  const signed = delta > 0 ? `+${formatCompact(delta)}` : formatCompact(delta);
  const detail = `${signed} in ${days} days · ${formatCompact(outstanding)} TODO/FIXME outstanding`;
  // Movement measured against the backlog it moved. With nothing left
  // outstanding, any movement is the whole of it — a zero bar next to
  // "Shrinking" would read as the opposite of what happened.
  const ratio =
    outstanding > 0
      ? clamp01(Math.abs(delta) / outstanding)
      : delta !== 0
        ? 1
        : 0;

  if (delta < 0) {
    return { ...base, verdict: "Shrinking", detail, tone: "good", ratio };
  }
  if (delta === 0) {
    return { ...base, verdict: "Flat", detail, tone: "steady", ratio };
  }
  // A quarter of the whole backlog added in one window is a different story
  // from a handful of new markers on a large, stable one.
  if (outstanding > 0 && delta / outstanding >= 0.25) {
    return { ...base, verdict: "Growing fast", detail, tone: "risk", ratio };
  }
  return { ...base, verdict: "Growing", detail, tone: "watch", ratio };
}

/** The four readings, in the order a maintainer would ask them. */
export function healthReadings(health: RepoHealth): HealthReading[] {
  return [maintenance(health), ownership(health), repair(health), debt(health)];
}

/** Supporting facts: specific, quotable, and not reducible to a verdict. */
export function healthFacts(health: RepoHealth): HealthFact[] {
  const facts: HealthFact[] = [];

  if (health.hotspot) {
    facts.push({
      key: "hotspot",
      label: "Change hotspot",
      value: health.hotspot.path,
      detail: `${formatCompact(health.hotspot.commits)} changes · ${formatCompact(health.hotspot.fix_commits)} fix-labelled`,
    });
  }

  const tracked = Math.max(0, health.tracked_files);
  if (tracked > 0) {
    facts.push({
      key: "freshness",
      label: "Touched this year",
      value: percent(Math.max(0, health.fresh_files) / tracked),
      detail: `${formatCompact(health.fresh_files)} of ${formatCompact(tracked)} tracked files`,
    });
  }

  facts.push({
    key: "commits",
    label: "Commits read",
    value: formatCompact(Math.max(0, health.total_commits)),
    detail: health.analysis_truncated
      ? "bounded analysis window"
      : "full commit history",
  });

  return facts;
}

/**
 * The monthly commit series as chart points. Months arrive gap-filled from
 * the backend, so a quiet month plots as a zero rather than disappearing
 * into a straight line between its neighbours.
 */
export function commitMonthPoints(
  health: RepoHealth,
): { date: string; value: number }[] {
  return health.commit_months.map((entry) => ({
    date: `${entry.month}-01`,
    value: Math.max(0, entry.commits),
  }));
}
