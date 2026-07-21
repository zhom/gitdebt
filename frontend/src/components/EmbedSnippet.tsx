import { useEffect, useRef, useState } from "react";
import { ChevronDown, Code2 } from "lucide-react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";

import { CopyButton } from "@/components/CopyButton";
import {
  DURATION,
  EASE_OUT,
  REDUCED_MOTION_DURATION,
} from "@/lib/motion";
import { MEDIA_RENDER_REVISION } from "@/lib/media";

export type EmbedState = {
  type?: "date" | "timeline";
  log?: boolean;
  from?: string;
  to?: string;
};

type Props = {
  apiBase: string;
  chartPath: string;
  linkHref: string;
  label: string;
  state?: EmbedState;
  variant?: "panel" | "menu";
};

type Mode = "markdown" | "html";
type Format = "svg" | "gif" | "png" | "webp";
type ThemeChoice = "auto" | "light" | "dark";

const STATIC_FORMATS: Format[] = ["svg", "png", "webp"];
const THEMES: { id: ThemeChoice; label: string }[] = [
  { id: "auto", label: "Auto" },
  { id: "light", label: "Light" },
  { id: "dark", label: "Dark" },
];

const DATE_RE = /^\d{4}-\d{2}-\d{2}$/;

function withFormat(path: string, format: Format): string {
  const qi = path.indexOf("?");
  const base = qi === -1 ? path : path.slice(0, qi);
  const query = qi === -1 ? "" : path.slice(qi);
  return base.replace(/\.svg$/, `.${format}`) + query;
}

function appendParams(url: string, params: string[]): string {
  if (params.length === 0) return url;
  const [base, query = ""] = url.split("?", 2);
  const search = new URLSearchParams(query);
  for (const param of params) {
    const [key, value = ""] = param.split("=", 2);
    search.set(key, value);
  }
  const next = search.toString();
  return next ? `${base}?${next}` : base;
}

function stateParams(state: EmbedState | undefined): string[] {
  if (!state) return [];
  const params: string[] = [];
  if (state.type) params.push(`type=${state.type}`);
  if (state.log) params.push("log=1");
  if (state.from && DATE_RE.test(state.from)) params.push(`from=${state.from}`);
  if (state.to && DATE_RE.test(state.to)) params.push(`to=${state.to}`);
  return params;
}

function withRef(href: string, ref: string): string {
  return href + (href.includes("?") ? "&" : "?") + `ref=${ref}`;
}

export function EmbedSnippet({
  apiBase,
  chartPath,
  linkHref,
  label,
  state,
  variant = "panel",
}: Props) {
  const [mode, setMode] = useState<Mode>("markdown");
  const [format, setFormat] = useState<Format>("svg");
  const [theme, setTheme] = useState<ThemeChoice>("auto");
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const reduceMotion = useReducedMotion();

  useEffect(() => {
    if (!open || variant !== "menu") return;
    function closeOnOutside(event: PointerEvent) {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    }
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      setOpen(false);
      triggerRef.current?.focus();
    }
    window.addEventListener("pointerdown", closeOnOutside);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("pointerdown", closeOnOutside);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [open, variant]);

  const supportsGif =
    /^\/api\/repos\/[^/]+\/[^/]+\/chart\.svg(?:\?|$)/.test(chartPath);
  const formats: readonly Format[] = supportsGif
    ? (["svg", "gif", "png", "webp"] as const)
    : STATIC_FORMATS;
  const selectedFormat = formats.includes(format) ? format : "svg";

  const formatParams =
    selectedFormat === "svg"
      ? ["animate=0"]
      : selectedFormat === "gif"
        ? ["motion=draw"]
        : [];
  const base = appendParams(
    `${apiBase}${withFormat(chartPath, selectedFormat)}`,
    [...stateParams(state), ...formatParams, `render=${MEDIA_RENDER_REVISION}`],
  );
  const lightUrl = appendParams(base, ["theme=light"]);
  const darkUrl = appendParams(base, ["theme=dark"]);
  const alt = `Star history of ${label}`;
  const page = withRef(linkHref, "readme");

  const flatUrl = theme === "dark" ? darkUrl : lightUrl;

  const picture = `<a href="${page}">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="${darkUrl}" />
    <img alt="${alt}" src="${lightUrl}" />
  </picture>
</a>`;
  const flat = `<a href="${page}">
  <img alt="${alt}" src="${flatUrl}" />
</a>`;
  const markdown =
    theme === "auto" ? picture : `[![${alt}](${flatUrl})](${page})`;
  const html = theme === "auto" ? picture : flat;

  const snippet = mode === "markdown" ? markdown : html;

  const tabClass = (active: boolean) =>
    `min-h-11 rounded-md px-3 py-2 font-mono text-base tracking-wide uppercase sm:min-h-0 sm:px-2.5 sm:py-1 sm:text-xs ${
      active
        ? "bg-accent text-accent-foreground"
        : "text-muted-foreground hover:bg-accent/60 hover:text-accent-foreground"
    }`;

  const controls = (
    <div className="flex flex-wrap items-center gap-3">
      <div className="grid grid-cols-1">
        <select
          name="theme"
          value={theme}
          onChange={(e) => setTheme(e.target.value as ThemeChoice)}
          aria-label="Embed theme"
          className="col-start-1 row-start-1 min-h-11 appearance-none rounded-md border border-input bg-background py-2 pr-8 pl-3 font-mono text-base text-foreground outline-none focus-visible:outline-2 focus-visible:-outline-offset-1 focus-visible:outline-ring sm:min-h-0 sm:py-1 sm:pr-7 sm:pl-2 sm:text-xs"
        >
          {THEMES.map((t) => (
            <option key={t.id} value={t.id}>
              {t.label}
            </option>
          ))}
        </select>
        <ChevronDown
          className="pointer-events-none col-start-1 row-start-1 mr-2 size-3.5 self-center justify-self-end text-muted-foreground"
          strokeWidth={2}
          aria-hidden="true"
        />
      </div>
      <div className="flex items-center gap-1" role="group" aria-label="Image format">
        {formats.map((f) => (
          <button
            key={f}
            type="button"
            aria-pressed={selectedFormat === f}
            onClick={() => setFormat(f)}
            className={tabClass(selectedFormat === f)}
          >
            {f}
          </button>
        ))}
      </div>
      <div className="flex items-center gap-1" role="group" aria-label="Embed format">
        <button
          type="button"
          aria-pressed={mode === "markdown"}
          onClick={() => setMode("markdown")}
          className={tabClass(mode === "markdown")}
        >
          Markdown
        </button>
        <button
          type="button"
          aria-pressed={mode === "html"}
          onClick={() => setMode("html")}
          className={tabClass(mode === "html")}
        >
          HTML
        </button>
      </div>
    </div>
  );

  const snippetBody = (
    <>
      <div className="relative border-t border-border">
        <pre className="max-h-48 overflow-auto px-5 py-4 pr-24 font-mono text-xs leading-relaxed text-foreground">
          <code>{snippet}</code>
        </pre>
        <CopyButton
          value={snippet}
          ariaLabel="Copy embed snippet"
          className="absolute top-3 right-3 inline-flex min-h-11 items-center gap-1.5 rounded-md border border-border bg-background/95 px-3 py-2 font-mono text-base text-muted-foreground backdrop-blur hover:bg-accent hover:text-accent-foreground sm:min-h-0 sm:px-2.5 sm:py-1 sm:text-xs"
          idleLabel="Copy"
        />
      </div>
      {selectedFormat === "gif" && (
        <p className="border-t border-border px-5 py-3 text-sm text-pretty text-muted-foreground">
          GIF draws once and uses more bandwidth. Auto emits separate light and dark assets.
        </p>
      )}
    </>
  );

  if (variant === "menu") {
    return (
      <div ref={rootRef} className="relative ml-auto">
        <button
          ref={triggerRef}
          type="button"
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
          className="inline-flex min-h-11 items-center gap-2 rounded-md border border-border bg-background px-3 py-2 font-mono text-sm text-foreground shadow-sm transition-colors duration-150 hover:bg-accent focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring sm:min-h-0 sm:text-xs"
        >
          <Code2 className="size-4" strokeWidth={1.75} aria-hidden="true" />
          Add to README
          <ChevronDown
            className={`size-3.5 text-muted-foreground transition-transform duration-150 motion-reduce:transition-none ${open ? "rotate-180" : ""}`}
            aria-hidden="true"
          />
        </button>
        <AnimatePresence>
          {open && (
            <motion.div
              initial={{ opacity: 0, y: reduceMotion ? 0 : -4, scale: reduceMotion ? 1 : 0.98 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: reduceMotion ? 0 : -3, scale: reduceMotion ? 1 : 0.985 }}
              transition={{
                duration: reduceMotion ? REDUCED_MOTION_DURATION : DURATION.enter,
                ease: EASE_OUT,
              }}
              className="absolute top-[calc(100%+0.65rem)] right-0 z-40 w-[min(42rem,calc(100vw-3rem))] origin-top-right overflow-hidden rounded-xl border border-border bg-card text-left shadow-xl"
            >
              <div className="space-y-3 px-5 py-4">
                <div>
                  <p className="text-sm font-semibold text-foreground">Put this media in your GitHub README</p>
                  <p className="mt-1 text-sm text-muted-foreground">Choose a format, copy the snippet, then paste it into README.md.</p>
                </div>
                {controls}
              </div>
              {snippetBody}
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    );
  }

  return (
    <figure className="card-panel overflow-hidden">
      <figcaption className="flex flex-wrap items-center justify-between gap-3 border-b border-border bg-muted/40 px-5 py-3">
        <div className="inline-flex items-center gap-2 font-mono text-xs tracking-wide text-muted-foreground uppercase">
          <span className="size-1.5 shrink-0 rounded-full bg-signal" aria-hidden="true" />
          Embed
        </div>
        {controls}
      </figcaption>
      {snippetBody}
    </figure>
  );
}
