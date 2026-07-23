import { useMemo, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { Check, ChevronDown } from "lucide-react";

import { CopyButton } from "@/components/CopyButton";
import { MEDIA_RENDER_REVISION } from "@/lib/media";
import {
  DURATION,
  EASE_OUT,
  REDUCED_MOTION_DURATION,
} from "@/lib/motion";
import { useRenderedTheme } from "@/lib/rendered-theme";

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

  const tabClass = (active: boolean) =>
    `dither-control min-h-11 rounded-md px-3 py-2 font-mono text-base tracking-wide uppercase sm:min-h-0 sm:px-2.5 sm:py-1 sm:text-xs ${
      active
        ? "text-accent-foreground"
        : "text-muted-foreground hover:text-accent-foreground"
    }`;

  const noMetrics = metrics.length === 0;

  return (
    <div className="space-y-8">
      <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.1fr)]">
        <div className="card-panel space-y-6 p-6">
          <div>
            <p className="mono-label">Metrics</p>
            <div className="mt-3 flex flex-wrap gap-2">
              {METRICS.map((m) => {
                const active = metrics.includes(m.id);
                return (
                  <label
                    key={m.id}
                    htmlFor={`badge-metric-${m.id}`}
                    className="dither-control group inline-flex min-h-11 cursor-pointer items-center gap-2 rounded-md border px-3 py-2 font-mono text-base text-muted-foreground outline-ring outline-offset-2 has-checked:text-foreground has-focus-visible:outline-2 hover:text-accent-foreground sm:min-h-0 sm:py-1.5 sm:text-sm"
                  >
                    <input
                      id={`badge-metric-${m.id}`}
                      name="metrics"
                      value={m.id}
                      type="checkbox"
                      checked={active}
                      onChange={() => toggleMetric(m.id)}
                      className="sr-only"
                    />
                    <span
                      className="flex size-5 shrink-0 items-center justify-center rounded-sm border border-input group-has-checked:border-(--dither-wave-1) group-has-checked:bg-(--dither-wave-1) sm:size-4"
                      aria-hidden="true"
                    >
                      <Check
                        className="size-3 text-background opacity-0 group-has-checked:opacity-100"
                        strokeWidth={3}
                      />
                    </span>
                    {m.label}
                  </label>
                );
              })}
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
                  className="mt-2 text-base text-destructive sm:text-sm"
                  role="alert"
                >
                  Pick at least one metric.
                </motion.p>
              )}
            </AnimatePresence>
          </div>

          <div>
            <p className="mono-label">Style</p>
            <div className="mt-3 grid grid-cols-1 gap-3 sm:grid-cols-2">
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
                    className="dither-panel group flex cursor-pointer flex-col gap-2 rounded-lg p-3 text-left text-base outline-ring outline-offset-2 has-checked:ring-2 has-checked:ring-(--dither-wave-1)/40 has-focus-visible:outline-2 sm:text-sm"
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
                    <span className="font-mono text-foreground">{s.label}</span>
                    <span className="text-muted-foreground">{s.desc}</span>
                  </label>
                );
              })}
            </div>
          </div>

          <div className="grid gap-5 sm:grid-cols-2">
            <div>
              <label htmlFor="badge-source" className="mono-label block">
                Source
              </label>
              <div className="mt-3 grid grid-cols-1">
                <select
                  id="badge-source"
                  name="source"
                  value={source}
                  onChange={(e) => setSource(e.target.value as BadgeSource)}
                  className="dither-control col-start-1 row-start-1 min-h-11 w-full appearance-none rounded-md border py-2 pr-9 pl-3 font-mono text-base text-foreground outline-none focus-visible:outline-2 focus-visible:-outline-offset-1 focus-visible:outline-ring sm:min-h-0 sm:text-sm"
                >
                  {SOURCES.map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.label}
                    </option>
                  ))}
                </select>
                <ChevronDown
                  className="pointer-events-none col-start-1 row-start-1 mr-3 size-4 self-center justify-self-end text-muted-foreground"
                  strokeWidth={2}
                  aria-hidden="true"
                />
              </div>
            </div>
            <div>
              <label htmlFor="badge-theme" className="mono-label block">
                Theme
              </label>
              <div className="mt-3 grid grid-cols-1">
                <select
                  id="badge-theme"
                  name="theme"
                  value={theme}
                  onChange={(e) => setTheme(e.target.value as ThemeChoice)}
                  className="dither-control col-start-1 row-start-1 min-h-11 w-full appearance-none rounded-md border py-2 pr-9 pl-3 font-mono text-base text-foreground outline-none focus-visible:outline-2 focus-visible:-outline-offset-1 focus-visible:outline-ring sm:min-h-0 sm:text-sm"
                >
                  {THEMES.map((t) => (
                    <option key={t.id} value={t.id}>
                      {t.label}
                    </option>
                  ))}
                </select>
                <ChevronDown
                  className="pointer-events-none col-start-1 row-start-1 mr-3 size-4 self-center justify-self-end text-muted-foreground"
                  strokeWidth={2}
                  aria-hidden="true"
                />
              </div>
            </div>
          </div>

          <div>
            <p className="mono-label">Animation</p>
            <label
              htmlFor="badge-animation"
              className="dither-control group mt-3 inline-flex min-h-11 cursor-pointer items-center gap-3 rounded-md border px-3 py-2 font-mono text-base text-foreground outline-ring outline-offset-2 has-focus-visible:outline-2 sm:text-sm"
            >
              <input
                id="badge-animation"
                name="animation"
                type="checkbox"
                role="switch"
                checked={animate}
                onChange={(event) => setAnimate(event.target.checked)}
                className="sr-only"
              />
              <span
                className="relative inline-flex h-5 w-9 shrink-0 items-center rounded-full bg-input group-has-checked:bg-(--dither-wave-1)"
                aria-hidden="true"
              >
                <span
                  className="inline-block size-4 translate-x-0.5 rounded-full bg-background transition-transform duration-200 ease-out group-has-checked:translate-x-4 motion-reduce:transition-none"
                />
              </span>
              {animate ? "Animated" : "Static"}
            </label>
          </div>
        </div>

        <div className="card-panel flex flex-col overflow-hidden">
          <div className="flex items-center justify-between gap-2 border-b border-border bg-muted/40 px-5 py-3">
            <div className="mono-label inline-flex items-center gap-2">
              <span className="size-1.5 shrink-0 rounded-full bg-(--dither-wave-2)" aria-hidden="true" />
              Live preview
            </div>
            <CopyButton
              value={theme === "auto" ? pictureEmbed : embedThemeUrl}
              ariaLabel={
                theme === "auto"
                  ? "Copy theme-aware badge embed"
                  : "Copy badge URL"
              }
              idleLabel={theme === "auto" ? "Embed" : "URL"}
              className="dither-control inline-flex min-h-11 items-center gap-1.5 rounded-md border px-3 py-2 font-mono text-base text-muted-foreground hover:text-accent-foreground sm:min-h-0 sm:px-2.5 sm:py-1 sm:text-xs"
            />
          </div>

          <div className="dither-badge-bed flex flex-1 items-center justify-center px-6 py-12">
            {noMetrics ? (
              <p className="text-base text-muted-foreground sm:text-sm">
                Pick a metric to preview your badge.
              </p>
            ) : (
              <img
                src={resolvedThemeUrl}
                alt={alt}
                decoding="async"
                className="block h-auto max-w-full"
              />
            )}
          </div>

          <p className="border-t border-border px-5 py-3 text-base text-pretty text-muted-foreground sm:text-sm">
            SVG previews can animate here. Copied README SVGs are always
            static because GitHub strips SVG motion; Auto emits separate light
            and dark assets.
          </p>
        </div>
      </div>

      {!noMetrics && (
        <BadgeEmbed markdown={markdown} html={html} tabClass={tabClass} />
      )}
    </div>
  );
}

function BadgeEmbed({
  markdown,
  html,
  tabClass,
}: {
  markdown: string;
  html: string;
  tabClass: (active: boolean) => string;
}) {
  const [mode, setMode] = useState<"markdown" | "html">("markdown");
  const snippet = mode === "markdown" ? markdown : html;

  return (
    <figure className="card-panel overflow-hidden">
      <figcaption className="flex flex-wrap items-center justify-between gap-3 border-b border-border bg-muted/40 px-5 py-3">
        <div className="mono-label inline-flex items-center gap-2">
          <span className="size-1.5 shrink-0 rounded-full bg-(--dither-wave-2)" aria-hidden="true" />
          Embed badge
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
      </figcaption>
      <div className="relative">
        <pre className="overflow-x-auto px-5 py-4 font-mono text-sm leading-relaxed text-foreground">
          <code>{snippet}</code>
        </pre>
        <CopyButton
          value={snippet}
          ariaLabel="Copy badge embed snippet"
          className="dither-control absolute top-3 right-3 inline-flex min-h-11 items-center gap-1.5 rounded-md border px-3 py-2 font-mono text-base text-muted-foreground backdrop-blur hover:text-accent-foreground sm:min-h-0 sm:px-2.5 sm:py-1 sm:text-xs"
        />
      </div>
    </figure>
  );
}
