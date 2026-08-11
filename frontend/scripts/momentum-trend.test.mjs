import assert from "node:assert/strict";
import { test } from "node:test";

import { rates, trendOf } from "../src/lib/momentum.ts";

const row = (d1, d7, d30, stars = 1000) => ({ repo: "o/r", stars, d1, d7, d30 });

test("windows only become comparable once divided into per-day rates", () => {
  // 100/day, 70/7d = 10/day, 300/30d = 10/day. Raw numbers would rank the
  // monthly total highest; rates show the day is ten times the month's pace.
  const r = rates(row(100, 70, 300));
  assert.equal(r.r1, 100);
  assert.equal(r.r7, 10);
  assert.equal(r.r30, 10);
});

test("a repo running hotter today than its month is climbing", () => {
  assert.equal(trendOf(row(100, 350, 300)), "rising");
});

test("a repo running colder today than its month is cooling", () => {
  // 5/day today against 10/day over the month.
  assert.equal(trendOf(row(5, 35, 300)), "fading");
});

test("a repo holding its pace is steady", () => {
  assert.equal(trendOf(row(10, 70, 300)), "steady");
});

/**
 * The regression this file exists for. Requiring d1 AND d30 left 93 of 99 rows
 * unknown, because the API returns a top-N per window and most repositories
 * appear in only one. Any short-versus-long pair answers the same question.
 */
test("direction is read from whichever pair of windows the repo actually has", () => {
  // Weekly and monthly only: 50/7 ≈ 7.1/day against 60/30 = 2/day.
  assert.equal(trendOf(row(null, 50, 60)), "rising");
  // Daily and weekly only: 1/day against 70/7 = 10/day.
  assert.equal(trendOf(row(1, 70, null)), "fading");
  // Daily and monthly, no weekly.
  assert.equal(trendOf(row(40, null, 300)), "rising");
});

test("one window alone is not a direction, and never guesses", () => {
  for (const only of [row(90, null, null), row(null, 90, null), row(null, null, 90)]) {
    assert.equal(trendOf(only), "unknown");
  }
  assert.equal(trendOf(row(null, null, null)), "unknown");
});

test("a zero-pace long window cannot produce a ratio", () => {
  // Dividing by it would be Infinity, which would render as "rising" on a repo
  // that has no monthly history at all.
  assert.equal(trendOf(row(10, 70, 0)), "unknown");
});

test("realistic board shape leaves few rows uncomparable", () => {
  // Repos ranked in one window commonly appear in an adjacent one; only the
  // genuinely single-window rows should come back unknown.
  const board = [
    row(120, 800, 3000),
    row(null, 420, 1500),
    row(60, 300, null),
    row(null, null, 900), // single window: honestly unknown
  ];
  const unknown = board.filter((r) => trendOf(r) === "unknown").length;
  assert.equal(unknown, 1, "only the single-window row is uncomparable");
});
