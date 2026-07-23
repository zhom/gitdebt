import assert from "node:assert/strict";
import { test } from "node:test";

import {
  BAYER4,
  PULSE_MAX_ALPHA,
  RasterBuffer,
  WAVE_AMPLITUDE,
  hashSeed,
  makeSurfaceMotion,
  makeWaves,
  paintPanel,
  stampPulse,
  waveOffset,
} from "../src/lib/dither.ts";

const FILL = [155, 123, 255];

function alphaAt(buf, x, y) {
  return buf.data[(y * buf.cols + x) * 4 + 3] / 255;
}

function withFakeFrames(run) {
  const frames = [];
  globalThis.requestAnimationFrame = (fn) => {
    frames.push(fn);
    return frames.length;
  };
  globalThis.cancelAnimationFrame = () => {};
  try {
    return run(frames);
  } finally {
    delete globalThis.requestAnimationFrame;
    delete globalThis.cancelAnimationFrame;
  }
}

test("hashSeed is stable and spreads distinct seeds", () => {
  assert.equal(hashSeed("stars"), hashSeed("stars"));
  assert.notEqual(hashSeed("stars"), hashSeed("commits"));
  assert.ok(Number.isInteger(hashSeed("")));
});

test("makeWaves is deterministic per seed and budgets its amplitude", () => {
  const a = makeWaves("owner/repo:stars");
  const b = makeWaves("owner/repo:stars");
  assert.deepEqual(a, b);
  assert.equal(a.length, 3);
  assert.notDeepEqual(a, makeWaves("owner/repo:commits"));
  const total = a.reduce((sum, w) => sum + w.amp, 0);
  assert.ok(Math.abs(total - WAVE_AMPLITUDE) < 1e-12);
  // Descending amplitude: the first wave carries the readable swell.
  assert.ok(a[0].amp > a[1].amp && a[1].amp > a[2].amp);
});

test("waveOffset stays inside the amplitude budget and moves with time", () => {
  const waves = makeWaves("seed");
  for (let i = 0; i <= 200; i++) {
    const v = waveOffset(waves, (i % 21) / 20, i * 0.037);
    assert.ok(Math.abs(v) <= WAVE_AMPLITUDE + 1e-12);
  }
  assert.notEqual(waveOffset(waves, 0.4, 0), waveOffset(waves, 0.4, 1.7));
  assert.equal(waveOffset(waves, 0.4, 2.5), waveOffset(waves, 0.4, 2.5));
  assert.equal(waveOffset([], 0.5, 3), 0);
});

test("stampPulse only brightens, never past the legibility ceiling", () => {
  const before = new RasterBuffer(40, 20);
  paintPanel(before, FILL, "gradient", 1);
  const after = new RasterBuffer(40, 20);
  paintPanel(after, FILL, "gradient", 1);
  stampPulse(after, FILL, { x: 20, y: 10, radius: 6, energy: 1 });

  let raised = 0;
  for (let y = 0; y < 20; y++) {
    for (let x = 0; x < 40; x++) {
      const delta = alphaAt(after, x, y) - alphaAt(before, x, y);
      assert.ok(delta >= -1 / 255, `cell ${x},${y} darkened`);
      assert.ok(delta <= PULSE_MAX_ALPHA + 1 / 255, `cell ${x},${y} overshot`);
      if (delta > 1 / 255) raised++;
    }
  }
  assert.ok(raised > 0, "a pulse must light cells");
});

test("stampPulse is a no-op below the envelope floor and outside its reach", () => {
  const buf = new RasterBuffer(40, 20);
  paintPanel(buf, FILL, "gradient", 0);
  const snapshot = [...buf.data];
  stampPulse(buf, FILL, { x: 20, y: 10, radius: 6, energy: 0 });
  assert.deepEqual([...buf.data], snapshot);
  stampPulse(buf, FILL, { x: 20, y: 10, radius: 0, energy: 1 });
  assert.deepEqual([...buf.data], snapshot);

  // A pulse at one corner leaves the far corner untouched.
  stampPulse(buf, FILL, { x: 1, y: 1, radius: 3, energy: 1 });
  const far = (19 * 40 + 39) * 4;
  assert.deepEqual([...buf.data.slice(far, far + 4)], snapshot.slice(far, far + 4));
});

test("stampPulse dithers its ring rather than filling a disc", () => {
  const buf = new RasterBuffer(40, 20);
  stampPulse(buf, FILL, { x: 20, y: 10, radius: 6, energy: 1 });
  let lit = 0;
  let dark = 0;
  for (let y = 4; y <= 16; y++) {
    for (let x = 14; x <= 26; x++) {
      if (alphaAt(buf, x, y) > 0) lit++;
      else dark++;
    }
  }
  assert.ok(lit > 0 && dark > 0, `ring was solid (${lit} lit, ${dark} dark)`);
  // Every lit cell cleared its own Bayer threshold.
  for (let y = 0; y < 20; y++)
    for (let x = 0; x < 40; x++)
      if (alphaAt(buf, x, y) > 0) assert.ok(BAYER4[y & 3][x & 3] < 1);
});

test("surface motion settles, pulses while hovered, and stops itself", () => {
  withFakeFrames((frames) => {
    const painted = [];
    const c = makeSurfaceMotion((m) => painted.push({ ...m }), {
      continuous: true,
    });
    c.enter(0.25, 0.75);
    let steps = 0;
    while (frames.length > 0 && steps < 60) {
      frames.shift()();
      steps++;
    }
    // Hover holds the loop open, so it never drains on its own.
    assert.equal(steps, 60);
    const settled = painted.at(-1);
    assert.equal(settled.intensity, 1);
    assert.equal(settled.px, 0.25);
    assert.equal(settled.py, 0.75);
    assert.ok(settled.pulse > 0.9, `pulse rose to ${settled.pulse}`);
    assert.ok(settled.time > 0.9, `time advanced ${settled.time}s`);

    c.down();
    frames.splice(0).forEach((fn) => fn());
    assert.ok(painted.at(-1).intensity > 1);

    c.leave();
    steps = 0;
    while (frames.length > 0 && steps < 400) {
      frames.shift()();
      steps++;
    }
    assert.ok(steps < 200, `leave settled in ${steps} frames`);
    assert.equal(painted.at(-1).intensity, 0);
    assert.equal(painted.at(-1).pulse, 0);
    assert.equal(frames.length, 0, "the loop must stop when idle");
  });
});

test("an off-screen surface schedules no frames until it returns", () => {
  withFakeFrames((frames) => {
    const painted = [];
    const c = makeSurfaceMotion((m) => painted.push(m.intensity), {
      continuous: true,
    });
    c.setVisible(false);
    c.enter(0.5, 0.5);
    c.move(0.6, 0.6);
    assert.equal(frames.length, 0);
    assert.equal(painted.length, 0);
    c.setVisible(true);
    assert.equal(frames.length, 1);
  });
});

test("a settled surface repaints on pointer move without a new frame", () => {
  withFakeFrames((frames) => {
    const c = makeSurfaceMotion(() => {});
    c.move(0.3, 0.3);
    assert.equal(frames.length, 0, "idle non-continuous surfaces stay idle");
  });
});
