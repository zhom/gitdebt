"use client";

import * as React from "react";

import { BRAND, paintSegment, type RGB } from "@/lib/dither";
import { cn } from "@/lib/utils";
import { CONTROL_FOCUS, useRasterCanvas } from "@/components/ui/dither-surface";

function SegmentSurface({ fill }: { fill: RGB }) {
  const canvasRef = useRasterCanvas((buf) => paintSegment(buf, fill));
  return (
    <canvas
      ref={canvasRef}
      aria-hidden="true"
      className="pointer-events-none absolute inset-0 -z-10 h-full w-full [image-rendering:pixelated]"
    />
  );
}

export type DitherSegmentedOption<T extends string> = {
  value: T;
  label: React.ReactNode;
  /** Optional trailing count. */
  badge?: React.ReactNode;
  disabled?: boolean;
};

export type DitherSegmentedProps<T extends string> = {
  value: T;
  options: ReadonlyArray<DitherSegmentedOption<T>>;
  onValueChange: (value: T) => void;
  /** `tablist` for view switches, `radiogroup` for exclusive settings. */
  role?: "tablist" | "radiogroup";
  "aria-label"?: string;
  "aria-labelledby"?: string;
  className?: string;
  itemClassName?: string;
  fill?: RGB;
};

/**
 * Exclusive choice where the selected item is distinguished by texture, not by
 * a lightness shift in the label.
 */
export function DitherSegmented<T extends string>({
  value,
  options,
  onValueChange,
  role = "tablist",
  className,
  itemClassName,
  fill = BRAND,
  ...aria
}: DitherSegmentedProps<T>) {
  const refs = React.useRef<Array<HTMLButtonElement | null>>([]);
  const itemRole = role === "tablist" ? "tab" : "radio";

  const move = (from: number, step: number) => {
    const total = options.length;
    for (let i = 1; i <= total; i++) {
      const next = (from + step * i + total * i) % total;
      const option = options[next];
      if (option.disabled) continue;
      onValueChange(option.value);
      refs.current[next]?.focus();
      return;
    }
  };

  return (
    <div
      role={role}
      aria-label={aria["aria-label"]}
      aria-labelledby={aria["aria-labelledby"]}
      className={cn("inline-flex flex-wrap items-center gap-1.5", className)}
      onKeyDown={(event) => {
        const index = options.findIndex((option) => option.value === value);
        if (index < 0) return;
        if (event.key === "ArrowRight" || event.key === "ArrowDown") {
          event.preventDefault();
          move(index, 1);
        } else if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
          event.preventDefault();
          move(index, -1);
        }
      }}
    >
      {options.map((option, index) => {
        const selected = option.value === value;
        return (
          <button
            key={option.value}
            ref={(node) => {
              refs.current[index] = node;
            }}
            type="button"
            role={itemRole}
            data-press="off"
            disabled={option.disabled}
            tabIndex={selected ? 0 : -1}
            aria-selected={itemRole === "tab" ? selected : undefined}
            aria-checked={itemRole === "radio" ? selected : undefined}
            onClick={() => onValueChange(option.value)}
            className={cn(
              "relative isolate inline-flex min-h-9 items-center gap-1.5 overflow-hidden rounded-md border px-2.5 py-1.5 font-mono text-[12px] transition-colors duration-150",
              "disabled:pointer-events-none disabled:opacity-40",
              selected
                ? "border-transparent text-foreground"
                : "border-border/60 text-muted-foreground hover:text-foreground",
              CONTROL_FOCUS,
              itemClassName,
            )}
          >
            {selected ? <SegmentSurface fill={fill} /> : null}
            <span className="relative">{option.label}</span>
            {option.badge !== undefined && option.badge !== null ? (
              <span className="relative text-[10px] tabular-nums opacity-70">
                {option.badge}
              </span>
            ) : null}
          </button>
        );
      })}
    </div>
  );
}
