import { useMemo, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { ChevronDown } from "lucide-react";

import { CopyButton } from "@/components/CopyButton";
import { BODY, EYEBROW, PANEL } from "@/components/style-tokens";
import { DitherCheckbox } from "@/components/ui/dither-checkbox";
import { DitherSegmented } from "@/components/ui/dither-segmented";
import { DitherSwitch } from "@/components/ui/dither-switch";
import { CONTROL } from "@/components/ui/dither-surface";
import { DitherSurface } from "@/components/ui/dither-surface";
import { INK } from "@/lib/dither";
import { MEDIA_RENDER_REVISION } from "@/lib/media";
import {
  DURATION,
  EASE_OUT,
  REDUCED_MOTION_DURATION,
} from "@/lib/motion";
import { useRenderedTheme } from "@/lib/rendered-theme";
import { cn } from "@/lib/utils";

type Metric = "stars" | "forks" | "downloads";
type BadgeStyle = "flat" | "modern" | "glass" | "terminal";
type BadgeSource = "auto" | "npm" | "crates" | "pypi" | "docker";
type ThemeChoice = "auto" | "light" | "dark";

const METRICS: { id: Metric; label: string }[] = [
  { id: "stars", label: "Stars" },
  { id: "forks", label: "Forks" },
  { id: "downloads", label: "Downloads" },
];

const STYLES: { id: BadgeStyle; label: string; desc: string }[] = [
  { id: "flat", label: "Flat", desc: "Classic shields look" },
  { id: "modern", label: "Modern", desc: "Rounded, soft shadow" },
  { id: "glass", label: "Glass", desc: "Translucent, blurred" },
  { id: "terminal", label: "Terminal", desc: "Mono, monochrome" },
];

const SOURCES: { id: BadgeSource; label: string }[] = [
  { id: "auto", label: "Auto" },
  { id: "npm", label: "npm" },
  { id: "crates", label: "crates" },
  { id: "pypi", label: "PyPI" },
  { id: "docker", label: "Docker" },
];

const THEMES: { id: ThemeChoice; label: string }[] = [
  { id: "auto", label: "Auto" },
  { id: "light", label: "Light" },
  { id: "dark", label: "Dark" },
];

type Props = {
  apiBase: string;
  owner: string;
  repo: string;
};

function badgeQuery(opts: {
  metrics: Metric[];
  style: BadgeStyle;
  animate: boolean;
  source: BadgeSource;
  theme: "light" | "dark";
}): string {
  const params = new URLSearchParams();
  params.set("metrics", opts.metrics.join(","));
  params.set("style", opts.style);
  params.set("animate", opts.animate ? "1" : "0");
  params.set("source", opts.source);
  params.set("theme", opts.theme);
  params.set("render", MEDIA_RENDER_REVISION);
  return params.toString();
}

export function BadgeStudio({ apiBase, owner, repo }: Props) {
  const [metrics, setMetrics] = useState<Metric[]>(["stars", "downloads"]);
  const [style, setStyle] = useState<BadgeStyle>("modern");
  const [animate, setAnimate] = useState(false);
  const [source, setSource] = useState<BadgeSource>("auto");
  const [theme, setTheme] = useState<ThemeChoice>("auto");
  const reduceMotion = useReducedMotion();
  const renderedTheme = useRenderedTheme();

  const badgeBase = `${apiBase}/api/repos/${owner}/${repo}/badge.svg`;

  function toggleMetric(id: Metric) {
    setMetrics((prev) => {
      const has = prev.includes(id);
      const next = has ? prev.filter((m) => m !== id) : [...prev, id];
      return METRICS.filter((m) => next.includes(m.id)).map((m) => m.id);
    });
  }

  const lightUrl = useMemo(
    () => `${badgeBase}?${badgeQuery({ metrics, style, animate, source, theme: "light" })}`,
    [badgeBase, metrics, style, animate, source],
  );
  const darkUrl = useMemo(
    () => `${badgeBase}?${badgeQuery({ metrics, style, animate, source, theme: "dark" })}`,
    [badgeBase, metrics, style, animate, source],
  );

  const resolvedThemeUrl =
    theme === "auto"
      ? renderedTheme === "dark"
        ? darkUrl
        : lightUrl
      : theme === "dark"
        ? darkUrl
        : lightUrl;

  const label = `${owner}/${repo}`;
  const alt = `${label} stats badge`;
  const linkHref = `https://gitdebt.com/${owner}/${repo}?ref=readme`;

  const embedLightUrl = `${badgeBase}?${badgeQuery({
    metrics,
    style,
    animate: false,
    source,
    theme: "light",
  })}`;
  const embedDarkUrl = `${badgeBase}?${badgeQuery({
    metrics,
    style,
    animate: false,
    source,
    theme: "dark",
  })}`;
  const embedThemeUrl = theme === "dark" ? embedDarkUrl : embedLightUrl;
  const pictureEmbed = `<a href="${linkHref}">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="${embedDarkUrl}" />
    <img alt="${alt}" src="${embedLightUrl}" />
  </picture>
</a>`;
  const flatEmbed = `<a href="${linkHref}">
  <img alt="${alt}" src="${embedThemeUrl}" />
</a>`;
  const markdown =
    theme === "auto"
      ? pictureEmbed
      : `[![${alt}](${embedThemeUrl})](${linkHref})`;
  const html = theme === "auto" ? pictureEmbed : flatEmbed;

  const noMetrics = metrics.length === 0;

  return (
    <div className="space-y-8">
      <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.1fr)]">
        <div className={cn(PANEL, "space-y-6 p-3.5")}>
          <div>
            <p className={EYEBROW}>Metrics</p>
            <div
              className="mt-3 flex flex-wrap gap-x-5 gap-y-1"
              role="group"
              aria-label="Metrics"
            >
              {METRICS.map((m) => (
                <DitherCheckbox
                  key={m.id}
                  id={`badge-metric-${m.id}`}
                  name="metrics"
                  value={m.id}
                  checked={metrics.includes(m.id)}
                  onCheckedChange={() => toggleMetric(m.id)}
                >
                  {m.label}
                </DitherCheckbox>
              ))}
            </div>
            <AnimatePresence initial={false}>
              {noMetrics && (
                <motion.p
                  initial={{
                    opacity: 0,
                    y: reduceMotion ? 0 : -4,
                  }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, transition: { duration: 0.12 } }}
                  transition={{
                    duration: reduceMotion
                      ? REDUCED_MOTION_DURATION
                      : DURATION.enter,
                    ease: EASE_OUT,
                  }}
                  className="mt-2 text-[11px] text-[var(--swatch-red)]"
                  role="alert"
                >
                  Pick at least one metric.
                </motion.p>
              )}
            </AnimatePresence>
          </div>

          <div>
            <p className={EYEBROW}>Style</p>
            <div className="mt-3 grid grid-cols-1 gap-2 sm:grid-cols-2">
              {STYLES.map((s) => {
                const previewUrl = `${badgeBase}?${badgeQuery({
                  metrics: metrics.length ? metrics : ["stars"],
                  style: s.id,
                  animate: false,
                  source,
                  theme: "dark",
                })}`;
                return (
                  <label
                    key={s.id}
                    htmlFor={`badge-style-${s.id}`}
                    className={cn(
                      PANEL,
                      "group flex cursor-pointer flex-col gap-2 p-3.5 text-left text-[13px] transition-[border-color,background-color] duration-150 hover:border-foreground/25 has-checked:border-foreground/40 has-checked:bg-card/60 has-focus-visible:ring-2 has-focus-visible:ring-accent/30",
                    )}
                  >
                    <input
                      id={`badge-style-${s.id}`}
                      name="style"
                      value={s.id}
                      type="radio"
                      checked={style === s.id}
                      onChange={() => setStyle(s.id)}
                      className="sr-only"
                    />
                    <span className="flex min-h-7 items-center">
                      <img
                        src={previewUrl}
                        alt=""
                        loading="lazy"
                        decoding="async"
                        className="block h-5 w-auto"
                      />
                    </span>
                    <span className="font-mono text-[12px] text-foreground">{s.label}</span>
                    <span className="text-[11px] text-muted-foreground">{s.desc}</span>
                  </label>
                );
              })}
            </div>
          </div>

          <div className="grid gap-5 sm:grid-cols-2">
            <div>
              <label htmlFor="badge-source" className={cn(EYEBROW, "block")}>
                Source
              </label>
              <div className="relative mt-3 grid grid-cols-1 items-center">
                <select
                  id="badge-source"
                  name="source"
                  value={source}
                  onChange={(e) => setSource(e.target.value as BadgeSource)}
                  className={cn(
                    CONTROL,
                    "col-start-1 row-start-1 appearance-none pr-9 text-foreground",
                  )}
                >
                  {SOURCES.map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.label}
                    </option>
                  ))}
                </select>
                <ChevronDown
                  className="pointer-events-none col-start-1 row-start-1 mr-3 size-3.5 justify-self-end text-muted-foreground"
                  strokeWidth={2}
                  aria-hidden="true"
                />
              </div>
            </div>
            <div>
              <label htmlFor="badge-theme" className={cn(EYEBROW, "block")}>
                Theme
              </label>
              <div className="relative mt-3 grid grid-cols-1 items-center">
                <select
                  id="badge-theme"
                  name="theme"
                  value={theme}
                  onChange={(e) => setTheme(e.target.value as ThemeChoice)}
                  className={cn(
                    CONTROL,
                    "col-start-1 row-start-1 appearance-none pr-9 text-foreground",
                  )}
                >
                  {THEMES.map((t) => (
                    <option key={t.id} value={t.id}>
                      {t.label}
                    </option>
                  ))}
                </select>
                <ChevronDown
                  className="pointer-events-none col-start-1 row-start-1 mr-3 size-3.5 justify-self-end text-muted-foreground"
                  strokeWidth={2}
                  aria-hidden="true"
                />
              </div>
            </div>
          </div>

          <div>
            <p className={EYEBROW} id="badge-animation-label">
              Animation
            </p>
            <div className="mt-2 inline-flex items-center gap-2">
              <DitherSwitch
                id="badge-animation"
                name="animation"
                checked={animate}
                onCheckedChange={setAnimate}
                aria-labelledby="badge-animation-label"
              />
              <span className="font-mono text-[12px] text-muted-foreground">
                {animate ? "Animated" : "Static"}
              </span>
            </div>
          </div>
        </div>

        <div className={cn(PANEL, "flex flex-col overflow-hidden")}>
          <div className="flex items-center justify-between gap-2 border-b border-border/40 px-4 py-3">
            <div className={EYEBROW}>Live preview</div>
            <CopyButton
              value={theme === "auto" ? pictureEmbed : embedThemeUrl}
              ariaLabel={
                theme === "auto"
                  ? "Copy theme-aware badge embed"
                  : "Copy badge URL"
              }
              idleLabel={theme === "auto" ? "Embed" : "URL"}
            />
          </div>

          <div className="dither-fallback relative isolate flex flex-1 items-center justify-center overflow-hidden px-6 py-12">
            <DitherSurface fill={INK} variant="gradient" edge={0.5} alpha={0.16} />
            {noMetrics ? (
              <p className="relative text-[13px] text-muted-foreground">
                Pick a metric to preview your badge.
              </p>
            ) : (
              <img
                src={resolvedThemeUrl}
                alt={alt}
                decoding="async"
                className="relative block h-auto max-w-full"
              />
            )}
          </div>

          <p className={cn(BODY, "border-t border-border/40 px-4 py-3")}>
            SVG previews can animate here. Copied README SVGs are always
            static because GitHub strips SVG motion; Auto emits separate light
            and dark assets.
          </p>
        </div>
      </div>

      {!noMetrics && <BadgeEmbed markdown={markdown} html={html} />}
    </div>
  );
}

const EMBED_FORMATS = [
  { value: "markdown" as const, label: "Markdown" },
  { value: "html" as const, label: "HTML" },
];

function BadgeEmbed({ markdown, html }: { markdown: string; html: string }) {
  const [mode, setMode] = useState<"markdown" | "html">("markdown");
  const snippet = mode === "markdown" ? markdown : html;

  return (
    <figure className={cn(PANEL, "overflow-hidden")}>
      <figcaption className="flex flex-wrap items-center justify-between gap-3 border-b border-border/40 px-4 py-3">
        <div className={EYEBROW}>Embed badge</div>
        <DitherSegmented
          role="radiogroup"
          aria-label="Embed format"
          value={mode}
          options={EMBED_FORMATS}
          onValueChange={setMode}
        />
      </figcaption>
      <div className="relative">
        <pre className="overflow-x-auto px-4 py-4 font-mono text-[12px] leading-relaxed text-foreground">
          <code>{snippet}</code>
        </pre>
        <CopyButton
          value={snippet}
          ariaLabel="Copy badge embed snippet"
          className="absolute top-3 right-3 backdrop-blur"
        />
      </div>
    </figure>
  );
}
