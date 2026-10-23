import assert from "node:assert/strict";
import { test } from "node:test";

import {
  arcFractions,
  calendarEasing,
  crossingTime,
  drawDelayAt,
  segmentAtX,
  thresholdLabel,
  traceCrossings,
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

test("the station on a trace is found by x", () => {
  const points = [{ x: 0 }, { x: 10 }, { x: 30 }];
  assert.deepEqual(segmentAtX(points, 20), { index: 1, fraction: 0.5 });
  assert.deepEqual(segmentAtX(points, -5), { index: 0, fraction: 0 });
  assert.deepEqual(segmentAtX(points, 99), { index: 1, fraction: 1 });
});

test("calendar easing spends time evenly in x, not in length", () => {
  // Flat for the first half of x, then a cliff: almost all of the length is
  // in the second half, yet the pen must be halfway in time at the cliff.
  const points = [
    { x: 0, y: 100 },
    { x: 50, y: 100 },
    { x: 100, y: 0 },
  ];
  const easing = calendarEasing(points, 2);
  const [start, middle, end] = easing.slice(7, -1).split(", ").map(Number);
  assert.equal(start, 0);
  assert.equal(end, 1);
  // 50 of a total length of 50 + 111.8 is about 0.309.
  assert.ok(Math.abs(middle - 50 / (50 + Math.hypot(50, 100))) < 1e-3);
});

test("calendar milestones arrive at their share of x", () => {
  const points = trace();
  const marks = traceMilestones(points, {
    baselineY: 300,
    fontSize: 10,
    duration: 1400,
    calendar: true,
  });
  const x0 = points[0].x;
  const span = points[points.length - 1].x - x0;
  for (const mark of marks) {
    assert.equal(mark.delay, Math.round(((mark.x - x0) / span) * 1400));
  }
});

test("crossings mark where one trace overtakes another", () => {
  // `a` rises steadily; `b` starts higher and flattens, so `a` overtakes it
  // once, at x = 50, well clear of the baseline.
  const a = [{ x: 0, y: 290 }, { x: 100, y: 90 }];
  const b = [{ x: 0, y: 250 }, { x: 50, y: 190 }, { x: 100, y: 180 }];
  const crossings = traceCrossings([a, b], { baselineY: 300, plotH: 280, xMax: 200 });
  assert.equal(crossings.length, 1);
  assert.ok(Math.abs(crossings[0].x - 50) < 2);
  assert.deepEqual([crossings[0].a, crossings[0].b], [0, 1]);
});

test("lines tangled on the baseline or flickering are not marked", () => {
  // Both near the baseline: a crossing there is noise.
  const low = traceCrossings(
    [
      [{ x: 0, y: 299 }, { x: 100, y: 290 }],
      [{ x: 0, y: 295 }, { x: 100, y: 294 }],
    ],
    { baselineY: 300, plotH: 280, xMax: 200 },
  );
  assert.equal(low.length, 0);
  // A zig-zag that swaps order every 6px never holds a new order.
  const zig = Array.from({ length: 30 }, (_, i) => ({ x: i * 6, y: 150 + (i % 2 ? 4 : -4) }));
  const flat = [{ x: 0, y: 150 }, { x: 180, y: 150 }];
  assert.equal(traceCrossings([zig, flat], { baselineY: 300, plotH: 280, xMax: 400 }).length, 0);
});

test("at most `limit` crossings, spaced apart, highest first", () => {
  const a = [{ x: 0, y: 100 }, { x: 600, y: 100 }];
  // `b` weaves through `a` every 100px with long holds between swaps.
  const b = Array.from({ length: 7 }, (_, i) => ({ x: i * 100, y: i % 2 ? 60 : 140 }));
  const crossings = traceCrossings([a, b], { baselineY: 300, plotH: 280, xMax: 700, limit: 2 });
  assert.equal(crossings.length, 2);
  assert.ok(crossings[1].x - crossings[0].x >= 90);
});

test("a crossing is dated from the readings, not the drawn path", () => {
  const DAY = 86_400_000;
  // `a` passes `b` exactly on day 100.
  const a = [{ at: 0, value: 0 }, { at: 200 * DAY, value: 200 }];
  const b = [{ at: 0, value: 50 }, { at: 200 * DAY, value: 150 }];
  const at = crossingTime(a, b, 92 * DAY, 30 * DAY);
  assert.ok(Math.abs(at - 100 * DAY) < DAY);
  // The same crossing from a different estimate is the same day.
  assert.ok(Math.abs(crossingTime(a, b, 107 * DAY, 30 * DAY) - at) < 1);
  // No crossing inside the window.
  assert.equal(crossingTime(a, b, 20 * DAY, 10 * DAY), null);
});
