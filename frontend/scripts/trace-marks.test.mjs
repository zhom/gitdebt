import assert from "node:assert/strict";
import { test } from "node:test";

import {
  arcFractions,
  drawDelayAt,
  thresholdLabel,
  traceMilestones,
} from "../src/lib/trace-marks.ts";

test("the pen is at the ends of its trace at the ends of the draw", () => {
  assert.equal(drawDelayAt(0, 900), 0);
  assert.equal(drawDelayAt(1, 900), 900);
  // EASE_DRAW is symmetric, so the midpoint of the length is the midpoint of
  // the time.
  assert.equal(drawDelayAt(0.5, 900), 450);
  // Eased in: a quarter of the length takes more than a quarter of the time.
  assert.ok(drawDelayAt(0.25, 900) > 225);
  assert.ok(drawDelayAt(0.75, 900) < 675);
});

test("the delay never leaves the draw", () => {
  assert.equal(drawDelayAt(-1, 900), 0);
  assert.equal(drawDelayAt(2, 900), 900);
});

test("arc fractions are cumulative and end at one", () => {
  const fractions = arcFractions([
    { x: 0, y: 0 },
    { x: 3, y: 4 },
    { x: 6, y: 8 },
  ]);
  assert.deepEqual(fractions, [0, 0.5, 1]);
});

test("threshold labels are lettered as the drawing letters a figure", () => {
  assert.equal(thresholdLabel(100), "100");
  assert.equal(thresholdLabel(10_000), "10K");
  assert.equal(thresholdLabel(1_000_000), "1M");
});

/** A cumulative trace on a 1000×300 sheet, growing tenfold every 200px. */
function trace() {
  const points = [];
  for (let i = 0; i <= 100; i += 1) {
    const x = 20 + i * 9;
    const value = Math.round(50 * 10 ** (i / 22));
    points.push({ x, y: 300 - (value / 2_000_000) * 280, value });
  }
  return points;
}

test("milestones mark the first crossing of each round number", () => {
  const points = trace();
  const marks = traceMilestones(points, { baselineY: 300, fontSize: 10 });
  assert.deepEqual(
    marks.map((m) => m.label),
    ["100", "1K", "10K", "100K", "1M"],
  );
  for (const mark of marks) {
    const at = points.findIndex((p) => p.value >= mark.threshold);
    assert.equal(mark.x, points[at].x);
    assert.ok(points[at - 1].value < mark.threshold);
    // The mark waits for the pen, which reaches later points later.
    assert.ok(mark.delay > 0 && mark.delay < 900);
  }
  for (let i = 1; i < marks.length; i += 1) {
    assert.ok(marks[i].delay > marks[i - 1].delay);
  }
});

test("a crossing the trace does not show is not marked", () => {
  const points = trace().filter((p) => p.value >= 1_500);
  const marks = traceMilestones(points, { baselineY: 300, fontSize: 10 });
  assert.ok(!marks.some((m) => m.threshold <= 1_000));
});

test("a mark that would collide with a reserved box is dropped", () => {
  const points = trace();
  const all = traceMilestones(points, { baselineY: 300, fontSize: 10 });
  const first = all[0];
  const marks = traceMilestones(points, {
    baselineY: 300,
    fontSize: 10,
    avoid: [{ x0: first.x - 60, y0: 0, x1: first.x + 10, y1: 300 }],
  });
  assert.ok(!marks.some((m) => m.threshold === first.threshold));
});

test("crowded crossings keep the earlier mark and drop the next", () => {
  // Every threshold crossed within a few pixels of the next.
  const points = [0, 50, 500, 5_000, 50_000, 500_000, 600_000, 700_000].map(
    (value, i) => ({
      x: i < 6 ? 400 + i * 3 : 400 + i * 200,
      y: 300 - (value / 700_000) * 280,
      value,
    }),
  );
  const marks = traceMilestones(points, { baselineY: 300, fontSize: 10 });
  assert.ok(marks.length >= 1);
  for (let i = 1; i < marks.length; i += 1) {
    assert.ok(marks[i].x - marks[i - 1].x > 10 || marks[i - 1].y - marks[i].y > 10);
  }
});
