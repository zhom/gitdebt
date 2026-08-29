import assert from "node:assert/strict";
import { test } from "node:test";

import {
  coverageLabel,
  formatThrough,
  historyFreshness,
  needsNotice,
  noticeText,
  seriesOpen,
  sourceDetail,
  sourceStroke,
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

/** Exact points through `at`, historical activity after it. `history_approximate`
 *  is true because the tail is — that is the contract, and it is exactly what
 *  makes the ordering inside `historyFreshness` load-bearing. */
const spliced = (end, at) => ({
  history_complete: true,
  history_kind: "stargazers_then_activity",
  history_approximate: true,
  history_status: "ready",
  history_coverage_end: end,
  history_splice_at: at,
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
  assert.match(noticeText(f), /only to applications that administer the repository/);
  assert.doesNotMatch(noticeText(f), /complete through/);
});

test("a spliced series keeps its exact half instead of being read as approximate", () => {
  // The payload carries history_approximate: true, so a classifier that tested
  // that flag before the kind would call this "archive" and the copy would
  // describe a mostly-exact curve as if none of it were.
  const f = historyFreshness(spliced("2026-08-15T09:12:44Z", "2026-07-20T13:47:16Z"));
  assert.equal(f.state, "spliced");
  assert.equal(f.through.toISOString(), "2026-08-15T09:12:44.000Z");
  assert.equal(f.splicedAt.toISOString(), "2026-07-20T13:47:16.000Z");
  assert.equal(seriesOpen(f), true);
  assert.ok(needsNotice(f));
});

test("a spliced series with no splice instant states the change without inventing a day", () => {
  // Older cached payload, or a backend that has not filled the column yet.
  // Naming a date we do not have would be the one unrecoverable error here.
  for (const snapshot of [
    spliced("2026-08-15T09:12:44Z", null),
    spliced("2026-08-15T09:12:44Z", "not-a-date"),
  ]) {
    const f = historyFreshness(snapshot);
    assert.equal(f.state, "spliced");
    assert.equal(f.splicedAt, null);
    assert.match(noticeText(f), /Two sources in one line\. Every point up to the join/);
    assert.doesNotMatch(noticeText(f), /joined on/);
    assert.doesNotMatch(noticeText(f), /Invalid Date/);
  }
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
    historyFreshness(spliced("2026-08-15T09:12:44Z", "2026-07-20T13:47:16Z")),
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
  spliced: historyFreshness(spliced("2026-08-15T09:12:44Z", "2026-07-20T13:47:16Z")),
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
  assert.equal(sourceLabel(STATES.archive), "Historical star data");
  // Both sources, in the order the line uses them. Naming one would be false
  // about half the curve.
  assert.equal(
    sourceLabel(STATES.spliced),
    "GitHub stargazer list, then historical star data",
  );
  assert.equal(sourceLabel(STATES.restricted), "No readable source");
  assert.equal(sourceLabel(STATES.unknown), "Source not established");
});

test("stateLabel says whether the series still receives points", () => {
  assert.equal(stateLabel(STATES.exact_current), "Still updating");
  assert.equal(stateLabel(STATES.archive), "Still updating");
  // The whole reason to splice: the series advances again. The label that
  // started this work said "No longer updating" over a repository that was
  // gaining stars every day.
  assert.equal(stateLabel(STATES.spliced), "Still updating");
  assert.equal(stateLabel(STATES.exact_frozen), "No longer updating");
  assert.equal(stateLabel(STATES.restricted), "Cannot be read");
  assert.equal(stateLabel(STATES.unknown), "Being read");
});

test("coverageLabel states a date or says the window is not established", () => {
  assert.equal(coverageLabel(STATES.exact_frozen), "Covers through July 20, 2026");
  assert.equal(coverageLabel(STATES.archive), "Covers through August 8, 2026");
  // Coverage is where the line ends, never where it changes method — the splice
  // instant is the detail's business, and conflating them would understate how
  // far a spliced series actually runs.
  assert.equal(coverageLabel(STATES.spliced), "Covers through August 15, 2026");
  assert.equal(coverageLabel(STATES.restricted), "Coverage window not established");
  assert.equal(coverageLabel(STATES.unknown), "Coverage window not established");
});

test("sourceDetail delegates the July 2026 sentences instead of retyping them", () => {
  // One wording, one module. A second copy is a second thing to get wrong.
  assert.equal(sourceDetail(STATES.exact_frozen), noticeText(STATES.exact_frozen));
  assert.equal(sourceDetail(STATES.restricted), noticeText(STATES.restricted));
  assert.equal(sourceDetail(STATES.spliced), noticeText(STATES.spliced));
  assert.match(sourceDetail(STATES.exact_current), /stargazer list/);
  assert.match(sourceDetail(STATES.archive), /unstars are not/);
  assert.match(sourceDetail(STATES.unknown), /has not established a source/);
  for (const freshness of Object.values(STATES)) {
    assert.equal(typeof sourceDetail(freshness), "string");
    assert.ok(sourceDetail(freshness).length > 0);
  }
});

test("sourceStroke depends only on the state, never on the data", () => {
  // If the dash pattern tracked coverage or event volume it would be a
  // completeness score wearing a line style, which is the one figure that must
  // never ship.
  const early = historyFreshness(exact("2026-07-02T00:00:00Z"));
  const late = historyFreshness(exact("2026-08-09T00:00:00Z"));
  assert.equal(early.state, "exact_frozen");
  assert.equal(late.state, "exact_frozen");
  assert.equal(sourceStroke(early), sourceStroke(late));

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
  assert.equal(sourceStroke(thinArchive), sourceStroke(fatArchive));

  // A spliced series is two sources, not a measured blend of them: its tail
  // pattern is one constant, and two spliced series with wildly different exact
  // halves must be drawn with the same line.
  const earlySplice = historyFreshness(spliced("2026-08-15T00:00:00Z", "2019-04-01T00:00:00Z"));
  const lateSplice = historyFreshness(spliced("2026-08-15T00:00:00Z", "2026-07-20T00:00:00Z"));
  assert.equal(sourceStroke(earlySplice), sourceStroke(lateSplice));

  // An exact series is an object line: measured, so it is drawn solid.
  assert.equal(sourceStroke(STATES.exact_current), "");
  assert.equal(sourceStroke(STATES.exact_frozen), "");
  // Spliced carries the pattern its tail is drawn with; the head stays solid.
  assert.equal(sourceStroke(STATES.spliced), "9 4");
  // Archive is a construction line: real and derived rather than observed.
  assert.equal(sourceStroke(STATES.archive), "5 4");
  // Nothing was measured, so the line is drawn as one that was never taken.
  assert.equal(sourceStroke(STATES.restricted), "1 3");
  assert.equal(sourceStroke(STATES.unknown), "1 3");

  // Every pattern is a legal SVG dash array, and every dashed state is
  // distinguishable from every other at a glance.
  const patterns = new Set();
  for (const freshness of Object.values(STATES)) {
    const dash = sourceStroke(freshness);
    assert.equal(typeof dash, "string");
    if (dash !== "") assert.match(dash, /^\d+(?:\.\d+)? \d+(?:\.\d+)?$/);
    patterns.add(dash);
  }
  assert.equal(patterns.size, 4, "one pattern per source, not one per state");

  assert.equal(sourceStroke.length, 1, "the stroke must never take a magnitude argument");
});

test("seriesOpen is true only where points are still arriving", () => {
  assert.equal(seriesOpen(STATES.exact_current), true);
  assert.equal(seriesOpen(STATES.archive), true);
  assert.equal(seriesOpen(STATES.spliced), true);
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

test("no copy names who GitHub does serve, because to an owner that reads as an offer", () => {
  // The bug that started this: a signed-in owner of a frozen repository read
  // "GitHub limited stargazer lists to a repository's own admins and
  // collaborators" as *so it works for me*. It does not. gitdebt reads GitHub
  // with its own application credentials, which administer nothing, and no
  // sign-in changes that — there is no repository connection flow to change it
  // with. Naming the exempt role in a product that cannot use it is an
  // invitation to a door that does not exist, so the vocabulary names the
  // restriction from gitdebt's side and never from the reader's.
  //
  // "administer"/"administers" is deliberately still allowed: it describes the
  // access gitdebt lacks, not a person the reader might be.
  const INVITATION = /\b(admins?|administrators?|collaborators?)\b/i;
  for (const freshness of Object.values(STATES)) {
    for (const text of [
      noticeText(freshness) ?? "",
      sourceLabel(freshness),
      stateLabel(freshness),
      coverageLabel(freshness),
      sourceDetail(freshness),
    ]) {
      assert.doesNotMatch(text, INVITATION, text);
    }
  }
});

test("the frozen and restricted notices still say what actually happened", () => {
  // Removing the false invitation must not remove the fact. The reader still
  // needs the date the series stops at, the reason it stops, and — because the
  // reader is often the repository's own signed-in owner — an explicit sentence
  // saying that signing in does not reopen it.
  const frozen = noticeText(STATES.exact_frozen);
  assert.match(frozen, /complete through July 20, 2026/);
  assert.match(
    frozen,
    /In July 2026 GitHub restricted stargazer lists to applications that administer the repository/,
  );
  assert.match(frozen, /gitdebt is not one of them/);
  assert.match(frozen, /signing in — even as this repository's owner — does not change/);
  assert.match(frozen, /the exact series ends there/);

  const restricted = noticeText(STATES.restricted);
  assert.match(restricted, /only to applications that administer the repository/);
  assert.match(restricted, /gitdebt is not one of them/);
  assert.match(restricted, /signing in — even as this repository's owner — does not change/);

  // Neither may hold out a later fix. "cannot read it today" or "until gitdebt
  // is granted access" would be a promise this module has no way to keep.
  for (const text of [frozen, restricted]) {
    assert.doesNotMatch(text, /\b(yet|for now|today|until|once|soon|restore[sd]?)\b/i, text);
  }
});

test("the archive detail warns that a flat stretch may be the source, not the repo", () => {
  const text = sourceDetail(STATES.archive);

  // The pre-existing half: an activity read, not a net count.
  assert.match(text, /attention signal rather than a net star count/);
  // The half that was missing. Historical star data does not record every
  // star and how much it records has varied, so most archive-backed charts
  // now flatten for a reason that has nothing to do with the repository. A
  // reader told only "attention signal" reads that shape as a stall.
  assert.match(text, /does not record every star/);
  assert.match(
    text,
    /flatter stretch can be the source thinning rather than this repository slowing/,
  );
  // Same rule as everywhere else: state the direction of the error, never its
  // size. A share or a gap would be most wrong exactly where it looks precise.
  assert.doesNotMatch(text, /\d+\s*%|\bpercent\b|\bfraction\b|\bmost\b|\bnearly all\b/i);
  assert.doesNotMatch(text, /\b\d+\s+(?:of|stars|events)\b/i);
});

test("the spliced notice discloses the method change without quantifying the gap", () => {
  const text = noticeText(STATES.spliced);

  // 1. The line changes method, on a stated date.
  assert.match(text, /joined on July 20, 2026/);
  // 2. Head and tail measure different things.
  assert.match(text, /one current stargazer, timestamped, read from GitHub's stargazer list/);
  assert.match(text, /counts star actions instead/);
  assert.match(text, /cannot see unstars/);
  // 3. The tail undercounts, so a flat tail is not evidence the repository
  //    stalled. This is the sentence the owner asked for: without it, a chart
  //    that is only reading badly looks like a project that stopped growing.
  assert.match(text, /does not record every star/);
  assert.match(text, /flatter tail can be the source thinning rather than this repository slowing/);
  // 4. And it says all of that with no figure of any kind. A share of events,
  //    a gap, or a completeness reading would be wrong exactly where it looks
  //    most precise, so the copy states the direction of the error and stops.
  assert.doesNotMatch(text, /\d+\s*%|\bpercent\b|\bfraction\b|\bmost\b|\bnearly all\b/i);
  assert.doesNotMatch(text, /\b\d+\s+(?:of|stars|events)\b/i);

  // The coverage date is the end of the line, not the join: a spliced series
  // runs past its splice, and saying otherwise would understate it.
  assert.notEqual(formatThrough(STATES.spliced.through), formatThrough(STATES.spliced.splicedAt));
  assert.match(coverageLabel(STATES.spliced), /August 15, 2026/);
});
