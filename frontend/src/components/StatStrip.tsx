import type { ReactNode } from "react";

import { cn } from "@/lib/utils";
import { EYEBROW, KPI, PANEL } from "@/components/style-tokens";

export type StatStripItem = {
  label: ReactNode;
  value: ReactNode;
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
  4: "sm:grid-cols-2 lg:grid-cols-4",
};

/**
 * The one treatment for a divided run of numbers. It replaces the hand-rolled
 * `border-y`-only strips: a single card, one padding value, hairline dividers.
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
        PANEL,
        "grid divide-y divide-border/40 sm:divide-x sm:divide-y-0",
        COLUMNS[cols],
        cols === 4 ? "sm:divide-y sm:divide-x lg:divide-y-0" : "",
        className,
      )}
    >
      {items.map((item, index) => (
        <div
          key={item.key ?? (typeof item.label === "string" ? item.label : index)}
          className="min-w-0 p-3.5"
        >
          <dt className={EYEBROW}>{item.label}</dt>
          <dd className={cn("mt-2", KPI)}>{item.value}</dd>
        </div>
      ))}
    </dl>
  );
}
