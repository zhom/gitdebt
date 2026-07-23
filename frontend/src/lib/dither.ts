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

/**
 * Exponential lerp toward a hover/press target: 16% of the remaining gap per
 * frame, self-terminating under 0.01 (~470ms settle, tau ~96ms). At most one
 * rAF is ever in flight, and none at all under reduced motion.
 */
export function makeIntensity(paint: (i: number) => void): IntensityController {
  let i = 0;
  let target = 0;
  let raf = 0;
  let hovered = false;
  const reduce = prefersReducedMotion();
  const tick = () => {
    const d = target - i;
    if (Math.abs(d) < 0.01) {
      i = target;
      paint(i);
      raf = 0;
      return;
    }
    i += d * 0.16;
    paint(i);
    raf = requestAnimationFrame(tick);
  };
  const to = (t: number) => {
    target = t;
    if (reduce) {
      i = t;
      paint(i);
    } else if (!raf) {
      raf = requestAnimationFrame(tick);
    }
  };
  return {
    enter: () => {
      hovered = true;
      to(1);
    },
    leave: () => {
      hovered = false;
      to(0);
    },
    down: () => to(1.5),
    up: () => to(hovered ? 1 : 0),
    repaint: () => paint(i),
    stop: () => {
      if (raf) cancelAnimationFrame(raf);
      raf = 0;
    },
  };
}
