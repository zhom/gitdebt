/**
 * The landing hero's ranked board, merged from the leaderboard's per-window
 * ranks at build time.
 *
 * `/leaderboard` merges four boards and ranks by the weekly pace because it has
 * the room to print every window beside it. The hero prints one movement figure
 * per row, so it ranks by the window it prints — the trailing month — and lets
 * a repository onto the board only if that window ranked it. Ranking by one
 * window while printing another makes an eight-row list read as non-monotonic
 * and broken, and a repository that only the weekly board knows would print an
 * em-dash in the hero's single headline column.
 *
 * Pure, with no DOM, no fetch and no wall clock, so the merge and the ordering
 * are unit tested under `node --test` (see `scripts/hero-momentum.test.mjs`).
 */

// Relative, with its extension: this module is covered by the Node test runner
// in `scripts/`, which resolves neither the `@/` alias nor an extensionless
// specifier.
import type { MomentumRow } from "./momentum.ts";

/** One row of `/api/leaderboard.json`. */
export type LeaderboardRow = {
  rank: number;
  repo: string;
  stars: number;
  velocity: number;
};

export type LeaderboardResponse = {
  metric: string;
  page: number;
  per_page: number;
  window_days: number;
  repos: LeaderboardRow[];
};

/** The three velocity boards, each `null` when its fetch degraded to omission. */
export type MomentumWindows = {
  d30: LeaderboardRow[] | null;
  d7: LeaderboardRow[] | null;
  d1: LeaderboardRow[] | null;
};

/**
 * Top `limit` repositories by stars gained in the trailing 30 days, carrying
 * every window each one was ranked in.
 *
 * The tighter windows are folded into rows that already exist rather than
 * seeding new ones: they supply the bar's leading tip and the trend arrow, not
 * eligibility. A window a repository is absent from stays `null`, never 0 —
 * the API returns a top-N per window, so absence means "not ranked here".
 */
export function heroMomentumRows(
  windows: MomentumWindows,
  limit: number,
): MomentumRow[] {
  const board = new Map<string, MomentumRow>();
  for (const row of windows.d30 ?? []) {
    const existing = board.get(row.repo);
    // The API ranks each repository once per board, but a duplicate slug must
    // not silently overwrite a higher figure.
    if (existing) {
      existing.stars = Math.max(existing.stars, row.stars);
      existing.d30 = Math.max(existing.d30 ?? 0, row.velocity);
      continue;
    }
    board.set(row.repo, {
      repo: row.repo,
      stars: row.stars,
      d1: null,
      d7: null,
      d30: row.velocity,
    });
  }

  const fold = (rows: LeaderboardRow[] | null, field: "d1" | "d7") => {
    for (const row of rows ?? []) {
      const existing = board.get(row.repo);
      if (!existing) continue;
      // Star totals are snapshotted per board and can differ between them;
      // take the largest rather than letting fold order decide.
      existing.stars = Math.max(existing.stars, row.stars);
      existing[field] = row.velocity;
    }
  };
  fold(windows.d7, "d7");
  fold(windows.d1, "d1");

  return [...board.values()]
    .sort((a, b) => {
      const pace = (b.d30 ?? 0) - (a.d30 ?? 0);
      if (pace !== 0) return pace;
      const size = b.stars - a.stars;
      if (size !== 0) return size;
      // Slug last, so a genuine tie orders the same way on every build rather
      // than inheriting whatever order the API happened to serve.
      return a.repo < b.repo ? -1 : a.repo > b.repo ? 1 : 0;
    })
    .slice(0, Math.max(0, limit));
}
