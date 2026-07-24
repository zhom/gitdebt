const HEX = "0123456789ABCDEF";

const clamp01 = (value: number) => Math.max(0, Math.min(1, value));

/** A bounded glyph grid derived from Pretext's measured monospace metrics. */
export function asciiGrid(
  width: number,
  height: number,
  glyphWidth: number,
  lineHeight: number,
): { cols: number; rows: number; cellWidth: number; cellHeight: number } {
  const cellWidth = Math.max(9, glyphWidth * 1.55);
  const cellHeight = Math.max(12, lineHeight * 1.18);
  return {
    cols: Math.max(12, Math.min(180, Math.ceil(Math.max(1, width) / cellWidth))),
    rows: Math.max(8, Math.min(80, Math.ceil(Math.max(1, height) / cellHeight))),
    cellWidth,
    cellHeight,
  };
}

/** Stable hexdump character selection with a very slow animated substitution. */
export function asciiGlyph(x: number, y: number, phase: number): string {
  const step = Math.floor(Math.max(0, phase) * 0.45);
  const hash = Math.imul(x + 11, 0x45d9f3b) ^ Math.imul(y + 17, 0x119de1f3) ^ step;
  return HEX[(hash >>> 0) & 15];
}

/**
 * Mix the sampled logo mask with a Bayer-screen-friendly density field.
 * Solid areas stay legible, edges dissolve, and the travelling wave only
 * modulates the texture—it never moves or deforms the logo itself.
 */
export function asciiDensity(
  maskAlpha: number,
  x: number,
  y: number,
  cols: number,
  rows: number,
  phase: number,
): number {
  const u = (x + 0.5) / Math.max(1, cols);
  const v = (y + 0.5) / Math.max(1, rows);
  const wave = 0.5 + 0.5 * Math.sin(u * Math.PI * 3.2 - phase * 1.4 + v * 2.1);
  const pulse = 0.92 + 0.08 * Math.sin(phase * 0.8);
  const edgeFade = clamp01(Math.min(u, 1 - u, v, 1 - v) * 8);
  const verticalInk = 0.42 + v * 0.38;
  return clamp01(maskAlpha * (verticalInk + wave * 0.2) * pulse * (0.55 + edgeFade * 0.45));
}

