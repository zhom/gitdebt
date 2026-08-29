import { type JSX } from "react";

import {
  BODY,
  CAPTION,
  HEADING,
  MEASURE,
} from "@/components/style-tokens";
import {
  coverageLabel,
  historyFreshness,
  seriesOpen,
  sourceDetail,
  sourceLabel,
  sourceStroke,
  stateLabel,
  type HistoryFreshness,
  type HistorySnapshot,
} from "@/lib/history-freshness";
import { cn } from "@/lib/utils";

/**
 * The drawing's title block: where this star series came from, said in the only
 * three terms gitdebt can actually observe — SOURCE, COVERAGE DATE, STATE.
 *
 * It is not a verification badge and it must never become one. gitdebt stores
 * star timestamps and an opaque event id — no actors, no stargazer profiles —
 * so nothing here is, or could be, a claim about who starred a repository or
 * about the owner's conduct. Every sentence's subject is gitdebt's read access.
 *
 * It also never states a count. An archive series records star actions and not
 * unstars, so it can exceed the repository's own star total; a "shows N of M"
 * gap would therefore be confidently wrong on exactly the repositories where it
 * would be most eye-catching. That reasoning belongs to `history-freshness.ts`,
 * which is where every user-facing string in this component comes from. This
 * file letters them and writes none of its own.
 *
 * The specimen line beside the block is the drawing's own way of saying how
 * certain an edge is, and it carries two readings:
 *
 *   PATTERN  = which source. Solid is an object line — an edge that was
 *              measured. Dashed is a construction line — real, derived. Fine
 *              dots are a line whose subject could not be measured at all. The
 *              patterns are fixed per state (`sourceStroke`), never scaled by
 *              anything the series measures, because a dash gap read off
 *              coverage would be the completeness score this module refuses to
 *              publish.
 *   INK      = which series, on a multi-series surface (`fill`). Hue never
 *              carries source; the pattern does, and the text says both.
 *
 * A stopped series terminates on a drawn end tick; an open one simply runs on.
 * The svg is `aria-hidden`, and every fact it encodes is also present as text,
 * so nothing on this block depends on it being seen — or on script running at
 * all. There is no animation that content waits for.
 */

export type ProvenanceVariant = "panel" | "inline" | "explainer";

/**
 * A series ink, as the comparison surfaces hand it over. Declared structurally
 * rather than imported so this module stays free of the multi-series chart's
 * own dependencies; any `[r, g, b]` satisfies it.
 */
export type SeriesInk = readonly [number, number, number];

export type SeriesProvenanceProps = {
  /** Subset of GET /api/repos/{owner}/{repo}/analyze. Null, undefined, or a
   *  pre-field payload classifies as "unknown" rather than throwing. Omit
   *  entirely for variant="explainer". */
  snapshot?: HistorySnapshot | null;
  /** owner/repo the series belongs to. Used only in copy; omit on explainer. */
  slug?: string;
  /** Default "panel". */
  variant?: ProvenanceVariant;
  /** Series ink on multi-series surfaces (pass comparisonColors()[slug]).
   *  When omitted the line is drawn in graphite. Ink never encodes source on a
   *  multi-series surface: ink = which series, pattern = which source. */
  fill?: SeriesInk;
  /** aria-labelledby target for variant="panel"; default "series-provenance". */
  headingId?: string;
  className?: string;
};

export function SeriesProvenance({
  snapshot,
  slug,
  variant = "panel",
  fill,
  headingId = "series-provenance",
  className,
}: SeriesProvenanceProps): JSX.Element {
  if (variant === "explainer") {
    return <ProvenanceExplainer className={className} />;
  }

  const freshness = historyFreshness(snapshot);

  if (variant === "inline") {
    return (
      <ProvenanceInline
        freshness={freshness}
        fill={fill}
        slug={slug}
        className={className}
      />
    );
  }

  return (
    <ProvenanceTitleBlock
      freshness={freshness}
      fill={fill}
      slug={slug}
      headingId={headingId}
      className={className}
    />
  );
}

/* -------------------------------------------------------------------------- *
 * The specimen line
 * -------------------------------------------------------------------------- */

/** Where a spliced specimen changes hand, as a fraction of its drawn length. */
const SPLICE_AT = 0.62;

type SpecimenProps = {
  freshness: HistoryFreshness;
  fill?: SeriesInk;
  /** Drawn length in svg user units; the element scales to its CSS width. */
  length: number;
  className?: string;
};

/**
 * How a draughtsman would letter this series' line.
 *
 * One fixed viewBox, so the element reserves its own height and no caller
 * needs a min-height guard. Geometry is computed here and emitted complete —
 * the only thing that ever moves is a dash offset along a length this function
 * already knows, so a tab that never animates still shows the finished line.
 */
function SeriesSpecimen({ freshness, fill, length, className }: SpecimenProps) {
  const dash = sourceStroke(freshness);
  const open = seriesOpen(freshness);
  const measured = freshness.state !== "restricted" && freshness.state !== "unknown";
  const ink = fill
    ? `rgb(${fill[0]} ${fill[1]} ${fill[2]})`
    : measured
      ? "var(--ink)"
      : "var(--ink-3)";
  const mid = Math.round(length * SPLICE_AT);
  const spliced = freshness.state === "spliced";
  const solidTo = spliced ? mid : dash === "" ? length : 0;

  return (
    <svg
      viewBox={`0 0 ${length} 12`}
      width={length}
      height="12"
      className={cn("block h-3 shrink-0 overflow-visible", className)}
      aria-hidden="true"
      focusable="false"
    >
      {/* The measured half. It inks in along its own length; the geometry is
          final before the animation runs, so nothing here waits to exist. */}
      {solidTo > 0 && (
        <line
          x1="0"
          y1="6"
          x2={solidTo}
          y2="6"
          stroke={ink}
          strokeWidth="1.5"
          strokeLinecap="round"
          className="inks-in"
          style={{ ["--draw-length" as string]: String(solidTo) }}
          vectorEffect="non-scaling-stroke"
        />
      )}

      {/* The derived half, drawn as the construction line it is. A derived
          line is square-ended, the way a drafted dash is; the line for a
          series that could not be measured at all is dotted, so its caps are
          round and each mark reads as a point rather than a short dash. */}
      {dash !== "" && (
        <line
          x1={solidTo}
          y1="6"
          x2={length}
          y2="6"
          stroke={ink}
          strokeWidth="1.5"
          strokeDasharray={dash}
          strokeLinecap={measured ? "butt" : "round"}
          vectorEffect="non-scaling-stroke"
        />
      )}

      {/* The join. A spliced line changes method at a real point, and the
          drawing marks that point rather than leaving it to be inferred. */}
      {spliced && (
        <line
          x1={mid}
          y1="1"
          x2={mid}
          y2="11"
          stroke={ink}
          strokeWidth="1"
          strokeLinecap="round"
          vectorEffect="non-scaling-stroke"
        />
      )}

      {/* The end tick. A series that stopped terminates on a drawn edge; one
          that is still receiving points simply runs on. */}
      {!open && measured && (
        <line
          x1={length - 0.75}
          y1="1"
          x2={length - 0.75}
          y2="11"
          stroke={ink}
          strokeWidth="1.5"
          strokeLinecap="round"
          vectorEffect="non-scaling-stroke"
        />
      )}
    </svg>
  );
}

/* -------------------------------------------------------------------------- *
 * Variants
 * -------------------------------------------------------------------------- */

/** The three fields, in the order a title block states them. */
function fields(freshness: HistoryFreshness): [string, string][] {
  return [
    ["Source", sourceLabel(freshness)],
    ["Coverage", coverageLabel(freshness)],
    ["State", stateLabel(freshness)],
  ];
}

function ProvenanceTitleBlock({
  freshness,
  fill,
  slug,
  headingId,
  className,
}: {
  freshness: HistoryFreshness;
  fill?: SeriesInk;
  slug?: string;
  headingId: string;
  className?: string;
}) {
  return (
    <aside
      className={cn(
        "cut-edge p-5 [--pad-x:1.25rem] [--pad-y:1.25rem]",
        className,
      )}
      aria-labelledby={headingId}
    >
      {/* The block's own head. The rule under it separates two real regions of
          the block — the title from the fields — which is the only reason a
          rule is allowed to exist here. */}
      <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-3 border-b border-rule pb-3.5">
        <h2 id={headingId} className={HEADING}>
          How this series was read
          {slug ? <span className="sr-only"> for {slug}</span> : null}
        </h2>
        <SeriesSpecimen freshness={freshness} fill={fill} length={120} />
      </div>

      {/* The fields. Every row lands on the same two columns regardless of how
          long its value runs, which is what makes it read as a block and not
          as three stacked pairs.
          The cells stretch rather than align on a baseline: a baseline-aligned
          row lets the two boxes start at different heights, and the dividing
          rule then arrives at the label in one place and at the value in
          another — a rule drawn in two pieces, which is worse than no rule.
          The label's extra half-step is what sets its cap line level with the
          value's first line, since the two are lettered at different leading. */}
      <dl className="grid grid-cols-[minmax(4.5rem,auto)_1fr] gap-x-6 pt-2">
        {fields(freshness).map(([term, value], index) => (
          <div key={term} className="contents">
            <dt
              className={cn(
                "drafted pt-2.5 pb-2",
                index > 0 && "border-t border-rule",
              )}
            >
              {term}
            </dt>
            <dd
              className={cn(
                "py-2 font-mono text-[0.75rem] leading-[1.5] text-ink-2",
                index > 0 && "border-t border-rule",
              )}
            >
              {value}
            </dd>
          </div>
        ))}
      </dl>

      <p className={cn(BODY, MEASURE, "mt-4")}>{sourceDetail(freshness)}</p>
    </aside>
  );
}

function ProvenanceInline({
  freshness,
  fill,
  slug,
  className,
}: {
  freshness: HistoryFreshness;
  fill?: SeriesInk;
  slug?: string;
  className?: string;
}) {
  // Unknown says one thing and stops. A coverage phrase next to "not
  // established" reads as a hedge about a date we simply do not have.
  const text =
    freshness.state === "unknown"
      ? sourceLabel(freshness)
      : [
          sourceLabel(freshness),
          coverageLabel(freshness),
          ...(seriesOpen(freshness) ? [] : [stateLabel(freshness)]),
        ].join(" · ");

  return (
    <div className={cn("flex items-center gap-2.5", className)}>
      <SeriesSpecimen freshness={freshness} fill={fill} length={64} />
      <span className={CAPTION}>
        {slug ? <span className="sr-only">{slug}: </span> : null}
        {text}
      </span>
    </div>
  );
}

/**
 * The two-source key.
 *
 * It describes the system rather than any one repository, which is why its copy
 * lives here instead of in `history-freshness.ts` and why it is the correct
 * degradation for a route with no subject: it makes no claim that could be
 * wrong about a repository, because it makes no claim about one at all.
 */
function ProvenanceExplainer({ className }: { className?: string }) {
  const exact = historyFreshness({
    history_complete: true,
    history_kind: "current_stargazers",
    history_approximate: false,
  });
  const archive = historyFreshness({
    history_complete: true,
    history_kind: "public_star_actions",
    history_approximate: true,
  });
  const spliced = historyFreshness({
    history_complete: true,
    history_kind: "stargazers_then_activity",
    history_approximate: true,
  });

  // Terms come from `sourceLabel`, so the key and the chart caption cannot drift
  // apart: one wording, one module, exactly as the fact rows use.
  const rows: { freshness: HistoryFreshness; definition: string }[] = [
    {
      freshness: exact,
      // Says what the restriction does to gitdebt, not who it exempts. The key
      // is read by owners too, and "served to admins" reads to an owner as a
      // capability they have; they do not, because gitdebt reads GitHub with
      // its own application credentials no matter who is signed in.
      definition:
        "Exact — one point per star, with its own timestamp. Since July 2026 GitHub serves this list only to applications that administer the repository, and gitdebt is not one of them, so a series read this way stops on a fixed date and no sign-in restarts it.",
    },
    {
      freshness: archive,
      definition:
        "Rebuilt from historical star data. Star actions are recorded and unstars are not, so it reads as an attention signal rather than a net star count. It keeps flowing for every public repository.",
    },
    {
      freshness: spliced,
      definition:
        "One line, two methods, joined on a fixed date. The exact list runs up to the join and star activity continues after it, so the tail counts actions rather than current stargazers and does not record every star. Every chart built this way names the date it changes method.",
    },
  ];

  return (
    <section className={className} aria-labelledby="series-provenance-key">
      <h2 id="series-provenance-key" className={HEADING}>
        Where a star series comes from
      </h2>

      {/* Three parallel columns on one grid. The specimen-and-name row is a
          shared row across all three, so the column whose source name runs to
          two lines cannot push its neighbours' definitions out of step —
          whatever the copy happens to be, every definition starts on the same
          line. */}
      <dl className="mt-6 grid gap-x-10 gap-y-8 md:grid-cols-3 md:grid-rows-[auto_1fr]">
        {rows.map((row) => (
          <div
            key={row.freshness.state}
            className="grid gap-y-3 md:row-span-2 md:grid-rows-subgrid"
          >
            <dt className="flex flex-col justify-end gap-2.5">
              <SeriesSpecimen freshness={row.freshness} length={120} />
              <span className="font-mono text-[0.75rem] leading-[1.5] text-ink">
                {sourceLabel(row.freshness)}
              </span>
            </dt>
            <dd className={cn(BODY, "m-0")}>{row.definition}</dd>
          </div>
        ))}
      </dl>

      <p className={cn(CAPTION, MEASURE, "mt-8")}>
        Every chart names its source and its coverage date. None of them states
        how much of a history is missing: an archive series counts re-stars and
        can exceed a repository's own star total, so that figure would be wrong
        exactly where it looks most precise.
      </p>
    </section>
  );
}
