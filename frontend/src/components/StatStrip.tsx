import type { ReactNode } from "react";

import { CAPTION, FIELD, FIGURE } from "@/components/style-tokens";
import { cn } from "@/lib/utils";

/**
 * The field block, and the strip it lines up in.
 *
 * A field block is the drawing's smallest complete statement: a FIELD label
 * naming what was measured, a FIGURE carrying the measurement, and an optional
 * CAPTION saying what the figure is of. Nothing else goes in one.
 *
 * The reason it is a component rather than three class names is alignment. Put
 * four of these side by side and the longest label decides where every figure
 * lands, which is the ragged parallel row this site is not allowed to ship.
 * `FIELD_ROWS` gives the run three rows and each cell claims all three with
 * `grid-rows-subgrid`, so the label line, the figure line and the caption line
 * share one baseline across every cell however long any single string is. Where
 * subgrid is unavailable the cells simply stack as they always did — the
 * alignment degrades, nothing breaks or disappears.
 */

/** The three rows a run of field blocks is laid on. */
export const FIELD_ROWS = "grid-rows-[auto_auto_auto]";

/** What one cell claims of that run. */
export const FIELD_CELL = "row-span-3 grid grid-rows-subgrid min-w-0";

export type FieldBlockProps = {
  label: ReactNode;
  value: ReactNode;
  caption?: ReactNode;
  /** `dl-cell` emits `<dt>`/`<dd>`, for a block inside a `<dl>` run. */
  as?: "div" | "dl-cell";
  /** Drafting red on the figure. Only ever a value the drawing is measuring. */
  measured?: boolean;
  className?: string;
};

export function FieldBlock({
  label,
  value,
  caption,
  as = "div",
  measured = false,
  className,
}: FieldBlockProps) {
  const Label = as === "dl-cell" ? "dt" : "p";
  const Value = as === "dl-cell" ? "dd" : "p";
  const Caption = as === "dl-cell" ? "dd" : "p";
  return (
    <div className={cn(FIELD_CELL, className)}>
      <Label className={FIELD}>{label}</Label>
      <Value
        className={cn(
          FIGURE,
          "mt-2.5 min-w-0 truncate",
          measured ? "text-signal" : "text-ink",
        )}
      >
        {value}
      </Value>
      {/* The third row exists on the run whether or not this cell fills it, so
          an absent caption leaves the cell beside it exactly where it was. */}
      {caption ? <Caption className={cn(CAPTION, "mt-2")}>{caption}</Caption> : null}
    </div>
  );
}

export type StatStripItem = {
  label: ReactNode;
  value: ReactNode;
  caption?: ReactNode;
  /** Falls back to the label when it is a plain string. */
  key?: string;
};

export type StatStripProps = {
  items: readonly StatStripItem[];
  /** Column count from the `sm` breakpoint up. Defaults to the item count. */
  columns?: 2 | 3 | 4;
  className?: string;
  "aria-label"?: string;
};

const COLUMNS: Record<2 | 3 | 4, string> = {
  2: "sm:grid-cols-2",
  3: "sm:grid-cols-3",
  4: "sm:grid-cols-4",
};

/**
 * A run of measured fields, enclosed by one drawn frame and divided by rules
 * that each separate two real cells. There is no rule on the outside of the
 * run — that edge is the frame's job, and two lines doing one job is how a
 * drawing turns into a wireframe.
 */
export function StatStrip({
  items,
  columns,
  className,
  ...aria
}: StatStripProps) {
  const cols = columns ?? (Math.min(4, Math.max(2, items.length)) as 2 | 3 | 4);
  return (
    <dl
      aria-label={aria["aria-label"]}
      className={cn(
        "grid divide-y divide-rule border border-rule-strong bg-paper sm:divide-x sm:divide-y-0",
        FIELD_ROWS,
        COLUMNS[cols],
        className,
      )}
    >
      {items.map((item, index) => (
        <FieldBlock
          key={item.key ?? (typeof item.label === "string" ? item.label : index)}
          as="dl-cell"
          label={item.label}
          value={item.value}
          caption={item.caption}
          className="p-4"
        />
      ))}
    </dl>
  );
}
