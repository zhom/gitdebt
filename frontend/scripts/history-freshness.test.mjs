import assert from "node:assert/strict";
import { test } from "node:test";

import {
  coverageLabel,
  formatThrough,
  historyFreshness,
  needsNotice,
  noticeText,
  seriesOpen,
  sourceDensity,
  sourceDetail,
  sourceLabel,
  stateLabel,
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

/* ------------------------------------------------------------------ *
 * Provenance vocabulary: source + date + state, and nothing else.
 * ------------------------------------------------------------------ */

/** One representative freshness per state, built through the real classifier. */
const STATES = {
  exact_current: historyFreshness(exact("2024-03-02T00:00:00Z")),
  exact_frozen: historyFreshness(exact("2026-07-20T13:47:16Z")),
  archive: historyFreshness({
    history_complete: true,
    history_kind: "public_star_actions",
    history_approximate: true,
    history_status: "ready",
    history_coverage_end: "2026-08-08T11:30:21Z",
  }),
  restricted: historyFreshness({
    history_complete: false,
    history_status: "restricted",
    history_coverage_end: null,
  }),
  unknown: historyFreshness({ history_complete: false, history_status: "queued" }),
};

test("every state classifies to itself, so the table below is exhaustive", () => {
  for (const [name, freshness] of Object.entries(STATES)) {
    assert.equal(freshness.state, name);
  }
});

test("sourceLabel names the method that produced the points", () => {
  assert.equal(sourceLabel(STATES.exact_current), "GitHub stargazer list");
  assert.equal(sourceLabel(STATES.exact_frozen), "GitHub stargazer list");
  assert.equal(sourceLabel(STATES.archive), "Public GH Archive star events");
  assert.equal(sourceLabel(STATES.restricted), "No readable source");
  assert.equal(sourceLabel(STATES.unknown), "Source not established");
});

test("stateLabel says whether the series still receives points", () => {
  assert.equal(stateLabel(STATES.exact_current), "Still updating");
  assert.equal(stateLabel(STATES.archive), "Still updating");
  assert.equal(stateLabel(STATES.exact_frozen), "No longer updating");
  assert.equal(stateLabel(STATES.restricted), "Cannot be read");
  assert.equal(stateLabel(STATES.unknown), "Being read");
});

test("coverageLabel states a date or says the window is not established", () => {
  assert.equal(coverageLabel(STATES.exact_frozen), "Covers through July 20, 2026");
  assert.equal(coverageLabel(STATES.archive), "Covers through August 8, 2026");
  assert.equal(coverageLabel(STATES.restricted), "Coverage window not established");
  assert.equal(coverageLabel(STATES.unknown), "Coverage window not established");
});

test("sourceDetail delegates the July 2026 sentences instead of retyping them", () => {
  // One wording, one module. A second copy is a second thing to get wrong.
  assert.equal(sourceDetail(STATES.exact_frozen), noticeText(STATES.exact_frozen));
  assert.equal(sourceDetail(STATES.restricted), noticeText(STATES.restricted));
  assert.match(sourceDetail(STATES.exact_current), /stargazer list/);
  assert.match(sourceDetail(STATES.archive), /unstars are not/);
  assert.match(sourceDetail(STATES.unknown), /has not established a source/);
  for (const freshness of Object.values(STATES)) {
    assert.equal(typeof sourceDetail(freshness), "string");
    assert.ok(sourceDetail(freshness).length > 0);
  }
});

test("sourceDensity depends only on the state, never on the data", () => {
  // If density tracked coverage or event volume it would be a completeness
  // score wearing a texture, which is the one figure that must never ship.
  const early = historyFreshness(exact("2026-07-02T00:00:00Z"));
  const late = historyFreshness(exact("2026-08-09T00:00:00Z"));
  assert.equal(early.state, "exact_frozen");
  assert.equal(late.state, "exact_frozen");
  assert.equal(sourceDensity(early), sourceDensity(late));

  const thinArchive = historyFreshness({
    history_complete: true,
    history_kind: "public_star_actions",
    history_approximate: true,
    history_coverage_end: "2019-01-01T00:00:00Z",
    history_event_count: 3,
  });
  const fatArchive = historyFreshness({
    history_complete: true,
    history_kind: "public_star_actions",
    history_approximate: true,
    history_coverage_end: "2026-08-09T00:00:00Z",
    history_event_count: 480_000,
  });
  assert.equal(sourceDensity(thinArchive), sourceDensity(fatArchive));

  assert.equal(sourceDensity(STATES.exact_current), 0.85);
  assert.equal(sourceDensity(STATES.exact_frozen), 0.85);
  assert.equal(sourceDensity(STATES.archive), 0.45);
  assert.equal(sourceDensity(STATES.restricted), 0.12);
  assert.equal(sourceDensity(STATES.unknown), 0.12);
  assert.equal(sourceDensity.length, 1, "density must never take a magnitude argument");
});

test("seriesOpen is true only where points are still arriving", () => {
  assert.equal(seriesOpen(STATES.exact_current), true);
  assert.equal(seriesOpen(STATES.archive), true);
  assert.equal(seriesOpen(STATES.exact_frozen), false);
  assert.equal(seriesOpen(STATES.restricted), false);
  assert.equal(seriesOpen(STATES.unknown), false);
});

test("no provenance copy in any state states a count, a share, or a verdict", () => {
  // Same ban as the notice, extended over every new string: a gap looks like a
  // precise fact and is not one. Dates are fine; quantities of stars are not.
  const COUNT = /\d{1,3},\d{3}|\b\d+\s+(?:of|stars)\b|\bshows\s+\d|\d+\s*%|\bpercent\b/i;
  const VERDICT = /\b(verified|unverified|suspicious|fake|score|complete(?:ness)?\s+score)\b/i;
  for (const freshness of Object.values(STATES)) {
    for (const text of [
      sourceLabel(freshness),
      stateLabel(freshness),
      coverageLabel(freshness),
      sourceDetail(freshness),
    ]) {
      assert.doesNotMatch(text, COUNT, text);
      assert.doesNotMatch(text, VERDICT, text);
    }
  }
});

test("no copy offers connecting a repository, because no such flow exists", () => {
  // `repo_star_grants` has no writer, no reader and no HTTP route; there is no
  // POST .../connect, no install link, and no grant field in the analyze
  // payload. Every one of these sentences renders beside a sign-in caption that
  // says in as many words that signing in does NOT restore a stargazer read, so
  // offering connection here would be both an invented capability and a
  // self-contradiction on the same card. If a connection flow ever ships, this
  // test is the thing that should be deleted first — deliberately.
  const CONNECT = /\bconnect(?:ing|ed|s|ion)?\b/i;
  for (const freshness of Object.values(STATES)) {
    for (const text of [
      noticeText(freshness) ?? "",
      sourceLabel(freshness),
      stateLabel(freshness),
      coverageLabel(freshness),
      sourceDetail(freshness),
    ]) {
      assert.doesNotMatch(text, CONNECT, text);
    }
  }
});

test("the frozen and restricted notices still say what actually happened", () => {
  // Removing the false remedy must not remove the fact. The reader still needs
  // the date the series stops at, and the reason it stops.
  const frozen = noticeText(STATES.exact_frozen);
  assert.match(frozen, /complete through July 20, 2026/);
  assert.match(frozen, /admins and collaborators/);
  assert.match(frozen, /gitdebt can no longer read/);

  const restricted = noticeText(STATES.restricted);
  assert.match(restricted, /admins and collaborators/);
  assert.match(restricted, /gitdebt cannot read it/);
});
