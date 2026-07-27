import assert from "node:assert/strict";
import { test } from "node:test";

import {
  commitMonthPoints,
  healthFacts,
  healthReadings,
} from "../src/lib/repo-health.ts";

/** A healthy, actively maintained repository. */
const base = {
  ready: true,
  repo: "acme/widget",
  stars: 12_400,
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
  commit_months: [
    { month: "2026-06", commits: 120 },
    { month: "2026-07", commits: 140 },
  ],
  tracked_files: 1_000,
  file_changes: 5_000,
  fix_changes: 600,
  fresh_files: 620,
  hotspot: { path: "src/core/render.ts", commits: 412, fix_commits: 88 },
  todo_delta_window: 0,
  todo_outstanding: 140,
};

const reading = (health, key) =>
  healthReadings(health).find((entry) => entry.key === key);

test("maintenance compares the two trailing windows", () => {
  assert.equal(reading(base, "maintenance").verdict, "Steady");
  assert.equal(
    reading({ ...base, commits_window: 600 }, "maintenance").verdict,
    "Speeding up",
  );
  assert.equal(
    reading({ ...base, commits_window: 200 }, "maintenance").verdict,
    "Slowing down",
  );
  assert.equal(
    reading({ ...base, commits_previous_window: 0 }, "maintenance").verdict,
    "Newly active",
  );
});

test("a quiet window is dormant, and says when the last commit landed", () => {
  const quiet = reading(
    { ...base, commits_window: 0, commits_previous_window: 0 },
    "maintenance",
  );
  assert.equal(quiet.verdict, "Dormant");
  assert.equal(quiet.tone, "risk");
  assert.equal(quiet.ratio, 0);
  assert.match(quiet.detail, /last one Jul 2026/);

  const stalled = reading({ ...base, commits_window: 0 }, "maintenance");
  assert.equal(stalled.verdict, "Went quiet");

  const never = reading(
    {
      ...base,
      commits_window: 0,
      commits_previous_window: 0,
      last_commit_day: null,
    },
    "maintenance",
  );
  assert.equal(never.detail, "No commits recorded in the last 90 days");
});

test("maintenance ratio is momentum, not volume", () => {
  assert.equal(reading(base, "maintenance").ratio, 0.5);
  assert.equal(
    reading({ ...base, commits_window: 1_200 }, "maintenance").ratio,
    0.75,
  );
  // A 12-commit project holding its pace reads the same as a 12k one.
  assert.equal(
    reading(
      { ...base, commits_window: 6, commits_previous_window: 6 },
      "maintenance",
    ).ratio,
    0.5,
  );
});

test("ownership buckets the bus factor and agrees with its own grammar", () => {
  assert.equal(reading(base, "ownership").verdict, "Shared");
  assert.equal(
    reading({ ...base, bus_factor: 1 }, "ownership").verdict,
    "One person carries it",
  );
  assert.equal(reading({ ...base, bus_factor: 1 }, "ownership").tone, "risk");
  assert.equal(reading({ ...base, bus_factor: 3 }, "ownership").verdict, "A few hands");
  assert.equal(
    reading({ ...base, bus_factor: 24 }, "ownership").verdict,
    "Broadly shared",
  );
  assert.equal(reading({ ...base, bus_factor: 24 }, "ownership").ratio, 1);

  assert.equal(
    reading({ ...base, bus_factor: 1, contributors: 240 }, "ownership").detail,
    "1 of 240 contributors writes half the commits",
  );
  assert.equal(
    reading({ ...base, bus_factor: 1, contributors: 1 }, "ownership").detail,
    "1 of 1 contributor writes half the commits",
  );
  assert.equal(
    reading(base, "ownership").detail,
    "6 of 240 contributors write half the commits",
  );
});

test("an unattributed repository says so instead of scoring zero", () => {
  const unattributed = reading(
    { ...base, bus_factor: 0, contributors: 0 },
    "ownership",
  );
  assert.equal(unattributed.verdict, "Not attributed");
  assert.equal(unattributed.tone, "steady");
  assert.equal(unattributed.ratio, 0);
});

test("repair load is the fix-labelled share of file changes", () => {
  assert.equal(reading(base, "repair").verdict, "Balanced");
  assert.equal(reading(base, "repair").detail, "12% of file changes came from fix-labelled commits");
  assert.equal(reading({ ...base, fix_changes: 100 }, "repair").verdict, "Mostly building");
  assert.equal(reading({ ...base, fix_changes: 1_500 }, "repair").verdict, "Repair-heavy");
  assert.equal(
    reading({ ...base, fix_changes: 2_500 }, "repair").verdict,
    "Mostly firefighting",
  );
  assert.equal(reading({ ...base, fix_changes: 2_500 }, "repair").ratio, 1);
  assert.equal(reading({ ...base, file_changes: 0 }, "repair").verdict, "Not measured");
  // A non-zero share must never round away to a flat "0%".
  assert.equal(
    reading({ ...base, fix_changes: 3 }, "repair").detail,
    "<1% of file changes came from fix-labelled commits",
  );
});

test("debt markers read direction first, then magnitude", () => {
  assert.equal(reading(base, "debt").verdict, "Flat");
  assert.equal(reading({ ...base, todo_delta_window: -12 }, "debt").verdict, "Shrinking");
  assert.equal(reading({ ...base, todo_delta_window: -12 }, "debt").tone, "good");
  assert.equal(reading({ ...base, todo_delta_window: 12 }, "debt").verdict, "Growing");
  assert.equal(
    reading({ ...base, todo_delta_window: 40 }, "debt").verdict,
    "Growing fast",
  );
  assert.equal(reading({ ...base, todo_delta_window: 40 }, "debt").tone, "risk");
  assert.equal(
    reading({ ...base, todo_delta_window: 12 }, "debt").detail,
    "+12 in 90 days · 140 TODO/FIXME outstanding",
  );
  assert.equal(
    reading({ ...base, todo_delta_window: 0, todo_outstanding: 0 }, "debt").verdict,
    "None tracked",
  );
});

test("debt movement with nothing left outstanding fills the meter", () => {
  const cleared = reading(
    { ...base, todo_delta_window: -89, todo_outstanding: 0 },
    "debt",
  );
  assert.equal(cleared.verdict, "Shrinking");
  assert.equal(cleared.ratio, 1, "a zero bar would read as the opposite");
  assert.equal(
    reading({ ...base, todo_delta_window: 35 }, "debt").ratio,
    0.25,
  );
});

test("every reading answers a question and stays inside the meter", () => {
  for (const entry of healthReadings(base)) {
    assert.ok(entry.question.endsWith("?"), `${entry.key} states its question`);
    assert.ok(entry.verdict.length > 0);
    assert.ok(entry.detail.length > 0);
    assert.ok(entry.ratio >= 0 && entry.ratio <= 1, `${entry.key} ratio in range`);
  }
});

test("facts cover the hotspot, freshness and analysis scope", () => {
  const facts = healthFacts(base);
  assert.deepEqual(
    facts.map((fact) => fact.key),
    ["hotspot", "freshness", "commits"],
  );
  assert.equal(facts[0].value, "src/core/render.ts");
  assert.equal(facts[0].detail, "412 changes · 88 fix-labelled");
  assert.equal(facts[1].value, "62%");
  assert.equal(facts[2].detail, "full commit history");
  assert.equal(
    healthFacts({ ...base, analysis_truncated: true })[2].detail,
    "bounded analysis window",
  );
});

test("facts drop what was not measured rather than printing zeroes", () => {
  const facts = healthFacts({ ...base, hotspot: null, tracked_files: 0 });
  assert.deepEqual(
    facts.map((fact) => fact.key),
    ["commits"],
  );
});

test("commit months become plottable points on the first of the month", () => {
  assert.deepEqual(commitMonthPoints(base), [
    { date: "2026-06-01", value: 120 },
    { date: "2026-07-01", value: 140 },
  ]);
});
