"use client";

import { INK, paintRail, type RGB } from "@/lib/dither";
import { cn } from "@/lib/utils";
import { useRasterCanvas } from "@/components/ui/dither-surface";

export type DitherRailProps = {
  fill?: RGB;
  className?: string;
};

/**
 * The 2px marker that flags the active row. One cell wide, dissolving downward,
 * so a selected row reads as textured rather than as a solid paint stripe.
 */
export function DitherRail({ fill = INK, className }: DitherRailProps) {
  const canvasRef = useRasterCanvas((buf) => paintRail(buf, fill), { cols: 1 });
  return (
    <span
      aria-hidden="true"
      className={cn(
        "pointer-events-none absolute inset-y-1.5 left-0 w-[2px]",
        className,
      )}
    >
      <canvas
        ref={canvasRef}
        className="absolute inset-0 h-full w-full [image-rendering:pixelated]"
      />
    </span>
  );
}
