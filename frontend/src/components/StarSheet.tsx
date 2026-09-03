import { useCallback, useId, useMemo, useRef, useState } from "react";
import type { HistoryPoint } from "@/lib/star-insights";
import {
  formatCompact,
  formatFullDate,
  formatMonthYear,
} from "@/lib/star-insights";
import { usePlotWidth } from "@/components/ChartFrame";

/**
 * The signature artifact: a repository's star history drawn as a dimensioned
 * technical drawing.
 *
 * A star history is a measurement of a thing over time, so it is drawn the way
 * a measurement is drawn. The trace is the object. Dimension lines span two
 * real points on it and carry the value of that span. Leaders point at real
 * data. The title block states where the numbers came from.
 *
 * Two rules govern everything here.
 *
 * 1. EVERY LINE TERMINATES ON SOMETHING REAL. There is no line in this drawing
 *    that does not measure, point at, or enclose an actual datum. That is what
 *    keeps drafting notation from decaying into ornament.
 *
 * 2. THE DRAWING IS COMPLETE BEFORE IT MOVES. The path is emitted with its
 *    final geometry and full opacity; the animation only slides a dash offset
 *    along a length computed here, deterministically, from the points
 *    themselves. If the animation never runs — reduced motion, a throttled
 *    tab, a screenshot pass, no hydration at all — the finished drawing is what
 *    renders. Nothing on this sheet is invisible waiting for a timeline.
 */

type Provenance = {
  /** e.g. "Historical data" — from history-freshness.ts. Never a count. */
  source: string;
  /** e.g. "Through Aug 2026". */
  coverage: string;
  /** e.g. "Complete" / "Updating". */
  state: string;
};

type Props = {
  repo: string;
  history: HistoryPoint[];
  provenance?: Provenance;
  /** Drawn height of the plot area. The sheet adds the title block below it. */
  height?: number;
  className?: string;
};

/**
 * The sheet is lettered at 1:1.
 *
 * A fixed viewBox stretched to fit letters its own type at whatever horizontal
 * scale the container imposes: on a 390px phone a 1000-unit sheet condenses
 * every label to 35% of its width, so the span dimension and the measured value
 * arrive squashed and unreadable. The viewBox width is therefore the MEASURED
 * width, which makes `preserveAspectRatio="none"` an identity transform for
 * type. Before the measurement lands the sheet draws at `FALLBACK_W` — a
 * complete drawing with every line and value already in the markup, only not
 * yet exactly proportioned.
 */
const FALLBACK_W = 1000;
/** Below this the sheet is a phone, and the value column has to give way. */
const NARROW = 520;
const PAD_L = 16;
const PAD_T = 28;
const PAD_B = 44; // clears the baseline dimension line and its span value

/** Cap the drawn polyline. A plot resolves at the pixel, not at the sample. */
const MAX_SAMPLES = 240;

type Pt = { x: number; y: number; date: string; stars: number };

/** Even-stride sample that always keeps the first and last real points. */
function sample(history: HistoryPoint[]): HistoryPoint[] {
  if (history.length <= MAX_SAMPLES) return history;
  const step = (history.length - 1) / (MAX_SAMPLES - 1);
  const out: HistoryPoint[] = [];
  for (let i = 0; i < MAX_SAMPLES; i += 1) {
    out.push(history[Math.round(i * step)]);
  }
  return out;
}

/**
 * Polyline length, summed here rather than measured from the DOM.
 *
 * `getTotalLength()` would need an effect, which means the first paint would
 * carry a wrong dash length and the stroke would animate to somewhere short of
 * its own end — the half-drawn line that reads as broken. Summing the segments
 * is exact for a polyline and identical on the server and the client.
 */
function polylineLength(pts: Pt[]): number {
  let total = 0;
  for (let i = 1; i < pts.length; i += 1) {
    total += Math.hypot(pts[i].x - pts[i - 1].x, pts[i].y - pts[i - 1].y);
  }
  return Math.ceil(total);
}

/** Whole months between two ISO dates, as a drawing states a span. */
function spanLabel(fromISO: string, toISO: string): string {
  const from = Date.parse(fromISO);
  const to = Date.parse(toISO);
  if (Number.isNaN(from) || Number.isNaN(to) || to <= from) return "";
  const months = Math.max(1, Math.round((to - from) / 2_629_800_000));
  const years = Math.floor(months / 12);
  const rest = months % 12;
  if (years === 0) return `${months} MO`;
  if (rest === 0) return `${years} YR`;
  return `${years} YR ${rest} MO`;
}

export function StarSheet({
  repo,
  history,
  provenance,
  height = 380,
  className,
}: Props) {
  const uid = useId();
  const svgRef = useRef<SVGSVGElement>(null);
  const frameRef = useRef<HTMLElement>(null);
  const viewW = usePlotWidth(frameRef, FALLBACK_W);
  const narrow = viewW < NARROW;
  /**
   * The value column: enough for the figure and, beneath it, the LONGEST date
   * the sheet can state — not the one it happens to be stating today.
   *
   * Sizing this to the current string is how it broke. At 96 the column held
   * 82px and "AUGUST 28, 2026" measures 81.8px, so the drawing cleared its own
   * right edge by two tenths of a pixel and looked correct for a week. The
   * moment the coverage date rolled into September the stamp grew to 96.7px and
   * "SEPTEMBER 3, 2026" was sliced by the SVG's own viewBox, because an SVG
   * root clips at its bounds.
   *
   * So the budget is the widest case with a real margin, measured in the
   * browser rather than estimated: "SEPTEMBER 30, 2026" is 104px at 10.5px with
   * 0.08em tracking; the stamp starts 14px past the dimension's end; text is
   * never set against the rim, so 8px is kept clear. 14 + 104 + 8 = 126.
   */
  const padR = narrow ? 74 : 126;
  const valueSize = narrow ? 15 : 19;
  const dateSize = narrow ? 9 : 10.5;
  /**
   * A narrow sheet states the month, a wide one states the day.
   *
   * A phone cannot spare 126px for a value column, and the abbreviated stamp
   * ("SEP 2026", 38.6px) sits inside the 60px a 74px column leaves. A drawing
   * that cannot letter a value inside its own sheet states a coarser one rather
   * than a clipped one.
   */
  const stampDate = (iso: string) =>
    ((narrow ? formatMonthYear(iso) : formatFullDate(iso)) ?? "").toUpperCase();
  /** Index the pointer is measuring. `null` means the drawing states its own
   *  latest value, which is what it must say when nobody is touching it. */
  const [probe, setProbe] = useState<number | null>(null);

  const geometry = useMemo(() => {
    const clean = history.filter(
      (p) => p && typeof p.stars === "number" && !Number.isNaN(Date.parse(p.date)),
    );
    if (clean.length < 2) return null;

    const pts = sample(clean);
    const t0 = Date.parse(pts[0].date);
    const t1 = Date.parse(pts[pts.length - 1].date);
    const maxStars = pts[pts.length - 1].stars || 1;
    const spanT = Math.max(1, t1 - t0);

    const plotW = viewW - PAD_L - padR;
    const plotH = height - PAD_T - PAD_B;

    const points: Pt[] = pts.map((p) => ({
      x: PAD_L + ((Date.parse(p.date) - t0) / spanT) * plotW,
      y: PAD_T + plotH - (p.stars / maxStars) * plotH,
      date: p.date,
      stars: p.stars,
    }));

    return {
      points,
      d: points.map((p, i) => `${i === 0 ? "M" : "L"}${p.x.toFixed(2)} ${p.y.toFixed(2)}`).join(" "),
      length: polylineLength(points),
      plotH,
      baselineY: PAD_T + plotH,
      first: points[0],
      last: points[points.length - 1],
    };
  }, [history, height, viewW, padR]);

  const onPointer = useCallback(
    (event: React.PointerEvent<SVGSVGElement>) => {
      const svg = svgRef.current;
      if (!svg || !geometry) return;
      const box = svg.getBoundingClientRect();
      if (box.width === 0) return;
      const vx = ((event.clientX - box.left) / box.width) * viewW;
      // Nearest sample on x. The drawing measures a real datum, never an
      // interpolated one: a dimension that lands between two readings is a
      // number the data never contained.
      let best = 0;
      let bestDx = Infinity;
      for (let i = 0; i < geometry.points.length; i += 1) {
        const dx = Math.abs(geometry.points[i].x - vx);
        if (dx < bestDx) {
          bestDx = dx;
          best = i;
        }
      }
      setProbe(best);
    },
    [geometry, viewW],
  );

  if (!geometry) {
    return (
      <p className="text-[0.8125rem] text-ink-3">
        Not enough history to draw {repo}.
      </p>
    );
  }

  const { points, d, length, baselineY, first, last } = geometry;
  const active = probe === null ? points[points.length - 1] : points[probe];
  const isProbing = probe !== null;
  const span = spanLabel(first.date, last.date);

  return (
    <figure ref={frameRef} className={className}>
      <svg
        ref={svgRef}
        viewBox={`0 0 ${viewW} ${height}`}
        width="100%"
        height={height}
        preserveAspectRatio="none"
        role="img"
        aria-labelledby={`${uid}-title`}
        className="block touch-pan-y select-none"
        onPointerMove={onPointer}
        onPointerLeave={() => setProbe(null)}
      >
        <title id={`${uid}-title`}>
          {`Star history for ${repo}: ${formatCompact(last.stars)} stars, ${formatFullDate(first.date)} to ${formatFullDate(last.date)}.`}
        </title>

        {/* ── The baseline. It encloses the plot and carries the time span, so
            it measures the drawing's whole x extent. ─────────────────────── */}
        <g stroke="var(--rule-strong)" strokeWidth="1" strokeLinecap="round">
          <line x1={PAD_L} y1={baselineY} x2={viewW - padR} y2={baselineY} />
          {/* Extension lines: the two vertical ticks a dimension springs from. */}
          <line x1={PAD_L} y1={baselineY - 4} x2={PAD_L} y2={baselineY + 10} />
          <line
            x1={viewW - padR}
            y1={baselineY - 4}
            x2={viewW - padR}
            y2={baselineY + 10}
          />
        </g>

        {/* The span dimension, drawn below the baseline between the two
            extension ticks, with the measured value lettered on it. */}
        <g className="extends" style={{ ["--draw-delay" as string]: "620ms" }}>
          <line
            x1={PAD_L}
            y1={baselineY + 22}
            x2={viewW - padR}
            y2={baselineY + 22}
            stroke="var(--rule-strong)"
            strokeWidth="1"
            strokeLinecap="round"
          />
        </g>
        <text
          x={(PAD_L + (viewW - padR)) / 2}
          y={baselineY + 22}
          dy="0.34em"
          textAnchor="middle"
          className="font-draft"
          fontSize="13"
          letterSpacing="0.1em"
          fill="var(--ink-3)"
        >
          {/* The value sits ON the dimension line, so the line is cut for it.
              A paint-order stroke in the paper colour is how a drawing opens a
              gap in a rule for its own lettering. */}
          <tspan
            stroke="var(--paper)"
            strokeWidth="7"
            paintOrder="stroke"
            strokeLinejoin="round"
          >
            {span}
          </tspan>
        </text>

        {/* ── The object: the trace. Emitted complete; the dash offset is the
            only thing that moves. ───────────────────────────────────────── */}
        <path
          d={d}
          fill="none"
          stroke="var(--ink)"
          strokeWidth="1.75"
          strokeLinejoin="round"
          strokeLinecap="round"
          className="inks-in"
          style={{ ["--draw-length" as string]: String(length) }}
          vectorEffect="non-scaling-stroke"
        />

        {/* ── The measured value. A leader drops from the datum to the
            baseline and a second runs out to the value at the right margin.
            Both terminate on real points. ───────────────────────────────── */}
        <g>
          {/* The notation, and only the notation.
              It waits for the trace, because until the trace arrives at this
              datum there is nothing here to measure. Previously all four of
              these were painted at t=0 while the trace was still travelling,
              so for the length of the draw an arrowhead sat at the far right
              of the sheet pointing at a point the object had not reached —
              the one thing a dimensioned drawing may never show. The delay is
              the draw's own duration, read from the same token, so the two
              cannot drift apart. */}
          <g
            className="measures"
            style={{ ["--draw-delay" as string]: "var(--duration-draw)" }}
          >
            <line
              x1={active.x}
              y1={active.y}
              x2={active.x}
              y2={baselineY}
              stroke="var(--signal)"
              strokeWidth="1"
              strokeDasharray="3 3"
              strokeLinecap="round"
              opacity={isProbing ? 1 : 0.55}
            />
            <line
              x1={active.x}
              y1={active.y}
              x2={viewW - padR + 10}
              y2={active.y}
              stroke="var(--signal)"
              strokeWidth="1"
              strokeLinecap="round"
              opacity={isProbing ? 1 : 0.55}
            />
            {/* The terminator: a filled arrowhead landing on the datum. */}
            <path
              d={`M${active.x} ${active.y} l-5 -3.2 l0 6.4 z`}
              fill="var(--signal)"
            />
            <circle cx={active.x} cy={active.y} r="2.5" fill="var(--signal)" />
          </g>

          {/* The value is content, so it is lettered at first paint and is
              never inside the group above. */}
          <text
            x={viewW - padR + 14}
            y={active.y}
            dy="0.32em"
            className="font-draft"
            fontSize={valueSize}
            fill="var(--signal)"
            style={{ fontVariantNumeric: "tabular-nums" }}
          >
            {formatCompact(active.stars)}
          </text>
          <text
            x={viewW - padR + 14}
            y={active.y + 15}
            className="font-draft"
            fontSize={dateSize}
            letterSpacing="0.08em"
            fill="var(--ink-3)"
          >
            {stampDate(active.date)}
          </text>
        </g>

        {/* The origin datum. The drawing states where the object begins.
            Its stamp sits over the start of the trace, so it is lettered with
            a paper halo — the same paint-order cut the span dimension uses to
            open a gap in its own rule. Without it the curve runs through the
            date and both become unreadable. */}
        <g>
          <circle cx={first.x} cy={first.y} r="2" fill="var(--ink-3)" />
          <text
            x={first.x}
            y={first.y - 12}
            className="font-draft"
            fontSize={dateSize}
            letterSpacing="0.08em"
            fill="var(--ink-3)"
          >
            <tspan
              stroke="var(--paper)"
              strokeWidth="5"
              paintOrder="stroke"
              strokeLinejoin="round"
            >
              {stampDate(first.date)}
            </tspan>
          </text>
        </g>
      </svg>

      {/* The title block. A drawing states its source on the sheet, and this
          product is required to state SOURCE, COVERAGE and STATE and nothing
          more — never a count, never a percentage. The strings arrive already
          written by history-freshness.ts; this block only letters them. */}
      {provenance && (
        <figcaption className="mt-3 flex justify-end">
          <dl className="cut-edge grid grid-cols-[auto_auto] gap-x-6 gap-y-1 px-4 py-3 [--pad-x:1rem] [--pad-y:0.75rem]">
            {[
              ["Source", provenance.source],
              ["Coverage", provenance.coverage],
              ["State", provenance.state],
            ].map(([label, value]) => (
              <div key={label} className="contents">
                <dt className="drafted">{label}</dt>
                <dd className="font-mono text-[0.75rem] text-ink-2">{value}</dd>
              </div>
            ))}
          </dl>
        </figcaption>
      )}

      {/* The reading behind the drawing. A chart that only exists as a picture
          is unreadable to a screen reader and to an agent; the numbers are on
          the page either way. */}
      <table className="sr-only">
        <caption>{`Star history for ${repo}`}</caption>
        <thead>
          <tr>
            <th scope="col">Date</th>
            <th scope="col">Stars</th>
          </tr>
        </thead>
        <tbody>
          {points.map((p) => (
            <tr key={p.date}>
              <td>{formatFullDate(p.date)}</td>
              <td>{p.stars}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </figure>
  );
}
