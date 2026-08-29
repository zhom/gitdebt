/**
 * The document title, corrected once a live route knows what it is showing.
 *
 * The defect this exists to fix: gitdebt prerenders a page only for the
 * repositories in its build catalog. Every other repository is resolved in the
 * browser, and five routes rendered a complete, correct report for a real
 * subject while the tab still said something else —
 *
 *   /404              "Page not found · gitdebt", showing a real repository
 *                     (and, for a single-segment path, a real user profile)
 *   /report?repo=     "Live GitHub repository report · gitdebt"
 *   /profile?login=   "Your GitHub profile report · gitdebt", which is simply
 *                     false for any login that is not the viewer's
 *   /compare?repos=   a fixed title while the URL names specific repositories
 *   /badges?repo=     a fixed title, while BadgeStudio calls history.replaceState
 *                     to write ?repo=owner/name into the address bar
 *
 * Nothing in the frontend assigned `document.title` anywhere, so nothing ever
 * corrected them.
 *
 * What this can and cannot fix, stated honestly: it fixes the tab, the
 * bookmark, the history entry and the window title — everything a person sees.
 * It cannot fix a social preview or a search snippet for a repository that has
 * no prerendered page, because a crawler reads the served HTML and never runs
 * this. That is a real limit of a fully static site, not something to paper
 * over: those routes are `noindex` precisely because their served HTML does not
 * describe their live content.
 */

/** Titles read as "<subject> · gitdebt", which is the site's existing shape. */
const SUFFIX = " · gitdebt";

/** Keeps the tag a browser actually shows, and matches Seo.astro's own limit. */
const MAX_TITLE = 70;

function truncate(value: string, maxLength: number): string {
  const normalized = value.trim().replace(/\s+/g, " ");
  const characters = Array.from(normalized);
  if (characters.length <= maxLength) return normalized;
  return `${characters.slice(0, maxLength - 1).join("")}…`;
}

function setMeta(selector: string, attribute: string, value: string): void {
  const node = document.head.querySelector(selector);
  if (node) node.setAttribute(attribute, value);
}

export type LiveSubject = {
  /** What this page is actually showing, e.g. "facebook/react". */
  subject: string;
  /** One sentence describing it. Falls back to the served description. */
  description?: string;
  /**
   * The URL this page would live at if it were prerendered, e.g.
   * "/facebook/react". Corrects the canonical and the og:url so a copied link
   * and a bookmark point somewhere meaningful.
   */
  path?: string;
  /** A subject-specific social image, when the API can render one. */
  image?: string;
};

/**
 * Retitle the document for the subject a live route has resolved.
 *
 * Safe to call repeatedly and safe to call with the same subject twice; it is
 * a plain assignment, not a queue. Call it only once real data has resolved —
 * retitling to a slug that then 404s is a worse lie than the generic title.
 */
export function setLiveSubject({
  subject,
  description,
  path,
  image,
}: LiveSubject): void {
  if (typeof document === "undefined") return;
  const trimmed = subject.trim();
  if (!trimmed) return;

  document.title = truncate(`${trimmed}${SUFFIX}`, MAX_TITLE);
  setMeta('meta[property="og:title"]', "content", document.title);
  setMeta('meta[name="twitter:title"]', "content", document.title);

  if (description) {
    const text = truncate(description, 160);
    setMeta('meta[name="description"]', "content", text);
    setMeta('meta[property="og:description"]', "content", text);
    setMeta('meta[name="twitter:description"]', "content", text);
  }

  if (path) {
    const url = new URL(path, window.location.origin).href;
    setMeta('link[rel="canonical"]', "href", url);
    setMeta('meta[property="og:url"]', "content", url);
  }

  if (image) {
    setMeta('meta[property="og:image"]', "content", image);
    setMeta('meta[property="og:image:url"]', "content", image);
    setMeta('meta[name="twitter:image"]', "content", image);
  }
}

/**
 * Put the served title back.
 *
 * A single-page transition that leaves a stale subject in the tab is the same
 * bug in the other direction, so a route that can un-resolve (a cleared search,
 * a failed lookup) restores what the server sent.
 */
export function restoreServedTitle(): void {
  if (typeof document === "undefined") return;
  const served = document.head.querySelector<HTMLMetaElement>(
    'meta[name="gitdebt-served-title"]',
  )?.content;
  if (served) document.title = served;
}
