import { setLiveSubject, type LiveSubject } from "@/lib/live-title";

/**
 * One guard in front of `setLiveSubject`, shared by every live island.
 *
 * `lib/live-title.ts` corrects a tab that is lying. This decides whether it is
 * lying at all, because the same island mounts on two kinds of route:
 *
 *   /404, /report   the server had no idea what this page would show, so its
 *                   title is generic and the resolved subject must replace it.
 *   /{login}        the server prerendered a page FOR this subject and wrote a
 *                   fuller title than a bare slug — "zhom — GitHub star history
 *                   and repo health". Overwriting that with "zhom" is the same
 *                   defect pointed the other way: a worse title in the tab, the
 *                   bookmark and the history entry.
 *
 * The test is the canonical link the server emitted. When it already points at
 * the path this subject would live at, the served page IS this subject's page
 * and its title is authoritative; anything else means the tab is describing a
 * different document than the one on screen. Reading the served canonical
 * rather than hard-coding each page's title formula means a page can reword its
 * own title without this file drifting away from it.
 */
function servedPath(): string | null {
  const href = document.head.querySelector<HTMLLinkElement>(
    'link[rel="canonical"]',
  )?.href;
  if (!href) return null;
  try {
    return new URL(href).pathname.replace(/\/+$/, "") || "/";
  } catch {
    return null;
  }
}

/**
 * Retitle the document for a resolved subject, unless the server already
 * published this exact page.
 *
 * Call it only once real data has come back. Retitling to a slug that then
 * turns out not to exist is a worse lie than the generic title it replaced.
 */
export function publishLiveSubject(subject: LiveSubject): void {
  if (typeof document === "undefined") return;
  const path = subject.path?.replace(/\/+$/, "") || subject.path;
  if (path && path === servedPath()) return;
  setLiveSubject(subject);
}
