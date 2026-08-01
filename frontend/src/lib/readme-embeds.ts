/**
 * The catalog of everything gitdebt can put in somebody else's README, and the
 * rules for putting it there correctly.
 *
 * Three surfaces publish this information and must never drift apart: the
 * clipboard prompt the "Ask an agent" button hands to a coding agent
 * (`agent-prompt.ts`), the human badge catalog at `/badges`, and the Markdown
 * the API serves at `/api/md/{path}`. The first two read this module; the
 * third is a Rust port in `backend/src/agent_embeds.rs`, held to byte equality
 * with it by `scripts/embed-parity.test.mjs` and the matching Rust test.
 *
 * Everything here is pure and deterministic given a slug and an API origin —
 * no wall clock, no fetch — so the snippet a visitor copies, the snippet the
 * API renders, and the snippet in the prompt are byte-identical.
 *
 * Relative specifiers with their extension: this module is covered by the Node
 * test runner in `scripts/`, which resolves neither the `@/` alias nor an
 * extensionless specifier.
 */

/** Every raster/vector encoding an asset route can answer with. */
export type EmbedFormat = "svg" | "png" | "webp" | "gif";

/** The surface an asset describes, which decides how it is grouped. */
export type EmbedGroup = "headline" | "health" | "social";

export type EmbedAsset = {
  id: string;
  /** Display name, also the Markdown heading the snippet sits under. */
  name: string;
  /** One line: what a reader learns from it. */
  purpose: string;
  /** Path under the API origin, including any asset-defining query. */
  path: string;
  /** Alt text. Descriptive, because README images are read by screen readers. */
  alt: string;
  /**
   * Whether light and dark variants are both worth publishing. False for
   * assets that ship one baked appearance (GIF-only motion, social PNGs).
   */
  themed: boolean;
  formats: EmbedFormat[];
  group: EmbedGroup;
  /** Where the asset earns its place, in the words an agent can act on. */
  placement: string;
};

/**
 * README assets are static by default and animation is opt-in, so no builder
 * here ever emits `animate=1`. GitHub sanitizes SMIL out of SVG in many
 * contexts anyway; `.gif` is the honest way to ship motion.
 */
export const STATIC_BY_DEFAULT =
  "Published snippets are static. Motion is opt-in: add `animate=1` to an SVG " +
  "URL, or use the `.gif` variant where one exists, because GitHub strips SVG " +
  "animation from README images in several contexts.";

/** Repository-health charts share a route shape, a caveat, and a placement. */
const HEALTH_CHARTS: {
  id: string;
  name: string;
  purpose: string;
  alt: string;
}[] = [
  {
    id: "heatmap",
    name: "Commit calendar",
    purpose: "Daily commit density across the last 52 weeks.",
    alt: "commit activity calendar",
  },
  {
    id: "commit-trend",
    name: "Maintenance pulse",
    purpose: "Commit volume over time, so a slowdown is visible rather than implied.",
    alt: "commit trend",
  },
  {
    id: "contributors",
    name: "Contributors",
    purpose: "Who is actually landing commits, ranked, with avatars inlined.",
    alt: "contributors",
  },
  {
    id: "bus-factor",
    name: "Ownership concentration",
    purpose: "How few people write half the commits.",
    alt: "bus factor",
  },
  {
    id: "lines",
    name: "Language activity",
    purpose: "Lines of code by language across the analyzed history.",
    alt: "language activity",
  },
  {
    id: "top-files",
    name: "File change frequency",
    purpose: "The files the most commits touch, dependency manifests excluded.",
    alt: "file change frequency",
  },
  {
    id: "bug-magnets",
    name: "Fix-labelled changes",
    purpose: "Files most often touched by commits whose message reads like a fix.",
    alt: "fix-labelled changes",
  },
  {
    id: "todo-trend",
    name: "TODO/FIXME movement",
    purpose: "Whether known debt markers are being added or paid down.",
    alt: "recent TODO and FIXME movement",
  },
];

/**
 * Everything embeddable for one repository, in the order a README would want
 * it: the badge row first, then the chart most projects came for, then the
 * evidence a reader would ask for next.
 */
export function repoEmbedAssets(slug: string): EmbedAsset[] {
  const base = `/api/repos/${slug}`;
  return [
    {
      id: "badge-metrics",
      name: "Metrics badge",
      purpose: "Stars and forks in one compact chip, served from gitdebt's cache.",
      path: `${base}/badge.svg?metrics=stars,forks`,
      alt: `${slug} stars and forks`,
      themed: true,
      formats: ["svg", "png", "webp"],
      group: "headline",
      placement:
        "the badge row directly under the project title, alongside CI and license badges",
    },
    {
      id: "badge-signal",
      name: "Earned signal badge",
      purpose:
        "One evidence-backed claim — actively maintained, community powered, star momentum, or contributor readiness.",
      path: `${base}/badge.svg?signal=active`,
      alt: `${slug} actively maintained`,
      themed: true,
      formats: ["svg", "png", "webp"],
      group: "headline",
      placement:
        "the badge row, but only for signals the repository has actually earned",
    },
    {
      id: "chart",
      name: "Star history",
      purpose: "The full cumulative star curve, served from Postgres.",
      path: `${base}/chart.svg`,
      alt: `${slug} star history`,
      themed: true,
      formats: ["svg", "png", "webp", "gif"],
      group: "headline",
      placement:
        "a `## Star history` section near the bottom of the README, above License",
    },
    {
      id: "card",
      name: "Repository card",
      purpose:
        "Stars, forks, contributors, languages, and a 90-day sparkline in one panel.",
      path: `${base}/card.svg`,
      alt: `${slug} repository statistics`,
      themed: true,
      formats: ["svg", "png", "webp", "gif"],
      group: "headline",
      placement: "an About or Project status section, or a docs-site sidebar",
    },
    {
      id: "usage",
      name: "Stars versus downloads",
      purpose:
        "Star growth against package-registry downloads, for projects that publish one.",
      path: `${base}/usage.svg`,
      alt: `${slug} stars versus package downloads`,
      themed: true,
      formats: ["svg", "png", "webp"],
      group: "headline",
      placement: "next to the star-history chart, when the project ships a package",
    },
    ...HEALTH_CHARTS.map((chart): EmbedAsset => ({
      id: chart.id,
      name: chart.name,
      purpose: chart.purpose,
      path: `${base}/stats/${chart.id}.svg`,
      alt: `${slug} ${chart.alt}`,
      themed: true,
      formats: ["svg", "png", "webp", "gif"],
      group: "health",
      placement:
        "a Project health or Contributing section, where a prospective contributor is already reading",
    })),
    {
      id: "og",
      name: "Social preview",
      purpose: "A 1200x630 PNG for link unfurls on social platforms and chat apps.",
      path: `${base}/og.png`,
      alt: `${slug} on gitdebt`,
      themed: false,
      formats: ["png", "webp"],
      group: "social",
      placement:
        "a docs-site `og:image` meta tag — not the README, where it would be redundant",
    },
  ];
}

/** Everything embeddable for one maintainer account or organization. */
export function profileEmbedAssets(login: string): EmbedAsset[] {
  const base = `/api/users/${login}`;
  return [
    {
      id: "card",
      name: "Maintainer card",
      purpose:
        "Aggregate public-repository totals for the account in one compact panel.",
      path: `${base}/card.svg`,
      alt: `${login} maintainer statistics`,
      themed: true,
      formats: ["svg", "png", "webp", "gif"],
      group: "headline",
      placement: "the top of a profile README, under the introduction",
    },
    {
      id: "chart",
      name: "Aggregate star history",
      purpose: "One curve summing star growth across every public repository owned.",
      path: `${base}/chart.svg`,
      alt: `Aggregate star history across ${login}'s public repositories`,
      themed: true,
      formats: ["svg", "png", "webp", "gif"],
      group: "headline",
      placement: "a profile README, below the card",
    },
    {
      id: "contributions",
      name: "Contribution footprint",
      purpose: "Authored work in owned projects versus other people's projects.",
      path: `${base}/stats/contributions.svg`,
      alt: `${login} contribution footprint`,
      themed: true,
      formats: ["svg", "png", "webp", "gif"],
      group: "health",
      placement: "a profile README, in place of a generic contribution-count widget",
    },
    {
      id: "languages",
      name: "Language footprint",
      purpose: "Lines of code by language across every analyzed owned repository.",
      path: `${base}/stats/languages.svg`,
      alt: `${login} language footprint`,
      themed: true,
      formats: ["svg", "png", "webp", "gif"],
      group: "health",
      placement: "a profile README, next to the contribution footprint",
    },
    {
      id: "commit-activity",
      name: "Commit activity",
      purpose: "Every commit landed in the last 52 weeks, summed across owned repos.",
      path: `${base}/stats/commit-activity.svg`,
      alt: `${login} commit activity`,
      themed: true,
      formats: ["svg", "png", "webp", "gif"],
      group: "health",
      placement: "a profile README, as the activity strip",
    },
    {
      id: "og",
      name: "Social preview",
      purpose: "A 1200x630 PNG for link unfurls.",
      path: `${base}/og.png`,
      alt: `${login} on gitdebt`,
      themed: false,
      formats: ["png", "webp"],
      group: "social",
      placement: "a personal site's `og:image` meta tag",
    },
  ];
}

/** Swap an asset path onto another format, preserving its query string. */
function withFormat(path: string, format: EmbedFormat): string {
  const query = path.indexOf("?");
  const file = query === -1 ? path : path.slice(0, query);
  const search = query === -1 ? "" : path.slice(query);
  return `${file.replace(/\.(svg|png|webp|gif)$/, `.${format}`)}${search}`;
}

function withParam(path: string, key: string, value: string): string {
  return `${path}${path.includes("?") ? "&" : "?"}${key}=${value}`;
}

/**
 * The absolute URL a README would carry: no cache-busting revision, no
 * attribution parameter. Attribution belongs on the surrounding link, never on
 * an image URL that a CDN has to key on.
 */
export function assetUrl(
  apiBase: string,
  asset: EmbedAsset,
  options: { theme?: "light" | "dark"; format?: EmbedFormat } = {},
): string {
  const format = options.format ?? asset.formats[0];
  let path = withFormat(asset.path, format);
  if (options.theme && asset.themed) path = withParam(path, "theme", options.theme);
  return `${apiBase}${path}`;
}

/** The gitdebt report an embed links back to, carrying README attribution. */
export function readmeLink(siteOrigin: string, path: string): string {
  const origin = siteOrigin.replace(/\/+$/, "");
  const route = path.startsWith("/") ? path : `/${path}`;
  return `${origin}${route}?ref=readme`;
}

/** `[![alt](url)](link)` against one baked theme. */
export function markdownEmbed(
  apiBase: string,
  asset: EmbedAsset,
  link: string,
  theme: "light" | "dark" = "dark",
): string {
  return `[![${asset.alt}](${assetUrl(apiBase, asset, { theme })})](${link})`;
}

/**
 * The theme-aware form. GitHub renders README images against the reader's OS
 * preference rather than the page, and an SVG cannot answer that itself
 * because its colors are baked, so both variants ship and `<picture>` chooses.
 */
export function pictureEmbed(
  apiBase: string,
  asset: EmbedAsset,
  link: string,
): string {
  return [
    `<a href="${link}">`,
    "  <picture>",
    `    <source media="(prefers-color-scheme: dark)" srcset="${assetUrl(apiBase, asset, { theme: "dark" })}" />`,
    `    <img alt="${asset.alt}" src="${assetUrl(apiBase, asset, { theme: "light" })}" />`,
    "  </picture>",
    "</a>",
  ].join("\n");
}

/** The snippet to publish: theme-aware where that is meaningful, Markdown otherwise. */
export function bestEmbed(
  apiBase: string,
  asset: EmbedAsset,
  link: string,
): string {
  return asset.themed
    ? pictureEmbed(apiBase, asset, link)
    : markdownEmbed(apiBase, asset, link);
}

/** The dialect `bestEmbed` returned, for fencing it in a code block. */
export function bestEmbedLanguage(asset: EmbedAsset): "html" | "markdown" {
  return asset.themed ? "html" : "markdown";
}

/** The rules that make a published embed correct rather than merely present. */
export const EMBED_RULES: string[] = [
  "No account, token, or API key is involved. Every URL is a plain public image.",
  "Themes are baked into each asset because GitHub renders README images " +
    "against the reader's OS preference, not the page. Publish both variants " +
    "with an HTML `<picture>` element, or pick one explicitly with " +
    "`theme=light` / `theme=dark`. There is no `theme=auto`.",
  STATIC_BY_DEFAULT,
  "Keep the surrounding link and its `?ref=readme` parameter. Attribution " +
    "lives on the link; the image URL stays plain so CDNs can cache it.",
  "Do not add cache-busting query parameters. Media is edge-cached for a few " +
    "hours by design and refreshes on its own.",
  "Alt text is not optional. Say what the image shows, not \"chart\".",
  "A repository nobody has analyzed yet renders a placeholder frame and queues " +
    "the work instead of failing. Load the page once, or wait a few minutes, " +
    "and the real chart replaces it at the same URL.",
];

/** Query parameters that change what an asset renders. */
export const QUERY_REFERENCE: {
  param: string;
  applies: string;
  effect: string;
}[] = [
  {
    param: "theme=light|dark",
    applies: "every SVG and raster asset",
    effect: "Bakes that palette into the output. Default is light.",
  },
  {
    param: "animate=1",
    applies: "SVG charts, cards, and badges",
    effect: "Opts into motion. Off by default; use the `.gif` variant where GitHub strips SVG animation.",
  },
  {
    param: "from=YYYY-MM-DD&to=YYYY-MM-DD",
    applies: "star-history charts",
    effect: "Inclusive date window. An invalid or inverted range is a 400.",
  },
  {
    param: "rebase=1",
    applies: "star-history charts",
    effect: "Starts every series at zero, so projects of different ages compare fairly.",
  },
  {
    param: "type=date|timeline",
    applies: "star-history charts",
    effect: "Calendar dates, or days-since-first-star.",
  },
  {
    param: "log=1",
    applies: "star-history charts",
    effect: "Logarithmic y axis.",
  },
  {
    param: "repos=owner/repo,owner/repo",
    applies: "/api/chart.svg",
    effect: "Overlays several repositories on one chart.",
  },
  {
    param: "metrics=stars,forks,downloads",
    applies: "/badge.svg",
    effect: "Chooses the chips and their order. `downloads` needs a published package.",
  },
  {
    param: "signal=active|community|momentum|contributor-ready",
    applies: "/badge.svg",
    effect: "Renders one evidence-backed claim instead of raw metrics.",
  },
  {
    param: "hide_border=1, hide_title=1, card_width=N",
    applies: "/card.svg",
    effect: "Trims the card for tight layouts.",
  },
];

/**
 * Star-history widgets a project may already carry. An agent should replace
 * these in place rather than stacking a second chart underneath.
 */
export const EXISTING_STAR_HISTORY_MARKERS: string[] = [
  "star-history.com",
  "api.star-history.com",
  "starchart.cc",
  "stars.medv.io",
  "seladb/starhistory",
];

/** Files worth checking beyond `README.md`, in the order they usually pay off. */
export const CANDIDATE_FILES: string[] = [
  "README.md at the repository root",
  "docs/index.md, docs/README.md, or a docs-site landing page",
  "website/ or site/ landing content, if the project publishes one",
  "CONTRIBUTING.md, where repository-health charts tell a contributor what they are joining",
  // An organization profile README is `profile/README.md` inside a repository
  // literally named `.github`. `.github/profile/README.md` is a path that
  // exists in no other checkout, so an agent told to look there finds nothing.
  "profile/README.md, when the checkout is the account's `.github` repository, " +
    "which is where an organization profile README lives",
];
