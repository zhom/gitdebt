import assert from "node:assert/strict";
import { test } from "node:test";

import { historyFreshness } from "../src/lib/history-freshness.ts";
import { MEDIA_RENDER_REVISION } from "../src/lib/media.ts";
import { provenanceReadmeBlock } from "../src/lib/provenance-embed.ts";
import { bestEmbed, readmeLink, repoEmbedAssets } from "../src/lib/readme-embeds.ts";

const API = "https://api.gitdebt.dev";
const SITE = "https://gitdebt.dev";
const SLUG = "facebook/react";

const frozen = {
  history_complete: true,
  history_kind: "current_stargazers",
  history_approximate: false,
  history_status: "ready",
  history_coverage_end: "2026-07-20T13:47:16Z",
};

const archive = {
  history_complete: true,
  history_kind: "public_star_actions",
  history_approximate: true,
  history_status: "ready",
  history_coverage_end: "2026-08-08T11:30:21Z",
};

const chartAsset = () => repoEmbedAssets(SLUG).find((a) => a.id === "chart");

function block(snapshot) {
  return provenanceReadmeBlock({
    apiBase: API,
    siteOrigin: SITE,
    slug: SLUG,
    snapshot,
  });
}

test("the picture half is byte-identical to the shared embed builder", () => {
  // /badges, /api/md and this block must hand out the same bytes. A second
  // formatter here is a second thing to drift from the Rust renderer.
  const expected = bestEmbed(API, chartAsset(), readmeLink(SITE, `/${SLUG}`));
  const { snippet, language } = block(frozen);
  assert.equal(language, "html");
  assert.ok(snippet.startsWith(`${expected}\n\n`), snippet);
});

test("the block is the embed, one blank line, and exactly one provenance line", () => {
  const { snippet } = block(archive);
  const [picture, ...rest] = snippet.split("\n\n");
  assert.equal(rest.length, 1);
  assert.equal(picture, bestEmbed(API, chartAsset(), readmeLink(SITE, `/${SLUG}`)));
  assert.match(rest[0], /^<sub>Star history source: .+<\/sub>$/);
});

test("ref=readme rides both anchors and neither image URL", () => {
  const { snippet } = block(frozen);
  const anchors = [...snippet.matchAll(/<a href="([^"]+)"/g)].map((m) => m[1]);
  assert.equal(anchors.length, 2, snippet);
  for (const href of anchors) {
    assert.equal(href, `${SITE}/${SLUG}?ref=readme`);
  }

  const images = [
    ...snippet.matchAll(/(?:srcset|src)="([^"]+)"/g),
  ].map((m) => m[1]);
  assert.ok(images.length >= 2, snippet);
  for (const url of images) {
    assert.ok(url.startsWith(`${API}/api/repos/${SLUG}/chart.`), url);
    assert.doesNotMatch(url, /ref=/, url);
  }
});

test("the on-page preview revision never reaches a copied snippet", () => {
  // MEDIA_RENDER_REVISION exists to keep the site's own previews off stale edge
  // objects. In somebody's README it would be a cache-busting parameter, which
  // the embed rules forbid outright.
  for (const snapshot of [frozen, archive]) {
    const { snippet } = block(snapshot);
    assert.doesNotMatch(snippet, /\brev=/, snippet);
    assert.ok(!snippet.includes(`=${MEDIA_RENDER_REVISION}`), snippet);
  }
});

test("an unestablished source publishes nothing rather than an empty claim", () => {
  for (const snapshot of [
    null,
    undefined,
    { not_found: true },
    { history_complete: false, history_status: "queued" },
  ]) {
    if (historyFreshness(snapshot).state !== "unknown") continue;
    assert.equal(block(snapshot), null);
  }
});

test("the provenance line states source, date and state — and never a count", () => {
  const COUNT = /\d{1,3},\d{3}|\b\d+\s+(?:of|stars)\b|\bshows\s+\d|\d+\s*%|\bpercent\b/i;
  const VERDICT = /\b(verified|unverified|suspicious|fake|score)\b/i;
  for (const [snapshot, expected] of [
    [frozen, "GitHub stargazer list · Covers through July 20, 2026 · No longer updating"],
    [archive, "Historical star data · Covers through August 8, 2026 · Still updating"],
  ]) {
    const line = block(snapshot).snippet.split("\n\n")[1];
    assert.ok(line.includes(expected), line);
    // Strip the URL before the ban: a slug or an origin is not prose.
    const prose = line.replace(/<a href="[^"]*">/g, "");
    assert.doesNotMatch(prose, COUNT, prose);
    assert.doesNotMatch(prose, VERDICT, prose);
  }
});
