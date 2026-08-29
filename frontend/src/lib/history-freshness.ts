/**
 * What a repository's star history actually covers, and why.
 *
 * One boolean cannot say this honestly. Since GitHub restricted the stargazer
 * list to a repository's own admins and collaborators (July 2026), a chart can
 * be behind for reasons that are not interchangeable: the exact series stopped
 * on the day the endpoint closed; the approximate series is still flowing; the
 * exact series stopped and approximate activity was spliced onto its end;
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
  /**
   * "current_stargazers" (exact) | "public_star_actions" (archive) |
   * "stargazers_then_activity" (exact through the splice, archive after it) |
   * "unavailable"
   */
  history_kind?: string;
  history_approximate?: boolean;
  history_status?: string;
  history_coverage_end?: string | null;
  /**
   * Where a spliced series changes method: exact points at or before this
   * instant, archive-derived points strictly after it. Optional like the rest —
   * a payload that omits it still classifies as `spliced`, and the copy states
   * the method change without naming the day rather than inventing one.
   */
  history_splice_at?: string | null;
  not_found?: boolean;
};

export type HistoryFreshness =
  /** Exact, from the stargazer list, and still being refreshed. */
  | { state: "exact_current"; through: Date | null }
  /** Exact, but the endpoint closed — the series stops on a fixed date. */
  | { state: "exact_frozen"; through: Date | null }
  /** Archive-derived: approximate by nature (no unstars), still flowing. */
  | { state: "archive"; through: Date | null }
  /**
   * Exact up to `splicedAt`, archive-derived after it. Two methods in one line,
   * which is exactly the case provenance exists to disclose: the head counts
   * current stargazers and the tail counts star actions, so the curve does not
   * mean the same thing on both sides of the join.
   */
  | { state: "spliced"; through: Date | null; splicedAt: Date | null }
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

  // Before the archive branch, and keyed on the kind alone. A spliced series is
  // approximate in its tail and therefore carries `history_approximate: true`,
  // so the test below would otherwise swallow it and describe a curve that is
  // mostly exact as if none of it were.
  if (snapshot.history_kind === "stargazers_then_activity") {
    return { state: "spliced", through, splicedAt: parseDate(snapshot.history_splice_at) };
  }

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

/**
 * Do we have anything worth telling a reader?
 *
 * `spliced` qualifies even though the series is healthy and still advancing:
 * the line changes meaning partway along, and a reader who takes the tail for
 * more of the same head is reading a different measurement than they think.
 */
export function needsNotice(freshness: HistoryFreshness): boolean {
  return (
    freshness.state === "exact_frozen" ||
    freshness.state === "restricted" ||
    freshness.state === "spliced"
  );
}

const MONTH_DAY_YEAR: Intl.DateTimeFormatOptions = {
  year: "numeric",
  month: "long",
  day: "numeric",
  timeZone: "UTC",
};

/**
 * A coverage instant in the reader's prose form. Two different instants share
 * it — where coverage ends, and where a spliced series changes method — because
 * a reader comparing them should not have to reconcile two date formats.
 */
export function formatThrough(through: Date | null): string | null {
  return through ? through.toLocaleDateString("en-US", MONTH_DAY_YEAR) : null;
}

/**
 * Who the restriction shuts out, said from gitdebt's side of it.
 *
 * These sentences used to name the exception — "only to a repository's own
 * admins and collaborators" — which is a true description of GitHub's rule and
 * the wrong sentence to put in front of the one reader most likely to see it.
 * A repository's own owner reads "admins and collaborators" as *so it should
 * work for me*, signs in, and finds the chart still stopped, because gitdebt
 * reads GitHub with its own application credentials and those administer
 * nothing. Naming the people who can read the list, in a product that cannot,
 * is an invitation to a door that does not exist.
 *
 * So: state the limit as it applies to gitdebt, and close the sign-in door in
 * the same breath. No remedy is offered because there is none to offer — there
 * is no repository connection flow, no endpoint, no install link and no grant
 * field in the analyze payload — and no sentence hints that one may appear,
 * which would be a promise this module cannot keep. The wording also has to
 * survive being read beside the sign-in caption on the same card, which already
 * says in as many words that signing in does not restore a stargazer read.
 *
 * The readers GitHub kept are one shared phrase rather than two hand-written
 * ones, because `NO_SIGN_IN` says "not one of them" and every sentence it joins
 * has to leave that pronoun the same antecedent to point at.
 */
const ADMINISTERING_APPS = "applications that administer the repository";
const RESTRICTION = `GitHub restricted stargazer lists to ${ADMINISTERING_APPS}`;
const NO_SIGN_IN =
  "gitdebt is not one of them, and signing in — even as this repository's owner — does not change what gitdebt can read";

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
 * connection flow, so no sentence here may offer connecting a repository as a
 * remedy the reader can take. It would be an invented capability, and it would
 * contradict both the privacy policy and the sign-in caption these sentences
 * are rendered beside. State the restriction; do not promise a way around it.
 *
 * The switch is exhaustive on purpose: `string | null` plus `strictNullChecks`
 * makes a new unhandled state a compile error rather than a silent `undefined`.
 */
export function noticeText(freshness: HistoryFreshness): string | null {
  const through = formatThrough(freshness.through);
  switch (freshness.state) {
    case "exact_frozen":
      return through
        ? `Star history is complete through ${through}. In July 2026 ${RESTRICTION}. ${NO_SIGN_IN}, so the exact series ends there.`
        : `In July 2026 ${RESTRICTION}. ${NO_SIGN_IN}, so the exact series ends where it does.`;
    case "restricted":
      return `GitHub serves this repository's stargazer list only to ${ADMINISTERING_APPS}. ${NO_SIGN_IN}.`;
    case "spliced":
      return spliceNotice(freshness.splicedAt);
    case "exact_current":
    case "archive":
    case "unknown":
      return null;
  }
}

/**
 * The hardest sentence in this module.
 *
 * Four facts have to land at once, and three of them pull against each other:
 *
 *  1. The line changes method partway along, on a stated date. A curve whose
 *     semantics change mid-line is precisely what provenance exists to
 *     disclose, and it is invisible in the drawing.
 *  2. The head counts *current stargazers*; the tail counts *star actions*.
 *     Different measurements, not different resolutions of the same one.
 *  3. The tail is an approximate read that does not record every star, so a
 *     tail that flattens may be the source thinning and not the repository
 *     losing momentum. Leaving this out lets an honest chart tell a lie about
 *     somebody's project, which is the failure this whole state exists to fix.
 *  4. None of that may be quantified. No count, no share, no gap: an archive
 *     series counts re-stars and can exceed the repository's own star total, so
 *     any figure would be wrong exactly where it looks most precise. "Does not
 *     record every star" says *some are missing* and refuses to say how many,
 *     which is the most that can be said truthfully.
 */
function spliceNotice(splicedAt: Date | null): string {
  const at = formatThrough(splicedAt);
  const head = at
    ? `Two sources in one line, joined on ${at}. Every point up to that date is one current stargazer, timestamped, read from GitHub's stargazer list.`
    : `Two sources in one line. Every point up to the join is one current stargazer, timestamped, read from GitHub's stargazer list.`;
  return `${head} After it the series counts star actions instead: an approximate read that cannot see unstars and does not record every star, so a flatter tail can be the source thinning rather than this repository slowing.`;
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
 * is unit-testable without a DOM, and so `sourceDetail` can delegate every
 * exceptional-state sentence straight to `noticeText`.
 * ------------------------------------------------------------------------- */

/** What produced the points. Not a judgement, a method. */
export function sourceLabel(freshness: HistoryFreshness): string {
  switch (freshness.state) {
    case "exact_current":
    case "exact_frozen":
      return "GitHub stargazer list";
    case "archive":
      return "Historical star data";
    // Both sources, in the order the line uses them. Naming only one would be
    // false about half the curve, and "then" is doing real work: it is the only
    // part of the label that says the series changes method partway along.
    case "spliced":
      return "GitHub stargazer list, then historical star data";
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
    // A spliced series genuinely advances again — that is the whole point of
    // splicing one. What changed is the method, and the method is the source
    // label's job to say, not this one's.
    case "spliced":
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

/** The paragraph under the mark. Three states delegate, so they are never retyped. */
export function sourceDetail(freshness: HistoryFreshness): string {
  switch (freshness.state) {
    case "exact_current":
      return "Every point is one star with its own timestamp, read from GitHub's stargazer list.";
    case "archive":
      // The second sentence is not decoration. Historical star data does not
      // record every star, and how much it records has varied over time, so a
      // flattening tail is as likely to be the source thinning as the
      // repository slowing. Saying only "attention signal" leaves the reader
      // to draw the wrong conclusion from the shape of the line.
      return "Rebuilt from historical star data. Star actions are recorded and unstars are not, so this is an attention signal rather than a net star count — and it does not record every star, so a flatter stretch can be the source thinning rather than this repository slowing.";
    case "exact_frozen":
    case "restricted":
    case "spliced":
      // `noticeText` is total for these three states; the fallback exists only
      // so the return type stays a plain string, and it asserts nothing.
      return noticeText(freshness) ?? UNESTABLISHED;
    case "unknown":
      return UNESTABLISHED;
  }
}

const UNESTABLISHED =
  "gitdebt has not established a source for this series yet. The chart appears once one is.";

/**
 * The line weight a drawing would give this series, as an SVG dash array.
 *
 * A technical drawing already has a vocabulary for "how certain is this edge",
 * and it is the dash pattern, not a tint or a texture. A solid line is an
 * object line: an edge that was measured. A dashed line is a construction
 * line: real, drawn, load-bearing, and derived rather than observed. A fine
 * dotted line is a line whose subject could not be measured at all.
 *
 * So:
 *
 *   exact              solid       — one point per star, each with a timestamp
 *   spliced            long dash   — the pattern its TAIL is drawn with; the
 *                                    head is the exact line and stays solid,
 *                                    which is the whole disclosure
 *   archive            short dash  — a construction line, derived from records
 *                                    of star actions rather than read directly
 *   restricted/unknown fine dots   — nothing was measured
 *
 * A fixed constant per state, and deliberately not a function of anything the
 * series measures. A dash gap scaled by coverage would be a completeness score
 * wearing a line style, and the counts rule forbids publishing one: an archive
 * series counts re-stars and can exceed the repository's own star total, so any
 * "how much of it do we have" figure is wrong exactly where it looks most
 * precise. The pattern says *which source*, nothing more — which is why this
 * takes a freshness and no magnitude.
 */
export function sourceStroke(freshness: HistoryFreshness): string {
  switch (freshness.state) {
    case "exact_current":
    case "exact_frozen":
      return "";
    // One pattern, applied to the tail only. It is longer than the archive
    // dash because it is not a different measurement from the head so much as
    // the same line continued by another method, and the drawing should read
    // as one line changing hand rather than two lines butted together.
    case "spliced":
      return "9 4";
    case "archive":
      return "5 4";
    case "restricted":
    case "unknown":
      return "1 3";
  }
}

/** True iff the series still receives points. Drives the mark's aperture. */
export function seriesOpen(freshness: HistoryFreshness): boolean {
  return (
    freshness.state === "exact_current" ||
    freshness.state === "archive" ||
    freshness.state === "spliced"
  );
}
