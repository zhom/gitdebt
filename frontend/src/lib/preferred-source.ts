/**
 * "Make us preferred on Google": the reader marks gitdebt.com as a preferred
 * source in their own Google account.
 *
 * Google's publisher library runs in manual mode, so the button is ours and
 * the library only opens Google's dialog. Every call goes through the
 * `PREFERRED_SOURCE` queue: the library may still be loading when a reader
 * clicks, and a queued click runs as soon as it is ready instead of being
 * dropped. Nothing here runs during the static render; the page script calls
 * `installPreferredSource` in the browser.
 */

export const PREFERRED_SOURCE_SCRIPT_ID = "google-preferred-source";
export const PREFERRED_SOURCE_SCRIPT_SRC =
  "https://news.google.com/swg/js/v1/publisher.js";
/** The attribute every button carries; the footer and the article card share it. */
export const PREFERRED_SOURCE_ATTRIBUTE = "data-prefer-on-google";

/**
 * Copy for every locale the site ships. The label stays identical in the
 * footer and on the article card so a reader recognises it. No ranking claim:
 * Google says a preferred source is more likely to appear in Top Stories and
 * can be badged in AI Mode and AI Overviews, and nothing more.
 */
export const PREFERRED_SOURCE_COPY = {
  en: {
    button: "Make us preferred on Google",
    heading: "See us more often in Google",
    description:
      "One click marks gitdebt as a preferred source in your Google account, so our articles are more likely to appear in your Top Stories and can be badged in AI Mode and AI Overviews.",
  },
} as const;

export type PreferredSourceLocale = keyof typeof PREFERRED_SOURCE_COPY;

export function preferredSourceCopy(locale: string) {
  const key = locale.toLowerCase().split("-")[0];
  return key in PREFERRED_SOURCE_COPY
    ? PREFERRED_SOURCE_COPY[key as PreferredSourceLocale]
    : PREFERRED_SOURCE_COPY.en;
}

export interface PreferredSourceApi {
  init(options: { theme: "light" | "dark"; lang: string }): void;
  addPreferredSource(): void;
}

type Callback = (api: PreferredSourceApi) => void;

interface QueueHost {
  PREFERRED_SOURCE?: Callback[];
}

/** The parts of `document` this module touches, so a test can supply its own. */
export interface PreferredSourceDocument {
  documentElement: {
    lang: string;
    getAttribute(name: string): string | null;
    classList: { contains(token: string): boolean };
  };
  head: { appendChild(node: unknown): unknown };
  getElementById(id: string): unknown;
  createElement(tag: "script"): {
    id: string;
    src: string;
    async: boolean;
    setAttribute(name: string, value: string): void;
  };
  addEventListener(
    type: "click",
    listener: (event: { target: unknown }) => void,
  ): void;
}

export interface PreferredSourceObserver {
  observe(target: unknown, options: { attributes: true; attributeFilter: string[] }): void;
}

/** The page's own theme. The site is light-first; a `dark` class or `data-theme="dark"` flips it. */
export function pageTheme(doc: PreferredSourceDocument): "light" | "dark" {
  const root = doc.documentElement;
  return root.getAttribute("data-theme") === "dark" ||
    root.classList.contains("dark")
    ? "dark"
    : "light";
}

export function pageLang(doc: PreferredSourceDocument): string {
  return doc.documentElement.lang || "en";
}

function enqueue(host: QueueHost, callback: Callback) {
  host.PREFERRED_SOURCE = host.PREFERRED_SOURCE || [];
  host.PREFERRED_SOURCE.push(callback);
}

/** Adds the library once, in manual mode. A second call finds the id and stops. */
export function loadPreferredSourceScript(doc: PreferredSourceDocument) {
  if (doc.getElementById(PREFERRED_SOURCE_SCRIPT_ID)) return;
  const script = doc.createElement("script");
  script.id = PREFERRED_SOURCE_SCRIPT_ID;
  script.src = PREFERRED_SOURCE_SCRIPT_SRC;
  script.async = true;
  script.setAttribute("preferred-sources-control", "manual");
  doc.head.appendChild(script);
}

function isPreferredSourceButton(target: unknown): boolean {
  const element = target as { closest?: (selector: string) => unknown } | null;
  return Boolean(element?.closest?.(`[${PREFERRED_SOURCE_ATTRIBUTE}]`));
}

const installed = new WeakSet<object>();

/**
 * Queue `init` with the page's theme and language, load the library, and
 * start Google's flow on a click of any `[data-prefer-on-google]` button.
 * `init` runs again when the root element's theme or language changes.
 * Safe to call more than once per document.
 */
export function installPreferredSource(
  host: QueueHost,
  doc: PreferredSourceDocument,
  createObserver?: (onChange: () => void) => PreferredSourceObserver,
) {
  if (installed.has(doc)) return;
  installed.add(doc);

  let api: PreferredSourceApi | null = null;
  let current = "";
  const init = () => {
    const theme = pageTheme(doc);
    const lang = pageLang(doc);
    if (`${theme}|${lang}` === current) return;
    current = `${theme}|${lang}`;
    enqueue(host, (ready) => {
      api = ready;
      ready.init({ theme, lang });
    });
  };

  init();
  loadPreferredSourceScript(doc);

  doc.addEventListener("click", (event) => {
    if (!isPreferredSourceButton(event.target)) return;
    if (api) api.addPreferredSource();
    else enqueue(host, (ready) => ready.addPreferredSource());
  });

  createObserver?.(init).observe(doc.documentElement, {
    attributes: true,
    attributeFilter: ["class", "data-theme", "lang"],
  });
}
