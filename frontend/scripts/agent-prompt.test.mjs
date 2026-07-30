import assert from "node:assert/strict";
import { test } from "node:test";

import {
  PLACEHOLDER_SLUG,
  profileAgentPrompt,
  repoAgentPrompt,
  starFacts,
} from "../src/lib/agent-prompt.ts";

const API = "https://api.gitdebt.com";
const SITE = "https://gitdebt.com";
const SLUG = "acme/widget";

/** Two years of daily-ish points, so `growthTrend` has something to judge. */
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

const full = () =>
  repoAgentPrompt({
    slug: SLUG,
    siteOrigin: SITE,
    apiBase: API,
    stars: starFacts(history, 7_300, false),
    health,
  });

test("star facts anchor on the series, not the wall clock", () => {
  const facts = starFacts(history, 7_300, false);
  assert.equal(facts.totalStars, 7_300);
  assert.equal(facts.gained30, 300);
  assert.equal(facts.gained90, 900);
  assert.equal(facts.trend, "steady");
  assert.equal(facts.firstStarMonth, "Jan 2024");
  assert.equal(facts.approximate, false);
});

test("an empty history yields nulls rather than invented zeroes", () => {
  const facts = starFacts([], null, false);
  assert.equal(facts.totalStars, null);
  assert.equal(facts.gained30, null);
  assert.equal(facts.gained90, null);
  assert.equal(facts.trend, null);
  assert.equal(facts.firstStarMonth, null);
});

test("the prompt carries the measured numbers so the agent invents none", () => {
  const prompt = full();
  assert.match(prompt, /7,300 GitHub stars/);
  assert.match(prompt, /\+900 in 90 days/);
  assert.match(prompt, /\+300 in 30/);
  assert.match(prompt, /Ownership: Shared/);
  assert.match(prompt, /Change hotspot: src\/core\/render\.ts/);
});

test("without measurements the prompt says where to read them instead", () => {
  const prompt = repoAgentPrompt({ slug: SLUG, siteOrigin: SITE, apiBase: API });
  assert.ok(!prompt.includes("GitHub stars ("));
  assert.match(prompt, /health\.json/);
  assert.match(prompt, /Do not write statistics into the README by hand/);
});

test("an archive-derived curve is never described as net stars", () => {
  const prompt = repoAgentPrompt({
    slug: SLUG,
    siteOrigin: SITE,
    apiBase: API,
    stars: starFacts(history, 7_300, true),
  });
  assert.match(prompt, /cannot see unstars/);
  assert.match(prompt, /never as net stars/);
});

test("a truncated analysis window is disclosed", () => {
  const prompt = repoAgentPrompt({
    slug: SLUG,
    siteOrigin: SITE,
    apiBase: API,
    health: { ...health, analysis_truncated: true },
  });
  assert.match(prompt, /bounded analysis window/);
});

test("the prompt ships complete, paste-ready snippets", () => {
  const prompt = full();
  assert.match(prompt, /```html/);
  assert.match(prompt, /prefers-color-scheme: dark/);
  assert.ok(prompt.includes(`${API}/api/repos/${SLUG}/chart.svg?theme=dark`));
  assert.ok(prompt.includes(`${API}/api/repos/${SLUG}/badge.svg?metrics=stars,forks&theme=light`));
  assert.ok(prompt.includes(`${SITE}/${SLUG}?ref=readme`));
});

test("the prompt tells the agent to replace an existing star-history widget", () => {
  const prompt = full();
  assert.match(prompt, /star-history\.com/);
  assert.match(prompt, /starchart\.cc/);
  assert.match(prompt, /Replace it in place/);
  assert.match(prompt, /Do not stack a second chart underneath/);
});

test("signal badges are gated on the earned-badges endpoint", () => {
  const prompt = full();
  assert.match(prompt, /earned-badges\.json/);
  assert.match(prompt, /Publish only the signals where `earned` is `true`/);
});

test("the prompt closes with verification, not a hope", () => {
  const prompt = full();
  assert.match(prompt, /confirm it answers 200/);
  assert.match(prompt, /Confirm you changed nothing else/);
});

test("the repository-less prompt resolves a slug from the git remote", () => {
  const prompt = repoAgentPrompt({
    slug: PLACEHOLDER_SLUG,
    siteOrigin: SITE,
    apiBase: API,
  });
  assert.match(prompt, /git remote get-url origin/);
  assert.ok(prompt.includes(PLACEHOLDER_SLUG));
  assert.match(prompt, /only serves public repositories/);
});

test("a trailing slash on the site origin never doubles up", () => {
  const prompt = repoAgentPrompt({
    slug: SLUG,
    siteOrigin: `${SITE}/`,
    apiBase: API,
  });
  assert.ok(!prompt.includes(`${SITE}//`));
});

test("the profile prompt explains where a profile README lives", () => {
  const prompt = profileAgentPrompt({
    login: "octocat",
    siteOrigin: SITE,
    apiBase: API,
    totalStars: 4_200,
    reposIncluded: 31,
  });
  assert.match(prompt, /`octocat\/octocat` for a user/);
  assert.match(prompt, /\.github\/profile\/README\.md/);
  assert.match(prompt, /4,200 stars across octocat's public repositories/);
  assert.ok(prompt.includes(`${API}/api/users/octocat/card.svg?theme=dark`));
});
