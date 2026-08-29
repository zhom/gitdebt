import * as React from "react";

import { cn } from "@/lib/utils";
import { Tick } from "@/components/ui/marks";

/**
 * The working controls, in one file because they are one system.
 *
 * They replace four separate canvas-backed controls that each painted their own
 * texture, and they keep those components' prop shapes so call sites changed
 * their import and nothing else. What they drop is the `fill` prop: a control's
 * colour is not a call site's decision.
 *
 * Every one of them is a rectangle with square corners. That is the whole
 * shape language — the only chamfer on this site belongs to a panel and to the
 * one primary action, so a control can never be mistaken for either.
 */

/** The shared field treatment: a drawn edge, paper under the pointer. */
export const CONTROL =
  "border border-rule-strong bg-transparent text-ink transition-[background-color,border-color,color] duration-[--duration-ui] hover:bg-paper hover:border-ink-3 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-signal";

/** A layer that genuinely floats above the sheet, and so genuinely casts. */
export const POPOVER =
  "lifted border border-rule-strong bg-paper text-ink";

/* ── Switch ─────────────────────────────────────────────────────────────── */

export type SwitchProps = Omit<
  React.ButtonHTMLAttributes<HTMLButtonElement>,
  "onChange" | "value"
> & {
  checked: boolean;
  onCheckedChange?: (checked: boolean) => void;
};

/**
 * A slide gauge: the knob travels the track and the track inks in behind it.
 * Square, because everything here is square.
 */
export const Switch = React.forwardRef<HTMLButtonElement, SwitchProps>(
  ({ checked, onCheckedChange, className, disabled, ...props }, ref) => (
    <button
      ref={ref}
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onCheckedChange?.(!checked)}
      className={cn(
        "relative inline-flex h-5 w-9 shrink-0 items-center border transition-[background-color,border-color] duration-[--duration-ui] outline-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-signal disabled:opacity-40",
        checked
          ? "border-signal bg-signal"
          : "border-rule-strong bg-transparent hover:border-ink-3",
        className,
      )}
      {...props}
    >
      <span
        aria-hidden="true"
        className={cn(
          "block h-3 w-3 transition-transform duration-[--duration-ui] motion-reduce:transition-none",
          checked ? "translate-x-[1.25rem] bg-paper" : "translate-x-[0.25rem] bg-ink-3",
        )}
      />
    </button>
  ),
);
Switch.displayName = "Switch";

/* ── Checkbox ───────────────────────────────────────────────────────────── */

export type CheckboxProps = Omit<
  React.ButtonHTMLAttributes<HTMLButtonElement>,
  "onChange" | "value"
> & {
  checked: boolean;
  onCheckedChange?: (checked: boolean) => void;
};

export const Checkbox = React.forwardRef<HTMLButtonElement, CheckboxProps>(
  ({ checked, onCheckedChange, className, disabled, ...props }, ref) => (
    <button
      ref={ref}
      type="button"
      role="checkbox"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onCheckedChange?.(!checked)}
      className={cn(
        "inline-flex size-4 shrink-0 items-center justify-center border transition-[background-color,border-color,color] duration-[--duration-ui] outline-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-signal disabled:opacity-40",
        checked
          ? "border-signal bg-signal text-signal-ink"
          : "border-rule-strong text-transparent hover:border-ink-3",
        className,
      )}
      {...props}
    >
      <Tick size={11} strokeWidth={2} />
    </button>
  ),
);
Checkbox.displayName = "Checkbox";

/* ── Segmented ──────────────────────────────────────────────────────────── */

export type SegmentedOption<T extends string> = {
  value: T;
  label: React.ReactNode;
  badge?: React.ReactNode;
  disabled?: boolean;
};

export type SegmentedProps<T extends string> = {
  value: T;
  options: ReadonlyArray<SegmentedOption<T>>;
  onValueChange: (value: T) => void;
  role?: "tablist" | "radiogroup";
  "aria-label"?: string;
  className?: string;
  itemClassName?: string;
};

/**
 * A segmented control whose selection is marked by a rule that slides beneath
 * the active segment.
 *
 * The indicator is one element translated across the track, not a border
 * toggled per segment, so the selection genuinely travels between two real
 * positions. It is a 2px rule in drafting red: the same mark the drawing uses
 * to say "this one", at the same weight.
 */
export function Segmented<T extends string>({
  value,
  options,
  onValueChange,
  role = "radiogroup",
  className,
  itemClassName,
  ...props
}: SegmentedProps<T>) {
  const index = Math.max(
    0,
    options.findIndex((option) => option.value === value),
  );
  const isTabs = role === "tablist";

  return (
    <div
      role={role}
      aria-label={props["aria-label"]}
      className={cn(
        "relative inline-grid auto-cols-fr grid-flow-col border border-rule-strong",
        className,
      )}
    >
      {options.map((option) => {
        const selected = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            role={isTabs ? "tab" : "radio"}
            {...(isTabs
              ? { "aria-selected": selected }
              : { "aria-checked": selected })}
            disabled={option.disabled}
            onClick={() => onValueChange(option.value)}
            className={cn(
              "relative z-10 inline-flex min-h-10 items-center justify-center gap-1.5 px-3 font-draft text-[0.75rem] tracking-[0.06em] uppercase transition-colors duration-[--duration-ui] outline-none focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-signal disabled:opacity-40",
              selected ? "text-ink" : "text-ink-3 hover:text-ink",
              itemClassName,
            )}
          >
            {option.label}
            {option.badge}
          </button>
        );
      })}

      {/* The indicator. It sits on the track's bottom edge and travels; it is
          never a second border drawn around the active item. */}
      <span
        aria-hidden="true"
        className="pointer-events-none absolute bottom-0 left-0 h-[2px] bg-signal transition-transform duration-[--duration-ui] ease-[--ease-land] motion-reduce:transition-none"
        style={{
          width: `${100 / options.length}%`,
          transform: `translateX(${index * 100}%)`,
        }}
      />
    </div>
  );
}

/* ── Separator ──────────────────────────────────────────────────────────── */

export type SeparatorProps = { className?: string };

/**
 * A rule between two regions.
 *
 * It exists only where there genuinely are two regions. It is not available as
 * ornament beside a label or under a heading — that line measures nothing, and
 * a line that measures nothing does not belong on this sheet.
 */
export function Separator({ className }: SeparatorProps) {
  return <hr className={cn("rule-drawn", className)} />;
}
