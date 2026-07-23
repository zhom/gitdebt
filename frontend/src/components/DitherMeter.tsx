"use client";

import {
  BAYER4,
  INK,
  OFF_TIER,
  SWATCH,
  clamp01,
  type RGB,
  type RasterBuffer,
} from "@/lib/dither";
import { cn } from "@/lib/utils";
import { useRasterCanvas } from "@/components/ui/dither-surface";

/**
 * Progress and magnitude rail.
 *
 * The filled span ramps its density left to right and is threshold-tested per
 * cell, so the bar dissolves at its head instead of ending on a hard edge. The
 * empty span is a low-density tint of the same grid, never a flat block.
 */
export function paintMeter(
  buf: RasterBuffer,
  ratio: number,
  fill: RGB,
  matrix: number[][] = BAYER4,
) {
  buf.clear();
  const filled = Math.round(buf.cols * clamp01(ratio));
  for (let y = 0; y < buf.rows; y++) {
    for (let x = 0; x < buf.cols; x++) {
      const threshold = matrix[y & 3][x & 3];
      if (x < filled) {
        const density = 0.35 + 0.65 * ((x + 0.5) / filled);
        const k = 0.3 + density * 0.7;
        const lit = density > threshold;
        buf.set(x, y, fill, lit ? k : k * OFF_TIER);
      } else {
        const lit = 0.25 > threshold;
        buf.set(x, y, SWATCH.grey, lit ? 0.2 : 0.06);
      }
    }
  }
}

export type DitherMeterProps = {
  /** Filled fraction, clamped to 0..1. */
  ratio: number;
  fill?: RGB;
  className?: string;
  /** Set to expose the bar as a progressbar rather than as decoration. */
  label?: string;
  /** Percent, 0..100, for the exposed `aria-valuenow`. */
  percent?: number;
};

export function DitherMeter({
  ratio,
  fill = INK,
  className,
  label,
  percent,
}: DitherMeterProps) {
  const canvasRef = useRasterCanvas((buf) => paintMeter(buf, ratio, fill));
  const semantics = label
    ? ({
        role: "progressbar" as const,
        "aria-label": label,
        "aria-valuemin": 0,
        "aria-valuemax": 100,
        "aria-valuenow": percent ?? Math.round(clamp01(ratio) * 100),
      })
    : { "aria-hidden": true as const };
  return (
    <div
      {...semantics}
      className={cn(
        "dither-fallback relative h-2 w-full overflow-hidden rounded-[2px]",
        className,
      )}
    >
      <canvas
        ref={canvasRef}
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 h-full w-full [image-rendering:pixelated]"
      />
    </div>
  );
}
