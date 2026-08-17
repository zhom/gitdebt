/**
 * The /badges control surface.
 *
 * Two exports sharing one fact: `BadgeStudio` builds the metrics chip, and
 * `EmbedCatalog` renders the target input plus every asset `readme-embeds.ts`
 * knows how to publish. They live in one module because a repository owner
 * should type their slug once rather than substitute it into twenty snippets by
 * hand, and the target has to reach both.
 *
 * The catalog is an island so the target can move, but Astro prerenders it, so
 * the default target's twenty figures and snippets are in the static HTML
 * before a byte of JavaScript runs.
 *
 * `readme-embeds.ts` is the only catalog: its output is held to byte equality
 * with the Rust renderer behind `/api/md`, so what this page shows, what an
 * agent fetches, and what the API serves cannot drift. Nothing here is
 * hand-maintained.
 */

import {
  useEffect,
  useId,
  useMemo,
  useState,
  type ReactNode,
  type SubmitEvent,
} from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { ArrowRight, ChevronDown } from "lucide-react";

import { CodeBlock, TOKEN_CLASS, tokenize } from "@/components/CodeBlock";
import { CopyButton } from "@/components/CopyButton";
import { ReportLayerGraphic } from "@/components/ReportLayerGraphic";
import {
  BODY,
  CAPTION,
  EYEBROW,
  HEADING,
  MEASURE,
  PANEL,
  SECTION_ACTION,
  SECTION_HEADER,
} from "@/components/style-tokens";
import { Button } from "@/components/ui/button";
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
import {
  assetUrl,
  bestEmbed,
  bestEmbedLanguage,
  profileEmbedAssets,
  readmeLink,
  repoEmbedAssets,
  type EmbedAsset,
} from "@/lib/readme-embeds";
import { useRenderedTheme } from "@/lib/rendered-theme";
import { cn } from "@/lib/utils";

type Metric = "stars" | "forks" | "downloads";
type BadgeSource = "auto" | "npm" | "crates" | "pypi" | "docker";
type ThemeChoice = "auto" | "light" | "dark";

const METRICS: { id: Metric; label: string }[] = [
  { id: "stars", label: "Stars" },
  { id: "forks", label: "Forks" },
  { id: "downloads", label: "Downloads" },
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
  /** Origin the README link points back at. Attribution rides this link only. */
  siteOrigin?: string;
};

function badgeQuery(opts: {
  metrics: Metric[];
  animate: boolean;
  source: BadgeSource;
  theme: "light" | "dark";
  /**
   * On-page previews carry the render revision that keeps the site off stale
   * edge objects. A published snippet never does: cache-busting parameters are
   * the one thing every embed rule on this page tells a reader not to add.
   */
  preview: boolean;
}): string {
  const params = new URLSearchParams();
  params.set("metrics", opts.metrics.join(","));
  params.set("animate", opts.animate ? "1" : "0");
  params.set("source", opts.source);
  params.set("theme", opts.theme);
  if (opts.preview) params.set("render", MEDIA_RENDER_REVISION);
  return params.toString();
}

export function BadgeStudio({
  apiBase,
  owner,
  repo,
  siteOrigin = "https://gitdebt.com",
}: Props) {
  const [metrics, setMetrics] = useState<Metric[]>(["stars", "downloads"]);
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
    () =>
      `${badgeBase}?${badgeQuery({ metrics, animate, source, theme: "light", preview: true })}`,
    [badgeBase, metrics, animate, source],
  );
  const darkUrl = useMemo(
    () =>
      `${badgeBase}?${badgeQuery({ metrics, animate, source, theme: "dark", preview: true })}`,
    [badgeBase, metrics, animate, source],
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
  const linkHref = readmeLink(siteOrigin, `/${label}`);

  const embedLightUrl = `${badgeBase}?${badgeQuery({
    metrics,
    animate: false,
    source,
    theme: "light",
    preview: false,
  })}`;
  const embedDarkUrl = `${badgeBase}?${badgeQuery({
    metrics,
    animate: false,
    source,
    theme: "dark",
    preview: false,
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
            SVG previews can animate here. Copied README SVGs stay static, so
            nobody's README moves without being asked; add{" "}
            <code className="font-mono text-foreground">animate=1</code> to the
            URL when you want the motion. Auto emits separate light and dark
            assets.
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
        <pre className="overflow-x-auto px-4 py-4 font-mono text-[12px] leading-relaxed">
          <code>
            {tokenize(snippet, mode === "html" ? "html" : "markdown").map(
              (token, index) => (
                <span key={index} className={TOKEN_CLASS[token.kind]}>
                  {token.text}
                </span>
              ),
            )}
          </code>
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

/* ------------------------------------------------------------------ *
 * The catalog
 * ------------------------------------------------------------------ */

/** `owner/repo`, the only shape every asset route accepts. */
const SLUG_RE = /^([A-Za-z0-9._-]+)\/([A-Za-z0-9._-]+)$/;

/** Assets whose natural aspect ratio needs the full row to stay legible. */
const FULL_WIDTH = new Set(["chart", "og", "heatmap", "commit-activity"]);

/** Accepts a slug, a GitHub URL, or a clone URL, and returns `owner/repo`. */
function parseSlug(value: string): string | null {
  const cleaned = value
    .trim()
    .replace(/^git@github\.com:/i, "")
    .replace(/^https?:\/\/(?:www\.)?github\.com\//i, "")
    .replace(/\.git$/i, "")
    .replace(/\/+$/, "");
  return SLUG_RE.test(cleaned) ? cleaned : null;
}

/**
 * The library's copy is written for both a page and a Markdown file, so it
 * carries backticks. Rendering them as `<code>` keeps one source of truth
 * instead of a second, hand-edited set of sentences for the page.
 */
function inlineCode(text: string): ReactNode[] {
  return text
    .split(/(`[^`]+`)/g)
    .filter((part) => part.length > 0)
    .map((part, index) =>
      part.startsWith("`") && part.endsWith("`") && part.length > 2 ? (
        <code key={index} className="font-mono text-foreground">
          {part.slice(1, -1)}
        </code>
      ) : (
        <span key={index}>{part}</span>
      ),
    );
}

/**
 * The on-page preview URL: the published asset plus the revision that keeps the
 * site off stale edge objects. It must never reach a copied snippet.
 */
function previewUrl(
  apiBase: string,
  asset: EmbedAsset,
  theme: "light" | "dark",
): string {
  const url = assetUrl(apiBase, asset, { theme });
  return `${url}${url.includes("?") ? "&" : "?"}render=${MEDIA_RENDER_REVISION}`;
}

function AssetFigure({
  apiBase,
  asset,
  link,
}: {
  apiBase: string;
  asset: EmbedAsset;
  link: string;
}) {
  const snippet = bestEmbed(apiBase, asset, link);
  const language = bestEmbedLanguage(asset);
  const light = previewUrl(apiBase, asset, "light");
  const dark = previewUrl(apiBase, asset, "dark");

  return (
    <figure
      className={cn(
        PANEL,
        "flex flex-col overflow-hidden",
        FULL_WIDTH.has(asset.id) && "sm:col-span-2",
      )}
    >
      <figcaption className="border-b border-border/40 px-4 py-3">
        <h3 className="text-[13px] text-foreground">{asset.name}</h3>
        <p className={cn(CAPTION, "mt-1")}>
          {inlineCode(asset.purpose)} Goes in {inlineCode(asset.placement)}.
        </p>
        <p className={cn(EYEBROW, "mt-2")}>
          {asset.formats.join(" · ")} ·{" "}
          {asset.themed ? "light + dark" : "single theme"}
        </p>
      </figcaption>

      <div className="flex flex-1 items-center justify-center bg-card/40 px-4 py-5">
        {asset.themed ? (
          <picture>
            <source media="(prefers-color-scheme: dark)" srcSet={dark} />
            <img
              src={light}
              alt={asset.alt}
              loading="lazy"
              decoding="async"
              className="block h-auto max-w-full"
            />
          </picture>
        ) : (
          <img
            src={light}
            alt={asset.alt}
            loading="lazy"
            decoding="async"
            className="block h-auto max-w-full"
          />
        )}
      </div>

      <div className="relative border-t border-border/40">
        <pre className="max-h-40 overflow-auto px-4 py-3 pr-24 font-mono text-[11px] leading-relaxed">
          <code>
            {tokenize(snippet, language).map((token, index) => (
              <span key={index} className={TOKEN_CLASS[token.kind]}>
                {token.text}
              </span>
            ))}
          </code>
        </pre>
        <CopyButton
          value={snippet}
          ariaLabel={`Copy the ${asset.name} embed snippet`}
          className="absolute top-2.5 right-3 backdrop-blur"
        />
      </div>
    </figure>
  );
}

/** Section header: heading, a live count, and one illustrative graphic. */
function CatalogHeader({
  id,
  title,
  meta,
  graphic,
}: {
  id: string;
  title: string;
  meta: string;
  graphic: "stars" | "health" | "readme";
}) {
  return (
    <div className="flex items-end justify-between gap-6">
      <div className="min-w-0">
        <h2 id={id} className={HEADING}>
          {title}
        </h2>
        <p className={cn(CAPTION, "mt-1")}>{meta}</p>
      </div>
      <figure aria-hidden="true" className="hidden shrink-0 sm:block">
        <div className="flex h-24 items-center overflow-hidden opacity-80">
          <ReportLayerGraphic kind={graphic} />
        </div>
        <figcaption className={cn(CAPTION, "mt-1 text-right")}>
          Illustrative — no repository values
        </figcaption>
      </figure>
    </div>
  );
}

export type EmbedCatalogProps = {
  apiBase: string;
  /** Origin the README links point back at. */
  siteOrigin: string;
  /** The repository every asset points at until the visitor changes it. */
  defaultSlug: string;
  /**
   * The provenance-stated README block for `defaultSlug`, composed at build
   * time by `provenanceReadmeBlock()`. Null when gitdebt has not established a
   * source for it — publishing an unestablished claim into somebody's README is
   * worse than publishing nothing, so the action disables itself instead.
   */
  defaultProvenance: string | null;
};

/**
 * Every embeddable asset for one repository and one account, pointed wherever
 * the visitor says.
 *
 * The catalog itself is `readme-embeds.ts`, so nothing here is a second,
 * hand-maintained list that could disagree with the API. No request is made
 * from this component: the previews are plain images and every snippet is
 * composed from pure functions.
 */
export function EmbedCatalog({
  apiBase,
  siteOrigin,
  defaultSlug,
  defaultProvenance,
}: EmbedCatalogProps) {
  const [slug, setSlug] = useState(defaultSlug);
  const [draft, setDraft] = useState(defaultSlug);
  const [error, setError] = useState<string | null>(null);
  const reduceMotion = useReducedMotion();
  const fieldId = useId().replaceAll(":", "");

  // A pasted `/badges?repo=owner/name` link lands on the right target. Read
  // after mount, never during render, so the prerendered HTML and the first
  // client render agree.
  useEffect(() => {
    const requested = new URLSearchParams(window.location.search).get("repo");
    const parsed = requested ? parseSlug(requested) : null;
    if (!parsed) return;
    setSlug(parsed);
    setDraft(parsed);
  }, []);

  const login = slug.split("/")[0];
  const repoAssets = useMemo(() => repoEmbedAssets(slug), [slug]);
  const profileAssets = useMemo(() => profileEmbedAssets(login), [login]);
  const repoLink = readmeLink(siteOrigin, `/${slug}`);
  const profileLink = readmeLink(siteOrigin, `/${login}`);

  const headline = repoAssets.filter(
    (asset) => asset.group !== "health",
  );
  const health = repoAssets.filter((asset) => asset.group === "health");

  // Composed at build time for one repository. A visitor-typed target has no
  // analyze payload on a static page, so its source is not established here.
  const provenance = slug === defaultSlug ? defaultProvenance : null;

  function apply(event: SubmitEvent<HTMLFormElement>) {
    event.preventDefault();
    const parsed = parseSlug(draft);
    if (!parsed) {
      setError("Enter a repository as owner/name.");
      return;
    }
    setError(null);
    setSlug(parsed);
    setDraft(parsed);
    const next = new URL(window.location.href);
    if (parsed === defaultSlug) next.searchParams.delete("repo");
    else next.searchParams.set("repo", parsed);
    window.history.replaceState(null, "", next);
  }

  return (
    <div>
      <form
        onSubmit={apply}
        className={cn(PANEL, "mt-8 flex flex-col gap-3 p-3.5 sm:flex-row sm:items-end")}
      >
        <div className="min-w-0 flex-1">
          <label htmlFor={`${fieldId}-repo`} className={cn(EYEBROW, "block")}>
            Repository
          </label>
          <div className="mt-2 flex min-h-10 items-center rounded-md border border-border/60 bg-background/60 font-mono text-[13px] transition-[border-color] duration-150 hover:border-foreground/25 focus-within:border-accent/70">
            <span className="pl-3 text-muted-foreground select-none">
              github.com/
            </span>
            <input
              id={`${fieldId}-repo`}
              name="repo"
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              placeholder="owner/name"
              autoCapitalize="off"
              autoCorrect="off"
              spellCheck={false}
              aria-invalid={error ? true : undefined}
              aria-describedby={`${fieldId}-hint`}
              className="w-full min-w-0 flex-1 bg-transparent py-2 pr-3 pl-1 text-foreground placeholder:text-muted-foreground/50 outline-none"
            />
          </div>
        </div>
        <Button type="submit" variant="outline">
          Point every asset here
          <ArrowRight />
        </Button>
      </form>

      <div className="mt-2 flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1">
        <p id={`${fieldId}-hint`} className={cn(CAPTION, MEASURE)}>
          Every URL, preview, and snippet below is pointed at {slug}. A
          repository nobody has analyzed yet renders a placeholder frame and
          queues the work, then fills in at the same URL.
        </p>
        <a href={`/${slug}`} className={SECTION_ACTION}>
          open the {slug} report
          <span
            aria-hidden="true"
            className="transition-transform duration-150 group-hover:translate-x-0.5 motion-reduce:transition-none"
          >
            →
          </span>
        </a>
      </div>

      <AnimatePresence initial={false}>
        {error && (
          <motion.p
            key={error}
            initial={{ opacity: 0, y: reduceMotion ? 0 : -4 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, transition: { duration: 0.12 } }}
            transition={{
              duration: reduceMotion ? REDUCED_MOTION_DURATION : DURATION.enter,
              ease: EASE_OUT,
            }}
            className="mt-2 text-[11px] text-[var(--swatch-red)]"
            role="alert"
          >
            {error}
          </motion.p>
        )}
      </AnimatePresence>

      <section aria-labelledby="studio-title" className="mt-12">
        <div className={SECTION_HEADER}>
          <h2 id="studio-title" className={HEADING}>
            Metrics badge
          </h2>
          <p className={CAPTION}>interactive</p>
        </div>
        <p className={cn("mt-6", BODY, MEASURE)}>
          Stars, forks, and package downloads in one chip. Pick the metrics and
          the theme, then copy Markdown or theme-aware HTML.
        </p>
        <div className="mt-6">
          <BadgeStudio
            apiBase={apiBase}
            owner={login}
            repo={slug.split("/")[1]}
            siteOrigin={siteOrigin}
          />
        </div>
      </section>

      <section
        id="repository"
        aria-labelledby="repository-title"
        className="mt-16 scroll-mt-24 border-t border-border/60 pt-12"
      >
        <CatalogHeader
          id="repository-title"
          title="Charts, cards, and previews"
          meta={`${headline.length} assets · ${slug}`}
          graphic="stars"
        />
        <p className={cn("mt-6", BODY, MEASURE)}>
          Each asset lists the encodings its path answers: swap{" "}
          <code className="font-mono text-foreground">chart.svg</code> for{" "}
          <code className="font-mono text-foreground">chart.gif</code> and
          nothing else changes. An animated SVG plays in a GitHub README, so GIF
          is for the surfaces that show one as a single static frame — npm,
          PyPI, Docker Hub, a CSS background.
        </p>
        <div className="mt-8 grid gap-x-12 gap-y-10 sm:grid-cols-2">
          {headline.flatMap((asset) => {
            const figure = (
              <AssetFigure
                key={asset.id}
                apiBase={apiBase}
                asset={asset}
                link={repoLink}
              />
            );
            // The provenance block belongs directly under the chart it states
            // the source of, not in a section of its own.
            return asset.id === "chart"
              ? [
                  figure,
                  <ProvenanceBlock
                    key="provenance"
                    slug={slug}
                    snippet={provenance}
                  />,
                ]
              : [figure];
          })}
        </div>
      </section>

      <section
        id="health"
        aria-labelledby="health-title"
        className="mt-16 scroll-mt-24 border-t border-border/60 pt-12"
      >
        <CatalogHeader
          id="health-title"
          title="Repository-health charts"
          meta={`${health.length} assets · calculated from the public Git history`}
          graphic="health"
        />
        <div className="mt-8 grid gap-x-12 gap-y-10 sm:grid-cols-2">
          {health.map((asset) => (
            <AssetFigure
              key={asset.id}
              apiBase={apiBase}
              asset={asset}
              link={repoLink}
            />
          ))}
        </div>
      </section>

      <section
        id="profile"
        aria-labelledby="profile-title"
        className="mt-16 scroll-mt-24 border-t border-border/60 pt-12"
      >
        <div className={SECTION_HEADER}>
          <h2 id="profile-title" className={HEADING}>
            Profile README embeds
          </h2>
          <p className={CAPTION}>{profileAssets.length} assets · {login}</p>
        </div>
        <p className={cn("mt-6", BODY, MEASURE)}>
          The same routes for an account or organization, summed across its
          public repositories. An organization profile README lives in{" "}
          <code className="font-mono text-foreground">profile/README.md</code>{" "}
          inside the repository named{" "}
          <code className="font-mono text-foreground">.github</code>.
        </p>
        <div className="mt-8 grid gap-x-12 gap-y-10 sm:grid-cols-2">
          {profileAssets.map((asset) => (
            <AssetFigure
              key={asset.id}
              apiBase={apiBase}
              asset={asset}
              link={profileLink}
            />
          ))}
        </div>
      </section>
    </div>
  );
}

/**
 * The chart embed with its source stated beside it, in Markdown text.
 *
 * Provenance is never a parameter on the image: there is no provenance-stamped
 * image route, so the sentence lives in the README next to the picture, and
 * both image URLs stay plain and cacheable.
 */
function ProvenanceBlock({
  slug,
  snippet,
}: {
  slug: string;
  snippet: string | null;
}) {
  return (
    <div className="sm:col-span-2">
      <div className="flex items-baseline justify-between gap-4">
        <h3 className="text-[13px] text-foreground">
          Star history with its source stated
        </h3>
        <p className={CAPTION}>README block</p>
      </div>
      <p className={cn(CAPTION, "mt-1", MEASURE)}>
        The chart above, plus one line naming which source gitdebt read the
        series from, the date it covers, and whether it still updates. Since
        July 2026 GitHub serves the stargazer list only to a repository's own
        admins and collaborators, so two charts on the same page can come from
        different sources — this says which.
      </p>
      {snippet ? (
        <CodeBlock
          className="mt-3"
          code={snippet}
          language="html"
          label={`README.md · ${slug} · source stated`}
          copyLabel="Copy block"
          copyAriaLabel="Copy the star-history block with its source line"
          maxHeightClass="max-h-48"
        />
      ) : (
        <div className={cn(PANEL, "mt-3 flex flex-wrap items-center gap-3 p-3.5")}>
          <Button type="button" variant="soft" size="sm" disabled>
            Copy block
          </Button>
          <p className={CAPTION}>
            Available once gitdebt has read this repository.
          </p>
        </div>
      )}
    </div>
  );
}
