import assert from "node:assert/strict";
import { test } from "node:test";

import { CATEGORY_SLOTS, comparisonPenIndices } from "../src/lib/pens.ts";

const FRAMEWORKS = [
  "facebook/react",
  "vuejs/vue",
  "sveltejs/svelte",
  "angular/angular",
  "solidjs/solid",
  "preactjs/preact",
];

test("a comparison never draws a series in the reserved drafting red", () => {
  assert.ok(!CATEGORY_SLOTS.includes(1));
  for (const index of comparisonPenIndices(FRAMEWORKS).values()) {
    assert.notEqual(index, 1);
  }
});

test("pens match the backend's README overlay for the same slugs", () => {
  // Read from /api/chart.svg?repos=<these six> rendered by `theme::pens_for`:
  // #607c42, #1a609e, #282c2f, #a25e2b, #7b6f66, #1e7777.
  const pens = comparisonPenIndices(FRAMEWORKS);
  assert.deepEqual(
    FRAMEWORKS.map((slug) => pens.get(slug)),
    [3, 2, 0, 4, 7, 6],
  );
});

test("the assignment depends on the set, not the order", () => {
  const forward = comparisonPenIndices(FRAMEWORKS);
  const backward = comparisonPenIndices([...FRAMEWORKS].reverse());
  for (const slug of FRAMEWORKS) assert.equal(forward.get(slug), backward.get(slug));
});

test("up to seven series always get distinct pens", () => {
  const seven = [...FRAMEWORKS, "emberjs/ember.js"];
  const pens = comparisonPenIndices(seven);
  assert.equal(new Set(pens.values()).size, 7);
});
