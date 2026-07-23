import { useEffect, useRef, useState } from "react";
import { ChevronDown, Code2 } from "lucide-react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { createPortal } from "react-dom";

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
  altText?: string;
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
  altText,
  state,
  variant = "panel",
}: Props) {
  const [mode, setMode] = useState<Mode>("markdown");
  const [format, setFormat] = useState<Format>("svg");
  const [theme, setTheme] = useState<ThemeChoice>("auto");
  const [animatedSvg, setAnimatedSvg] = useState(false);
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ top: 0, left: 0, width: 384, above: false });
  const reduceMotion = useReducedMotion();

  useEffect(() => {
    if (!open || variant !== "menu") return;
    function closeOnOutside(event: PointerEvent) {
      const target = event.target as Node;
      if (!rootRef.current?.contains(target) && !panelRef.current?.contains(target)) setOpen(false);
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

  useEffect(() => {
    if (!open || variant !== "menu") return;
    const place = () => {
      const trigger = triggerRef.current;
      if (!trigger) return;
      const rect = trigger.getBoundingClientRect();
      const width = Math.min(384, window.innerWidth - 16);
      const panelHeight = panelRef.current?.getBoundingClientRect().height ?? 230;
      const above = rect.bottom + 8 + panelHeight > window.innerHeight && rect.top > panelHeight + 8;
      setPosition({
        width,
        left: Math.max(8, Math.min(window.innerWidth - width - 8, rect.right - width)),
        top: above ? Math.max(8, rect.top - panelHeight - 8) : rect.bottom + 8,
        above,
      });
    };
    place();
    const frame = window.requestAnimationFrame(place);
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => {
      window.cancelAnimationFrame(frame);
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
    };
  }, [open, variant]);

  const supportsGif =
    /^\/api\/repos\/[^/]+\/[^/]+\/chart\.svg(?:\?|$)/.test(chartPath);
  const formats: readonly Format[] = supportsGif
    ? (["svg", "gif", "png", "webp"] as const)
    : STATIC_FORMATS;
  const selectedFormat = formats.includes(format) ? format : "svg";

  const formatParams =
    selectedFormat === "svg" ? [`animate=${animatedSvg ? "1" : "0"}`] : [];
  const base = appendParams(
    `${apiBase}${withFormat(chartPath, selectedFormat)}`,
    [...stateParams(state), ...formatParams, `render=${MEDIA_RENDER_REVISION}`],
  );
  const lightUrl = appendParams(base, ["theme=light"]);
  const darkUrl = appendParams(base, ["theme=dark"]);
  const alt = altText ?? `Star history of ${label}`;
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

  const controls = (
    <div className="grid grid-cols-3 gap-2">
      <label className="mono-label grid gap-1">
        Theme
        <select
          name="theme"
          value={theme}
          onChange={(e) => setTheme(e.target.value as ThemeChoice)}
          className="dither-control min-h-9 appearance-none bg-background px-2 font-mono text-xs text-foreground outline-none"
        >
          {THEMES.map((t) => (
            <option key={t.id} value={t.id}>
              {t.label}
            </option>
          ))}
        </select>
      </label>
      <label className="mono-label grid gap-1">
        Image
        <select
          value={selectedFormat}
          onChange={(event) => setFormat(event.target.value as Format)}
          className="dither-control min-h-9 appearance-none bg-background px-2 font-mono text-xs text-foreground outline-none"
        >
          {formats.map((value) => <option key={value} value={value}>{value.toUpperCase()}</option>)}
        </select>
      </label>
      <label className="mono-label grid gap-1">
        Snippet
        <select
          value={mode}
          onChange={(event) => setMode(event.target.value as Mode)}
          className="dither-control min-h-9 appearance-none bg-background px-2 font-mono text-xs text-foreground outline-none"
        >
          <option value="markdown">Markdown</option>
          <option value="html">HTML</option>
        </select>
      </label>
      {selectedFormat === "svg" && <button
          type="button"
          aria-pressed={animatedSvg}
          onClick={() => setAnimatedSvg((value) => !value)}
          className={`dither-control col-span-3 min-h-9 px-3 text-left font-mono text-xs ${animatedSvg ? "text-foreground" : "text-muted-foreground"}`}
        >
          {animatedSvg ? "● Animated SVG" : "○ Static SVG"}
        </button>}
    </div>
  );

  const snippetBody = (
    <>
      <div className="flex items-center justify-between gap-4 border-t border-border px-4 py-3">
        <p className="min-w-0 truncate font-mono text-xs text-muted-foreground">
          {mode === "markdown" ? "README.md" : "HTML"} · {selectedFormat.toUpperCase()} · {theme}
        </p>
        <CopyButton
          value={snippet}
          ariaLabel="Copy embed snippet"
          className="dither-primary inline-flex min-h-9 shrink-0 items-center gap-1.5 rounded-md px-3 py-2 font-mono text-xs"
          idleLabel="Copy embed"
        />
      </div>
      {selectedFormat === "gif" && (
        <p className="border-t border-border px-5 py-3 text-sm text-pretty text-muted-foreground">
          GIF loops a wave animation and uses more bandwidth than SVG. Auto emits separate light and dark assets.
        </p>
      )}
    </>
  );

  if (variant === "menu") {
    return (
      <div
        ref={rootRef}
        className={`relative ml-auto ${open ? "z-50" : ""}`}
      >
        <button
          ref={triggerRef}
          type="button"
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
          className="dither-control inline-flex min-h-11 items-center gap-2 rounded-md border px-3 py-2 font-mono text-sm text-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring sm:min-h-0 sm:text-xs"
        >
          <Code2 className="size-4" strokeWidth={1.75} aria-hidden="true" />
          Add to README
          <ChevronDown
            className={`size-3.5 text-muted-foreground transition-transform duration-150 motion-reduce:transition-none ${open ? "rotate-180" : ""}`}
            aria-hidden="true"
          />
        </button>
        {typeof document !== "undefined" && createPortal(
          <AnimatePresence>
          {open && (
            <motion.div
              ref={panelRef}
              initial={{ opacity: 0, y: reduceMotion ? 0 : -4, scale: reduceMotion ? 1 : 0.98 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: reduceMotion ? 0 : -3, scale: reduceMotion ? 1 : 0.985 }}
              transition={{
                duration: reduceMotion ? REDUCED_MOTION_DURATION : DURATION.enter,
                ease: EASE_OUT,
              }}
              style={{ top: position.top, left: position.left, width: position.width, transformOrigin: position.above ? "bottom right" : "top right" }}
              className="dither-menu fixed z-[100] overflow-hidden text-left"
            >
              <div className="space-y-3 px-4 py-3">
                <div>
                  <p className="text-sm font-semibold text-foreground">Put this media in your GitHub README</p>
                  <p className="mt-0.5 text-xs text-muted-foreground">Choose once, copy, paste into README.md.</p>
                </div>
                {controls}
              </div>
              {snippetBody}
            </motion.div>
          )}
          </AnimatePresence>, document.body)}
      </div>
    );
  }

  return (
    <figure className="overflow-hidden border-y border-border">
      <figcaption className="flex flex-wrap items-center justify-between gap-3 border-b border-border bg-muted/40 px-5 py-3">
        <div className="mono-label inline-flex items-center gap-2">
          <span className="size-1.5 shrink-0 rounded-full bg-(--dither-wave-2)" aria-hidden="true" />
          Embed
        </div>
        {controls}
      </figcaption>
      {snippetBody}
    </figure>
  );
}
