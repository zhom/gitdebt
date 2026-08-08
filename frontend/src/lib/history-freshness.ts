/**
 * What a repository's star history actually covers, and why.
 *
 * One boolean cannot say this honestly. Since GitHub restricted the stargazer
 * list to a repository's own admins and collaborators (July 2026), a chart can
 * be behind for reasons that are not interchangeable: the exact series stopped
 * on the day the endpoint closed; the approximate series is still flowing;
 * gitdebt was never able to read this repository at all. Each deserves a
 * different sentence, and none of them is the owner's fault — most owners have
 * no idea the endpoint changed.
 *
 * Pure, and deliberately free of DOM and framework imports, so the states can
 * be unit-tested under `node --test` (see `scripts/history-freshness.test.mjs`).
 */

/** The subset of the analyze response this needs. Additive and optional, so an
 * older cached payload degrades to `unknown` rather than throwing. */
export type HistorySnapshot = {
  history_complete?: boolean;
  /** "current_stargazers" (exact) | "public_star_actions" (archive) | "unavailable" */
  history_kind?: string;
  history_approximate?: boolean;
  history_status?: string;
  history_coverage_end?: string | null;
  not_found?: boolean;
};

export type HistoryFreshness =
  /** Exact, from the stargazer list, and still being refreshed. */
  | { state: "exact_current"; through: Date | null }
  /** Exact, but the endpoint closed — the series stops on a fixed date. */
  | { state: "exact_frozen"; through: Date | null }
  /** Archive-derived: approximate by nature (no unstars), still flowing. */
  | { state: "archive"; through: Date | null }
  /** GitHub will not serve this repository's stargazers to gitdebt at all. */
  | { state: "restricted"; through: Date | null }
  /** Nothing to say yet: cold, queued, or a payload too old to classify. */
  | { state: "unknown"; through: null };

/**
 * The date the endpoint closed. A complete *exact* series whose coverage stops
 * on or after this is frozen by the restriction rather than merely stale — the
 * distinction the reader needs, because one of them can still resolve itself
 * and the other cannot.
 */
const RESTRICTION_DATE = Date.UTC(2026, 6, 1); // 2026-07-01

function parseDate(value: string | null | undefined): Date | null {
  if (!value) return null;
  const at = new Date(value);
  return Number.isNaN(at.getTime()) ? null : at;
}

export function historyFreshness(snapshot: HistorySnapshot | null | undefined): HistoryFreshness {
  if (!snapshot || snapshot.not_found) return { state: "unknown", through: null };
  const through = parseDate(snapshot.history_coverage_end);

  // Terminal park: nothing is queued and nothing will be queued.
  if (snapshot.history_status === "restricted") return { state: "restricted", through };

  if (!snapshot.history_complete) return { state: "unknown", through: null };

  const archive =
    snapshot.history_approximate === true || snapshot.history_kind === "public_star_actions";
  if (archive) return { state: "archive", through };

  // Exact series. Frozen iff it stops at or after the restriction date — an
  // exact series that ends earlier stopped for some older reason and would be
  // mislabelled by this notice.
  if (through && through.getTime() >= RESTRICTION_DATE) {
    return { state: "exact_frozen", through };
  }
  return { state: "exact_current", through };
}

/** Do we have anything worth telling a reader? */
export function needsNotice(freshness: HistoryFreshness): boolean {
  return freshness.state === "exact_frozen" || freshness.state === "restricted";
}

const MONTH_DAY_YEAR: Intl.DateTimeFormatOptions = {
  year: "numeric",
  month: "long",
  day: "numeric",
  timeZone: "UTC",
};

export function formatThrough(through: Date | null): string | null {
  return through ? through.toLocaleDateString("en-US", MONTH_DAY_YEAR) : null;
}

/**
 * The notice copy.
 *
 * Deliberately states a date and no counts. A star gap looks like a precise
 * fact and is not one: an archive series counts re-stars and can exceed the
 * repository's own star count, so publishing "shows N of M" would be confidently
 * wrong on exactly the repositories where it is most eye-catching.
 *
 * Tone rule: this describes gitdebt's read access, never the owner's conduct.
 */
export function noticeText(freshness: HistoryFreshness): string | null {
  const through = formatThrough(freshness.through);
  switch (freshness.state) {
    case "exact_frozen":
      return through
        ? `Star history is complete through ${through}. In July 2026 GitHub limited stargazer lists to a repository's own admins and collaborators, so this chart no longer updates unless the repository is connected to gitdebt.`
        : `In July 2026 GitHub limited stargazer lists to a repository's own admins and collaborators, so this chart no longer updates unless the repository is connected to gitdebt.`;
    case "restricted":
      return `GitHub serves this repository's stargazer list only to its own admins and collaborators, so gitdebt cannot read it. Connecting the repository restores the chart.`;
    default:
      return null;
  }
}
