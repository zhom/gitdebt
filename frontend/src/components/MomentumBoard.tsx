import { useMemo } from "react";

import { CAPTION, DATUM, FIELD } from "@/components/style-tokens";
import { rates, trendOf, type MomentumRow, type Trend } from "@/lib/momentum";
import { formatCompact } from "@/lib/star-insights";
import { cn } from "@/lib/utils";

export type { MomentumRow, Trend };

/**
 * The ranking, drawn as a dimension stack.
 *
 * Every row is one measurement taken from a shared datum at the left of the
 * pace column: a hairline running out to the window the board ranks by, with a
 * heavier line over it running out to the next window in, both on one per-day
 * scale. A short heavy line past a long hairline is a repository spiking today;
 * a hairline with almost no heavy line over it is one coasting on last week.
 *
 * It is SVG, not a canvas, and it does not animate. What was here before was a
 * single canvas painting fifty rippling textured bars at sixty frames a second,
 * which meant the entire ranking was invisible to a screen reader, to an agent,
 * and to any browser that had not run the script yet. The numbers now sit in
 * the markup, the bars are drawn from the same numbers, and nothing on this
 * board waits for JavaScript to exist.
 */
export type MomentumVariant = "full" | "compact";

export type MomentumBoardProps = {
  rows: MomentumRow[];
  variant?: MomentumVariant;
};

/**
 * Track and grid, per variant.
 *
 * Both class strings are literals rather than composed at runtime, because
 * Tailwind's scanner reads source text. The pace column is hidden below `sm`,
 * so each variant states the narrow grid first and the wide one after it.
 */
const GRID: Record<MomentumVariant, string> = {
  full: "grid grid-cols-[2.5rem_minmax(0,1fr)_3.75rem_3.75rem_3.75rem_4.25rem] sm:grid-cols-[2.75rem_8rem_minmax(0,1fr)_4rem_4rem_4rem_4.5rem]",
  compact:
    "grid grid-cols-[2.5rem_minmax(0,1fr)_4rem_4.25rem] sm:grid-cols-[2.75rem_5.5rem_minmax(0,1fr)_4.25rem_4.5rem]",
};

const TREND_LABEL: Record<Trend, string> = {
  rising: "climbing, faster today than its monthly pace",
  steady: "steady, today matches its monthly pace",
  fading: "cooling, slower today than its monthly pace",
  unknown: "ranked in only one window, so there is nothing to compare",
};

/**
 * Every compact row is ranked monthly, so `trendOf`'s long side is always the
 * month there while its short side is whichever tighter window ranked the repo.
 * The copy names the month and leaves the short side unquoted rather than
 * claiming a day the row may not have.
 */
const TREND_LABEL_COMPACT: Record<Trend, string> = {
  rising: "climbing, its recent pace beats its monthly pace",
  steady: "steady, its recent pace matches its monthly pace",
  fading: "cooling, its recent pace trails its monthly pace",
  unknown: "ranked in only one window, so there is nothing to compare",
};

const nf = new Intl.NumberFormat("en-US");
const fmt = (value: number | null) => (value === null ? "—" : nf.format(value));
const clamp01 = (value: number) => Math.min(1, Math.max(0, value));

type Band = {
  /** 0..1 extent from the rate of the window this board ranks by. */
  share: number;
  /** 0..1 extent of the next window in, on the same per-day scale. */
  head: number;
};

/**
 * The direction mark: a terminator, not a glyph borrowed from a font.
 *
 * Direction is carried by the shape — an arrowhead up, an arrowhead down, a
 * rule for level, a point for nothing-to-compare — and never by colour alone.
 * Each row also states its direction in words to a screen reader.
 */
function TrendMark({ trend }: { trend: Trend }) {
  return (
    <svg
      width="9"
      height="9"
      viewBox="0 0 9 9"
      aria-hidden="true"
      focusable="false"
      className="shrink-0 text-ink-3"
    >
      {trend === "rising" && <path d="M4.5 1 8 7H1z" fill="currentColor" />}
      {trend === "fading" && <path d="M4.5 8 1 2h7z" fill="currentColor" />}
      {trend === "steady" && (
        <path
          d="M1 4.5h7"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
        />
      )}
      {trend === "unknown" && <circle cx="4.5" cy="4.5" r="1.25" fill="currentColor" />}
    </svg>
  );
}

/**
 * One row's measurement. The datum tick at the left is the origin every bar on
 * the board is measured from, which is what makes fifty rows comparable at a
 * glance; the tick at the end of the hairline is that measurement's terminator.
 */
function PaceBar({ band }: { band: Band }) {
  const body = Math.max(0.6, band.share * 100);
  const head = band.head * 100;
  return (
    <svg
      viewBox="0 0 100 12"
      preserveAspectRatio="none"
      width="100%"
      height="12"
      aria-hidden="true"
      focusable="false"
      className="block"
    >
      {/* `preserveAspectRatio="none"` lets the track fill its column at any
          width; `vector-effect` keeps every stroke at its true drafting weight
          while the x axis stretches under it. */}
      <g strokeLinecap="butt">
        <line
          x1="0.4"
          y1="1.5"
          x2="0.4"
          y2="10.5"
          stroke="var(--rule-strong)"
          strokeWidth="1"
          vectorEffect="non-scaling-stroke"
        />
        <line
          x1="0.4"
          y1="6"
          x2={body}
          y2="6"
          stroke="var(--ink-3)"
          strokeWidth="1"
          vectorEffect="non-scaling-stroke"
        />
        <line
          x1={body}
          y1="3"
          x2={body}
          y2="9"
          stroke="var(--rule-strong)"
          strokeWidth="1"
          vectorEffect="non-scaling-stroke"
        />
        {head > 0.6 && (
          <line
            x1="0.4"
            y1="6"
            x2={head}
            y2="6"
            stroke="var(--ink)"
            strokeWidth="2"
            vectorEffect="non-scaling-stroke"
          />
        )}
      </g>
    </svg>
  );
}

export function MomentumBoard({ rows, variant = "full" }: MomentumBoardProps) {
  const compact = variant === "compact";

  const bands = useMemo<Band[]>(() => {
    const all = rows.map(rates);
    // One rule, read off whichever window the board ranks by: the hairline is
    // that window's rate, and the heavier line over it is the next window in.
    // The full board ranks weekly, so week over day; the compact board ranks
    // monthly, so month over week. Pairing month with day instead would leave
    // most compact rows headless — the daily board ranks an order of magnitude
    // fewer repositories.
    const body = all.map((r) => (compact ? r.r30 : r.r7));
    const tip = all.map((r) => (compact ? r.r7 : r.r1));
    const fastestBody = Math.max(1, ...body.map((r) => r ?? 0));
    const fastestTip = Math.max(1, ...tip.map((r) => r ?? 0));
    // Square root: star rates are heavy-tailed, and a linear map leaves
    // everything below the leader an indistinguishable stub.
    return rows.map((_, index) => ({
      share: clamp01(Math.sqrt((body[index] ?? 0) / fastestBody)),
      head: clamp01(Math.sqrt((tip[index] ?? 0) / fastestTip)),
    }));
  }, [rows, compact]);

  const grid = GRID[variant];
  const head = cn(FIELD, "px-2 py-2 text-right");

  return (
    <div className="border border-rule-strong bg-paper">
      <div className={cn(grid, "items-center border-b border-rule-strong")}>
        <span className={cn(head, "text-left")}>#</span>
        <span className={cn(head, "hidden text-left sm:block")}>
          {compact ? "Pace" : "Momentum"}
        </span>
        <span className={cn(head, "text-left")}>Repository</span>
        {!compact && <span className={head}>+1d</span>}
        {!compact && <span className={head}>+7d</span>}
        <span className={head}>+30d</span>
        <span className={head}>Stars</span>
      </div>

      <ol>
        {rows.map((row, index) => {
          const trend = trendOf(row);
          return (
            <li key={row.repo}>
              <a
                href={`/${row.repo}`}
                className={cn(
                  grid,
                  "min-h-11 items-center border-b border-rule text-ink-2 outline-none transition-colors duration-[--duration-ui] last:border-b-0 hover:bg-table hover:text-ink focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-signal",
                )}
              >
                <span className={cn(DATUM, "px-2 text-ink-3")}>{index + 1}</span>
                <span className="hidden px-2 sm:block">
                  <PaceBar band={bands[index]} />
                </span>
                <span className="flex min-w-0 items-center gap-2 px-2">
                  <TrendMark trend={trend} />
                  <span className="min-w-0 truncate font-mono text-[0.8125rem] text-ink">
                    {row.repo}
                  </span>
                  <span className="sr-only">
                    , {compact ? TREND_LABEL_COMPACT[trend] : TREND_LABEL[trend]}
                  </span>
                </span>
                {!compact && (
                  <span className={cn(DATUM, "px-2 text-right text-ink")}>
                    {fmt(row.d1)}
                  </span>
                )}
                {!compact && (
                  <span className={cn(DATUM, "px-2 text-right text-ink")}>
                    {fmt(row.d7)}
                  </span>
                )}
                {/* Compact leads with the window it ranks by, so that column
                    carries the emphasis the full board gives its tighter
                    windows. Compact numerals are what fit the column. */}
                <span
                  className={cn(
                    DATUM,
                    "px-2 text-right",
                    compact ? "text-ink" : "text-ink-3",
                  )}
                >
                  {compact
                    ? row.d30 === null
                      ? "—"
                      : `+${formatCompact(row.d30)}`
                    : fmt(row.d30)}
                </span>
                <span className={cn(DATUM, "px-2 text-right text-ink-3")}>
                  {compact ? formatCompact(row.stars) : nf.format(row.stars)}
                </span>
              </a>
            </li>
          );
        })}
      </ol>

      {rows.length === 0 && (
        <p className={cn(CAPTION, "px-4 py-6")}>
          No repositories are ranked in this window yet.
        </p>
      )}
    </div>
  );
}
