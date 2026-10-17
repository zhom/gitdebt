/**
 * Embed mode: the page runs inside X's frame.
 *
 * X frames the page without letting the document scroll, so a framed page
 * locks the document and scrolls `[data-sheet-scroll]` instead (`globals.css`).
 * Every link opens in a new tab: the rest of the site refuses to be framed, and
 * sign-in cannot work in a third-party frame. Nothing is hidden before this
 * runs, so the page is complete without it.
 */

export const FRAMED_CLASS = "framed";

interface EmbedWindow {
  self: unknown;
  top: unknown;
  open(url: string, target: string, features: string): unknown;
}

interface EmbedClick {
  button: number;
  metaKey: boolean;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  defaultPrevented: boolean;
  target: unknown;
  preventDefault(): void;
}

interface EmbedDocument {
  documentElement: { classList: { add(token: string): void } };
  addEventListener(
    type: "click",
    listener: (event: EmbedClick) => void,
    capture: boolean,
  ): void;
}

export function isFramed(win: Pick<EmbedWindow, "self" | "top">): boolean {
  try {
    return win.self !== win.top;
  } catch {
    return true;
  }
}

/** The URL a click should open in a new tab, or null to let it through. */
export function leavingHref(event: EmbedClick): string | null {
  if (event.defaultPrevented || event.button !== 0) return null;
  if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return null;
  const target = event.target as {
    closest?: (selector: string) => { getAttribute(name: string): string | null; href: string } | null;
  } | null;
  const anchor = target?.closest?.("a[href]");
  if (!anchor) return null;
  const raw = anchor.getAttribute("href") ?? "";
  if (raw.startsWith("#")) return null;
  return anchor.href;
}

export function installEmbedMode(win: EmbedWindow, doc: EmbedDocument) {
  if (!isFramed(win)) return;
  doc.documentElement.classList.add(FRAMED_CLASS);
  // Capture phase, and no stopPropagation, so the site's own listeners still see the click.
  doc.addEventListener(
    "click",
    (event) => {
      const href = leavingHref(event);
      if (!href) return;
      event.preventDefault();
      win.open(href, "_blank", "noopener");
    },
    true,
  );
}
