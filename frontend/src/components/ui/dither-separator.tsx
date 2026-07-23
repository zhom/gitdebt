"use client";

import { INK, paintSeparator, type RGB } from "@/lib/dither";
import { cn } from "@/lib/utils";
import { useRasterCanvas } from "@/components/ui/dither-surface";

export type DitherSeparatorProps = {
  fill?: RGB;
  className?: string;
};

/** Decorative rule: a hairline that dissolves toward both ends. */
export function DitherSeparator({ fill = INK, className }: DitherSeparatorProps) {
  const canvasRef = useRasterCanvas((buf) => paintSeparator(buf, fill), {
    rows: 1,
  });
  return (
    <div
      role="separator"
      aria-orientation="horizontal"
      className={cn("dither-fallback relative h-[2px] w-full", className)}
    >
      <canvas
        ref={canvasRef}
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 h-full w-full [image-rendering:pixelated]"
      />
    </div>
  );
}
