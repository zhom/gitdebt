import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

import { CodeBlock } from "@/components/CodeBlock";
import { CAPTION, FIELD, PANEL } from "@/components/style-tokens";
import { Button } from "@/components/ui/button";
import { POPOVER, Segmented, Switch } from "@/components/ui/controls";
import { Leader } from "@/components/ui/marks";
import { cn } from "@/lib/utils";

/**
 * The embed builder: four choices and the snippet they produce.
 *
 * It used to carry three hand-rolled `<select>` fields with a drawn chevron on
 * each, a switch, a status line restating the switch, and a second copy of the
 * `<pre>` that `CodeBlock` already owns. The choices are the same; the chrome
 * around them is gone. Every control is now the house control, and the one line
 * of prose left is the only one that says something the controls do not — what
 * GIF is actually for.
 *
 * Two shapes, one body: a panel where the page has room for it, and a popover
 * anchored to a chart's own caption where it does not. The popover is portalled
 * because those captions sit inside clipped panels, and a menu that is cut off
 * by the figure it belongs to is worse than no menu.
 */

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

/**
 * Asset routes wired through the animated-GIF rasterizer. GIF is the only
 * motion a surface that renders an SVG as a single static frame can carry — a
 * GitHub README is not one of those, so it is a fallback, not the default.
 */
const GIF_ASSET_RE =
  /^\/api\/(?:(?:repos\/[^/]+\/[^/]+|users\/[^/]+)\/chart|chart|(?:repos\/[^/]+\/[^/]+|users\/[^/]+)\/(?:card|stats\/[^/?]+))\.svg(?:\?|$)/;

const THEMES: { value: ThemeChoice; label: string }[] = [
  { value: "auto", label: "Auto" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

const FORMATS: { value: Format; label: string }[] = [
  { value: "svg", label: "SVG" },
  { value: "gif", label: "GIF" },
  { value: "png", label: "PNG" },
  { value: "webp", label: "WebP" },
];

const MODES: { value: Mode; label: string }[] = [
  { value: "markdown", label: "Markdown" },
  { value: "html", label: "HTML" },
];

const DATE_RE = /^\d{4}-\d{2}-\d{2}$/;

/**
 * A panel title in the drawing's hand, one step below `HEADING`.
 *
 * `style-tokens.ts` has no step between `HEADING` and prose, and this is a
 * title rather than a field label — using `FIELD` for it would put the same
 * tracked-out costume on the heading and on the controls beneath it, which is
 * exactly the tell the type scale exists to prevent.
 */
const SUBHEAD = "font-draft text-[1.0625rem] leading-[1.2] text-ink";

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

/** A named choice on the drawing: the field label, then the control. */
function Field({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <div className="grid gap-2">
      <span className={FIELD}>{label}</span>
      {children}
    </div>
  );
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
  const [position, setPosition] = useState({ top: 0, left: 0, width: 384 });
  const controlId = useId().replaceAll(":", "");

  useEffect(() => {
    if (!open || variant !== "menu") return;
    function closeOnOutside(event: PointerEvent) {
      const target = event.target as Node;
      if (
        !rootRef.current?.contains(target) &&
        !panelRef.current?.contains(target)
      ) {
        setOpen(false);
      }
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
      const height = panelRef.current?.getBoundingClientRect().height ?? 260;
      const above =
        rect.bottom + 8 + height > window.innerHeight && rect.top > height + 8;
      setPosition({
        width,
        left: Math.max(
          8,
          Math.min(window.innerWidth - width - 8, rect.right - width),
        ),
        top: above ? Math.max(8, rect.top - height - 8) : rect.bottom + 8,
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

  // `animate=1` rides only an explicitly requested SVG. The published catalog
  // stays static by default, so this parameter exists here and nowhere else:
  // it is a choice this visitor made, in this builder, for their own README.
  const formatParams = selectedFormat === "svg" && animate ? ["animate=1"] : [];
  // No `render=` here. Every URL this component builds is published into
  // somebody's README and nowhere else — there is no on-page preview to bust a
  // cache for — so a revision parameter would pin a permanent README to one
  // renderer revision, split the CDN cache key, and contradict both the
  // `no cache-busting parameters` rule /badges states and the plain URLs the
  // golden `readme-embeds` library and `/api/md` emit for the same assets.
  const base = appendParams(`${apiBase}${withFormat(chartPath, selectedFormat)}`, [
    ...stateParams(state),
    ...formatParams,
  ]);
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
    <div className="grid gap-4">
      <Field label="Theme">
        <Segmented
          aria-label="Asset theme"
          value={theme}
          options={THEMES}
          onValueChange={setTheme}
          className="w-full"
        />
      </Field>
      <Field label="Format">
        <Segmented
          aria-label="Asset format"
          value={selectedFormat}
          options={FORMATS.filter((f) => f.value !== "gif" || supportsGif)}
          onValueChange={setSelectedFormat}
          className="w-full"
        />
      </Field>
      <Field label="Snippet">
        <Segmented
          aria-label="Snippet dialect"
          value={mode}
          options={MODES}
          onValueChange={setMode}
          className="w-full"
        />
      </Field>
      {selectedFormat === "svg" && (
        <div className="flex min-h-11 items-center justify-between gap-4">
          <span className={FIELD} id={`${controlId}-motion`}>
            Motion
          </span>
          <Switch
            checked={animate}
            onCheckedChange={setAnimate}
            aria-labelledby={`${controlId}-motion`}
          />
        </div>
      )}
    </div>
  );

  const snippetBlock = (
    <CodeBlock
      code={snippet}
      language={mode}
      label={`${mode === "markdown" ? "README.md" : "HTML"} · ${selectedFormat.toUpperCase()} · ${theme}`}
      copyLabel="Copy embed"
      copyAriaLabel="Copy embed snippet"
      maxHeightClass="max-h-48"
    />
  );

  const gifNote = selectedFormat === "gif" && (
    <p className={cn(CAPTION, "mt-3")}>
      GIF is for surfaces that show an SVG as a single static frame — npm, PyPI,
      Docker Hub, a CSS background. A GitHub README animates the SVG itself, for
      a fraction of the bytes.
    </p>
  );

  if (variant === "menu") {
    return (
      <div ref={rootRef} className={cn("relative ml-auto", open && "z-50")}>
        <Button
          ref={triggerRef}
          variant="quiet"
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
        >
          Add to README
          <Leader size={13} />
        </Button>

        {typeof document !== "undefined" &&
          open &&
          createPortal(
            <div
              ref={panelRef}
              style={{
                top: position.top,
                left: position.left,
                width: position.width,
              }}
              /* `lands` is the house arrival: a half-pixel overshoot over one
                 frame budget. The panel does not exist until it is asked for,
                 so nothing on the page is waiting on this to be readable. */
              className={cn(POPOVER, "lands fixed z-[100] text-left")}
            >
              <div className="space-y-4 p-4">
                <div>
                  <p className={SUBHEAD}>Add to README</p>
                  <p className={cn(CAPTION, "mt-1.5")}>
                    Choose once, copy, paste into README.md.
                  </p>
                </div>
                {controls}
              </div>
              <div className="border-t border-rule p-4">
                {snippetBlock}
                {gifNote}
              </div>
            </div>,
            document.body,
          )}
      </div>
    );
  }

  return (
    <figure className={cn(PANEL, "space-y-4")}>
      <figcaption className={FIELD}>Embed</figcaption>
      {controls}
      {snippetBlock}
      {gifNote}
    </figure>
  );
}
