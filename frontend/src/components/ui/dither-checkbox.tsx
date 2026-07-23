"use client";

import * as React from "react";

import { BRAND, SWATCH, paintCheckbox, type RGB } from "@/lib/dither";
import { cn } from "@/lib/utils";
import { CONTROL_FOCUS, useRasterCanvas } from "@/components/ui/dither-surface";

/** `size-4` box at CELL = 2. */
const BOX_CELLS = 8;

export type DitherCheckboxProps = Omit<
  React.ButtonHTMLAttributes<HTMLButtonElement>,
  "onChange" | "type" | "role" | "aria-checked"
> & {
  checked: boolean;
  onCheckedChange?: (checked: boolean) => void;
  /** Canvas fill for the checked field. */
  fill?: RGB;
};

export const DitherCheckbox = React.forwardRef<
  HTMLButtonElement,
  DitherCheckboxProps
>(
  (
    {
      checked,
      onCheckedChange,
      fill = BRAND,
      className,
      children,
      disabled,
      onClick,
      ...props
    },
    ref,
  ) => {
    const canvasRef = useRasterCanvas(
      (buf) => paintCheckbox(buf, fill, SWATCH.grey, checked),
      { cols: BOX_CELLS, rows: BOX_CELLS },
    );
    return (
      <button
        ref={ref}
        type="button"
        role="checkbox"
        aria-checked={checked}
        disabled={disabled}
        data-press="off"
        onClick={(event) => {
          onClick?.(event);
          if (!event.defaultPrevented) onCheckedChange?.(!checked);
        }}
        className={cn(
          "inline-flex min-h-10 items-center gap-2 rounded-md text-left font-mono text-[13px] text-foreground",
          "disabled:pointer-events-none disabled:opacity-40",
          CONTROL_FOCUS,
          className,
        )}
        {...props}
      >
        <span className="relative size-4 shrink-0">
          <canvas
            ref={canvasRef}
            aria-hidden="true"
            width={BOX_CELLS}
            height={BOX_CELLS}
            className="pointer-events-none absolute inset-0 h-full w-full [image-rendering:pixelated]"
          />
        </span>
        {children ? <span className="relative">{children}</span> : null}
      </button>
    );
  },
);
DitherCheckbox.displayName = "DitherCheckbox";
