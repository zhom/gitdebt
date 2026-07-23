"use client";

import * as React from "react";

import { BRAND, SWATCH, paintSwitchTrack, type RGB } from "@/lib/dither";
import { cn } from "@/lib/utils";
import { CONTROL_FOCUS, useRasterCanvas } from "@/components/ui/dither-surface";

/** Track is 36x20 css px at CELL = 2. */
const TRACK_COLS = 18;
const TRACK_ROWS = 10;

export type DitherSwitchProps = Omit<
  React.ButtonHTMLAttributes<HTMLButtonElement>,
  "onChange" | "type" | "role" | "aria-checked"
> & {
  checked: boolean;
  onCheckedChange?: (checked: boolean) => void;
  fill?: RGB;
};

export const DitherSwitch = React.forwardRef<
  HTMLButtonElement,
  DitherSwitchProps
>(
  (
    {
      checked,
      onCheckedChange,
      fill = BRAND,
      className,
      disabled,
      onClick,
      ...props
    },
    ref,
  ) => {
    const canvasRef = useRasterCanvas(
      (buf) => paintSwitchTrack(buf, fill, SWATCH.grey, checked),
      { cols: TRACK_COLS, rows: TRACK_ROWS },
    );
    return (
      <button
        ref={ref}
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        data-press="off"
        onClick={(event) => {
          onClick?.(event);
          if (!event.defaultPrevented) onCheckedChange?.(!checked);
        }}
        className={cn(
          "inline-flex size-10 items-center justify-center rounded-md",
          "disabled:pointer-events-none disabled:opacity-40",
          CONTROL_FOCUS,
          className,
        )}
        {...props}
      >
        <span className="relative inline-flex h-5 w-9 items-center overflow-hidden rounded-[3px]">
          <canvas
            ref={canvasRef}
            aria-hidden="true"
            width={TRACK_COLS}
            height={TRACK_ROWS}
            className="pointer-events-none absolute inset-0 h-full w-full [image-rendering:pixelated]"
          />
          <span
            className={cn(
              "relative size-3.5 rounded-[2px] bg-foreground transition-transform duration-150 motion-reduce:transition-none",
              checked ? "translate-x-[19px]" : "translate-x-[3px]",
            )}
          />
        </span>
      </button>
    );
  },
);
DitherSwitch.displayName = "DitherSwitch";
