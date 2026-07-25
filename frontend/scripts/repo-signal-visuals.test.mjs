import assert from "node:assert/strict";
import { test } from "node:test";

import {
  AGE_LABEL,
  AGE_ORDER,
  ageRingAtPoint,
  layoutAgeRings,
  layoutFileCouplings,
} from "../src/lib/repo-signal-visuals.ts";

test("age rings preserve semantic order and normalize file and churn shares", () => {
  assert.equal(AGE_LABEL.two_to_three_years, "Changed 1–3 years ago");
  const rings = layoutAgeRings([
    { range: "older", files: 10, changes: 5 },
    { range: "within_year", files: 3, changes: 9 },
    { range: "this_month", files: 5, changes: 20 },
    { range: "within_year", files: 2, changes: 1 },
  ]);

  assert.deepEqual(
    rings.map((ring) => ring.range),
    AGE_ORDER,
  );
  assert.deepEqual(
    rings.map((ring) => ring.files),
    [5, 5, 0, 10],
  );
  assert.deepEqual(
    rings.map((ring) => ring.fileShare),
    [0.25, 0.25, 0, 0.5],
  );
  assert.deepEqual(
    rings.map((ring) => ring.changeIntensity),
    [1, 0.5, 0, 0.125],
  );
  for (let index = 1; index < rings.length; index += 1) {
    assert.ok(rings[index].innerRadius > rings[index - 1].outerRadius);
  }
});

test("age rings clamp invalid counts and pointer hit testing excludes gaps", () => {
  const rings = layoutAgeRings([
    { range: "this_month", files: -2, changes: Number.NaN },
  ]);
  assert.equal(rings[0].files, 0);
  assert.equal(rings[0].changes, 0);
  assert.equal(ageRingAtPoint(125, 100, 200, 200, rings), 0);
  assert.equal(ageRingAtPoint(145, 100, 200, 200, rings), 1);
  assert.equal(ageRingAtPoint(136, 100, 200, 200, rings), null);
  assert.equal(ageRingAtPoint(100, 100, 200, 200, rings), null);
  assert.equal(ageRingAtPoint(1, 1, 0, 200, rings), null);
});

const COUPLINGS = [
  {
    source: "src/router.ts",
    target: "src/routes.ts",
    cochanges: 12,
    fix_commits: 5,
  },
  {
    source: "src/router.ts",
    target: "tests/router.test.ts",
    cochanges: 9,
    fix_commits: 4,
  },
  {
    source: "docs/guide.md",
    target: "src/routes.ts",
    cochanges: 3,
    fix_commits: 0,
  },
  {
    source: "src/routes.ts",
    target: "src/router.ts",
    cochanges: 2,
    fix_commits: 1,
  },
  {
    source: "src/ignored.ts",
    target: "src/ignored.ts",
    cochanges: 100,
    fix_commits: 100,
  },
];

test("coupling layout merges duplicate edges and ranks fix-heavy evidence", () => {
  const layout = layoutFileCouplings(COUPLINGS);
  assert.equal(layout.edges.length, 3);
  assert.deepEqual(layout.edges[0], {
    source: "src/router.ts",
    target: "src/routes.ts",
    cochanges: 14,
    fixCommits: 6,
    strength: 1,
    cluster: "src",
  });
  assert.ok(layout.clusters[0].fixWeight >= layout.clusters.at(-1).fixWeight);
  assert.ok(layout.nodes.some((node) => node.cluster === "tests"));
  assert.ok(layout.nodes.some((node) => node.cluster === "docs"));
});

test("coupling positions are deterministic, bounded, and input-order independent", () => {
  const forward = layoutFileCouplings(COUPLINGS, 8, 8);
  const reverse = layoutFileCouplings([...COUPLINGS].reverse(), 8, 8);
  assert.deepEqual(forward, reverse);
  for (const node of forward.nodes) {
    assert.ok(node.x >= 0.08 && node.x <= 0.92);
    assert.ok(node.y >= 0.12 && node.y <= 0.88);
    assert.ok(node.label.length <= 18);
  }
});

test("coupling layout is empty when no honest relationship remains", () => {
  assert.deepEqual(
    layoutFileCouplings([
      {
        source: "same.ts",
        target: "same.ts",
        cochanges: 10,
        fix_commits: 4,
      },
      {
        source: "",
        target: "other.ts",
        cochanges: 2,
        fix_commits: 1,
      },
    ]),
    { nodes: [], edges: [], clusters: [] },
  );
});
