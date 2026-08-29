import { useMemo, useRef, useState } from "react";

import {
  ChartFrame,
  DATE_RE,
  clamp01,
  fullDate,
  pathData,
  shortDate,
  spanLabel,
  stride,
  usePlotWidth,
  valueFraction,
  type ChartAxis,
  type Sample,
} from "@/components/ChartFrame";
import { EmbedSnippet } from "@/components/EmbedSnippet";
import { CAPTION } from "@/components/style-tokens";
import { formatCompact } from "@/lib/star-insights";

/**
 * Several series on one sheet, drawn as an overlay of plotted traces.
 *
 * The rule that shapes this file: HUE IS NEVER THE ONLY DISTINCTION. Every
 * trace carries a pen colour AND a dash pattern AND its own label lettered at
 * its own line end on a leader, so the sheet is readable in greyscale, on a
 * projector, and by someone who cannot tell two of the pens apart. A legend
 * parked in a corner asks the reader to hold eight colour-to-name pairs in
 * their head while looking somewhere else; a leader answers at the line.
 *
 * Like every chart here it is SVG, present in the markup before any script
 * runs. The canvas it replaces drew nothing at all without JavaScript, and it
 * opened with a left-to-right wipe that hid the data until the wipe finished —
 * the exact failure the site's motion rule exists to forbid.
 */

export type ComparisonChartPoint = { date: string; stars: number };
export type ComparisonChartSeries = {
  slug: string;
  points: ComparisonChartPoint[];
};

/** Kept as a triplet because build-time callers pass it straight to the
 *  provenance mark, which paints on a canvas and cannot read `oklch()`. */
export type RGB = readonly [number, number, number];

type Props = {
  apiBase: string;
  path: string;
  caption: string;
  embedLink: string;
  label: string;
  series: ComparisonChartSeries[];
  height?: number;
};

const PAD_L = 14;
const PAD_T = 30;
const PAD_B = 46;

/**
 * Vertical room one end-label block needs: an identifier over a value.
 * Measured, not guessed — the value line sits 13 below its identifier and
 * descends about 3 more, so anything under 26 lets the next identifier's caps
 * bite into the value above it.
 */
const LABEL_PITCH = 28;

/** Cap the readable table per series. Every row is still a real reading. */
const MAX_ROWS = 80;

/**
 * The plotter set: eight pens, each with its own dash. The sRGB triplets are
 * the exact colours of `--pen-1` … `--pen-8` in globals.css, converted once so
 * that a canvas fill and a stroked path are the same ink rather than two
 * neighbouring inks that look like a mistake.
 */
const PENS: { stroke: string; rgb: RGB; dash?: string }[] = [
  { stroke: "var(--pen-1)", rgb: [40, 44, 47] },
  { stroke: "var(--pen-2)", rgb: [204, 41, 31], dash: "9 4" },
  { stroke: "var(--pen-3)", rgb: [26, 96, 158], dash: "1.5 3.5" },
  { stroke: "var(--pen-4)", rgb: [96, 124, 66], dash: "12 3.5 1.5 3.5" },
  { stroke: "var(--pen-5)", rgb: [162, 94, 43], dash: "4.5 4" },
  { stroke: "var(--pen-6)", rgb: [106, 88, 138], dash: "18 4" },
  { stroke: "var(--pen-7)", rgb: [30, 119, 119], dash: "1.5 3 9 3" },
  { stroke: "var(--pen-8)", rgb: [123, 111, 102], dash: "6 3 1.5 3 1.5 3" },
];

/** FNV-1a over the slug. The assignment must not move between renders, or a
 *  repository changes pen when an unrelated one is added to the sheet. */
function hashSlug(slug: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < slug.length; i += 1) {
    h ^= slug.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

/** Stable within a comparison and collision-free until the pen set is full. */
function comparisonPens(slugs: string[]): Map<string, number> {
  const pens = new Map<string, number>();
  const occupied = new Set<number>();
  for (const slug of [...new Set(slugs)].sort()) {
    const preferred = hashSlug(slug.toLowerCase()) % PENS.length;
    let index = preferred;
    for (let attempt = 0; attempt < PENS.length; attempt += 1) {
      index = (preferred + attempt) % PENS.length;
      if (!occupied.has(index)) break;
    }
    occupied.add(index);
    pens.set(slug, index);
  }
  return pens;
}

/**
 * Consumed at build time by the vs and compare pages, which hand the triplet to
 * `SeriesProvenance` so the provenance mark takes the same hue as the trace.
 * The name, the signature and the determinism are load-bearing there.
 */
export function comparisonColors(slugs: string[]): Map<string, RGB> {
  const colors = new Map<string, RGB>();
  for (const [slug, index] of comparisonPens(slugs)) {
    colors.set(slug, PENS[index].rgb);
  }
  return colors;
}

/* ── Reading a series at an arbitrary point on the shared axis ───────────── */

type ParsedPoint = { at: number; value: number; date: string };
type ParsedSeries = {
  slug: string;
  pen: number;
  points: ParsedPoint[];
};

function sampleTimeline(points: ParsedPoint[], fraction: number): Sample {
  const position = clamp01(fraction) * (points.length - 1);
  const low = Math.floor(position);
  const high = Math.min(points.length - 1, Math.ceil(position));
  const local = position - low;
  const before = points[low];
  const after = points[high];
  return {
    at: before.at + (after.at - before.at) * local,
    value: before.value + (after.value - before.value) * local,
    approximate: low !== high,
  };
}

function sampleDate(points: ParsedPoint[], target: number): Sample | null {
  if (target < points[0].at) return null;
  const last = points[points.length - 1];
  if (target >= last.at) {
    return { at: last.at, value: last.value, approximate: target !== last.at };
  }
  let lo = 0;
  let hi = points.length - 1;
  while (lo < hi) {
    const mid = Math.floor((lo + hi) / 2);
    if (points[mid].at < target) lo = mid + 1;
    else hi = mid;
  }
  const high = lo;
  const low = Math.max(0, high - 1);
  const before = points[low];
  const after = points[high];
  const span = Math.max(1, after.at - before.at);
  const local = (target - before.at) / span;
  return {
    at: target,
    value: before.value + (after.value - before.value) * local,
    approximate: target !== before.at && target !== after.at,
  };
}

function readAt(
  series: ParsedSeries,
  fraction: number,
  axis: ChartAxis,
  minDate: number,
  maxDate: number,
): Sample | null {
  if (axis === "timeline") return sampleTimeline(series.points, fraction);
  return sampleDate(
    series.points,
    minDate + clamp01(fraction) * Math.max(1, maxDate - minDate),
  );
}

/* ── The sheet ───────────────────────────────────────────────────────────── */

export function ComparisonSheet({
  apiBase,
  path,
  caption,
  embedLink,
  label,
  series,
  height = 460,
}: Props) {
  const [axis, setAxis] = useState<ChartAxis>("date");
  const [logScale, setLogScale] = useState(false);
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");

  const validFrom = DATE_RE.test(from) ? from : "";
  const validTo = DATE_RE.test(to) ? to : "";
  // An inverted range is reported rather than silently applied: emptying the
  // series without saying why reads as a broken chart.
  const inverted = Boolean(validFrom && validTo && validFrom > validTo);
  const appliedFrom = inverted ? "" : validFrom;
  const appliedTo = inverted ? "" : validTo;

  const visibleSeries = useMemo(() => {
    const fromMs = appliedFrom
      ? Date.parse(`${appliedFrom}T00:00:00Z`)
      : -Infinity;
    const toMs = appliedTo ? Date.parse(`${appliedTo}T23:59:59Z`) : Infinity;
    return series
      .map((item) => ({
        ...item,
        points: item.points.filter((point) => {
          const at = Date.parse(point.date);
          return Number.isFinite(at) && at >= fromMs && at <= toMs;
        }),
      }))
      .filter((item) => item.points.length >= 2);
  }, [appliedFrom, appliedTo, series]);

  return (
    <ChartFrame
      title={caption}
      axis={axis}
      onAxisChange={setAxis}
      logScale={logScale}
      onLogScaleChange={setLogScale}
      from={from}
      to={to}
      onFromChange={setFrom}
      onToChange={setTo}
      onReset={() => {
        setFrom("");
        setTo("");
      }}
      inverted={inverted}
      embed={
        <EmbedSnippet
          apiBase={apiBase}
          chartPath={path}
          linkHref={embedLink}
          label={label}
          state={{
            type: axis,
            log: logScale,
            from: appliedFrom,
            to: appliedTo,
          }}
          variant="menu"
        />
      }
    >
      {/* The plot states its own shortfall — the sheet does not need a second
          copy of that sentence, and two copies is how they drift apart. */}
      <ComparisonPlot
        series={visibleSeries}
        axis={axis}
        logScale={logScale}
        height={height}
      />
    </ChartFrame>
  );
}

/* ── The plot ────────────────────────────────────────────────────────────── */

/** Fit an identifier to the gutter. Owner first, then the repository name
 *  alone, then a hard cut — never a string running off the sheet. */
function fitLabel(slug: string, budget: number): string {
  if (slug.length <= budget) return slug;
  const name = slug.slice(slug.indexOf("/") + 1);
  if (name.length <= budget) return name;
  return `${name.slice(0, Math.max(1, budget - 1))}…`;
}

/** Push labels apart so no two overlap, then hold them inside the plot. */
function spreadLabels(
  desired: { key: string; y: number }[],
  top: number,
  bottom: number,
): Map<string, number> {
  const sorted = [...desired].sort((a, b) => a.y - b.y);
  const placed = sorted.map((item) => ({ ...item }));
  for (let i = 0; i < placed.length; i += 1) {
    const floor = i === 0 ? top : placed[i - 1].y + LABEL_PITCH;
    placed[i].y = Math.max(placed[i].y, floor);
  }
  for (let i = placed.length - 1; i >= 0; i -= 1) {
    const ceiling =
      i === placed.length - 1 ? bottom : placed[i + 1].y - LABEL_PITCH;
    placed[i].y = Math.min(placed[i].y, ceiling);
  }
  return new Map(placed.map((item) => [item.key, Math.round(item.y * 100) / 100]));
}

function ComparisonPlot({
  series,
  axis,
  logScale,
  height,
}: {
  series: ComparisonChartSeries[];
  axis: ChartAxis;
  logScale: boolean;
  height: number;
}) {
  const rootRef = useRef<HTMLDivElement>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const width = usePlotWidth(rootRef);
  const [probe, setProbe] = useState<number | null>(null);

  const parsed = useMemo<ParsedSeries[]>(() => {
    const pens = comparisonPens(series.map((item) => item.slug));
    return series
      .map((item) => ({
        slug: item.slug,
        pen: pens.get(item.slug) ?? 0,
        points: item.points
          .map((point) => ({
            at: Date.parse(point.date),
            value: point.stars,
            date: point.date,
          }))
          .filter(
            (point) =>
              Number.isFinite(point.at) && Number.isFinite(point.value),
          )
          .sort((a, b) => a.at - b.at),
      }))
      .filter((item) => item.points.length >= 2);
  }, [series]);

  const rightGutter = Math.round(Math.min(210, Math.max(96, width * 0.26)));
  const plotW = Math.max(40, width - PAD_L - rightGutter);
  const plotH = Math.max(40, height - PAD_T - PAD_B);
  const baselineY = PAD_T + plotH;
  const plotR = PAD_L + plotW;

  const minDate = Math.min(...parsed.map((item) => item.points[0].at));
  const maxDate = Math.max(
    ...parsed.map((item) => item.points[item.points.length - 1].at),
  );
  const maxValue = Math.max(
    1,
    ...parsed.flatMap((item) => item.points.map((point) => point.value)),
  );
  const dateSpan = Math.max(1, maxDate - minDate);

  const traces = useMemo(() => {
    return parsed.map((item) => {
      const vertices = stride(item.points, 400).map((point, index, all) => ({
        x:
          PAD_L +
          (axis === "timeline"
            ? index / Math.max(1, all.length - 1)
            : (point.at - minDate) / dateSpan) *
            plotW,
        y:
          PAD_T + plotH - valueFraction(point.value, maxValue, logScale) * plotH,
      }));
      return {
        slug: item.slug,
        pen: PENS[item.pen],
        points: item.points,
        d: pathData(vertices),
        start: vertices[0],
        end: vertices[vertices.length - 1],
        last: item.points[item.points.length - 1],
      };
    });
  }, [axis, dateSpan, logScale, maxValue, minDate, parsed, plotH, plotW]);

  const labelY = spreadLabels(
    traces.map((trace) => ({ key: trace.slug, y: trace.end.y })),
    PAD_T + 4,
    baselineY - 8,
  );

  const charBudget = Math.max(6, Math.floor((rightGutter - 22) / 6));

  const readings = traces.map((trace) => {
    const source = parsed.find((item) => item.slug === trace.slug)!;
    const reading =
      probe === null
        ? {
            at: trace.last.at,
            value: trace.last.value,
            approximate: false,
          }
        : readAt(source, probe, axis, minDate, maxDate);
    return { trace, reading };
  });

  function onPointer(event: React.PointerEvent<SVGSVGElement>) {
    const svg = svgRef.current;
    if (!svg) return;
    const box = svg.getBoundingClientRect();
    if (box.width === 0) return;
    const vx = ((event.clientX - box.left) / box.width) * width;
    setProbe(clamp01((vx - PAD_L) / plotW));
  }

  const scaleLabel = `${formatCompact(maxValue)} STARS${logScale ? " · LOG" : ""}`;
  const probeX = probe === null ? null : PAD_L + probe * plotW;

  // The dimension under the baseline states what is being measured across the
  // sheet's x extent: the whole span at rest, the station being read while the
  // pointer is on it. One line, one value — never two values fighting for the
  // same place on the same rule.
  const extentLabel =
    axis === "timeline" ? "LIFETIME 0–100%" : spanLabel(minDate, maxDate);
  const stationLabel =
    probe === null
      ? null
      : axis === "timeline"
        ? `${Math.round(probe * 100)}% OF LIFETIME`
        : shortDate(minDate + probe * dateSpan).toUpperCase();
  const dimensionLabel = stationLabel ?? extentLabel;

  // The wrapper is rendered either way, so the width measurement is attached
  // before the sheet has two drawable series and is already correct on the
  // render where the second one arrives.
  if (traces.length < 2) {
    return (
      <div ref={rootRef}>
        <p className={`${CAPTION} py-6`} aria-live="polite">
          This range needs at least two repositories with recorded star history.
        </p>
      </div>
    );
  }

  return (
    <div ref={rootRef}>
      <svg
        ref={svgRef}
        viewBox={`0 0 ${width} ${height}`}
        width="100%"
        height={height}
        preserveAspectRatio="none"
        role="img"
        aria-label={`Star history compared across ${traces
          .map((trace) => trace.slug)
          .join(", ")}`}
        className="block touch-pan-y select-none"
        onPointerMove={onPointer}
        onPointerDown={onPointer}
        onPointerLeave={() => setProbe(null)}
      >
        {/* ── The frame of the measurement: one baseline carrying the shared
            time span, one vertical extension carrying the value range. No
            gridlines: a gridline measures nothing. ──────────────────────── */}
        <g stroke="var(--rule-strong)" strokeWidth="1" strokeLinecap="round">
          <line x1={PAD_L} y1={baselineY} x2={plotR} y2={baselineY} />
          <line x1={PAD_L} y1={baselineY - 4} x2={PAD_L} y2={baselineY + 9} />
          <line x1={plotR} y1={baselineY - 4} x2={plotR} y2={baselineY + 9} />
          <line x1={PAD_L} y1={PAD_T} x2={PAD_L} y2={baselineY} />
          <line x1={PAD_L - 4} y1={PAD_T} x2={PAD_L + 6} y2={PAD_T} />
        </g>

        <text
          x={PAD_L + 10}
          y={PAD_T - 9}
          className="font-draft tnum"
          fontSize="12"
          letterSpacing="0.09em"
          fill="var(--ink-3)"
        >
          {scaleLabel}
        </text>

        {dimensionLabel && (
          <>
            <g className="extends" style={{ ["--draw-delay" as string]: "540ms" }}>
              <line
                x1={PAD_L}
                y1={baselineY + 22}
                x2={plotR}
                y2={baselineY + 22}
                stroke="var(--rule-strong)"
                strokeWidth="1"
                strokeLinecap="round"
              />
            </g>
            <text
              x={(PAD_L + plotR) / 2}
              y={baselineY + 22}
              dy="0.34em"
              textAnchor="middle"
              className="font-draft tnum"
              fontSize="13"
              letterSpacing="0.1em"
              fill={stationLabel ? "var(--signal)" : "var(--ink-3)"}
            >
              <tspan
                stroke="var(--paper)"
                strokeWidth="7"
                paintOrder="stroke"
                strokeLinejoin="round"
              >
                {dimensionLabel}
              </tspan>
            </text>
          </>
        )}

        {/* ── The objects. Pen colour and dash pattern both, so no reading
            depends on telling two hues apart. ──────────────────────────── */}
        {traces.map((trace) => (
          <path
            key={trace.slug}
            d={trace.d}
            fill="none"
            stroke={trace.pen.stroke}
            strokeWidth="1.5"
            strokeDasharray={trace.pen.dash}
            strokeLinejoin="round"
            strokeLinecap="round"
            vectorEffect="non-scaling-stroke"
          />
        ))}

        {/* ── The section line: where the sheet is being read. It terminates
            on the baseline, and the value it measures is lettered on the
            dimension below. ────────────────────────────────────────────── */}
        {probeX !== null && (
          <g>
            <line
              x1={probeX}
              y1={PAD_T}
              x2={probeX}
              y2={baselineY}
              stroke="var(--signal)"
              strokeWidth="1"
              strokeDasharray="3 3"
              strokeLinecap="round"
            />
            <path
              d={`M${probeX.toFixed(2)} ${baselineY} l-4 8 l8 0 z`}
              fill="var(--signal)"
            />
          </g>
        )}

        {/* ── Every series labelled at its own line end, on a leader. ───── */}
        {readings.map(({ trace, reading }) => {
          const y = labelY.get(trace.slug) ?? trace.end.y;
          // With no reading the pointer is left of where this series begins,
          // so the leader lands on its first plotted point — the nearest thing
          // it actually has. A leader never points at nothing.
          const markY =
            reading === null
              ? trace.start.y
              : PAD_T +
                plotH -
                valueFraction(reading.value, maxValue, logScale) * plotH;
          const markX =
            reading === null
              ? trace.start.x
              : probeX === null
                ? trace.end.x
                : Math.min(probeX, trace.end.x);
          return (
            <g key={trace.slug}>
              <path
                d={`M${markX.toFixed(2)} ${markY.toFixed(2)} L${(plotR + 8).toFixed(2)} ${y.toFixed(2)} L${(plotR + 13).toFixed(2)} ${y.toFixed(2)}`}
                fill="none"
                stroke={trace.pen.stroke}
                strokeWidth="1"
                strokeLinecap="round"
                strokeLinejoin="round"
                opacity="0.75"
              />
              <circle
                cx={Number(markX.toFixed(2))}
                cy={Number(markY.toFixed(2))}
                r="2.5"
                fill={trace.pen.stroke}
              />
              <text
                x={plotR + 17}
                y={y}
                dy="-0.1em"
                className="font-draft tnum"
                fontSize="11"
                letterSpacing="0.06em"
                fill={trace.pen.stroke}
              >
                {fitLabel(trace.slug, charBudget)}
              </text>
              <text
                x={plotR + 17}
                y={y + 13}
                className="font-draft tnum"
                fontSize="13"
                fill="var(--ink)"
              >
                {reading === null
                  ? "—"
                  : `${reading.approximate ? "≈" : ""}${formatCompact(reading.value)}`}
              </text>
            </g>
          );
        })}
      </svg>

      <p className="sr-only" aria-live="polite">
        {readings
          .map(
            ({ trace, reading }) =>
              `${trace.slug}: ${
                reading === null
                  ? "no reading yet"
                  : `${Math.round(reading.value).toLocaleString()} stars on ${fullDate(reading.at)}`
              }.`,
          )
          .join(" ")}
      </p>

      {traces.map((trace) => (
        <table className="sr-only" key={trace.slug}>
          <caption>{`Star history for ${trace.slug}`}</caption>
          <thead>
            <tr>
              <th scope="col">Date</th>
              <th scope="col">Stars</th>
            </tr>
          </thead>
          <tbody>
            {stride(trace.points, MAX_ROWS).map((point) => (
              <tr key={point.date}>
                <td>{fullDate(point.at)}</td>
                <td>{point.value}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ))}
    </div>
  );
}
