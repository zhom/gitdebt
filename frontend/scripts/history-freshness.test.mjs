import assert from "node:assert/strict";
import { test } from "node:test";

import {
  formatThrough,
  historyFreshness,
  needsNotice,
  noticeText,
} from "../src/lib/history-freshness.ts";

const exact = (end) => ({
  history_complete: true,
  history_kind: "current_stargazers",
  history_approximate: false,
  history_status: "ready",
  history_coverage_end: end,
});

test("an exact series that stops after the restriction is frozen, not merely stale", () => {
  // zhom/donutbrowser: complete, exact, last star 2026-07-20 — the week GitHub
  // closed the endpoint. It cannot resolve itself, so it needs the notice.
  const f = historyFreshness(exact("2026-07-20T13:47:16Z"));
  assert.equal(f.state, "exact_frozen");
  assert.ok(needsNotice(f));
  assert.match(noticeText(f), /complete through July 20, 2026/);
});

test("an exact series that stopped BEFORE the restriction is not blamed on it", () => {
  // Stopping in 2024 has some older cause; claiming the July 2026 restriction
  // froze it would be a confident falsehood.
  const f = historyFreshness(exact("2024-03-02T00:00:00Z"));
  assert.equal(f.state, "exact_current");
  assert.equal(needsNotice(f), false);
  assert.equal(noticeText(f), null);
});

test("an archive-backed series is approximate but flowing, and carries no notice", () => {
  const f = historyFreshness({
    history_complete: true,
    history_kind: "public_star_actions",
    history_approximate: true,
    history_status: "ready",
    history_coverage_end: "2026-08-08T11:30:21Z",
  });
  assert.equal(f.state, "archive");
  assert.equal(needsNotice(f), false);
});

test("a restricted park is terminal and says so without mentioning a date", () => {
  const f = historyFreshness({
    history_complete: false,
    history_status: "restricted",
    history_coverage_end: null,
  });
  assert.equal(f.state, "restricted");
  assert.ok(needsNotice(f));
  assert.match(noticeText(f), /admins and collaborators/);
  assert.doesNotMatch(noticeText(f), /complete through/);
});

test("cold, missing, and pre-field payloads degrade to silence rather than a wrong claim", () => {
  for (const snapshot of [
    null,
    undefined,
    { not_found: true },
    { history_complete: false, history_status: "queued" },
    { history_complete: true }, // older payload: no kind, no coverage end
  ]) {
    const f = historyFreshness(snapshot);
    if (f.state !== "unknown") {
      // The last case is legitimately classifiable as exact_current; what must
      // never happen is a notice asserting a restriction we cannot see.
      assert.equal(needsNotice(f), false);
    } else {
      assert.equal(noticeText(f), null);
    }
  }
});

test("the notice never states a star count", () => {
  // An archive series counts re-stars and can exceed the repository's own star
  // count, so a "shows N of M" gap would be confidently wrong exactly where it
  // is most prominent. Dates only — years are fine, counts are not.
  const COUNT = /\d{1,3},\d{3}|\b\d+\s+(?:of|stars)\b|\bshows\s+\d/i;
  for (const state of [
    historyFreshness(exact("2026-07-20T13:47:16Z")),
    historyFreshness(exact(null)),
    historyFreshness({ history_complete: false, history_status: "restricted" }),
  ]) {
    const text = noticeText(state);
    if (text) assert.doesNotMatch(text, COUNT, text);
  }
});

test("a malformed coverage date degrades to no date rather than Invalid Date", () => {
  const f = historyFreshness(exact("not-a-date"));
  assert.equal(f.through, null);
  assert.equal(formatThrough(f.through), null);
});
