import assert from "node:assert/strict";
import { test } from "node:test";

import { heroMomentumRows } from "../src/lib/hero-momentum.ts";

const row = (rank, repo, stars, velocity) => ({ rank, repo, stars, velocity });

test("the board is ranked by the window it prints, the trailing month", () => {
  const rows = heroMomentumRows(
    {
      d30: [row(1, "a/one", 1000, 300), row(2, "b/two", 2000, 200)],
      // The weekly board disagrees; it supplies the tip, never the order.
      d7: [row(1, "b/two", 2000, 90), row(2, "a/one", 1000, 10)],
      d1: null,
    },
    8,
  );
  assert.deepEqual(
    rows.map((r) => r.repo),
    ["a/one", "b/two"],
  );
  assert.equal(rows[0].d30, 300);
  assert.equal(rows[0].d7, 10);
});

test("a repo the month never ranked stays off the hero", () => {
  // The hero prints one movement figure per row, so a repository with no
  // 30-day rank would print an em-dash in its only headline column.
  const rows = heroMomentumRows(
    {
      d30: [row(1, "a/one", 1000, 300)],
      d7: [row(1, "c/three", 5000, 900)],
      d1: [row(1, "d/four", 400, 120)],
    },
    8,
  );
  assert.deepEqual(
    rows.map((r) => r.repo),
    ["a/one"],
  );
});

test("a window the repo is absent from stays null, not zero", () => {
  const [only] = heroMomentumRows(
    { d30: [row(1, "a/one", 1000, 300)], d7: null, d1: null },
    8,
  );
  assert.equal(only.d7, null);
  assert.equal(only.d1, null);
});

test("stars come from the richest board that ranked the repo", () => {
  // Each board snapshots its own star total; a stale one must not win.
  const [only] = heroMomentumRows(
    {
      d30: [row(1, "a/one", 990, 300)],
      d7: [row(1, "a/one", 1010, 70)],
      d1: [row(1, "a/one", 1005, 10)],
    },
    8,
  );
  assert.equal(only.stars, 1010);
});

test("ties break on stars then slug, so two builds emit the same order", () => {
  // Real boards tie: today's live data has three repos within one star/day.
  const order = () =>
    heroMomentumRows(
      {
        d30: [
          row(1, "z/last", 400, 43),
          row(2, "a/first", 400, 43),
          row(3, "m/big", 9000, 43),
        ],
        d7: null,
        d1: null,
      },
      8,
    ).map((r) => r.repo);
  assert.deepEqual(order(), ["m/big", "a/first", "z/last"]);
  assert.deepEqual(order(), order());
});

test("every board unavailable is an empty board, not a throw", () => {
  assert.deepEqual(heroMomentumRows({ d30: null, d7: null, d1: null }, 8), []);
  assert.deepEqual(heroMomentumRows({ d30: [], d7: null, d1: null }, 8), []);
});

test("limit caps a long board and returns a short one whole", () => {
  const many = Array.from({ length: 20 }, (_, i) =>
    row(i + 1, `o/r${i}`, 100, 100 - i),
  );
  assert.equal(heroMomentumRows({ d30: many, d7: null, d1: null }, 8).length, 8);
  assert.equal(
    heroMomentumRows({ d30: many.slice(0, 3), d7: null, d1: null }, 8).length,
    3,
  );
});
