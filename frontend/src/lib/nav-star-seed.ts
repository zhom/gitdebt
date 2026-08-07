import { loadBuildRepoSnapshot } from "@/lib/build-repo-snapshot";

/** The repository the nav badge counts: gitdebt itself. */
export const GITDEBT_REPO = "zhom/gitdebt";
export const GITDEBT_REPO_URL = `https://github.com/${GITDEBT_REPO}`;

let warned = false;

/**
 * Build-time seed for the nav star badge, or `null` when the API was
 * unreachable.
 *
 * This is a *seed*, not the source of truth. It exists so the served HTML
 * already carries the real number — no layout shift, and something correct for
 * a reader without JS. `NavStarCount` re-reads the live count after mount,
 * which is what keeps a statically built badge from freezing at whatever the
 * count happened to be on deploy day, and what reveals the badge at all when
 * this returns `null`.
 *
 * Deliberately not wired to `staticCatalogRequired()`: an unreachable API must
 * not fail a release over a nav ornament, and the client refresh makes a
 * missed seed recoverable rather than permanent.
 *
 * Lives here rather than in `SiteHeader.astro` for the `warned` flag alone.
 * Astro re-runs a component's frontmatter for every page, so a warning emitted
 * there fired once per page — 542 identical lines, which is how a real signal
 * becomes noise nobody reads. Module state persists across those renders; the
 * snapshot request itself was already shared this way.
 */
export async function navStarSeed(): Promise<number | null> {
  const snapshot = await loadBuildRepoSnapshot(GITDEBT_REPO);
  const stars =
    snapshot.data && Number.isFinite(snapshot.data.total_stars)
      ? snapshot.data.total_stars
      : null;
  if (stars === null && !warned) {
    warned = true;
    console.warn(
      `[SiteHeader] star badge has no build-time seed for ${GITDEBT_REPO}; ` +
        `falling back to the client refresh` +
        (snapshot.error ? ` (${snapshot.error})` : ""),
    );
  }
  return stars;
}
