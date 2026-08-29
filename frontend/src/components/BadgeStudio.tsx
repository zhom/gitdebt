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
 * the default target's figures and snippets are in the static HTML before a
 * byte of JavaScript runs.
 *
 * `readme-embeds.ts` is the only catalog: its output is held to byte equality
 * with the Rust renderer behind `/api/md`, so what this page shows, what an
 * agent fetches, and what the API serves cannot drift. Nothing here is
 * hand-maintained.
 *
 * This was 899 lines, and most of the excess was one option stated twice: two
 * copy actions for the same snippet, two hand-rolled `<pre>` blocks where
 * `CodeBlock` already exists, a hand-drawn chevron on two native selects, the
 * animation switch restated as a sentence of prose, and a decorative animated
 * graphic on every section header with a caption apologising that it was not
 * real data. One control column, one preview, one snippet.
 */

import {
  useEffect,
  useId,
  useMemo,
  useState,
  type ReactNode,
  type SubmitEvent,
} from "react";

import { CodeBlock } from "@/components/CodeBlock";
import {
  BODY,
  CAPTION,
  DATUM,
  FIELD,
  HEADING,
  MEASURE,
  PANEL,
  SECTION_ACTION,
  SECTION_HEADER,
} from "@/components/style-tokens";
import { Button } from "@/components/ui/button";
import { CONTROL, Checkbox, Segmented, Switch } from "@/components/ui/controls";
import { MEDIA_RENDER_REVISION } from "@/lib/media";
import { setLiveSubject } from "@/lib/live-title";
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

/**
 * The house checkbox and switch are drawn at their true size — 16px and 20px,
 * because that is how big those marks are on a drawing. A pointer target is not
 * a mark, so it is extended past the mark with a transparent overlay rather
 * than by inflating the graphic. 44px in both axes, both controls.
 */
const CHECKBOX_TARGET = "relative before:absolute before:-inset-3.5 before:content-['']";
const SWITCH_TARGET =
  "relative before:absolute before:-inset-y-3 before:-inset-x-1 before:content-['']";

/** A native select, wearing the field treatment every control here shares. */
const SELECT = "block min-h-11 w-full px-3 font-mono text-[0.8125rem]";

type Metric = "stars" | "forks" | "downloads";
type BadgeSource = "auto" | "npm" | "crates" | "pypi" | "docker";
type ThemeChoice = "auto" | "light" | "dark";
type Dialect = "markdown" | "html";

const METRICS: { id: Metric; label: string }[] = [
  { id: "stars", label: "Stars" },
  { id: "forks", label: "Forks" },
  { id: "downloads", label: "Downloads" },
];

const SOURCES: { id: BadgeSource; label: string }[] = [
  { id: "auto", label: "Auto" },
  { id: "npm", label: "npm" },
  { id: "crates", label: "crates.io" },
  { id: "pypi", label: "PyPI" },
  { id: "docker", label: "Docker Hub" },
];

const THEMES: { value: ThemeChoice; label: string }[] = [
  { value: "auto", label: "Auto" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

const DIALECTS: { value: Dialect; label: string }[] = [
  { value: "markdown", label: "Markdown" },
  { value: "html", label: "HTML" },
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
  const [dialect, setDialect] = useState<Dialect>("markdown");
  const renderedTheme = useRenderedTheme();
  const uid = useId().replaceAll(":", "");

  const badgeBase = `${apiBase}/api/repos/${owner}/${repo}/badge.svg`;

  function toggleMetric(id: Metric) {
    setMetrics((prev) => {
      const next = prev.includes(id)
        ? prev.filter((m) => m !== id)
        : [...prev, id];
      return METRICS.filter((m) => next.includes(m.id)).map((m) => m.id);
    });
  }

  /**
   * One builder for four URLs. A preview carries the render revision and may
   * animate; a published URL does neither, ever — those are the only two
   * differences, and stating them once is what stops them drifting apart.
   */
  const url = (assetTheme: "light" | "dark", preview: boolean) =>
    `${badgeBase}?${badgeQuery({ metrics, animate: preview && animate, source, theme: assetTheme, preview })}`;

  const baked: "light" | "dark" =
    theme === "auto" ? (renderedTheme === "dark" ? "dark" : "light") : theme;
  const previewUrl = url(baked, true);

  const label = `${owner}/${repo}`;
  const alt = `${label} stats badge`;
  const linkHref = readmeLink(siteOrigin, `/${label}`);

  const snippet =
    theme === "auto"
      ? `<a href="${linkHref}">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="${url("dark", false)}" />
    <img alt="${alt}" src="${url("light", false)}" />
  </picture>
</a>`
      : dialect === "markdown"
        ? `[![${alt}](${url(theme, false)})](${linkHref})`
        : `<a href="${linkHref}">
  <img alt="${alt}" src="${url(theme, false)}" />
</a>`;

  const noMetrics = metrics.length === 0;

  return (
    <div className="grid gap-x-10 gap-y-8 lg:grid-cols-[minmax(0,17rem)_minmax(0,1fr)]">
      {/* ── The control column. One statement of each option, and no more. ── */}
      <div className="space-y-7">
        <fieldset>
          <legend className={FIELD}>Metrics</legend>
          <div className="mt-2">
            {METRICS.map((m) => (
              <div key={m.id} className="flex min-h-11 items-center gap-3">
                <Checkbox
                  checked={metrics.includes(m.id)}
                  onCheckedChange={() => toggleMetric(m.id)}
                  aria-labelledby={`${uid}-${m.id}`}
                  className={CHECKBOX_TARGET}
                />
                <span id={`${uid}-${m.id}`} className="text-[0.875rem] text-ink">
                  {m.label}
                </span>
              </div>
            ))}
          </div>
          {noMetrics && (
            <p role="alert" className="mt-1 text-[0.8125rem] text-signal">
              Pick at least one metric.
            </p>
          )}
        </fieldset>

        <div>
          <label htmlFor={`${uid}-source`} className={FIELD}>
            Downloads from
          </label>
          <select
            id={`${uid}-source`}
            name="source"
            value={source}
            onChange={(event) => setSource(event.target.value as BadgeSource)}
            className={cn(CONTROL, SELECT, "mt-2")}
          >
            {SOURCES.map((s) => (
              <option key={s.id} value={s.id}>
                {s.label}
              </option>
            ))}
          </select>
        </div>

        <div>
          {/* `Segmented` forwards only `aria-label`, so the drawn label and the
              accessible name are stated separately and identically. */}
          <p className={FIELD}>Theme</p>
          <Segmented
            aria-label="Theme"
            value={theme}
            options={THEMES}
            onValueChange={setTheme}
            className="mt-2 w-full"
          />
        </div>

        <div className="flex min-h-11 items-center justify-between gap-4">
          <span className={FIELD} id={`${uid}-motion`}>
            Motion in the preview
          </span>
          <Switch
            checked={animate}
            onCheckedChange={setAnimate}
            aria-labelledby={`${uid}-motion`}
            className={SWITCH_TARGET}
          />
        </div>

        {/* The dialect only changes anything once a single theme is baked: an
            `auto` badge is two assets and `<picture>` is the only markup that
            can carry both, so there is nothing to choose. */}
        {theme !== "auto" && (
          <div>
            <p className={FIELD}>Snippet</p>
            <Segmented
              aria-label="Snippet dialect"
              value={dialect}
              options={DIALECTS}
              onValueChange={setDialect}
              className="mt-2 w-full"
            />
          </div>
        )}
      </div>

      {/* ── The specimen, and the snippet that publishes it. ──────────────── */}
      <div className="min-w-0 space-y-6">
        <figure className="border border-rule-strong bg-paper">
          <figcaption className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1 border-b border-rule px-3 py-3">
            <span className={FIELD}>Specimen</span>
            <span className={cn(DATUM, "min-w-0 truncate text-ink-3")}>
              {label}
            </span>
          </figcaption>
          <div className="flex min-h-36 items-center justify-center bg-table px-4 py-8">
            {noMetrics ? (
              <p className={CAPTION}>Pick a metric to draw the badge.</p>
            ) : (
              <img
                src={previewUrl}
                alt={alt}
                decoding="async"
                className="block h-auto max-w-full"
              />
            )}
          </div>
          <p className={cn(CAPTION, "border-t border-rule px-3 py-3")}>
            The specimen may move; a published snippet never does. Add{" "}
            <code className="font-mono text-ink">animate=1</code> to the URL when
            you want the motion in a README.
          </p>
        </figure>

        {!noMetrics && (
          <CodeBlock
            code={snippet}
            language={theme === "auto" ? "html" : dialect}
            label={`${theme === "auto" || dialect === "html" ? "HTML" : "README.md"} · ${label} · ${theme}`}
            copyLabel="Copy badge"
            copyAriaLabel="Copy the metrics badge snippet"
            maxHeightClass="max-h-48"
          />
        )}
      </div>
    </div>
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
        <code key={index} className="font-mono text-ink">
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
function previewAssetUrl(
  apiBase: string,
  asset: EmbedAsset,
  theme: "light" | "dark",
): string {
  const url = assetUrl(apiBase, asset, { theme });
  return `${url}${url.includes("?") ? "&" : "?"}render=${MEDIA_RENDER_REVISION}`;
}

/**
 * One asset: what it is, what it looks like, and the exact markup to publish
 * it. The three regions sit on one gutter and the snippet is anchored to the
 * bottom, so a row of figures lines up whatever length their descriptions run.
 */
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
  const light = previewAssetUrl(apiBase, asset, "light");
  const dark = previewAssetUrl(apiBase, asset, "dark");

  return (
    <figure
      className={cn(
        "flex h-full flex-col border border-rule-strong bg-paper",
        FULL_WIDTH.has(asset.id) && "sm:col-span-2",
      )}
    >
      <figcaption className="border-b border-rule px-3 py-3">
        <div className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1">
          <h3 className="font-draft text-[1.0625rem] leading-[1.2] text-ink">{asset.name}</h3>
          <span className={cn(DATUM, "text-ink-3")}>
            {asset.formats.join(" · ")}
            {asset.themed ? " · light + dark" : ""}
          </span>
        </div>
        <p className={cn(CAPTION, "mt-1.5")}>
          {inlineCode(asset.purpose)} Goes in {inlineCode(asset.placement)}.
        </p>
      </figcaption>

      <div className="flex flex-1 items-center justify-center bg-table px-3 py-6">
        {/* One `<img>`, with the dark variant added as a `<source>` when the
            asset actually has one. A `<picture>` with no source is exactly its
            own `<img>`, so the two branches were one branch. */}
        <picture>
          {asset.themed && (
            <source media="(prefers-color-scheme: dark)" srcSet={dark} />
          )}
          <img
            src={light}
            alt={asset.alt}
            loading="lazy"
            decoding="async"
            className="block h-auto max-w-full"
          />
        </picture>
      </div>

      <div className="border-t border-rule">
        <CodeBlock
          className="border-0"
          code={snippet}
          language={language}
          label={asset.themed ? "HTML" : "README.md"}
          copyLabel="Copy"
          copyAriaLabel={`Copy the ${asset.name} embed snippet`}
          maxHeightClass="max-h-36"
        />
      </div>
    </figure>
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
    retitle(parsed);
  }, []);

  const login = slug.split("/")[0];
  const repoAssets = useMemo(() => repoEmbedAssets(slug), [slug]);
  const profileAssets = useMemo(() => profileEmbedAssets(login), [login]);
  const repoLink = readmeLink(siteOrigin, `/${slug}`);
  const profileLink = readmeLink(siteOrigin, `/${login}`);

  const headline = repoAssets.filter((asset) => asset.group !== "health");
  const health = repoAssets.filter((asset) => asset.group === "health");

  // Composed at build time for one repository. A visitor-typed target has no
  // analyze payload on a static page, so its source is not established here.
  const provenance = slug === defaultSlug ? defaultProvenance : null;

  /**
   * The tab follows the URL. The address bar is rewritten below, and nothing
   * used to correct the title with it, so a bookmark of
   * `/badges?repo=vercel/next.js` was filed under the generic catalog title.
   * No `path` is passed: this page really is prerendered at `/badges`, so its
   * canonical must not be rewritten to a query URL.
   */
  function retitle(target: string) {
    setLiveSubject({
      subject: `${target} README embeds`,
      description: `Every gitdebt README embed and badge, pointed at ${target}.`,
    });
  }

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
    retitle(parsed);
  }

  return (
    <div>
      <form
        onSubmit={apply}
        className={cn(PANEL, "mt-8 flex flex-col gap-4 sm:flex-row sm:items-end")}
      >
        <div className="min-w-0 flex-1">
          <label htmlFor={`${fieldId}-repo`} className={FIELD}>
            Repository
          </label>
          <div className="mt-2 flex min-h-11 items-center border border-rule-strong bg-paper font-mono text-[0.8125rem] transition-colors duration-[--duration-ui] hover:border-ink-3 focus-within:border-ink-3">
            <span className="pl-3 text-ink-3 select-none">github.com/</span>
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
              aria-describedby={error ? `${fieldId}-error` : `${fieldId}-hint`}
              className="w-full min-w-0 flex-1 bg-transparent py-2 pr-3 pl-1 text-ink outline-none placeholder:text-ink-3"
            />
          </div>
        </div>
        <Button type="submit" variant="primary">
          Point every asset here
        </Button>
      </form>

      {error && (
        <p
          id={`${fieldId}-error`}
          role="alert"
          className="mt-2 text-[0.8125rem] text-signal"
        >
          {error}
        </p>
      )}

      <div className="mt-3 flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1">
        <p id={`${fieldId}-hint`} className={cn(CAPTION, MEASURE)}>
          Every URL, preview and snippet below is pointed at {slug}. A repository
          nobody has read yet renders a placeholder frame and queues the work,
          then fills in at the same URL.
        </p>
        <a href={`/${slug}`} className={SECTION_ACTION}>
          open the {slug} report
        </a>
      </div>

      <section aria-labelledby="studio-title" className="mt-14">
        <div className={SECTION_HEADER}>
          <h2 id="studio-title" className={HEADING}>
            Metrics badge
          </h2>
          <p className={CAPTION}>Stars, forks and package downloads in one chip</p>
        </div>
        <div className="mt-8">
          <BadgeStudio
            apiBase={apiBase}
            owner={login}
            repo={slug.split("/")[1]}
            siteOrigin={siteOrigin}
          />
        </div>
      </section>

      <CatalogSection
        id="repository"
        title="Charts, cards and previews"
        count={headline.length}
        subject={slug}
        lead={
          <>
            Each figure lists the encodings its path answers: swap{" "}
            <code className="font-mono text-ink">chart.svg</code> for{" "}
            <code className="font-mono text-ink">chart.gif</code> and nothing else
            changes.
          </>
        }
      >
        {headline.flatMap((asset) => {
          const figure = (
            <AssetFigure
              key={asset.id}
              apiBase={apiBase}
              asset={asset}
              link={repoLink}
            />
          );
          // The provenance block belongs directly under the chart whose source
          // it states, not in a section of its own.
          return asset.id === "chart"
            ? [
                figure,
                <ProvenanceBlock key="provenance" slug={slug} snippet={provenance} />,
              ]
            : [figure];
        })}
      </CatalogSection>

      <CatalogSection
        id="health"
        title="Repository-health charts"
        count={health.length}
        subject={slug}
        lead="Calculated from the public commit history, not from stars — which is why they can disagree with the star curve, and why it matters when they do."
      >
        {health.map((asset) => (
          <AssetFigure
            key={asset.id}
            apiBase={apiBase}
            asset={asset}
            link={repoLink}
          />
        ))}
      </CatalogSection>

      <CatalogSection
        id="profile"
        title="Profile README embeds"
        count={profileAssets.length}
        subject={login}
        lead={
          <>
            The same routes for an account, summed across its public
            repositories. An organization's profile README lives in{" "}
            <code className="font-mono text-ink">profile/README.md</code> inside
            the repository named{" "}
            <code className="font-mono text-ink">.github</code>.
          </>
        }
      >
        {profileAssets.map((asset) => (
          <AssetFigure
            key={asset.id}
            apiBase={apiBase}
            asset={asset}
            link={profileLink}
          />
        ))}
      </CatalogSection>
    </div>
  );
}

/**
 * One section of the catalog: a heading, the count it actually rendered, one
 * paragraph, and the grid.
 *
 * The count is stated beside the heading because it is a measured quantity —
 * the length of the array below it, never a number typed into the copy.
 */
function CatalogSection({
  id,
  title,
  count,
  subject,
  lead,
  children,
}: {
  id: string;
  title: string;
  count: number;
  subject: string;
  lead: ReactNode;
  children: ReactNode;
}) {
  return (
    <section
      id={id}
      aria-labelledby={`${id}-title`}
      className="mt-16 scroll-mt-24 border-t border-rule pt-12"
    >
      <div className={SECTION_HEADER}>
        <h2 id={`${id}-title`} className={HEADING}>
          {title}
        </h2>
        <p className={cn(DATUM, "text-ink-3")}>
          {count} assets · {subject}
        </p>
      </div>
      <p className={cn("mt-3", BODY, MEASURE)}>{lead}</p>
      <div className="mt-8 grid items-stretch gap-6 sm:grid-cols-2">
        {children}
      </div>
    </section>
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
      <div className={SECTION_HEADER}>
        <h3 className="font-draft text-[1.0625rem] leading-[1.2] text-ink">
          Star history with its source stated
        </h3>
        <p className={cn(DATUM, "text-ink-3")}>README block</p>
      </div>
      <p className={cn("mt-2", CAPTION, MEASURE)}>
        The chart above, plus one line naming the source gitdebt read the series
        from, the date it covers, and whether it still updates. Two charts on the
        same page can come from different sources; this says which.
      </p>
      {snippet ? (
        <CodeBlock
          className="mt-4"
          code={snippet}
          language="html"
          label={`README.md · ${slug} · source stated`}
          copyLabel="Copy block"
          copyAriaLabel="Copy the star-history block with its source line"
          maxHeightClass="max-h-48"
        />
      ) : (
        <div className={cn(PANEL, "mt-4 flex flex-wrap items-center gap-4")}>
          <Button type="button" variant="quiet" disabled>
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
