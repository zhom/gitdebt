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

  // `/{login}.md` is the documented Markdown representation of a profile, and
  // a login can never contain a dot, so the suffix is dropped before the
  // login grammar rejects it outright.
  const login = profileLogin(baseRepoSegment(segment));
  return login ? `/${login}` : null;
}

/**
 * Drop the `.md` representation suffix from a repository segment. `.md` is the
 * one alternate representation the site itself emits, so `/{owner}/{repo}.md`
 * means the report for `{owner}/{repo}` rather than a repository literally
 * named `{repo}.md`. Nothing else is stripped: `.json`, `.csv`, `.svg` and
 * `.png` are API paths that this site never emits as pages, and `owner/tool.js`
 * is an ordinary repository name.
 *
 * @param {string} segment
 * @returns {string}
 */
export function baseRepoSegment(segment) {
  return segment.length > 3 && segment.endsWith(".md")
    ? segment.slice(0, -3)
    : segment;
}

/**
 * The one place a two-segment path becomes a repository. Both the 404 URL
 * fallback and the live report parse the same grammar, so they cannot drift
 * apart on suffix stripping, reserved owners, or relative segments.
 *
 * @param {string} owner
 * @param {string} segment
 * @returns {{ owner: string; repo: string } | null}
 */
function repoSlug(owner, segment) {
  const repo = baseRepoSegment(segment);
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
  return { owner: owner.toLowerCase(), repo: repo.toLowerCase() };
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

  let decoded;
  try {
    decoded = segments.map((segment) => decodeURIComponent(segment));
  } catch {
    return null;
  }

  const slug = repoSlug(decoded[0], decoded[1]);
  return slug ? `/${slug.owner}/${slug.repo}` : null;
}

/**
 * The repository a client-side live report should analyze, read from the
 * document location. `?repo=` wins over the path because `/report?repo=…` is
 * how the homepage lookup hands a slug over; the path form is what the 404
 * fallback sees when `github.com/{owner}/{repo}` is rewritten here.
 *
 * The location is used raw rather than decoded: an escape sequence never
 * survives the repository grammar, so decoding could only widen what enqueues
 * an analysis.
 *
 * @param {unknown} pathname
 * @param {unknown} search
 * @returns {{ owner: string; repo: string } | null}
 */
export function liveReportRepo(pathname, search) {
  const queryRepo =
    typeof search === "string" ? new URLSearchParams(search).get("repo") : null;
  const pathRepo =
    typeof pathname === "string" ? pathname.replace(/^\/+|\/+$/g, "") : "";

  const segments = (queryRepo ?? pathRepo).trim().split("/");
  if (segments.length !== 2 || segments.some((segment) => !segment)) {
    return null;
  }
  return repoSlug(segments[0], segments[1]);
}
