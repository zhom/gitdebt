import assert from "node:assert/strict";
import { test } from "node:test";

import {
  brailleGlyph,
  contourSignal,
  mosaicSignal,
  normalizeSignals,
  pageInsightForPath,
  signalHash,
  signalSeed,
} from "../src/lib/signal-art.ts";

test("signal hashes are deterministic, bounded, and page-specific", () => {
  const repo = signalSeed("denoland/deno");
  assert.equal(repo, signalSeed("denoland/deno"));
  assert.notEqual(repo, signalSeed("oven-sh/bun"));
  for (let x = 0; x < 20; x += 1) {
    const value = signalHash(repo, x, x * 3);
    assert.ok(value >= 0 && value < 1);
  }
});

test("all three data fields remain bounded and respond to identity", () => {
  const a = signalSeed("a/repo");
  const b = signalSeed("b/repo");
  const values = [3, 21, 144];
  for (const phase of [0, 1, 10]) {
    const mosaic = mosaicSignal(a, 4, 3, 20, 9, phase, values);
    const contour = contourSignal(a, 0.4, 0.7, phase, values);
    assert.ok(mosaic >= 0 && mosaic <= 1);
    assert.ok(contour >= 0 && contour <= 1);
  }
  assert.notEqual(
    mosaicSignal(a, 4, 3, 20, 9, 1, values),
    mosaicSignal(b, 4, 3, 20, 9, 1, values),
  );
  assert.notEqual(
    contourSignal(a, 0.4, 0.7, 1, values),
    contourSignal(b, 0.4, 0.7, 1, values),
  );
});

test("braille activity is deterministic and uses only braille glyphs", () => {
  const seed = signalSeed("zhom");
  const glyph = brailleGlyph(seed, 2, 9, 4, [3, 5, 8]);
  assert.equal(glyph, brailleGlyph(seed, 2, 9, 4, [3, 5, 8]));
  assert.match(glyph, /^[\u2800-\u28ff]$/u);
});

test("signal normalization handles empty and non-finite inputs", () => {
  assert.deepEqual(normalizeSignals([]), [0.5]);
  assert.deepEqual(normalizeSignals([Number.NaN, Number.POSITIVE_INFINITY]), [0.5]);
  assert.deepEqual(normalizeSignals([0, 5, 10]), [0, 0.5, 1]);
});

test("route insight selects repository, profile, and comparison grammars", () => {
  const repo = pageInsightForPath("/denoland/deno");
  assert.equal(repo.kind, "repository");
  assert.equal(repo.mode, "mosaic");
  assert.match(repo.question, /denoland\/deno/);

  const profile = pageInsightForPath("/zhom");
  assert.equal(profile.kind, "profile");
  assert.equal(profile.mode, "braille");
  assert.match(profile.answer, /stargazer identities/);

  const comparison = pageInsightForPath(
    "/vs/denoland/deno/oven-sh/bun?mode=date",
  );
  assert.equal(comparison.kind, "comparison");
  assert.equal(comparison.mode, "contour");
  assert.deepEqual(comparison.labels, ["denoland/deno", "oven-sh/bun"]);
});
