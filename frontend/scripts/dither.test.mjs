import assert from "node:assert/strict";
import { test } from "node:test";

import {
  BAYER4,
  CELL,
  INK,
  MARK,
  OFF_TIER,
  RasterBuffer,
  SWATCH,
  clamp01,
  densityFor,
  gridFor,
  makeIntensity,
  paintCheckbox,
  paintPanel,
  paintRail,
  paintSegment,
  paintSeparator,
  paintSwitchTrack,
  prefersReducedMotion,
} from "../src/lib/dither.ts";

const FILL = [200, 100, 50];

function alphaAt(buf, x, y) {
  return buf.data[(y * buf.cols + x) * 4 + 3] / 255;
}

function rgbAt(buf, x, y) {
  const i = (y * buf.cols + x) * 4;
  return [buf.data[i], buf.data[i + 1], buf.data[i + 2]];
}

/** The alpha the spec requires for an interior cell. */
function expected(density, intensity, lit) {
  const k = (0.3 + density * 0.7) * (1 + 0.22 * intensity);
  return clamp01(lit ? k : k * OFF_TIER);
}

function quantise(a) {
  return Math.round(clamp01(a) * 255 + 0.5 - 0.5) / 255;
}

test("BAYER4 holds the 16 ordered thresholds", () => {
  const flat = BAYER4.flat();
  assert.equal(flat.length, 16);
  assert.equal(Math.min(...flat), 0.5 / 16);
  assert.equal(Math.max(...flat), 15.5 / 16);
  assert.equal(new Set(flat).size, 16);
  assert.deepEqual(BAYER4[0], [0.5 / 16, 8.5 / 16, 2.5 / 16, 10.5 / 16]);
});

test("clamp01 pins both ends", () => {
  assert.equal(clamp01(-1), 0);
  assert.equal(clamp01(0.25), 0.25);
  assert.equal(clamp01(2), 1);
});

test("gridFor divides by the cell size and clamps both ends", () => {
  assert.deepEqual(gridFor(200, 40), { cols: 100, rows: 20 });
  assert.deepEqual(gridFor(200, 40, 4), { cols: 50, rows: 10 });
  // Floors at 4 cells, so a hairline still has a grid.
  assert.deepEqual(gridFor(0, 2), { cols: 4, rows: 4 });
  assert.deepEqual(gridFor(-30, 1), { cols: 4, rows: 4 });
  // Caps at 960x600 backing cells.
  assert.deepEqual(gridFor(100000, 100000), { cols: 960, rows: 600 });
  // Never emits NaN for an unmeasured box.
  assert.deepEqual(gridFor(Number.NaN, Number.NaN), { cols: 4, rows: 4 });
  assert.equal(CELL, 2);
});

test("RasterBuffer stores premultiplied RGBA", () => {
  const buf = new RasterBuffer(2, 2);
  buf.set(1, 1, [200, 100, 50], 0.5);
  assert.deepEqual(rgbAt(buf, 1, 1), [100, 50, 25]);
  assert.equal(buf.data[(1 * 2 + 1) * 4 + 3], 128);
  buf.clear();
  assert.equal(buf.data.every((v) => v === 0), true);
  // Alpha is clamped, not wrapped.
  buf.set(0, 0, [200, 100, 50], 4);
  assert.deepEqual(rgbAt(buf, 0, 0), [200, 100, 50]);
  assert.equal(alphaAt(buf, 0, 0), 1);
  buf.opaque();
  assert.equal(alphaAt(buf, 1, 1), 1);
});

test("densityFor follows the variant ramps", () => {
  assert.equal(densityFor("gradient", 0, 10), 0.25 + 0.75 * 0.05);
  assert.equal(densityFor("gradient", 9, 10), 0.25 + 0.75 * 0.95);
  assert.equal(densityFor("dotted", 5, 10), 0.5);
  assert.equal(densityFor("hatched", 5, 10), 0.75);
  assert.equal(densityFor("solid", 5, 10), 0.75);
});

test("paintPanel lights a cell exactly when density beats the threshold", () => {
  const buf = new RasterBuffer(8, 8);
  paintPanel(buf, FILL, "gradient", 0, { edge: null });
  for (let y = 1; y < 7; y++) {
    const density = densityFor("gradient", y, 8);
    for (let x = 1; x < 7; x++) {
      const lit = density > BAYER4[y & 3][x & 3];
      assert.equal(
        alphaAt(buf, x, y),
        quantise(expected(density, 0, lit)),
        `cell ${x},${y}`,
      );
    }
  }
});

test("intensity lowers the threshold and brightens the field", () => {
  const rest = new RasterBuffer(8, 8);
  const hover = new RasterBuffer(8, 8);
  const press = new RasterBuffer(8, 8);
  paintPanel(rest, FILL, "gradient", 0, { edge: null });
  paintPanel(hover, FILL, "gradient", 1, { edge: null });
  paintPanel(press, FILL, "gradient", 1.5, { edge: null });

  let lifted = 0;
  for (let y = 0; y < 8; y++) {
    const density = densityFor("gradient", y, 8);
    for (let x = 0; x < 8; x++) {
      const threshold = BAYER4[y & 3][x & 3];
      const litRest = density > threshold;
      const litHover = density > threshold - 0.1;
      if (!litRest && litHover) lifted++;
      assert.equal(alphaAt(hover, x, y), quantise(expected(density, 1, litHover)));
      assert.equal(
        alphaAt(press, x, y),
        quantise(expected(density, 1.5, density > threshold - 0.15)),
      );
      assert.ok(alphaAt(hover, x, y) >= alphaAt(rest, x, y) - 1e-9);
    }
  }
  assert.ok(lifted > 0, "hover must light cells that were dark at rest");
});

test("dotted punches real holes, hatched skips its off diagonal", () => {
  const dotted = new RasterBuffer(8, 8);
  paintPanel(dotted, FILL, "dotted", 0, { edge: null });
  let holes = 0;
  for (let y = 0; y < 8; y++) {
    for (let x = 0; x < 8; x++) {
      const lit = 0.5 > BAYER4[y & 3][x & 3] - 0.12;
      if (!lit) {
        holes++;
        assert.equal(alphaAt(dotted, x, y), 0);
      } else {
        assert.equal(alphaAt(dotted, x, y), quantise(expected(0.5, 0, true)));
      }
    }
  }
  assert.ok(holes > 0);

  const hatched = new RasterBuffer(8, 8);
  paintPanel(hatched, FILL, "hatched", 0, { edge: null });
  for (let y = 1; y < 7; y++) {
    for (let x = 1; x < 7; x++) {
      if (((x + y) & 3) >= 2) assert.equal(alphaAt(hatched, x, y), 0);
      else assert.ok(alphaAt(hatched, x, y) > 0);
    }
  }
});

test("solid forces every cell lit", () => {
  const buf = new RasterBuffer(8, 8);
  paintPanel(buf, FILL, "solid", 0, { edge: null });
  for (let y = 0; y < 8; y++)
    for (let x = 0; x < 8; x++)
      assert.equal(alphaAt(buf, x, y), quantise(expected(0.75, 0, true)));
});

test("the edge frame is a full 1-cell rectangle at 0.5 + 0.25 * intensity", () => {
  for (const [intensity, alpha] of [
    [0, 0.5],
    [1, 0.75],
    [1.5, 0.875],
  ]) {
    const buf = new RasterBuffer(6, 5);
    paintPanel(buf, FILL, "gradient", intensity);
    for (let x = 0; x < 6; x++) {
      assert.equal(alphaAt(buf, x, 0), quantise(alpha));
      assert.equal(alphaAt(buf, x, 4), quantise(alpha));
    }
    for (let y = 0; y < 5; y++) {
      assert.equal(alphaAt(buf, 0, y), quantise(alpha));
      assert.equal(alphaAt(buf, 5, y), quantise(alpha));
    }
  }
  // `edge: null` leaves the border to the field, `edge: n` pins it.
  const bare = new RasterBuffer(6, 5);
  paintPanel(bare, FILL, "gradient", 1, { edge: null });
  assert.notEqual(alphaAt(bare, 3, 0), quantise(0.75));
  const pinned = new RasterBuffer(6, 5);
  paintPanel(pinned, FILL, "gradient", 1, { edge: 0.5 });
  assert.equal(alphaAt(pinned, 3, 0), quantise(0.5));
});

test("paintPanel is deterministic for identical inputs", () => {
  const a = new RasterBuffer(24, 12);
  const b = new RasterBuffer(24, 12);
  paintPanel(a, SWATCH.blue, "gradient", 0.37);
  paintPanel(b, SWATCH.blue, "gradient", 0.37);
  assert.deepEqual([...a.data], [...b.data]);
  // Repainting the same buffer clears the previous frame first.
  paintPanel(a, SWATCH.blue, "dotted", 1);
  paintPanel(a, SWATCH.blue, "gradient", 0.37);
  assert.deepEqual([...a.data], [...b.data]);
});

test("checkbox paints a muted frame when off and a marked field when on", () => {
  const off = new RasterBuffer(8, 8);
  paintCheckbox(off, INK, SWATCH.grey, false);
  assert.equal(alphaAt(off, 0, 0), quantise(0.6));
  assert.equal(alphaAt(off, 7, 7), quantise(0.6));
  assert.equal(alphaAt(off, 4, 4), 0);

  const on = new RasterBuffer(8, 8);
  paintCheckbox(on, INK, SWATCH.grey, true);
  const k = 0.3 + 0.8 * 0.7;
  const marked = new Set(MARK.map(([x, y]) => `${x},${y}`));
  let lit = 0;
  for (let y = 0; y < 8; y++) {
    for (let x = 0; x < 8; x++) {
      if (marked.has(`${x},${y}`)) {
        assert.equal(alphaAt(on, x, y), quantise(0.95));
        continue;
      }
      const isLit = 0.8 > BAYER4[y & 3][x & 3];
      if (isLit) lit++;
      assert.equal(alphaAt(on, x, y), quantise(isLit ? k : k * OFF_TIER));
    }
  }
  // Density 0.8 clears 13 of the 16 thresholds, so the field reads as a solid
  // block with a few cells tinted down rather than punched out.
  assert.equal(BAYER4.flat().filter((t) => 0.8 > t).length, 13);
  assert.equal(lit, 42);
});

test("switch track snaps between an on ramp and an off wash", () => {
  const on = new RasterBuffer(18, 10);
  paintSwitchTrack(on, INK, SWATCH.grey, true);
  const density = 0.25 + 0.75 * (9.5 / 10);
  assert.equal(
    alphaAt(on, 0, 9),
    quantise(0.3 + density * 0.7),
    "bottom row is the densest",
  );
  assert.ok(alphaAt(on, 0, 9) > alphaAt(on, 0, 0));

  const off = new RasterBuffer(18, 10);
  paintSwitchTrack(off, INK, SWATCH.grey, false);
  for (let y = 0; y < 10; y++) {
    for (let x = 0; x < 18; x++) {
      const lit = 0.2 > BAYER4[y & 3][x & 3];
      assert.equal(alphaAt(off, x, y), quantise(lit ? 0.18 : 0.06));
      assert.deepEqual(
        rgbAt(off, x, y),
        SWATCH.grey.map((c) => (c * clamp01(lit ? 0.18 : 0.06) + 0.5) | 0),
      );
    }
  }
});

test("the selected segment is opaque and framed", () => {
  const buf = new RasterBuffer(20, 9);
  paintSegment(buf, INK);
  for (let i = 3; i < buf.data.length; i += 4) assert.equal(buf.data[i], 255);
  // The frame keeps its premultiplied 0.5 ink.
  assert.deepEqual(rgbAt(buf, 10, 0), [119, 119, 119]);
  // The field still ramps downward.
  assert.ok(rgbAt(buf, 10, 7)[0] > rgbAt(buf, 10, 1)[0]);
});

test("separator dissolves toward both ends", () => {
  const buf = new RasterBuffer(40, 1);
  paintSeparator(buf, INK);
  assert.equal(alphaAt(buf, 0, 0), 0, "first cell is always dissolved away");
  const left = alphaAt(buf, 2, 0);
  const middle = alphaAt(buf, 20, 0);
  assert.ok(middle > left);
  assert.equal(middle, quantise(0.35 + 0.45 * clamp01(Math.min(20.5 / 40, 1 - 20.5 / 40) / 0.5)));
  assert.equal(alphaAt(buf, 39, 0), 0);
});

test("rail fades downward and drops invisible rows", () => {
  const buf = new RasterBuffer(1, 16);
  paintRail(buf, INK);
  assert.ok(alphaAt(buf, 0, 0) > alphaAt(buf, 0, 12));
  assert.equal(alphaAt(buf, 0, 15), 0);
});

test("makeIntensity settles on the target and stops its own loop", () => {
  const frames = [];
  const painted = [];
  globalThis.requestAnimationFrame = (fn) => {
    frames.push(fn);
    return frames.length;
  };
  globalThis.cancelAnimationFrame = () => {};
  try {
    const ctrl = makeIntensity((i) => painted.push(i));
    ctrl.enter();
    let steps = 0;
    while (frames.length > 0 && steps < 200) {
      frames.shift()();
      steps++;
    }
    assert.ok(steps < 40, `settled in ${steps} frames`);
    assert.equal(painted.at(-1), 1);
    // Press overshoots, release returns to the hover target.
    ctrl.down();
    while (frames.length > 0) frames.shift()();
    assert.equal(painted.at(-1), 1.5);
    ctrl.up();
    while (frames.length > 0) frames.shift()();
    assert.equal(painted.at(-1), 1);
    ctrl.leave();
    while (frames.length > 0) frames.shift()();
    assert.equal(painted.at(-1), 0);
    // Monotone approach: 16% of the remaining gap per frame.
    assert.ok(painted.length > 8);
  } finally {
    delete globalThis.requestAnimationFrame;
    delete globalThis.cancelAnimationFrame;
  }
});

test("prefersReducedMotion is false without a matchMedia implementation", () => {
  assert.equal(prefersReducedMotion(), false);
});
