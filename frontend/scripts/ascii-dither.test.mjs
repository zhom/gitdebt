import assert from "node:assert/strict";
import { test } from "node:test";

import {
  asciiDensity,
  asciiGlyph,
  asciiGrid,
} from "../src/lib/ascii-dither.ts";

test("ASCII grid follows measured glyph metrics and stays bounded", () => {
  assert.deepEqual(asciiGrid(900, 300, 6, 10), {
    cols: 97,
    rows: 25,
    cellWidth: 9.3,
    cellHeight: 12,
  });
  assert.equal(asciiGrid(100_000, 100_000, 1, 1).cols, 180);
  assert.equal(asciiGrid(100_000, 100_000, 1, 1).rows, 80);
});

test("hexdump glyphs are deterministic and restricted to hexadecimal", () => {
  const a = asciiGlyph(12, 8, 0);
  assert.equal(a, asciiGlyph(12, 8, 0));
  assert.match(a, /^[0-9A-F]$/);
  assert.notEqual(a, asciiGlyph(12, 8, 8));
});

test("mask alpha gates density while animation remains bounded", () => {
  assert.equal(asciiDensity(0, 4, 4, 20, 10, 0), 0);
  for (const phase of [0, 1, 10, 100]) {
    const density = asciiDensity(1, 10, 5, 20, 10, phase);
    assert.ok(density >= 0 && density <= 1);
  }
  assert.ok(
    asciiDensity(1, 10, 5, 20, 10, 0) >
      asciiDensity(0.25, 10, 5, 20, 10, 0),
  );
});

