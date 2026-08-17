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
 *
 * It also describes only what the product does TODAY. There is no repository
 * connection flow — no endpoint, no install link, no grant field in the analyze
 * payload — so no sentence here may offer connecting a repository as a
 * remedy the reader can take. It would be an invented capability, and it would
 * contradict both the privacy policy and the sign-in caption these sentences
 * are rendered beside. State the restriction; do not promise a way around it.
 */
export function noticeText(freshness: HistoryFreshness): string | null {
  const through = formatThrough(freshness.through);
  const restriction =
    "In July 2026 GitHub limited stargazer lists to a repository's own admins and collaborators";
  switch (freshness.state) {
    case "exact_frozen":
      return through
        ? `Star history is complete through ${through}. ${restriction}, so gitdebt can no longer read new stars from that list and this chart stops there.`
        : `${restriction}, so gitdebt can no longer read new stars from that list and this chart stops where it does.`;
    case "restricted":
      return `GitHub serves this repository's stargazer list only to its own admins and collaborators, so gitdebt cannot read it.`;
    default:
      return null;
  }
}

/* ------------------------------------------------------------------------- *
 * Provenance vocabulary.
 *
 * Three facts, and only three: which SOURCE produced the series, the DATE its
 * coverage runs to, and the STATE of that read. Never a count, never a share,
 * never a score, and never a sentence whose subject is the repository owner —
 * every one of these describes gitdebt's read access.
 *
 * They live here rather than in the component so the wording exists once and
 * is unit-testable without a DOM, and so `sourceDetail` can delegate the two
 * July-2026 sentences straight to `noticeText`.
 * ------------------------------------------------------------------------- */

/** What produced the points. Not a judgement, a method. */
export function sourceLabel(freshness: HistoryFreshness): string {
  switch (freshness.state) {
    case "exact_current":
    case "exact_frozen":
      return "GitHub stargazer list";
    case "archive":
      return "Historical star data";
    case "restricted":
      return "No readable source";
    case "unknown":
      return "Source not established";
  }
}

/** Whether the series still receives points, said in words. */
export function stateLabel(freshness: HistoryFreshness): string {
  switch (freshness.state) {
    case "exact_current":
    case "archive":
      return "Still updating";
    case "exact_frozen":
      return "No longer updating";
    case "restricted":
      return "Cannot be read";
    case "unknown":
      return "Being read";
  }
}

/** How far the series runs. A date, never a proportion of anything. */
export function coverageLabel(freshness: HistoryFreshness): string {
  const through = formatThrough(freshness.through);
  return through ? `Covers through ${through}` : "Coverage window not established";
}

/** The paragraph under the mark. Two states delegate, so they are never retyped. */
export function sourceDetail(freshness: HistoryFreshness): string {
  switch (freshness.state) {
    case "exact_current":
      return "Every point is one star with its own timestamp, read from GitHub's stargazer list.";
    case "archive":
      return "Rebuilt from historical star data. Star actions are recorded and unstars are not, so this is an attention signal rather than a net star count.";
    case "exact_frozen":
    case "restricted":
      // `noticeText` is total for these two states; the fallback exists only so
      // the return type stays a plain string, and it asserts nothing.
      return noticeText(freshness) ?? UNESTABLISHED;
    case "unknown":
      return UNESTABLISHED;
  }
}

const UNESTABLISHED =
  "gitdebt has not established a source for this series yet. The chart appears once one is.";

/**
 * Dither coverage for the provenance mark, 0..1.
 *
 * A fixed constant per state, and deliberately not a function of anything the
 * series measures. A density derived from coverage would be a completeness
 * score wearing a texture, and the counts rule forbids publishing one: an
 * archive series counts re-stars and can exceed the repository's own star
 * total, so any "how much of it do we have" figure is wrong exactly where it
 * looks most precise. Density here says *which source*, nothing more.
 */
export function sourceDensity(freshness: HistoryFreshness): number {
  switch (freshness.state) {
    case "exact_current":
    case "exact_frozen":
      return 0.85;
    case "archive":
      return 0.45;
    case "restricted":
    case "unknown":
      return 0.12;
  }
}

/** True iff the series still receives points. Drives the mark's aperture. */
export function seriesOpen(freshness: HistoryFreshness): boolean {
  return freshness.state === "exact_current" || freshness.state === "archive";
}
