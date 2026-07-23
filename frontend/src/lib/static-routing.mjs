// Static-hosting routing contract:
// - Astro builds with `build.format: "file"` and `trailingSlash: "never"`, so
//   `/about` is backed by `about.html` and `/owner/repo` by `owner/repo.html`.
//   Directory-style output (`about/index.html`) normalizes to `/about/` and
//   can loop against the no-trailing-slash canonical URLs.
// - The host applies `_redirects` before matching assets. Never add a
//   catch-all like `/:owner/:repo -> /report`, or it will shadow every
//   generated repository snapshot.
// - The static-first fallback lives in the custom `noindex` 404 page: its
//   client shell uses this module to recognize a valid, non-reserved
//   two-segment repo slug and renders the live report in place without
//   changing the address bar. All other missing paths stay genuine 404s.
// - `scripts/audit-routing.mjs` fails the build if this contract regresses.
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
 * Return the canonical repository URL for a valid two-segment slug.
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
  return `/${slug}`;
}
