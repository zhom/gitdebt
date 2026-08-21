/**
 * Why a repository page is not indexable — and, more importantly, whether that
 * is a fact about the repository or a defect in the build that produced it.
 *
 * `[owner]/[repo].astro` used to collapse both into one boolean:
 *
 *     const noindex = data === null || fetchError !== null || !hasHistory;
 *
 * A repository with nothing to show yet is legitimately not indexable. A page
 * whose analyze read failed is a build that could not *ask*, wearing the same
 * costume. Once the two are one boolean, nothing downstream can tell them
 * apart: four local builds of identical code emitted 439, 514, 380 and 154
 * indexable pages out of the same 542, and every one of them finished with
 * "SEO audit: passed".
 *
 * The robots meta is deliberately unchanged — every reason below still emits
 * `noindex,follow`. This module exists so the build can say WHY, not so search
 * engines see something different.
 */

/**
 * The meta tag a page carries when it was de-indexed because the build could
 * not read its snapshot.
 *
 * `scripts/audit-seo.mjs` runs as plain `.mjs` under `node` with no type
 * stripping, so it cannot import this module and keeps its own copy of these
 * two strings. `repo-indexability.test.mjs` asserts the copies are identical.
 *
 * The marker can never reach production: a production build
 * (`STATIC_CATALOG_REQUIRED=1`) throws on an unreadable snapshot before the
 * page renders, so the marker appearing in output is itself proof that the
 * build was not a production one.
 */
export const BUILD_DEFECT_META = {
  name: "gitdebt:build-defect",
  unreachable: "snapshot-unreachable",
} as const;

export type NoindexReason =
  /** The backend answered, and this repository has no star history yet. */
  | "no-history"
  /** The backend answered that this repository is not public. */
  | "not-found"
  /**
   * The build could not read a snapshot at all: network failure, timeout, or an
   * error status. Nothing about this outcome is a fact about the repository.
   */
  | "unreachable";

export type RepoIndexability = {
  /** Exactly the boolean the page passed to `<Seo noindex>` before the split. */
  noindex: boolean;
  reason: NoindexReason | null;
  /** `reason === "unreachable"`, named for the thing callers act on. */
  buildDefect: boolean;
};

export type RepoSnapshotOutcome = {
  /** The backend returned an analyze payload — whatever that payload contains. */
  hasSnapshot: boolean;
  /** Message from a failed analyze read; null when the read succeeded. */
  fetchError: string | null;
  /** The backend reported the repository as missing, private, or deleted. */
  notFound: boolean;
  /** The returned payload carries at least one dated point. */
  hasHistory: boolean;
};

const DEFECT: RepoIndexability = {
  noindex: true,
  reason: "unreachable",
  buildDefect: true,
};

export function repoIndexability({
  hasSnapshot,
  fetchError,
  notFound,
  hasHistory,
}: RepoSnapshotOutcome): RepoIndexability {
  // A failed read outranks every other reason. It is the only outcome that
  // says nothing about the repository, and the emptiness that follows from it
  // is a consequence of the failure rather than a finding about the project.
  if (fetchError !== null) return DEFECT;
  if (notFound) {
    return { noindex: true, reason: "not-found", buildDefect: false };
  }
  // No payload, no error, no tombstone: the build never got an answer it can
  // describe, which is the same defect by a quieter route.
  if (!hasSnapshot) return DEFECT;
  if (!hasHistory) {
    return { noindex: true, reason: "no-history", buildDefect: false };
  }
  return { noindex: false, reason: null, buildDefect: false };
}
