"use client";

import { useId, type JSX } from "react";
import { motion, useReducedMotion } from "motion/react";

import { DitherCellPattern } from "@/components/DitherCellPattern";
import {
  BODY,
  CAPTION,
  EYEBROW,
  HEADING,
  MEASURE,
  PANEL,
  SECTION_ACTION,
} from "@/components/style-tokens";
import { useInView } from "@/components/ui/use-in-view";
import { SWATCH, type RGB } from "@/lib/dither";
import {
  coverageLabel,
  historyFreshness,
  seriesOpen,
  sourceDensity,
  sourceDetail,
  sourceLabel,
  stateLabel,
  type HistoryFreshness,
  type HistorySnapshot,
} from "@/lib/history-freshness";
import { cn } from "@/lib/utils";

/**
 * Where a star series came from, said in the only three terms gitdebt can
 * actually observe: SOURCE, COVERAGE DATE, STATE.
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
 * which is where every user-facing string in this component comes from.
 *
 * The mark carries three readings at once:
 *   - DENSITY  = which source (a fixed constant per state, never derived from
 *                data, because a density read off coverage would be a
 *                completeness score wearing a texture),
 *   - HUE      = which series, on a multi-series surface (`fill`), falling back
 *                to a source-keyed categorical swatch when there is only one,
 *   - APERTURE = the state. A series that still receives points spans the full
 *                band and dissolves at its trailing edge under a slow scan; a
 *                series that stopped ends early against a hard rule and does
 *                not move at all.
 *
 * Under `prefers-reduced-motion` the scan is dropped and the aperture still
 * reads, so nothing is lost — the MomentumBoard rule. The svg is decoration:
 * it is `aria-hidden`, and every fact it encodes is also present as text.
 */

export type ProvenanceVariant = "panel" | "inline" | "explainer";

export type SeriesProvenanceProps = {
  /** Subset of GET /api/repos/{owner}/{repo}/analyze. Null, undefined, or a
   *  pre-field payload classifies as "unknown" rather than throwing. Omit
   *  entirely for variant="explainer". */
  snapshot?: HistorySnapshot | null;
  /** owner/repo the series belongs to. Used only in copy; omit on explainer. */
  slug?: string;
  /** Default "panel". */
  variant?: ProvenanceVariant;
  /** Series hue on multi-series surfaces (pass comparisonColors()[slug]).
   *  When omitted the source-keyed swatch is used. Hue never encodes source on
   *  a multi-series surface: hue = which series, density = which source. */
  fill?: RGB;
  /** {apiBase}/auth/github/start?return_to=… . Omit to render no action. */
  signInHref?: string;
  /** aria-labelledby target for variant="panel"; default "series-provenance". */
  headingId?: string;
  className?: string;
};

/**
 * Categorical, source-keyed, and stable — assigned by source, never cycled by
 * index. `grey` is the documented "no data" fill, which is what a restricted or
 * unestablished series honestly is. `--accent` is reserved for focus and
 * selection and is not spent here.
 */
const SOURCE_FILL: Record<HistoryFreshness["state"], RGB> = {
  exact_current: SWATCH.blue,
  exact_frozen: SWATCH.blue,
  archive: SWATCH.purple,
  restricted: SWATCH.grey,
  unknown: SWATCH.grey,
};

/** Band geometry, in svg user units. */
const BAND = { width: 160, height: 24, stop: 115, fade: 24, scan: 18 };

/** Seconds for one pass of the scan. */
const SCAN_SECONDS = 3.6;

const rgbCss = ([r, g, b]: RGB) => `rgb(${r} ${g} ${b})`;

export function SeriesProvenance({
  snapshot,
  slug,
  variant = "panel",
  fill,
  signInHref,
  headingId = "series-provenance",
  className,
}: SeriesProvenanceProps): JSX.Element {
  if (variant === "explainer") {
    return <ProvenanceExplainer className={className} />;
  }

  const freshness = historyFreshness(snapshot);
  const resolved = fill ?? SOURCE_FILL[freshness.state];

  if (variant === "inline") {
    return (
      <ProvenanceInline
        freshness={freshness}
        fill={resolved}
        slug={slug}
        className={className}
      />
    );
  }

  return (
    <ProvenancePanel
      freshness={freshness}
      fill={resolved}
      slug={slug}
      signInHref={signInHref}
      headingId={headingId}
      className={className}
    />
  );
}

/* -------------------------------------------------------------------------- *
 * The mark
 * -------------------------------------------------------------------------- */

type BandProps = {
  freshness: HistoryFreshness;
  fill: RGB;
  /** Rendered CSS width. The viewBox is fixed, so geometry never changes. */
  width: number;
  height: number;
};

/**
 * One fixed-viewBox svg, so it reserves its own height and no caller needs a
 * `min-h` guard. It owns its own IntersectionObserver through `useInView`,
 * which is what stops it painting off-screen and in a hidden tab.
 */
function ProvenanceBand({ freshness, fill, width, height }: BandProps) {
  const reduceMotion = useReducedMotion();
  const [ref, inView] = useInView<SVGSVGElement>();
  const raw = useId();
  const id = raw.replaceAll(":", "");

  const open = seriesOpen(freshness);
  const density = sourceDensity(freshness);
  const known = freshness.state !== "unknown";
  const color = rgbCss(fill);
  // Reduced motion drops the scan entirely; off-screen parks it at its start.
  // Both settle to one deterministic frame, and the aperture still reads.
  const scanning = open && !reduceMotion;
  const running = scanning && inView;

  return (
    <svg
      ref={ref}
      viewBox={`0 0 ${BAND.width} ${BAND.height}`}
      width={width}
      height={height}
      className="pointer-events-none block shrink-0"
      aria-hidden="true"
      focusable="false"
    >
      <defs>
        <DitherCellPattern id={`${id}-cells`} density={density} fill={color} />
        <linearGradient
          id={`${id}-fade`}
          x1="0"
          x2={BAND.width}
          gradientUnits="userSpaceOnUse"
        >
          <stop offset="0" stopColor="#fff" />
          <stop
            offset={(BAND.width - BAND.fade) / BAND.width}
            stopColor="#fff"
          />
          <stop offset="1" stopColor="#fff" stopOpacity="0" />
        </linearGradient>
        <mask id={`${id}-mask`}>
          <rect
            width={BAND.width}
            height={BAND.height}
            fill={`url(#${id}-fade)`}
          />
        </mask>
      </defs>

      {/* Empty track. On its own — no dither at all — this is what "no source
          established" looks like, which is the whole of the unknown state. */}
      <rect
        width={BAND.width}
        height={BAND.height}
        fill="var(--muted)"
        opacity="0.35"
      />
      <rect
        x="0.5"
        y="0.5"
        width={BAND.width - 1}
        height={BAND.height - 1}
        fill="none"
        stroke="var(--border)"
      />

      {known && (
        <g mask={open ? `url(#${id}-mask)` : undefined}>
          <rect
            width={open ? BAND.width : BAND.stop}
            height={BAND.height}
            fill={`url(#${id}-cells)`}
          />
          {scanning && (
            <motion.rect
              width={BAND.scan}
              height={BAND.height}
              fill={color}
              opacity="0.22"
              initial={{ x: -BAND.scan }}
              animate={running ? { x: [-BAND.scan, BAND.width] } : { x: -BAND.scan }}
              transition={
                running
                  ? {
                      duration: SCAN_SECONDS,
                      ease: "easeInOut",
                      repeat: Infinity,
                    }
                  : { duration: 0 }
              }
            />
          )}
        </g>
      )}

      {/* The hard end rule. A stopped series must look stopped. */}
      {known && !open && (
        <rect
          x={BAND.stop}
          width="1.5"
          height={BAND.height}
          fill={color}
          opacity="0.9"
        />
      )}
    </svg>
  );
}

/* -------------------------------------------------------------------------- *
 * Variants
 * -------------------------------------------------------------------------- */

function ProvenancePanel({
  freshness,
  fill,
  slug,
  signInHref,
  headingId,
  className,
}: {
  freshness: HistoryFreshness;
  fill: RGB;
  slug?: string;
  signInHref?: string;
  headingId: string;
  className?: string;
}) {
  // The action exists only where a reader can act on it, and it describes
  // exactly what sign-in does. There is no connect endpoint, so no wording here
  // may imply one.
  const showAction =
    Boolean(signInHref) &&
    (freshness.state === "exact_frozen" || freshness.state === "restricted");

  const facts: [string, string][] = [
    ["Source", sourceLabel(freshness)],
    ["Coverage", coverageLabel(freshness)],
    ["State", stateLabel(freshness)],
  ];

  return (
    <aside className={cn(PANEL, "p-3.5", className)} aria-labelledby={headingId}>
      <h2 id={headingId} className={HEADING}>
        How this series was read
        {slug ? <span className="sr-only"> for {slug}</span> : null}
      </h2>

      <div className="mt-3">
        <ProvenanceBand
          freshness={freshness}
          fill={fill}
          width={BAND.width}
          height={BAND.height}
        />
      </div>

      <p className={cn(BODY, MEASURE, "mt-3")}>{sourceDetail(freshness)}</p>

      <dl className="mt-4 grid gap-x-12 gap-y-3 sm:grid-cols-3">
        {facts.map(([term, value]) => (
          <div key={term} className="space-y-1">
            <dt className={EYEBROW}>{term}</dt>
            <dd className="text-[13px]">{value}</dd>
          </div>
        ))}
      </dl>

      {showAction && (
        <div className="mt-4">
          <a className={SECTION_ACTION} href={signInHref}>
            Sign in with GitHub →
          </a>
          <p className={cn(CAPTION, MEASURE, "mt-1.5")}>
            Signing in lets gitdebt recognise the repositories you own and queue
            them for analysis. It does not restore a stargazer read on its own.
          </p>
        </div>
      )}
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
  fill: RGB;
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
      <ProvenanceBand freshness={freshness} fill={fill} width={96} height={14} />
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
  const raw = useId();
  const id = `${raw.replaceAll(":", "")}-provenance`;
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

  const rows: { freshness: HistoryFreshness; fill: RGB; term: string; definition: string }[] = [
    {
      freshness: exact,
      fill: SWATCH.blue,
      term: "GitHub stargazer list",
      definition:
        "Exact — one point per star, with its own timestamp. Since July 2026 GitHub serves this list only to a repository's own admins and collaborators, so a series read this way stops on a fixed date.",
    },
    {
      freshness: archive,
      fill: SWATCH.purple,
      term: "Public GH Archive star events",
      definition:
        "Rebuilt from the public event archive. Star actions are recorded and unstars are not, so it reads as an attention signal rather than a net star count. It keeps flowing for every public repository.",
    },
  ];

  return (
    <section className={className} aria-labelledby={id}>
      <h2 id={id} className={HEADING}>
        Where a star series comes from
      </h2>
      <dl className="mt-6 grid gap-x-12 gap-y-8 sm:grid-cols-2">
        {rows.map((row) => (
          <div key={row.term} className="space-y-2">
            <dt className="space-y-2">
              <ProvenanceBand
                freshness={row.freshness}
                fill={row.fill}
                width={BAND.width}
                height={BAND.height}
              />
              <span className="block text-[13px]">{row.term}</span>
            </dt>
            <dd className={cn(BODY, MEASURE)}>{row.definition}</dd>
          </div>
        ))}
      </dl>
      <p className={cn(CAPTION, MEASURE, "mt-6")}>
        Every chart names its source and its coverage date. None of them states
        how much of a history is missing: an archive series counts re-stars and
        can exceed a repository's own star total, so that figure would be wrong
        exactly where it looks most precise.
      </p>
    </section>
  );
}
