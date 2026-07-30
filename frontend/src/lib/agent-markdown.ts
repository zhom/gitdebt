/**
 * The Markdown representation served at `<any page URL>.md`.
 *
 * These files exist for agents, and an agent asking about a repository wants
 * two things: the numbers, and what it can do with them. So every repository
 * page carries its measured star and health figures, the exact paste-ready
 * README snippets for that repository, and the rules that make a published
 * embed correct — with `/badges.md` holding the complete asset catalog so the
 * per-repository files stay short enough to read in full.
 *
 * Pure and deterministic given its inputs. Fetching happens in the route.
 *
 * Relative specifiers with their extension: the Node test runner in `scripts/`
 * resolves neither the `@/` alias nor an extensionless specifier.
 */

import {
  PLACEHOLDER_SLUG,
  repoAgentPrompt,
  type StarFacts,
} from "./agent-prompt.ts";
import { healthFacts, healthReadings, type RepoHealth } from "./repo-health.ts";
import {
  EMBED_RULES,
  QUERY_REFERENCE,
  bestEmbed,
  bestEmbedLanguage,
  profileEmbedAssets,
  readmeLink,
  repoEmbedAssets,
  type EmbedAsset,
} from "./readme-embeds.ts";
import { formatCompact, formatMonthYear, starMilestones, type HistoryPoint } from "./star-insights.ts";

export type RepoFacts = {
  stars: StarFacts;
  history: HistoryPoint[];
  createdAt: string | null;
  coverageEnd: string | null;
  eventCount: number | null;
};

export type AgentPage =
  | { kind: "static"; path: string; title: string; description: string }
  | {
      kind: "repo";
      slug: string;
      updatedAt: string | null;
      facts: RepoFacts | null;
      health: RepoHealth | null;
      notFound: boolean;
    }
  | {
      kind: "profile";
      login: string;
      totalStars: number | null;
      reposIncluded: number | null;
      firstYear: number | null;
    }
  | { kind: "category"; slug: string; name: string; description: string; repos: string[] }
  | {
      kind: "comparison";
      first: string;
      second: string;
      facts: Record<string, RepoFacts | null>;
    };

function absolute(site: string, path: string): string {
  return new URL(path, site).href;
}

function documentHeader(title: string, canonical: string): string {
  return `# ${title}\n\nCanonical HTML: ${canonical}`;
}

function bullet(lines: string[]): string {
  return lines.map((line) => `- ${line}`).join("\n");
}

/**
 * A GitHub-flavoured table. Pipes are escaped in every cell, including inside
 * code spans, which GFM requires — `theme=light|dark` would otherwise open a
 * phantom column.
 */
function table(headers: string[], rows: string[][]): string {
  const cell = (value: string) => value.replaceAll("|", "\\|");
  const head = `| ${headers.map(cell).join(" | ")} |`;
  const rule = `| ${headers.map(() => "---").join(" | ")} |`;
  const body = rows.map((row) => `| ${row.map(cell).join(" | ")} |`);
  return [head, rule, ...body].join("\n");
}

function fence(language: string, body: string): string {
  return `\`\`\`${language}\n${body}\n\`\`\``;
}

/** A fence that can hold fenced content of its own. */
function outerFence(language: string, body: string): string {
  return `\`\`\`\`${language}\n${body}\n\`\`\`\``;
}

/** One asset as a heading, its purpose, and its paste-ready snippet. */
function assetSection(
  apiBase: string,
  asset: EmbedAsset,
  link: string,
  level: string,
): string {
  return [
    `${level} ${asset.name}`,
    "",
    asset.purpose,
    "",
    `Goes in ${asset.placement}.`,
    "",
    fence(bestEmbedLanguage(asset), bestEmbed(apiBase, asset, link)),
  ].join("\n");
}

/** The shared rules block, identical wherever embedding is documented. */
function rulesSection(): string {
  return `## Embedding rules\n\n${bullet(EMBED_RULES)}`;
}

function parameterSection(): string {
  return [
    "## Query parameters",
    "",
    table(
      ["Parameter", "Applies to", "Effect"],
      QUERY_REFERENCE.map((entry) => [
        `\`${entry.param}\``,
        entry.applies,
        entry.effect,
      ]),
    ),
  ].join("\n");
}

function repoMarkdown(
  page: Extract<AgentPage, { kind: "repo" }>,
  site: string,
  apiBase: string,
): string {
  const { slug } = page;
  const canonical = absolute(site, `/${slug}`);
  const link = readmeLink(site, `/${slug}`);
  const assets = repoEmbedAssets(slug);
  const byId = new Map(assets.map((asset) => [asset.id, asset]));

  const sections: string[] = [
    documentHeader(`${slug} — GitHub star history and repository health`, canonical),
  ];

  if (page.notFound) {
    sections.push(
      `> GitHub does not expose \`${slug}\` as a public repository. gitdebt never ` +
        "analyzes or counts private repositories.",
      `Check the owner and name, or open https://github.com/${slug} directly.`,
    );
    return `${sections.join("\n\n")}\n`;
  }

  sections.push(
    "> Analytics for a public GitHub repository: star history from gitdebt's " +
      "Postgres cache, repository health computed from the public Git history. " +
      "Private repositories are never analyzed or counted.",
  );

  // --- Measured figures -----------------------------------------------------
  const facts = page.facts;
  if (facts) {
    const rows: string[][] = [];
    const stars = facts.stars;
    if (stars.totalStars !== null) {
      rows.push(["GitHub stars", stars.totalStars.toLocaleString()]);
    }
    if (stars.gained90 !== null) {
      rows.push([
        stars.approximate ? "Star actions, trailing 90d" : "Stars gained, trailing 90d",
        `+${stars.gained90.toLocaleString()}`,
      ]);
    }
    if (stars.gained30 !== null) {
      rows.push([
        stars.approximate ? "Star actions, trailing 30d" : "Stars gained, trailing 30d",
        `+${stars.gained30.toLocaleString()}`,
      ]);
    }
    if (stars.trend) {
      rows.push(["Pace against lifetime average", stars.trend]);
    }
    if (stars.firstStarMonth) {
      rows.push(["Star history begins", stars.firstStarMonth]);
    }
    const created = formatMonthYear(facts.createdAt);
    if (created) rows.push(["Repository created", created]);
    const coverage = formatMonthYear(facts.coverageEnd);
    if (coverage) rows.push(["Data through", coverage]);

    if (rows.length > 0) {
      sections.push(`## Star snapshot\n\n${table(["Metric", "Value"], rows)}`);
    }

    if (stars.approximate) {
      sections.push(
        "This curve is public GH Archive star *activity*: it records star " +
          "actions and cannot see unstars, so it is an attention signal rather " +
          "than a net-star series" +
          (facts.eventCount !== null
            ? `. ${facts.eventCount.toLocaleString()} star actions observed`
            : "") +
          ". The GitHub star total above is the headline figure.",
      );
    }

    const milestones = starMilestones(facts.history);
    if (milestones.length > 0) {
      sections.push(
        `## Milestones\n\n${table(
          ["Threshold", "First reached"],
          milestones.map((milestone) => [
            formatCompact(milestone.threshold),
            formatMonthYear(milestone.date) ?? milestone.date,
          ]),
        )}`,
      );
    }
  }

  const health = page.health;
  if (health?.ready) {
    sections.push(
      `## Repository health\n\n${table(
        ["Reading", "Question", "Verdict", "Evidence"],
        healthReadings(health).map((reading) => [
          reading.label,
          reading.question,
          reading.verdict,
          reading.detail,
        ]),
      )}`,
    );
    sections.push(
      `${table(
        ["Fact", "Value", "Detail"],
        healthFacts(health).map((fact) => [
          fact.label,
          `\`${fact.value}\``,
          fact.detail,
        ]),
      )}`,
    );
    if (health.analysis_truncated) {
      sections.push(
        "Repository-health figures describe a bounded analysis window rather " +
          "than the full commit history. Say so if you quote them.",
      );
    }
  } else {
    sections.push(
      "## Repository health\n\n" +
        "No completed analysis backs this repository yet. Analysis is queued on " +
        `first request and lands asynchronously; \`${apiBase}/api/repos/${slug}/health.json\` ` +
        "answers `202 {\"ready\": false}` until then, and `200` with the summary " +
        "afterwards.",
    );
  }

  // --- Embedding ------------------------------------------------------------
  sections.push(
    "## Put this in a README\n\n" +
      `Every asset below is a plain public image URL for \`${slug}\`. No account, ` +
      "token, build step, or GitHub Action is involved. Paste a snippet as-is: " +
      "it already carries light and dark variants, alt text, and the link back " +
      "to the report.",
  );

  for (const id of ["badge-metrics", "chart", "card"]) {
    const asset = byId.get(id);
    if (asset) sections.push(assetSection(apiBase, asset, link, "###"));
  }

  sections.push(
    `### Repository-health charts\n\n` +
      "Same `<picture>` shape, different path. Add at most two, in a Project " +
      "health or Contributing section.\n\n" +
      table(
        ["Chart", "URL", "Shows"],
        assets
          .filter((asset) => asset.group === "health")
          .map((asset) => [
            asset.name,
            `\`${apiBase}${asset.path}\``,
            asset.purpose,
          ]),
      ),
  );

  sections.push(
    "### Earned signal badges\n\n" +
      `Read \`${apiBase}/api/repos/${slug}/earned-badges.json\` first: it returns one ` +
      "entry per signal with an `earned` boolean. Publish only earned signals — " +
      "an unearned one renders greyed out and claims nothing.\n\n" +
      `URL shape: \`${apiBase}/api/repos/${slug}/badge.svg?signal=SIGNAL&theme=dark\` ` +
      "where `SIGNAL` is `active`, `community`, `momentum`, or `contributor-ready`.",
  );

  sections.push(rulesSection());
  sections.push(
    `The complete asset catalog, with every snippet, is at ${absolute(site, "/badges.md")}.`,
  );

  // --- Data surfaces --------------------------------------------------------
  sections.push(
    `## Live data\n\n${bullet([
      `Repository on GitHub: https://github.com/${slug}`,
      `Star history JSON: ${apiBase}/api/repos/${slug}/stars.json`,
      `Star history CSV: ${apiBase}/api/repos/${slug}/stars.csv`,
      `Repository-health summary: ${apiBase}/api/repos/${slug}/health.json`,
      `Repository-health detail: ${apiBase}/api/repos/${slug}/stats.json`,
      `Earned badges: ${apiBase}/api/repos/${slug}/earned-badges.json`,
      `Queue and ETA snapshot: ${apiBase}/api/repos/${slug}/progress.json`,
      `Queue and ETA stream (SSE): ${apiBase}/api/repos/${slug}/progress`,
    ])}`,
  );

  if (page.updatedAt) {
    sections.push(`Catalog snapshot: ${page.updatedAt}`);
  }

  return `${sections.join("\n\n")}\n`;
}

function profileMarkdown(
  page: Extract<AgentPage, { kind: "profile" }>,
  site: string,
  apiBase: string,
): string {
  const { login } = page;
  const canonical = absolute(site, `/${login}`);
  const link = readmeLink(site, `/${login}`);
  const assets = profileEmbedAssets(login);

  const sections: string[] = [
    documentHeader(`${login} — public GitHub profile statistics`, canonical),
    `> Aggregate statistics across public repositories owned by ${login}. ` +
      "Private repositories are ignored.",
  ];

  const rows: string[][] = [];
  if (page.totalStars !== null) {
    rows.push(["Stars across public repositories", page.totalStars.toLocaleString()]);
  }
  if (page.reposIncluded !== null) {
    rows.push(["Repositories counted", page.reposIncluded.toLocaleString()]);
  }
  if (page.firstYear !== null) {
    rows.push(["Active since", String(page.firstYear)]);
  }
  if (rows.length > 0) {
    sections.push(`## Snapshot\n\n${table(["Metric", "Value"], rows)}`);
  }

  sections.push(
    "## Put this in a profile README\n\n" +
      "A profile README lives in a repository named after the account itself — " +
      `\`${login}/${login}\` for a user, \`.github/profile/README.md\` for an ` +
      "organization.",
  );

  for (const asset of assets.filter((entry) => entry.group === "headline")) {
    sections.push(assetSection(apiBase, asset, link, "###"));
  }

  sections.push(
    `### Footprint charts\n\n${table(
      ["Chart", "URL", "Shows"],
      assets
        .filter((asset) => asset.group === "health")
        .map((asset) => [asset.name, `\`${apiBase}${asset.path}\``, asset.purpose]),
    )}`,
  );

  sections.push(rulesSection());

  sections.push(
    `## Live data\n\n${bullet([
      `GitHub profile: https://github.com/${login}`,
      `Aggregate analysis: ${apiBase}/api/users/${login}/analyze`,
      `Profile statistics JSON: ${apiBase}/api/users/${login}/stats.json`,
    ])}`,
  );

  return `${sections.join("\n\n")}\n`;
}

/** One line of star evidence for a repository inside a comparison. */
function comparisonRow(slug: string, facts: RepoFacts | null): string[] {
  const stars = facts?.stars;
  return [
    `\`${slug}\``,
    stars?.totalStars !== null && stars?.totalStars !== undefined
      ? stars.totalStars.toLocaleString()
      : "—",
    stars?.gained90 !== null && stars?.gained90 !== undefined
      ? `+${stars.gained90.toLocaleString()}`
      : "—",
    stars?.gained30 !== null && stars?.gained30 !== undefined
      ? `+${stars.gained30.toLocaleString()}`
      : "—",
    stars?.trend ?? "—",
    stars?.firstStarMonth ?? "—",
  ];
}

function comparisonMarkdown(
  page: Extract<AgentPage, { kind: "comparison" }>,
  site: string,
  apiBase: string,
): string {
  const path = `/vs/${page.first}/${page.second}`;
  const canonical = absolute(site, path);
  const repos = `${page.first},${page.second}`;
  const overlay = `${apiBase}/api/chart.svg?repos=${encodeURIComponent(repos)}`;
  const link = readmeLink(site, path);

  const sections: string[] = [
    documentHeader(`${page.first} versus ${page.second}`, canonical),
    "> Star history, growth, and repository-health signals for two public " +
      "GitHub repositories, on one timeline.",
    `## Star comparison\n\n${table(
      ["Repository", "Stars", "90d", "30d", "Pace", "History from"],
      [
        comparisonRow(page.first, page.facts[page.first] ?? null),
        comparisonRow(page.second, page.facts[page.second] ?? null),
      ],
    )}`,
    `## Overlay chart\n\n` +
      "One chart, both series. Append `&rebase=1` to start each series at zero " +
      "when the projects are different ages, or `&from=`/`&to=` for a window.\n\n" +
      fence(
        "html",
        [
          `<a href="${link}">`,
          "  <picture>",
          `    <source media="(prefers-color-scheme: dark)" srcset="${overlay}&theme=dark" />`,
          `    <img alt="Star history of ${page.first} versus ${page.second}" src="${overlay}&theme=light" />`,
          "  </picture>",
          "</a>",
        ].join("\n"),
      ),
    `## Individual reports\n\n${bullet([
      `${page.first}: ${absolute(site, `/${page.first}`)} (Markdown: ${absolute(site, `/${page.first}.md`)})`,
      `${page.second}: ${absolute(site, `/${page.second}`)} (Markdown: ${absolute(site, `/${page.second}.md`)})`,
    ])}`,
    rulesSection(),
  ];

  return `${sections.join("\n\n")}\n`;
}

function categoryMarkdown(
  page: Extract<AgentPage, { kind: "category" }>,
  site: string,
  apiBase: string,
): string {
  const canonical = absolute(site, `/compare/${page.slug}`);
  const sections: string[] = [
    documentHeader(`${page.name} — GitHub repository comparison`, canonical),
    `> ${page.description}`,
  ];

  if (page.repos.length > 0) {
    sections.push(
      `## Repositories in this category\n\n${table(
        ["Repository", "Report", "Markdown"],
        page.repos.map((slug) => [
          `\`${slug}\``,
          absolute(site, `/${slug}`),
          absolute(site, `/${slug}.md`),
        ]),
      )}`,
    );
    sections.push(
      "## Overlay every repository on one chart\n\n" +
        fence(
          "markdown",
          `![${page.name} star history](${apiBase}/api/chart.svg?repos=${encodeURIComponent(
            page.repos.join(","),
          )}&rebase=1&theme=dark)`,
        ),
    );
  }

  sections.push(
    "Public repositories only. Open the canonical HTML page for the " +
      "interactive timeline and the per-repository health columns.",
  );

  return `${sections.join("\n\n")}\n`;
}

/**
 * `/badges.md` is the complete catalog: every asset, every snippet, in a form
 * an agent can act on without a second request. Per-repository files point
 * here rather than repeating it thousands of times.
 */
function badgeCatalogMarkdown(site: string, apiBase: string): string {
  const canonical = absolute(site, "/badges");
  const link = readmeLink(site, `/${PLACEHOLDER_SLUG}`);
  const repoAssets = repoEmbedAssets(PLACEHOLDER_SLUG);
  const profileAssets = profileEmbedAssets("LOGIN");

  const sections: string[] = [
    documentHeader("Everything gitdebt can embed in a README", canonical),
    "> Star-history charts, a metrics badge, evidence-backed signal badges, " +
      "repository and maintainer cards, eight repository-health charts, and a " +
      "social preview. Every asset is a plain public image URL.",
    `Replace \`${PLACEHOLDER_SLUG}\` with a lowercased \`owner/repo\` slug, and ` +
      "`LOGIN` with a GitHub account name. Nothing else needs to change: no " +
      "account, no token, no build step, no GitHub Action.",
    rulesSection(),
    "## Repository assets",
  ];

  for (const asset of repoAssets) {
    sections.push(assetSection(apiBase, asset, link, "###"));
  }

  sections.push("## Profile assets");
  for (const asset of profileAssets) {
    sections.push(
      assetSection(apiBase, asset, readmeLink(site, "/LOGIN"), "###"),
    );
  }

  sections.push(
    "## Multi-repository overlay\n\n" +
      "One chart, several series, for a comparison table or a docs page.\n\n" +
      fence(
        "markdown",
        `![Star history comparison](${apiBase}/api/chart.svg?repos=owner%2Frepo%2Cother%2Frepo&rebase=1&theme=dark)`,
      ),
  );

  sections.push(parameterSection());

  sections.push(
    "## Ready-made agent prompt\n\n" +
      "The `/badges` page and every repository report carry an *Ask an agent* " +
      "button that copies this prompt, filled in for the repository being " +
      "viewed. The generic form:",
  );
  sections.push(
    outerFence(
      "markdown",
      repoAgentPrompt({
        slug: PLACEHOLDER_SLUG,
        siteOrigin: site,
        apiBase,
      }).trimEnd(),
    ),
  );

  return `${sections.join("\n\n")}\n`;
}

function staticMarkdown(
  page: Extract<AgentPage, { kind: "static" }>,
  site: string,
  apiBase: string,
): string {
  if (page.path === "badges") return badgeCatalogMarkdown(site, apiBase);

  const canonical = absolute(site, `/${page.path}`);
  return [
    documentHeader(page.title, canonical),
    "",
    `> ${page.description}`,
    "",
    "gitdebt reports star history, growth, contributors, ownership " +
      "concentration, language activity, file change frequency, fix-labelled " +
      "changes, maintenance cadence, and README-ready media for public GitHub " +
      "repositories.",
    "",
    bullet([
      `Repository report: ${absolute(site, "/report")}`,
      `Repository leaderboard: ${absolute(site, "/leaderboard")}`,
      `Compare repositories: ${absolute(site, "/compare")}`,
      `README asset catalog: ${absolute(site, "/badges.md")}`,
      `API behaviour and methodology: ${absolute(site, "/about")}`,
      `Agent index: ${absolute(site, "/llms.txt")}`,
    ]),
    "",
  ].join("\n");
}

export function renderAgentMarkdown(
  page: AgentPage,
  site: string,
  apiBase: string,
): string {
  switch (page.kind) {
    case "repo":
      return repoMarkdown(page, site, apiBase);
    case "profile":
      return profileMarkdown(page, site, apiBase);
    case "comparison":
      return comparisonMarkdown(page, site, apiBase);
    case "category":
      return categoryMarkdown(page, site, apiBase);
    default:
      return staticMarkdown(page, site, apiBase);
  }
}

export function markdownResponse(body: string, canonical: string): Response {
  return new Response(body, {
    headers: {
      "Cache-Control": "public, max-age=3600, s-maxage=86400",
      "Content-Type": "text/markdown; charset=utf-8",
      Link: `<${canonical}>; rel=\"canonical\"`,
      "X-Robots-Tag": "noindex, follow",
    },
  });
}
