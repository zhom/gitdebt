import assert from "node:assert/strict";
import { test } from "node:test";

import {
  buildComparisonMetrics,
  contributorConcentration,
  dominantLanguage,
  hotFileFixShare,
  trailingActivity,
} from "../src/lib/comparison-insights.ts";

const stats = {
  ready: true,
  total_commits: 1_000,
  analyzed_commits: 100,
  attributed_commits: 100,
  analysis_scope_commits: 1_000,
  analysis_truncated: false,
  bus_factor: 2,
  files: [
    { path: "a.ts", commits: 20, fix_commits: 5 },
    { path: "b.rs", commits: 30, fix_commits: 5 },
  ],
  authors: [{ commits: 60 }, { commits: 40 }],
  commit_days: [
    { date: "2026-01-01", value: 4 },
    { date: "2026-03-30", value: 3 },
    { date: "2026-03-31", value: 0 },
  ],
  todo_days: [
    { date: "2026-01-01", value: 5 },
    { date: "2026-03-31", value: 8 },
  ],
  languages: [
    { language: "Rust", files: 5, code: 600, blank: 30, comment: 50 },
    { language: "TypeScript", files: 7, code: 400, blank: 20, comment: 40 },
  ],
};

test("dominant language uses code, not file count", () => {
  assert.deepEqual(dominantLanguage(stats.languages), {
    language: "Rust",
    code: 600,
    share: 0.6,
  });
});

test("trailing activity uses the newest recorded date instead of wall clock", () => {
  assert.deepEqual(trailingActivity(stats.commit_days, 90), {
    commits: 7,
    activeDays: 2,
    previousCommits: 0,
  });
});

test("ownership and hot-file signals preserve their documented denominators", () => {
  assert.equal(contributorConcentration(stats), 0.6);
  assert.equal(hotFileFixShare(stats), 0.2);
});

test("comparison metrics expose unavailable states instead of invented zeroes", () => {
  const metrics = buildComparisonMetrics([
    {
      slug: "a/ready",
      totalStars: 100,
      createdAt: "2020-01-01T00:00:00Z",
      history: [
        { date: "2026-01-01", stars: 90 },
        { date: "2026-03-31", stars: 100 },
      ],
      stats,
      usage: {
        forks: 12,
        downloads: {
          npm: { total: 500 },
          crates: null,
          pypi: null,
          docker: null,
        },
      },
    },
    {
      slug: "b/pending",
      totalStars: 20,
      createdAt: null,
      history: [],
      stats: null,
      usage: null,
      analysisPending: true,
    },
  ]);
  assert.equal(
    metrics.find((metric) => metric.label === "Dominant language").values[1]
      .display,
    "Analysis running",
  );
  assert.equal(
    metrics.find((metric) => metric.label === "Forks").values[1].display,
    "Not available",
  );
  assert.equal(
    metrics.find((metric) => metric.label === "Largest package audience")
      .values[0].note,
    "npm",
  );
});
