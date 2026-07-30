/**
 * The prompt behind "Ask an agent to add this to my repo".
 *
 * A coding agent lands in somebody's checkout with no idea what gitdebt is,
 * which URLs are real, or where a star-history chart belongs. This module
 * writes it all down once: the measured numbers (so the agent never invents a
 * statistic), the exact paste-ready snippets, the rules that make a published
 * embed correct, and the places to look beyond `README.md`.
 *
 * Pure and deterministic — the same repository and the same snapshot always
 * produce the same prompt, so what a visitor copies is what the `.md`
 * representation documents.
 *
 * Relative specifiers with their extension: the Node test runner in `scripts/`
 * resolves neither the `@/` alias nor an extensionless specifier.
 */

import type { RepoHealth } from "./repo-health.ts";
import { healthReadings } from "./repo-health.ts";
import {
  CANDIDATE_FILES,
  EMBED_RULES,
  EXISTING_STAR_HISTORY_MARKERS,
  QUERY_REFERENCE,
  bestEmbed,
  bestEmbedLanguage,
  profileEmbedAssets,
  readmeLink,
  repoEmbedAssets,
  type EmbedAsset,
} from "./readme-embeds.ts";
import {
  formatCompact,
  formatMonthYear,
  gainedInTrailingDays,
  growthTrend,
  type HistoryPoint,
} from "./star-insights.ts";

/** The placeholder slug the repository-less prompt carries. */
export const PLACEHOLDER_SLUG = "OWNER/REPO";

export type StarFacts = {
  totalStars: number | null;
  gained30: number | null;
  gained90: number | null;
  trend: "accelerating" | "steady" | "slowing" | null;
  /** First point of the cached series, already formatted ("Mar 2013"). */
  firstStarMonth: string | null;
  /**
   * True when the curve is GH Archive star *activity* rather than a stargazer
   * snapshot. The distinction has to survive into the prompt: an agent that
   * writes "net stars" about an activity series is publishing a wrong claim.
   */
  approximate: boolean;
};

export type RepoPromptInput = {
  slug: string;
  siteOrigin: string;
  apiBase: string;
  stars?: StarFacts | null;
  health?: RepoHealth | null;
};

export type ProfilePromptInput = {
  login: string;
  siteOrigin: string;
  apiBase: string;
  totalStars?: number | null;
  reposIncluded?: number | null;
};

/**
 * Star facts derived from a cumulative history series. Windows are anchored on
 * the series' own last point rather than the wall clock, which is what keeps a
 * prerendered prompt identical to the one a visitor copies from the live page.
 */
export function starFacts(
  history: HistoryPoint[],
  totalStars: number | null,
  approximate: boolean,
): StarFacts {
  return {
    totalStars,
    gained30: gainedInTrailingDays(history, 30),
    gained90: gainedInTrailingDays(history, 90),
    trend: growthTrend(history),
    firstStarMonth: formatMonthYear(history[0]?.date ?? null),
    approximate,
  };
}

function bullet(lines: string[]): string {
  return lines.map((line) => `- ${line}`).join("\n");
}

function numbered(lines: string[]): string {
  return lines.map((line, index) => `${index + 1}. ${line}`).join("\n");
}

function fence(language: string, body: string): string {
  return `\`\`\`${language}\n${body}\n\`\`\``;
}

/** One asset as a headed, fenced, paste-ready block. */
function snippetBlock(
  apiBase: string,
  asset: EmbedAsset,
  link: string,
  heading: string,
): string {
  return `${heading}\n\n${fence(bestEmbedLanguage(asset), bestEmbed(apiBase, asset, link))}`;
}

/** The measured-facts block. Empty when nothing has been measured yet. */
function repoEvidence(input: RepoPromptInput): string[] {
  const facts: string[] = [];
  const stars = input.stars;

  if (stars?.totalStars !== null && stars?.totalStars !== undefined) {
    const window: string[] = [];
    if (stars.gained90 !== null) {
      window.push(`+${stars.gained90.toLocaleString()} in 90 days`);
    }
    if (stars.gained30 !== null) {
      window.push(`+${stars.gained30.toLocaleString()} in 30`);
    }
    const pace =
      stars.trend === "accelerating"
        ? ", running ahead of its lifetime pace"
        : stars.trend === "slowing"
          ? ", below its lifetime pace"
          : stars.trend === "steady"
            ? ", in line with its lifetime pace"
            : "";
    facts.push(
      `${stars.totalStars.toLocaleString()} GitHub stars${
        window.length > 0 ? ` (${window.join(", ")})` : ""
      }${pace}.`,
    );
    if (stars.approximate) {
      facts.push(
        "The star curve is public GH Archive star activity, not a net-star " +
          "series: it records star actions and cannot see unstars. Describe it " +
          "as star activity, never as net stars.",
      );
    }
  }
  if (stars?.firstStarMonth) {
    facts.push(`Star history begins ${stars.firstStarMonth}.`);
  }

  const health = input.health;
  if (health?.ready) {
    for (const reading of healthReadings(health)) {
      facts.push(`${reading.label}: ${reading.verdict} — ${reading.detail}.`);
    }
    if (health.hotspot) {
      facts.push(
        `Change hotspot: ${health.hotspot.path} (${formatCompact(health.hotspot.commits)} changes, ` +
          `${formatCompact(health.hotspot.fix_commits)} fix-labelled).`,
      );
    }
    if (health.analysis_truncated) {
      facts.push(
        "Repository-health figures describe a bounded analysis window, not the " +
          "full history. Say so if you quote them in prose.",
      );
    }
  }

  return facts;
}

/**
 * The prompt for one repository.
 *
 * `slug` may be `PLACEHOLDER_SLUG`, in which case the prompt opens by telling
 * the agent to resolve the real slug from the checkout's git remote.
 */
export function repoAgentPrompt(input: RepoPromptInput): string {
  const { slug, apiBase } = input;
  const generic = slug === PLACEHOLDER_SLUG;
  const siteOrigin = input.siteOrigin.replace(/\/+$/, "");
  const report = `${siteOrigin}/${slug}`;
  const link = readmeLink(siteOrigin, `/${slug}`);
  const assets = repoEmbedAssets(slug);
  const byId = new Map(assets.map((asset) => [asset.id, asset]));
  const chart = byId.get("chart")!;
  const badge = byId.get("badge-metrics")!;
  const card = byId.get("card")!;
  const health = assets.filter((asset) => asset.group === "health");
  const evidence = repoEvidence(input);

  const sections: string[] = [];

  sections.push(
    `# Add gitdebt analytics to the ${generic ? "project's" : `${slug}`} README`,
  );

  sections.push(
    `gitdebt (${siteOrigin}) turns public GitHub data into plain image URLs: ` +
      "star history, a metrics badge, and repository-health charts. No account, " +
      "token, build step, or GitHub Action is involved — the URLs below are " +
      "already live and already pointed at this project.",
  );

  if (generic) {
    sections.push(
      "## Step 0 — resolve the repository\n\n" +
        "Run `git remote get-url origin` and take the `owner/repo` slug from it. " +
        `Replace every \`${PLACEHOLDER_SLUG}\` below with that slug, lowercased. ` +
        "If the remote is not a public GitHub repository, stop and say so: " +
        "gitdebt only serves public repositories.",
    );
  }

  if (evidence.length > 0) {
    sections.push(
      `## What gitdebt has measured\n\n${bullet(evidence)}\n\n` +
        `Use these numbers if you write prose around the images. Do not invent ` +
        `others. Every figure is re-checkable at ` +
        `${apiBase}/api/repos/${slug}/health.json and ` +
        `${apiBase}/api/repos/${slug}/stars.json.`,
    );
  } else {
    sections.push(
      "## Numbers\n\n" +
        "Do not write statistics into the README by hand — they go stale. The " +
        "images below are regenerated from live data. If you need a figure for " +
        `prose, read it from ${apiBase}/api/repos/${slug}/health.json.`,
    );
  }

  sections.push(
    "## What to add\n\n" +
      "Paste these snippets as-is. They are complete, and they already carry " +
      "light and dark variants plus alt text.",
  );

  sections.push(
    snippetBlock(
      apiBase,
      badge,
      link,
      `### 1. Metrics badge — ${badge.placement}`,
    ),
  );

  sections.push(
    snippetBlock(
      apiBase,
      chart,
      link,
      `### 2. Star history — ${chart.placement}`,
    ) +
      "\n\nGive it a `## Star history` heading of its own if the README does not " +
      "already have one.",
  );

  sections.push(
    snippetBlock(
      apiBase,
      card,
      link,
      `### 3. Repository card (optional) — ${card.placement}`,
    ),
  );

  sections.push(
    "### 4. Repository-health charts (optional)\n\n" +
      "Each of these is the same `<picture>` shape as above, with a different " +
      "path. Add at most two, and only where a reader would want them — " +
      "typically a Project health or Contributing section. More than that reads " +
      "as clutter and slows the page down.\n\n" +
      bullet(
        health.map(
          (asset) => `\`${apiBase}${asset.path}\` — ${asset.name}: ${asset.purpose}`,
        ),
      ),
  );

  sections.push(
    "### 5. Earned signal badge (optional)\n\n" +
      `Fetch \`${apiBase}/api/repos/${slug}/earned-badges.json\` first. It returns ` +
      "one entry per signal with an `earned` boolean. Publish only the signals " +
      "where `earned` is `true` — an unearned signal renders greyed out and " +
      "claims nothing.\n\n" +
      `Badge URL shape: \`${apiBase}/api/repos/${slug}/badge.svg?signal=SIGNAL&theme=dark\`, ` +
      "where `SIGNAL` is `active`, `community`, `momentum`, or `contributor-ready`.",
  );

  sections.push(`## Rules\n\n${bullet(EMBED_RULES)}`);

  sections.push(
    "## If the project already shows a star-history chart\n\n" +
      "Replace it in place. Keep the surrounding heading and prose; swap only " +
      "the image and the link it wraps. Do not stack a second chart underneath. " +
      "Search the repository for these markers:\n\n" +
      bullet(EXISTING_STAR_HISTORY_MARKERS.map((marker) => `\`${marker}\``)),
  );

  sections.push(
    `## Where else to look\n\n${bullet(CANDIDATE_FILES)}\n\n` +
      "Only touch a file where the addition genuinely belongs. An unrelated " +
      "docs page does not need a commit calendar.",
  );

  sections.push(
    "## Tuning\n\n" +
      "Query parameters, if the defaults do not fit:\n\n" +
      bullet(
        QUERY_REFERENCE.map(
          (entry) => `\`${entry.param}\` (${entry.applies}) — ${entry.effect}`,
        ),
      ),
  );

  sections.push(
    "## Finish\n\n" +
      numbered([
        "Request each URL you added and confirm it answers 200 with an image content type.",
        "Confirm every image is wrapped in the link with `?ref=readme` and carries alt text.",
        "Confirm you changed nothing else: no reformatting, no reflowed prose, no reordered badges beyond the one you inserted.",
        `Report what you added and where, and link the full report: ${report}`,
      ]),
  );

  return `${sections.join("\n\n")}\n`;
}

/** The prompt for a maintainer or organization profile README. */
export function profileAgentPrompt(input: ProfilePromptInput): string {
  const { login, apiBase } = input;
  const siteOrigin = input.siteOrigin.replace(/\/+$/, "");
  const link = readmeLink(siteOrigin, `/${login}`);
  const assets = profileEmbedAssets(login);
  const card = assets.find((asset) => asset.id === "card")!;
  const chart = assets.find((asset) => asset.id === "chart")!;
  const rest = assets.filter((asset) => asset.group === "health");

  const evidence: string[] = [];
  if (input.totalStars !== null && input.totalStars !== undefined) {
    evidence.push(
      `${input.totalStars.toLocaleString()} stars across ${login}'s public repositories` +
        (input.reposIncluded ? ` (${input.reposIncluded.toLocaleString()} repositories counted)` : "") +
        ".",
    );
  }

  const sections: string[] = [
    `# Add gitdebt profile analytics to ${login}'s profile README`,
    `gitdebt (${siteOrigin}) renders aggregate public-repository statistics for ` +
      "an account as plain image URLs. No account, token, or GitHub Action is " +
      "involved. A profile README lives in a repository named after the account " +
      `itself — \`${login}/${login}\` for a user, \`.github/profile/README.md\` for ` +
      "an organization. Create it if it does not exist.",
  ];

  if (evidence.length > 0) {
    sections.push(
      `## What gitdebt has measured\n\n${bullet(evidence)}\n\n` +
        `Re-checkable at ${apiBase}/api/users/${login}/stats.json.`,
    );
  }

  sections.push(
    "## What to add\n\nPaste these as-is; both carry light and dark variants.",
    snippetBlock(apiBase, card, link, `### 1. Maintainer card — ${card.placement}`),
    snippetBlock(apiBase, chart, link, `### 2. Aggregate star history — ${chart.placement}`),
    "### 3. Optional footprint charts\n\n" +
      bullet(
        rest.map(
          (asset) => `\`${apiBase}${asset.path}\` — ${asset.name}: ${asset.purpose}`,
        ),
      ),
    `## Rules\n\n${bullet(EMBED_RULES)}`,
    "## Finish\n\n" +
      numbered([
        "Request each URL and confirm it answers 200 with an image content type.",
        "Confirm every image keeps its link wrapper and alt text.",
        `Report what you added, and link the full report: ${siteOrigin}/${login}`,
      ]),
  );

  return `${sections.join("\n\n")}\n`;
}
