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

const full = () =>
  repoAgentPrompt({
    slug: SLUG,
    siteOrigin: SITE,
    apiBase: API,
    stars: starFacts(history, 7_300, false),
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
  // Repository-health readings are deliberately not in this prompt; the API's
  // own report carries them. The prompt points the agent at health.json.
  assert.ok(!prompt.includes("Ownership:"));
  assert.match(prompt, /health\.json/);
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
  // This prompt is executed by a coding agent, so a wrong file location is a
  // wrong `mkdir -p`. An organization profile README lives at
  // `profile/README.md` inside a repository literally named `.github`; the path
  // `.github/profile/README.md` names no repository at all.
  assert.match(
    prompt,
    /a repository named `\.github` with the file at `profile\/README\.md`/,
  );
  assert.ok(!prompt.includes(".github/profile/README.md"));
  assert.match(prompt, /4,200 stars across octocat's public repositories/);
  assert.ok(prompt.includes(`${API}/api/users/octocat/card.svg?theme=dark`));
});
