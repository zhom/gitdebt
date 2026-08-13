import {
  useEffect,
  useId,
  useRef,
  useState,
  type ReactNode,
  type SelectHTMLAttributes,
} from "react";
import { ChevronDown, Code2 } from "lucide-react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { createPortal } from "react-dom";

import { CodeBlock } from "@/components/CodeBlock";
import { CAPTION, EYEBROW, PANEL } from "@/components/style-tokens";
import { Button } from "@/components/ui/button";
import { DitherSwitch } from "@/components/ui/dither-switch";
import { CONTROL, POPOVER } from "@/components/ui/dither-surface";
import {
  DURATION,
  EASE_OUT,
  REDUCED_MOTION_DURATION,
} from "@/lib/motion";
import { cn } from "@/lib/utils";

/** Native select plus the chevron every select in the product carries. */
function SelectField({
  children,
  className,
  ...props
}: SelectHTMLAttributes<HTMLSelectElement> & { children: ReactNode }) {
  return (
    <span className="relative grid grid-cols-1 items-center">
      <select
        {...props}
        className={cn(
          CONTROL,
          "col-start-1 row-start-1 appearance-none pr-9 text-foreground",
          className,
        )}
      >
        {children}
      </select>
      <ChevronDown
        className="pointer-events-none col-start-1 row-start-1 mr-3 size-3.5 justify-self-end text-muted-foreground"
        strokeWidth={2}
        aria-hidden="true"
      />
    </span>
  );
}

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

/** Surfaces with real raster motion that survives GitHub's SVG sanitizer. */
const GIF_ASSET_RE =
  /^\/api\/(?:(?:repos\/[^/]+\/[^/]+|users\/[^/]+)\/chart|chart|(?:repos\/[^/]+\/[^/]+|users\/[^/]+)\/(?:card|stats\/[^/?]+))\.svg(?:\?|$)/;
const THEMES: { id: ThemeChoice; label: string }[] = [
  { id: "auto", label: "Auto" },
  { id: "light", label: "Light" },
  { id: "dark", label: "Dark" },
];
const FORMATS: { id: Format; label: string }[] = [
  { id: "svg", label: "SVG" },
  { id: "gif", label: "GIF" },
  { id: "png", label: "PNG" },
  { id: "webp", label: "WebP" },
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
  const [theme, setTheme] = useState<ThemeChoice>("auto");
  const [selectedFormat, setSelectedFormat] = useState<Format>("svg");
  const [animate, setAnimate] = useState(false);
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ top: 0, left: 0, width: 384, above: false });
  const reduceMotion = useReducedMotion();
  const controlId = useId().replaceAll(":", "");

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

  const supportsGif = GIF_ASSET_RE.test(chartPath);
  useEffect(() => {
    if (selectedFormat === "gif" && !supportsGif) setSelectedFormat("svg");
  }, [selectedFormat, supportsGif]);

  const formatParams =
    selectedFormat === "svg" && animate ? ["animate=1"] : [];
  // No `render=` here. Every URL this component builds is published into
  // somebody's README and nowhere else — there is no on-page preview to bust a
  // cache for — so a revision parameter would pin a permanent README to one
  // renderer revision, split the CDN cache key, and contradict both the
  // `no cache-busting parameters` rule /badges states and the plain URLs the
  // golden `readme-embeds` library and `/api/md` emit for the same assets.
  const base = appendParams(
    `${apiBase}${withFormat(chartPath, selectedFormat)}`,
    [...stateParams(state), ...formatParams],
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
    <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
      <label className={`${EYEBROW} grid gap-1.5`}>
        Theme
        <SelectField
          name="theme"
          value={theme}
          onChange={(e) => setTheme(e.target.value as ThemeChoice)}
        >
          {THEMES.map((t) => (
            <option key={t.id} value={t.id}>
              {t.label}
            </option>
          ))}
        </SelectField>
      </label>
      <label className={`${EYEBROW} grid gap-1.5`}>
        Format
        <SelectField
          name="format"
          value={selectedFormat}
          onChange={(event) =>
            setSelectedFormat(event.target.value as Format)
          }
        >
          {FORMATS.filter(
            (format) => format.id !== "gif" || supportsGif,
          ).map((format) => (
            <option key={format.id} value={format.id}>
              {format.label}
            </option>
          ))}
        </SelectField>
      </label>
      <label className={`${EYEBROW} grid gap-1.5`}>
        Snippet
        <SelectField
          value={mode}
          onChange={(event) => setMode(event.target.value as Mode)}
        >
          <option value="markdown">Markdown</option>
          <option value="html">HTML</option>
        </SelectField>
      </label>
      {selectedFormat === "svg" && (
        <div className="flex items-center justify-between gap-3 sm:col-span-3">
          <span className={EYEBROW}>SVG motion</span>
          <DitherSwitch
            id={`${controlId}-motion`}
            checked={animate}
            onCheckedChange={setAnimate}
            aria-label="Animate SVG where the viewer supports it"
          />
        </div>
      )}
      <span
        id={`${controlId}-animated`}
        className="font-mono text-[11px] tracking-[0.12em] text-muted-foreground uppercase sm:col-span-3"
      >
        {selectedFormat === "svg"
          ? animate
            ? "SVG · animated where supported"
            : "SVG · default · static README frame"
          : selectedFormat === "gif"
            ? "Animated GIF · GitHub-safe motion"
            : `${selectedFormat.toUpperCase()} · static raster`}
      </span>
    </div>
  );

  const snippetBody = (
    <>
      <div className="border-t border-border/40 p-3">
        <CodeBlock
          code={snippet}
          language={mode}
          label={`${mode === "markdown" ? "README.md" : "HTML"} · ${selectedFormat.toUpperCase()} · ${theme}`}
          copyLabel="Copy embed"
          copyAriaLabel="Copy embed snippet"
          maxHeightClass="max-h-48"
        />
      </div>
      {selectedFormat === "gif" && (
        <p className={cn(CAPTION, "border-t border-border/40 px-4 py-3")}>
          GIF carries real dither motion through GitHub's image proxy and uses
          more bandwidth than SVG. Auto emits separate light and dark assets.
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
        <Button
          ref={triggerRef}
          variant="soft"
          size="sm"
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
        >
          <Code2 className="size-4" strokeWidth={1.75} aria-hidden="true" />
          Add to README
          <ChevronDown
            className={`size-3.5 text-muted-foreground transition-transform duration-150 motion-reduce:transition-none ${open ? "rotate-180" : ""}`}
            aria-hidden="true"
          />
        </Button>
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
              className={cn(POPOVER, "fixed z-[100] overflow-hidden text-left")}
            >
              <div className="space-y-3 px-4 py-3">
                <div>
                  <p className="text-[13px] font-normal text-foreground">Put this media in your GitHub README</p>
                  <p className={cn(CAPTION, "mt-1")}>Choose once, copy, paste into README.md.</p>
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
    <figure className={cn(PANEL, "overflow-hidden")}>
      <figcaption className="flex flex-wrap items-center justify-between gap-3 border-b border-border/40 px-4 py-3">
        <div className={EYEBROW}>Embed</div>
        {controls}
      </figcaption>
      {snippetBody}
    </figure>
  );
}
