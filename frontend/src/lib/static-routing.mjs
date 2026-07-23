// Static-hosting routing contract:
// - Astro builds with `build.format: "file"` and `trailingSlash: "never"`, so
//   `/about` is backed by `about.html` and `/owner/repo` by `owner/repo.html`.
//   Directory-style output (`about/index.html`) normalizes to `/about/` and
//   can loop against the no-trailing-slash canonical URLs.
// - The host applies `_redirects` before matching assets. Never add a
//   catch-all like `/:owner/:repo -> /report`, or it will shadow every
//   generated repository snapshot.
// - Maintainer profiles occupy the ROOT path: `/{login}` is backed by
//   `{login}.html`, so `github.com/<name>` rewrites to `gitdebt.com/<name>`.
//   Astro matches by segment count, so `/{login}` and `/{owner}/{repo}`
//   coexist. `/u/{login}` stays a 301 to `/{login}` for existing links.
// - The static-first fallback lives in the custom `noindex` 404 page: its
//   client shell uses this module to recognize a valid, non-reserved
//   two-segment repo slug or one-segment login and renders the live report
//   in place without changing the address bar. All other missing paths stay
//   genuine 404s.
// - `scripts/audit-routing.mjs` fails the build if this contract regresses.

/**
 * Every first path segment the app itself owns. A GitHub login that collides
 * with one of these is simply not published as a profile: the application
 * route wins, and the collision is never resolved at request time.
 */
export const RESERVED_FIRST_SEGMENTS = Object.freeze(
  new Set([
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
  ]),
);

const REPO_SEGMENT_RE = /^[A-Za-z0-9._-]+$/;

/** GitHub's own login grammar: alphanumeric with interior hyphens, ≤39. */
const LOGIN_RE = /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,37}[A-Za-z0-9])?$/;

/**
 * @param {unknown} segment
 * @returns {boolean}
 */
export function isReservedFirstSegment(segment) {
  return (
    typeof segment === "string" &&
    RESERVED_FIRST_SEGMENTS.has(segment.toLowerCase())
  );
}

/**
 * Normalize a candidate profile login, or return null when it is malformed or
 * reserved by the application itself.
 *
 * @param {unknown} value
 * @returns {string | null}
 */
export function profileLogin(value) {
  if (typeof value !== "string") return null;
  const login = value.trim().toLowerCase();
  if (!LOGIN_RE.test(login) || isReservedFirstSegment(login)) return null;
  return login;
}

/**
 * Return the canonical profile URL for a single-segment path.
 *
 * @param {string} pathname
 * @returns {string | null}
 */
export function missingProfileReportTarget(pathname) {
  if (typeof pathname !== "string") return null;

  const encoded = pathname.replace(/^\/+|\/+$/g, "");
  if (!encoded || encoded.includes("/")) return null;

  let segment;
  try {
    segment = decodeURIComponent(encoded);
  } catch {
    return null;
  }

  const login = profileLogin(segment);
  return login ? `/${login}` : null;
}

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
    isReservedFirstSegment(owner)
  ) {
    return null;
  }

  const slug = `${owner.toLowerCase()}/${repo.toLowerCase()}`;
  return `/${slug}`;
}
