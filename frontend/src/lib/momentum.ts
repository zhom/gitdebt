/**
 * Reading a repository's momentum from the leaderboard's per-window ranks.
 *
 * The API ranks separately for a 1-, 7- and 30-day window and returns a top-N
 * of each. Those three lists describe the same repositories from three angles,
 * and the question a trending board exists to answer — is this climbing or
 * cooling — lives in the comparison between them, not in any one.
 *
 * Pure, with no DOM or framework imports, so the rules can be unit tested under
 * `node --test` (see `scripts/momentum-trend.test.mjs`).
 */

/**
 * One repository across every window the board loaded.
 *
 * `null` means "not ranked in that window", which is not zero: a repository can
 * lead the day and sit outside the month's top ranks. Rendering that absence as
 * `0` would invent a fact, and would read as "gained nothing".
 */
export type MomentumRow = {
  repo: string;
  stars: number;
  /** Stars gained in the trailing 1 / 7 / 30 days. */
  d1: number | null;
  d7: number | null;
  d30: number | null;
};

export type Trend = "rising" | "steady" | "fading" | "unknown";

/** Ratio of the short window's rate to the long one above which a repo climbs. */
const RISING = 1.2;
/** And below which it is cooling. */
const FADING = 0.8;

/**
 * Per-day rates, which is what makes the three windows comparable at all.
 *
 * 70 stars in a week and 300 in a month are not on the same scale until they
 * are divided; once they are, both are 10/day and the comparison is real.
 */
export function rates(row: MomentumRow) {
  return {
    r1: row.d1,
    r7: row.d7 === null ? null : row.d7 / 7,
    r30: row.d30 === null ? null : row.d30 / 30,
  };
}

/**
 * Direction, from the widest pair of windows this repository actually has.
 *
 * Requiring the daily *and* monthly rank left 93 of 99 rows unknown on a real
 * board, because most repositories appear in only one window's top-N. Any
 * short-versus-long pair answers the same question — a repo whose weekly pace
 * outruns its monthly pace is climbing just as truly as one whose day outruns
 * its month — so this takes the widest pair present and returns `unknown` only
 * when there is genuinely nothing to compare.
 */
export function trendOf(row: MomentumRow): Trend {
  const { r1, r7, r30 } = rates(row);
  // Pick the pair structurally, widest span first. Choosing by value instead
  // would make a repository holding exactly its pace — the textbook "steady"
  // case, where every window reports the same rate — look like it had nothing
  // to compare.
  let short: number | null = null;
  let long: number | null = null;
  if (r1 !== null && r30 !== null) [short, long] = [r1, r30];
  else if (r1 !== null && r7 !== null) [short, long] = [r1, r7];
  else if (r7 !== null && r30 !== null) [short, long] = [r7, r30];

  if (short === null || long === null || long <= 0) return "unknown";
  const ratio = short / long;
  if (ratio >= RISING) return "rising";
  if (ratio <= FADING) return "fading";
  return "steady";
}

/**
 * The pace the board ranks by: the weekly rate, or the day extrapolated when a
 * repository is ranked only there.
 */
export function weeklyPace(row: MomentumRow): number {
  return row.d7 ?? (row.d1 === null ? 0 : row.d1 * 7);
}
