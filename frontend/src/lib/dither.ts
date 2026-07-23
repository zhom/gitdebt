/**
 * Ordered-dither engine.
 *
 * Bayer 4x4 ordered dithering (Bayer, 1973) is public-domain math. Every
 * dithered surface in the product is a low-resolution `<canvas>` upscaled with
 * `image-rendering: pixelated`, where per-cell alpha comes from a threshold
 * test against the matrix. The single rule: vary alpha, never shade. The same
 * fill RGB at different alphas reads correctly on any background.
 *
 * This module is pure. It reads no DOM API at import time, so it can be unit
 * tested under `node --test` (see `scripts/dither.test.mjs`).
 */

/** Bayer 4x4 thresholds, 0.03125 ... 0.96875. Sampled as `BAYER4[y & 3][x & 3]`. */
export const BAYER4: number[][] = [
  [0, 8, 2, 10],
  [12, 4, 14, 6],
  [3, 11, 1, 9],
  [15, 7, 13, 5],
].map((row) => row.map((v) => (v + 0.5) / 16));

/** CSS px per dither cell. Identical for every interactive component. */
export const CELL = 2;

/** Unlit cells tint the surface, they never punch a hole. */
export const OFF_TIER = 0.4;

export const clamp01 = (t: number) => (t < 0 ? 0 : t > 1 ? 1 : t);

export type RGB = readonly [number, number, number];
export type Variant = "gradient" | "dotted" | "hatched" | "solid";

/** Foreground ink, as a canvas fill. Mirrors `--ink` in globals.css. */
export const INK: RGB = [237, 237, 237];

/**
 * Brand violet. Textured surfaces are tinted rather than near-white: an ink
 * surface at 2px cells is half dark cells, so dark label text on it fights the
 * texture instead of reading against it.
 */
export const BRAND: RGB = [155, 123, 255];

export type SwatchName =
  | "green"
  | "blue"
  | "purple"
  | "pink"
  | "orange"
  | "red"
  | "grey";

/** Categorical fills. `grey` is documented as "no data". */
export const SWATCH: Record<SwatchName, RGB> = {
  green: [40, 210, 110],
  blue: [53, 143, 243],
  purple: [150, 110, 255],
  pink: [240, 90, 190],
  orange: [255, 150, 50],
  red: [240, 70, 70],
  grey: [92, 92, 100],
};

const MAX_COLS = 960;
const MAX_ROWS = 600;

/** Backing-store size for a css box. */
export const gridFor = (w: number, h: number, cell = CELL) => ({
  cols: cellCount(w, cell, MAX_COLS),
  rows: cellCount(h, cell, MAX_ROWS),
});

function cellCount(size: number, cell: number, cap: number) {
  const raw = Number.isFinite(size) && cell > 0 ? Math.round(size / cell) : 0;
  return Math.max(4, Math.min(cap, raw));
}

type MutableImageData = {
  readonly width: number;
  readonly height: number;
  readonly data: Uint8ClampedArray;
};

function createImage(cols: number, rows: number): ImageData {
  const Ctor = (globalThis as { ImageData?: typeof ImageData }).ImageData;
  if (typeof Ctor === "function") return new Ctor(cols, rows);
  const shim: MutableImageData = {
    width: cols,
    height: rows,
    data: new Uint8ClampedArray(cols * rows * 4),
  };
  return shim as unknown as ImageData;
}

/**
 * Reusable premultiplied RGBA buffer. Callers push it with a single
 * `putImageData` per frame, which is an order of magnitude cheaper than
 * per-cell `fillRect`.
 */
export class RasterBuffer {
  readonly cols: number;
  readonly rows: number;
  readonly image: ImageData;
  readonly data: Uint8ClampedArray;

  constructor(cols: number, rows: number) {
    this.cols = cols;
    this.rows = rows;
    this.image = createImage(cols, rows);
    this.data = this.image.data;
  }

  clear() {
    this.data.fill(0);
  }

  set(x: number, y: number, [r, g, b]: RGB, a: number) {
    const ac = clamp01(a);
    const i = (y * this.cols + x) * 4;
    this.data[i] = (r * ac + 0.5) | 0;
    this.data[i + 1] = (g * ac + 0.5) | 0;
    this.data[i + 2] = (b * ac + 0.5) | 0;
    this.data[i + 3] = (ac * 255 + 0.5) | 0;
  }

  /**
   * Raises a cell's alpha, keeping `fill` as the colour. Only correct when the
   * cell already holds the same fill, which is the case for overlays painted
   * onto a single-fill panel.
   */
  add(x: number, y: number, fill: RGB, a: number) {
    const i = (y * this.cols + x) * 4;
    this.set(x, y, fill, this.data[i + 3] / 255 + a);
  }

  /** Forces every cell opaque, keeping the premultiplied RGB. */
  opaque() {
    for (let i = 3; i < this.data.length; i += 4) this.data[i] = 255;
  }
}

/** Density ramp for a variant at row `y` of `rows`. */
export function densityFor(variant: Variant, y: number, rows: number) {
  if (variant === "gradient") return 0.25 + 0.75 * ((y + 0.5) / rows);
  if (variant === "dotted") return 0.5;
  return 0.75;
}

/**
 * The paint function. `intensity` is 0 at rest, 1 on hover, 1.5 on press: it
 * lowers the threshold (more cells light) and brightens what is already lit.
 */
export function paintPanel(
  buf: RasterBuffer,
  fill: RGB,
  variant: Variant,
  intensity: number,
  opts: { edge?: number | null; matrix?: number[][]; alpha?: number } = {},
) {
  const m = opts.matrix ?? BAYER4;
  const bias = variant === "dotted" ? 0.12 : 0;
  // Beds sit behind content and must stay quiet; controls paint at full
  // strength so their texture reads as a surface.
  const scale = opts.alpha ?? 1;
  buf.clear();
  for (let y = 0; y < buf.rows; y++) {
    const density = densityFor(variant, y, buf.rows);
    for (let x = 0; x < buf.cols; x++) {
      if (variant === "hatched" && ((x + y) & 3) >= 2) continue; // 2-on/2-off, period 4
      const lit =
        variant === "solid" ||
        density > m[y & 3][x & 3] - 0.1 * intensity - bias;
      if (variant === "dotted" && !lit) continue; // real holes
      const k = (0.3 + density * 0.7) * (1 + 0.22 * intensity) * scale;
      buf.set(x, y, fill, lit ? k : k * OFF_TIER);
    }
  }
  const edge =
    opts.edge === null
      ? null
      : (opts.edge ?? clamp01(0.5 + 0.25 * intensity)) * scale;
  if (edge === null) return;
  for (let x = 0; x < buf.cols; x++)
    for (const y of [0, buf.rows - 1]) buf.set(x, y, fill, edge);
  for (let y = 0; y < buf.rows; y++)
    for (const x of [0, buf.cols - 1]) buf.set(x, y, fill, edge);
}

/** Chunky pixel checkmark on the 8x8 cell grid of a `size-4` box at CELL=2. */
export const MARK: ReadonlyArray<readonly [number, number]> = [
  [1, 3],
  [1, 4],
  [2, 4],
  [2, 5],
  [3, 5],
  [3, 6],
  [4, 4],
  [4, 5],
  [5, 3],
  [5, 4],
  [6, 2],
  [6, 3],
];

const MARK_INK: RGB = [245, 245, 248];

/** Checkbox box: unchecked is a 1-cell muted frame, checked is a flat field. */
export function paintCheckbox(
  buf: RasterBuffer,
  fill: RGB,
  muted: RGB,
  checked: boolean,
  matrix: number[][] = BAYER4,
) {
  buf.clear();
  const { cols, rows } = buf;
  if (!checked) {
    for (let x = 0; x < cols; x++) {
      buf.set(x, 0, muted, 0.6);
      buf.set(x, rows - 1, muted, 0.6);
    }
    for (let y = 0; y < rows; y++) {
      buf.set(0, y, muted, 0.6);
      buf.set(cols - 1, y, muted, 0.6);
    }
    return;
  }
  const density = 0.8; // flat, no ramp: 15 of 16 thresholds pass
  const k = 0.3 + density * 0.7; // 0.86
  for (let y = 0; y < rows; y++) {
    for (let x = 0; x < cols; x++) {
      const lit = density > matrix[y & 3][x & 3];
      buf.set(x, y, fill, lit ? k : k * OFF_TIER);
    }
  }
  for (const [x, y] of MARK) {
    if (x < cols && y < rows) buf.set(x, y, MARK_INK, 0.95);
  }
}

/** Switch track. The texture snaps between states, it never crossfades. */
export function paintSwitchTrack(
  buf: RasterBuffer,
  fill: RGB,
  muted: RGB,
  on: boolean,
  matrix: number[][] = BAYER4,
) {
  buf.clear();
  for (let y = 0; y < buf.rows; y++) {
    const density = on ? 0.25 + 0.75 * ((y + 0.5) / buf.rows) : 0.2;
    const k = 0.3 + density * 0.7;
    for (let x = 0; x < buf.cols; x++) {
      const lit = density > matrix[y & 3][x & 3];
      if (on) buf.set(x, y, fill, lit ? k : k * OFF_TIER);
      else buf.set(x, y, muted, lit ? 0.18 : 0.06);
    }
  }
}

/**
 * Selected segment of a segmented control: the button's rest paint, forced
 * opaque so the texture reads as a filled chip rather than a wash.
 */
export function paintSegment(
  buf: RasterBuffer,
  fill: RGB,
  matrix: number[][] = BAYER4,
) {
  paintPanel(buf, fill, "gradient", 0, { edge: 0.5, matrix });
  buf.opaque();
}

/** Decorative rule: a line whose cells dissolve toward both ends. */
export function paintSeparator(
  buf: RasterBuffer,
  fill: RGB,
  matrix: number[][] = BAYER4,
) {
  buf.clear();
  for (let x = 0; x < buf.cols; x++) {
    const t = (x + 0.5) / buf.cols;
    const e = clamp01(Math.min(t, 1 - t) / 0.5);
    const fade = e * e;
    for (let y = 0; y < buf.rows; y++) {
      if (fade <= matrix[y & 3][x & 3]) continue;
      buf.set(x, y, fill, 0.35 + 0.45 * e);
    }
  }
}

/** Active-row rail: a 1-cell column that fades downward. */
export function paintRail(
  buf: RasterBuffer,
  fill: RGB,
  matrix: number[][] = BAYER4,
) {
  buf.clear();
  for (let y = 0; y < buf.rows; y++) {
    const density = 1 - (y + 0.5) / buf.rows;
    const lit = density > matrix[y & 3][0];
    const alpha = lit ? 0.35 + 0.65 * density : 0.12 * density;
    if (alpha <= 0.004) continue;
    for (let x = 0; x < buf.cols; x++) buf.set(x, y, fill, alpha);
  }
}

/** FNV-1a over the seed string. Keeps wave sets stable across renders. */
export function hashSeed(seed: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < seed.length; i++) {
    h ^= seed.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

function mulberry32(state: number) {
  let a = state >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** One sine component of a surface's undulation. */
export type Wave = {
  /** Cycles across the full width. */
  readonly freq: number;
  /** Cycles per second. */
  readonly speed: number;
  readonly phase: number;
  /** Density amplitude. The set sums to `WAVE_AMPLITUDE`. */
  readonly amp: number;
};

/** Total density swing a wave set can produce, at full gain. */
export const WAVE_AMPLITUDE = 0.09;

/**
 * Wave set for a series, derived from `seed` alone so a chart ripples the same
 * way on every render and across a remount.
 */
export function makeWaves(seed: string, count = 3): Wave[] {
  const random = mulberry32(hashSeed(seed));
  const raw: { freq: number; speed: number; phase: number; weight: number }[] =
    [];
  let total = 0;
  for (let i = 0; i < count; i++) {
    const weight = 1 / (i + 1.6);
    total += weight;
    raw.push({
      freq: 0.8 + i * 1.1 + random() * 1.4,
      speed: (i % 2 === 0 ? 1 : -1) * (0.09 + random() * 0.14),
      phase: random() * Math.PI * 2,
      weight,
    });
  }
  return raw.map((w) => ({
    freq: w.freq,
    speed: w.speed,
    phase: w.phase,
    amp: (w.weight / total) * WAVE_AMPLITUDE,
  }));
}

/**
 * Signed density offset at column fraction `u` and time `t` in seconds.
 * Bounded by `WAVE_AMPLITUDE`.
 */
export function waveOffset(
  waves: readonly Wave[],
  u: number,
  t: number,
): number {
  let sum = 0;
  for (const w of waves) {
    sum += w.amp * Math.sin((w.freq * u + w.speed * t) * Math.PI * 2 + w.phase);
  }
  return sum;
}

/** Breathing disc stamped into a painted panel at the pointer. */
export type PulseSpec = {
  /** Centre in cell coordinates. */
  x: number;
  y: number;
  /** Ring radius in cells. */
  radius: number;
  /** 0..1 envelope. Below `PULSE_FLOOR` the stamp is skipped entirely. */
  energy: number;
};

/** Envelope below which a pulse contributes nothing visible. */
export const PULSE_FLOOR = 0.004;

/**
 * Ceiling on the alpha a pulse may add. The label sits on this surface, so the
 * bed can brighten but never close on the text contrast.
 */
export const PULSE_MAX_ALPHA = 0.22;

/**
 * Adds a dithered ring plus a soft core to an already-painted single-fill
 * buffer. Only the ring's bounding box is walked, so cost is independent of
 * the panel size.
 */
export function stampPulse(
  buf: RasterBuffer,
  fill: RGB,
  pulse: PulseSpec,
  matrix: number[][] = BAYER4,
) {
  if (pulse.energy <= PULSE_FLOOR || pulse.radius <= 0) return;
  const width = Math.max(1.5, pulse.radius * 0.45);
  const reach = pulse.radius + width * 2;
  const x0 = Math.max(0, Math.floor(pulse.x - reach));
  const x1 = Math.min(buf.cols - 1, Math.ceil(pulse.x + reach));
  const y0 = Math.max(0, Math.floor(pulse.y - reach));
  const y1 = Math.min(buf.rows - 1, Math.ceil(pulse.y + reach));
  for (let y = y0; y <= y1; y++) {
    const dy = y + 0.5 - pulse.y;
    for (let x = x0; x <= x1; x++) {
      const dx = x + 0.5 - pulse.x;
      const d = Math.sqrt(dx * dx + dy * dy);
      if (d > reach) continue;
      const band = (d - pulse.radius) / width;
      const ring = Math.exp(-band * band);
      const core = 0.4 * clamp01(1 - d / pulse.radius);
      const p = (ring + core) * pulse.energy;
      if (p <= matrix[y & 3][x & 3] * 0.85) continue;
      buf.add(x, y, fill, Math.min(PULSE_MAX_ALPHA, p * 0.3));
    }
  }
}

export const prefersReducedMotion = () =>
  typeof matchMedia === "function" &&
  matchMedia("(prefers-reduced-motion: reduce)").matches;

export type IntensityController = {
  enter(): void;
  leave(): void;
  down(): void;
  up(): void;
  repaint(): void;
  stop(): void;
};

/** Everything a surface needs to paint one frame. */
export type SurfaceMotion = {
  /** 0 rest, 1 hover, 1.5 press. */
  intensity: number;
  /** Accumulated animation seconds. Frozen while the loop is idle. */
  time: number;
  /** Pointer position in normalised host coordinates. */
  px: number;
  py: number;
  /** 0..1 pulse envelope, independent of `intensity` so it can outlive a press. */
  pulse: number;
};

export type SurfaceController = {
  enter(px?: number, py?: number): void;
  move(px: number, py: number): void;
  leave(): void;
  down(px?: number, py?: number): void;
  up(): void;
  /** Off-screen hosts stop burning frames without losing their state. */
  setVisible(visible: boolean): void;
  repaint(): void;
  stop(): void;
};

const FRAME_MS = 1000 / 60;
const INTENSITY_SETTLED = 0.005;
/** Per-frame lerp fractions at 60Hz, made frame-rate independent below. */
const INTENSITY_RATE = 0.16;
const PULSE_RATE = 0.1;

/**
 * Hover/press/pulse state for one canvas.
 *
 * A single self-throttling rAF drives every animated channel. It stops as soon
 * as intensity settles and no continuous motion is owed, pauses while the host
 * is off-screen, and never starts at all under reduced motion, where every
 * transition collapses to a single paint at the target.
 */
export function makeSurfaceMotion(
  paint: (m: SurfaceMotion) => void,
  opts: { continuous?: boolean } = {},
): SurfaceController {
  const m: SurfaceMotion = {
    intensity: 0,
    time: 0,
    px: 0.5,
    py: 0.5,
    pulse: 0,
  };
  let targetIntensity = 0;
  let targetPulse = 0;
  let hovered = false;
  let visible = true;
  let raf = 0;
  let last = 0;
  const reduce = prefersReducedMotion();
  const continuous = (opts.continuous ?? false) && !reduce;

  const running = () =>
    Math.abs(targetIntensity - m.intensity) >= INTENSITY_SETTLED ||
    Math.abs(targetPulse - m.pulse) >= PULSE_FLOOR ||
    (continuous && hovered);

  const tick = (now?: number) => {
    raf = 0;
    const stamp = typeof now === "number" ? now : last + FRAME_MS;
    const dt =
      last === 0 ? FRAME_MS / 1000 : Math.min(0.05, (stamp - last) / 1000);
    last = stamp;
    const frames = (dt * 1000) / FRAME_MS;
    const di = targetIntensity - m.intensity;
    m.intensity =
      Math.abs(di) < 0.01
        ? targetIntensity
        : m.intensity + di * (1 - Math.pow(1 - INTENSITY_RATE, frames));
    const dp = targetPulse - m.pulse;
    m.pulse =
      Math.abs(dp) < PULSE_FLOOR
        ? targetPulse
        : m.pulse + dp * (1 - Math.pow(1 - PULSE_RATE, frames));
    if (continuous) m.time += dt;
    // Landing exactly on the target is what lets the loop terminate; a residue
    // below the settle epsilon would otherwise persist until the next gesture.
    const done = !running();
    if (done) {
      m.intensity = targetIntensity;
      m.pulse = targetPulse;
    }
    paint(m);
    if (done) last = 0;
    else schedule();
  };

  function schedule() {
    if (raf || reduce || !visible || !running()) return;
    raf = requestAnimationFrame(tick);
  }

  const to = (intensity: number) => {
    targetIntensity = intensity;
    targetPulse = hovered && continuous ? 1 : 0;
    if (reduce) {
      m.intensity = intensity;
      m.pulse = 0;
      m.time = 0;
      paint(m);
      return;
    }
    schedule();
  };

  const at = (px?: number, py?: number) => {
    if (typeof px === "number" && typeof py === "number") {
      m.px = clamp01(px);
      m.py = clamp01(py);
    }
  };

  return {
    enter: (px, py) => {
      hovered = true;
      at(px, py);
      to(1);
    },
    move: (px, py) => {
      at(px, py);
      schedule();
    },
    leave: () => {
      hovered = false;
      to(0);
    },
    down: (px, py) => {
      at(px, py);
      to(1.5);
    },
    up: () => to(hovered ? 1 : 0),
    setVisible: (next) => {
      visible = next;
      if (!visible) {
        if (raf) cancelAnimationFrame(raf);
        raf = 0;
        last = 0;
        return;
      }
      if (running()) schedule();
    },
    repaint: () => paint(m),
    stop: () => {
      if (raf) cancelAnimationFrame(raf);
      raf = 0;
    },
  };
}

/**
 * Intensity-only view of `makeSurfaceMotion`, for surfaces that own no pointer
 * position: 16% of the remaining gap per frame, self-terminating under 0.01
 * (~470ms settle, tau ~96ms).
 */
export function makeIntensity(paint: (i: number) => void): IntensityController {
  const c = makeSurfaceMotion((m) => paint(m.intensity));
  return {
    enter: () => c.enter(),
    leave: () => c.leave(),
    down: () => c.down(),
    up: () => c.up(),
    repaint: () => c.repaint(),
    stop: () => c.stop(),
  };
}
