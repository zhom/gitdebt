const RESERVED_FIRST_SEGMENTS = new Set([
  "_astro",
  "404",
  "about",
  "api",
  "badges",
  "compare",
  "favicon.ico",
  "leaderboard",
  "privacy",
  "profile",
  "report",
  "robots.txt",
  "sitemap-index.xml",
  "sitemaps",
  "terms",
  "u",
  "vs",
]);

const REPO_SEGMENT_RE = /^[A-Za-z0-9._-]+$/;

/**
 * Return the live-report URL for a missing two-segment repository snapshot.
 * Existing static files are served by Cloudflare Pages before its 404 page
 * runs this helper, so this is intentionally a client-side fallback.
 *
 * @param {string} pathname
 * @returns {string | null}
 */
export function missingRepoReportTarget(pathname) {
  if (typeof pathname !== "string") return null;

  const encoded = pathname.replace(/^\/+|\/+$/g, "");
  const segments = encoded.split("/");
  if (segments.length !== 2 || segments.some((segment) => !segment)) {
    return null;
  }

  let owner;
  let repo;
  try {
    [owner, repo] = segments.map((segment) => decodeURIComponent(segment));
  } catch {
    return null;
  }

  if (
    !REPO_SEGMENT_RE.test(owner) ||
    !REPO_SEGMENT_RE.test(repo) ||
    owner === "." ||
    owner === ".." ||
    repo === "." ||
    repo === ".." ||
    RESERVED_FIRST_SEGMENTS.has(owner.toLowerCase())
  ) {
    return null;
  }

  const slug = `${owner.toLowerCase()}/${repo.toLowerCase()}`;
  return `/report?repo=${encodeURIComponent(slug)}`;
}
