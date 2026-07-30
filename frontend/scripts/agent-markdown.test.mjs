import assert from "node:assert/strict";
import { test } from "node:test";

import { renderAgentMarkdown } from "../src/lib/agent-markdown.ts";
import { starFacts } from "../src/lib/agent-prompt.ts";

const API = "https://api.gitdebt.com";
const SITE = "https://gitdebt.com/";
const SLUG = "acme/widget";

const history = (() => {
  const points = [];
  const start = Date.UTC(2024, 0, 1);
  for (let day = 0; day <= 730; day += 1) {
    points.push({
      date: new Date(start + day * 86_400_000).toISOString().slice(0, 10),
      stars: day * 10,
    });
  }
  return points;
})();

const health = {
  ready: true,
  repo: SLUG,
  stars: 7_300,
  archived: false,
  analyzed_at: "2026-07-20T00:00:00Z",
  window_days: 90,
  total_commits: 8_400,
  attributed_commits: 8_000,
  analysis_truncated: false,
  bus_factor: 6,
  contributors: 240,
  top_author_commits: 900,
  commits_window: 400,
  commits_previous_window: 400,
  last_commit_day: "2026-07-19",
  commit_months: [{ month: "2026-07", commits: 140 }],
  tracked_files: 1_000,
  file_changes: 5_000,
  fix_changes: 600,
  fresh_files: 620,
  hotspot: { path: "src/core/render.ts", commits: 412, fix_commits: 88 },
  todo_delta_window: 0,
  todo_outstanding: 140,
};

const repoPage = (overrides = {}) => ({
  kind: "repo",
  slug: SLUG,
  updatedAt: "2026-07-29T00:00:00Z",
  notFound: false,
  facts: {
    stars: starFacts(history, 7_300, false),
    history,
    createdAt: "2023-11-02T00:00:00Z",
    coverageEnd: "2025-12-31",
    eventCount: null,
  },
  health,
  ...overrides,
});

const render = (page) => renderAgentMarkdown(page, SITE, API);

/** Every GFM table row in a document, split into cells. */
function tableRows(markdown) {
  return markdown
    .split("\n")
    .filter((line) => line.startsWith("|"))
    .map((line) => line.slice(1, -1).split(/(?<!\\)\|/));
}

test("a repository page leads with its canonical HTML", () => {
  const out = render(repoPage());
  assert.match(out, /^# acme\/widget — GitHub star history and repository health\n/);
  assert.ok(out.includes(`Canonical HTML: ${SITE}${SLUG}`));
});

test("a repository page carries measured figures, not adjectives", () => {
  const out = render(repoPage());
  assert.match(out, /\| GitHub stars \| 7,300 \|/);
  assert.match(out, /\| Stars gained, trailing 90d \| \+900 \|/);
  assert.match(out, /\| Stars gained, trailing 30d \| \+300 \|/);
  assert.match(out, /\| Pace against lifetime average \| steady \|/);
  assert.match(out, /\| Repository created \| Nov 2023 \|/);
});

test("milestones name the month a threshold was first crossed", () => {
  const out = render(repoPage());
  assert.match(out, /## Milestones/);
  assert.match(out, /\| 100 \| Jan 2024 \|/);
  assert.match(out, /\| 1k \| Apr 2024 \|/);
});

test("health readings arrive as verdict plus evidence", () => {
  const out = render(repoPage());
  assert.match(out, /## Repository health/);
  assert.match(out, /\| Ownership \| How many people could walk away\? \| Shared \|/);
  assert.match(out, /\| Change hotspot \| `src\/core\/render\.ts` \|/);
});

test("an unanalyzed repository says so instead of implying zero", () => {
  const out = render(repoPage({ health: null }));
  assert.match(out, /No completed analysis backs this repository yet/);
  assert.ok(!out.includes("| Ownership |"));
});

test("an archive-derived curve is labelled as star activity", () => {
  const out = render(
    repoPage({
      facts: {
        ...repoPage().facts,
        stars: starFacts(history, 7_300, true),
        eventCount: 8_812,
      },
    }),
  );
  assert.match(out, /\| Star actions, trailing 90d \| \+900 \|/);
  assert.match(out, /cannot see unstars/);
  assert.match(out, /8,812 star actions observed/);
});

test("a tombstoned repository documents the tombstone and stops", () => {
  const out = render(repoPage({ notFound: true, facts: null, health: null }));
  assert.match(out, /does not expose `acme\/widget` as a public repository/);
  assert.ok(!out.includes("## Put this in a README"));
});

test("a repository page ships paste-ready snippets for its own slug", () => {
  const out = render(repoPage());
  assert.match(out, /## Put this in a README/);
  assert.ok(out.includes(`${API}/api/repos/${SLUG}/chart.svg?theme=dark`));
  assert.ok(out.includes(`${SITE}${SLUG}?ref=readme`));
  assert.match(out, /prefers-color-scheme: dark/);
  assert.match(out, /earned-badges\.json/);
});

test("a repository page lists the machine-readable surfaces", () => {
  const out = render(repoPage());
  for (const path of [
    "stars.json",
    "stars.csv",
    "health.json",
    "stats.json",
    "progress.json",
  ]) {
    assert.ok(out.includes(`${API}/api/repos/${SLUG}/${path}`), `missing ${path}`);
  }
});

test("every table row has as many cells as its header", () => {
  for (const page of [
    repoPage(),
    { kind: "static", path: "badges", title: "Badges", description: "d" },
    {
      kind: "comparison",
      first: "acme/widget",
      second: "other/thing",
      facts: { "acme/widget": repoPage().facts, "other/thing": null },
    },
  ]) {
    let width = 0;
    for (const row of tableRows(render(page))) {
      const cells = row.length;
      if (row.every((cell) => cell.trim() === "---")) {
        assert.equal(cells, width, "separator width diverged from its header");
        continue;
      }
      width = cells;
    }
  }
});

test("a pipe inside a cell is escaped rather than opening a column", () => {
  const out = render({
    kind: "static",
    path: "badges",
    title: "Badges",
    description: "d",
  });
  assert.match(out, /`theme=light\\\|dark`/);
});

test("the badge catalog is the complete reference", () => {
  const out = render({
    kind: "static",
    path: "badges",
    title: "Badges",
    description: "d",
  });
  assert.match(out, /## Repository assets/);
  assert.match(out, /## Profile assets/);
  assert.match(out, /## Query parameters/);
  assert.match(out, /## Ready-made agent prompt/);
  assert.match(out, /OWNER\/REPO/);
});

test("the embedded prompt is fenced so its own fences survive", () => {
  const out = render({
    kind: "static",
    path: "badges",
    title: "Badges",
    description: "d",
  });
  const outer = out.split("````");
  assert.equal(outer.length, 3, "the prompt is not wrapped in exactly one long fence");
  assert.match(outer[1], /```html/);
});

test("an ordinary static page stays short and links onward", () => {
  const out = render({
    kind: "static",
    path: "about",
    title: "About gitdebt",
    description: "How gitdebt works.",
  });
  assert.match(out, /^# About gitdebt\n/);
  assert.ok(out.includes(`${SITE}badges.md`));
  assert.ok(out.includes(`${SITE}llms.txt`));
});

test("a profile page carries its aggregate figures and its own assets", () => {
  const out = render({
    kind: "profile",
    login: "octocat",
    totalStars: 4_200,
    reposIncluded: 31,
    firstYear: 2015,
  });
  assert.match(out, /\| Stars across public repositories \| 4,200 \|/);
  assert.match(out, /\| Active since \| 2015 \|/);
  assert.ok(out.includes(`${API}/api/users/octocat/card.svg?theme=dark`));
  assert.match(out, /\.github\/profile\/README\.md/);
});

test("a comparison page tabulates both repositories and overlays them", () => {
  const out = render({
    kind: "comparison",
    first: "acme/widget",
    second: "other/thing",
    facts: { "acme/widget": repoPage().facts, "other/thing": null },
  });
  assert.match(out, /\| `acme\/widget` \| 7,300 \| \+900 \| \+300 \| steady \|/);
  assert.match(out, /\| `other\/thing` \| — \| — \| — \| — \| — \|/);
  assert.ok(out.includes("/api/chart.svg?repos=acme%2Fwidget%2Cother%2Fthing"));
});

test("a category page lists its members and one overlay", () => {
  const out = render({
    kind: "category",
    slug: "frontend-frameworks",
    name: "Frontend frameworks",
    description: "React, Vue, Svelte.",
    repos: ["facebook/react", "vuejs/vue"],
  });
  assert.ok(out.includes(`${SITE}facebook/react.md`));
  assert.ok(out.includes("rebase=1"));
});
