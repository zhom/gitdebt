import { BAYER4, OFF_TIER } from "@/lib/dither";

export type DitherCellPatternProps = {
  id: string;
  /** Flat coverage, 0..1. Cells above the matrix threshold are fully lit. */
  density?: number;
  fill: string;
  /** SVG user units per cell. */
  cell?: number;
};

/**
 * An SVG `<pattern>` whose cells come from the same 4x4 threshold test the
 * canvas surfaces use. A flat density is the one case `<pattern>` can express
 * honestly: a ramp needs per-cell rects, which the canvas renderers emit.
 */
export function DitherCellPattern({
  id,
  density = 0.5,
  fill,
  cell = 2,
}: DitherCellPatternProps) {
  const cells: { x: number; y: number; opacity: number }[] = [];
  for (let y = 0; y < 4; y++) {
    for (let x = 0; x < 4; x++) {
      const lit = density > BAYER4[y][x];
      const alpha = (0.3 + density * 0.7) * (lit ? 1 : OFF_TIER);
      if (alpha <= 0.02) continue;
      cells.push({ x, y, opacity: Math.round(alpha * 100) / 100 });
    }
  }
  const size = cell * 4;
  return (
    <pattern
      id={id}
      width={size}
      height={size}
      patternUnits="userSpaceOnUse"
    >
      {cells.map((c) => (
        <rect
          key={`${c.x}-${c.y}`}
          x={c.x * cell}
          y={c.y * cell}
          width={cell}
          height={cell}
          fill={fill}
          fillOpacity={c.opacity}
        />
      ))}
    </pattern>
  );
}
