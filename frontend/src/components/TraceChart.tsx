import { useCallback, useId, useMemo, useRef, useState } from "react";

import {
  MAX_DRAWN_VERTICES,
  clamp01,
  fullDate,
  pathData,
  polylineLength,
  sampleAt,
  shortDate,
  spanLabel,
  setTextNode,
  stride,
  useLeaderFollow,
  usePlotWidth,
  valueFraction,
  type ChartAxis,
} from "@/components/ChartFrame";
import { formatCompact } from "@/lib/star-insights";
import {
  calendarEasing,
  segmentAtX,
  traceMilestones,
} from "@/lib/trace-marks";

/**
 * One measured series, drawn as a dimensioned trace.
 *
 * This replaces a canvas: a low-resolution pixel buffer painted on a seeded
 * wave set and upscaled without smoothing. It looked like a texture and it had
 * one fatal property — with no JavaScript there was NOTHING on the page. No
 * line, no axis, no number, not even alt text. A chart that exists only once a
 * script has run is not a chart.
 *
 * So it is an SVG now, and three rules govern it.
 *
 * 1. EVERY LINE TERMINATES ON SOMETHING REAL. The baseline spans the plot and
 *    carries the time span. The vertical extension spans the value range and
 *    carries the maximum. The leader points at the reading under the pointer.
 *    There is no gridline, because a gridline measures nothing: it is graph
 *    paper drawn behind a drawing.
 *
 * 2. THE DRAWING IS COMPLETE BEFORE IT MOVES. The path is emitted with its
 *    final geometry and full opacity; the animation only slides a dash offset
 *    along a length summed here from the points themselves. If the animation
 *    never runs, the finished drawing is what renders.
 *
 * 3. A LINE IS A LINE. There is no shaded region under the curve. A drawing
 *    plots a value, it does not flood the area beneath it — the fill was never
 *    data, only ink spent on the space below the data.
 */

export type TracePoint = { date: string; value: number };

type Props = {
  points: TracePoint[];
  axis?: ChartAxis;
  logScale?: boolean;
  height?: number;
  valueLabel?: string;
  valueFormatter?: (value: number) => string;
  /** False draws the sheet at its latest reading and ignores the pointer. */
  interactive?: boolean;
  /** Mark round-number crossings. Only a cumulative series has them. */
  milestones?: boolean;
  className?: string;
};

const PAD_L = 14;
const PAD_T = 30;
/** Clears the baseline, its extension ticks, and the span dimension below. */
const PAD_B = 46;

/** Cap the readable table. Every reading is still a real one. */
const MAX_ROWS = 240;

/** A star history draws over longer than a small series: in calendar time
 *  (`calendarEasing`) its pen climb is the star rate, and a launch or a viral
 *  week needs the time to read as the leap it was. */
const STAR_DRAW_MS = 1400;
const DRAW_MS = 900;

type Parsed = { at: number; value: number; date: string };
type Vertex = { x: number; y: number; at: number; value: number };

export function TraceChart({
  points,
  axis = "date",
  logScale = false,
  height = 360,
  valueLabel = "stars",
  valueFormatter = (value) => Math.round(value).toLocaleString(),
  interactive = true,
  milestones = false,
  className,
}: Props) {
  const uid = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const dropRef = useRef<SVGLineElement>(null);
  const leaderRef = useRef<SVGLineElement>(null);
  const headRef = useRef<SVGGElement>(null);
  const valueRef = useRef<SVGTextElement>(null);
  const stampRef = useRef<SVGTextElement>(null);
  const width = usePlotWidth(rootRef);
  /** Fraction of the plot the pointer is measuring. `null` means the drawing
   *  states its own latest reading, which is what it must say untouched. */
  const [probe, setProbe] = useState<number | null>(null);

  const parsed = useMemo<Parsed[]>(
    () =>
      points
        .map((point) => ({
          at: Date.parse(point.date),
          value: point.value,
          date: point.date,
        }))
        .filter(
          (point) => Number.isFinite(point.at) && Number.isFinite(point.value),
        )
        .sort((a, b) => a.at - b.at),
    [points],
  );

  const rightGutter = Math.round(Math.min(140, Math.max(84, width * 0.15)));
  const plotW = Math.max(40, width - PAD_L - rightGutter);
  const plotH = Math.max(40, height - PAD_T - PAD_B);
  const baselineY = PAD_T + plotH;
  const plotR = PAD_L + plotW;

  const geometry = useMemo(() => {
    if (parsed.length < 2) return null;
    const max = Math.max(1, ...parsed.map((point) => point.value));
    const t0 = parsed[0].at;
    const t1 = parsed[parsed.length - 1].at;
    const spanT = Math.max(1, t1 - t0);
    const drawn = stride(parsed, MAX_DRAWN_VERTICES);

    const vertices: Vertex[] = drawn.map((point, index) => ({
      x:
        PAD_L +
        (axis === "timeline"
          ? index / Math.max(1, drawn.length - 1)
          : (point.at - t0) / spanT) *
          plotW,
      y: PAD_T + plotH - valueFraction(point.value, max, logScale) * plotH,
      at: point.at,
      value: point.value,
    }));

    return {
      max,
      vertices,
      d: pathData(vertices),
      length: polylineLength(vertices),
      easing: calendarEasing(vertices),
      first: vertices[0],
      last: vertices[vertices.length - 1],
      span: spanLabel(t0, t1),
    };
  }, [axis, logScale, parsed, plotH, plotW]);

  const onPointer = useCallback(
    (event: React.PointerEvent<SVGSVGElement>) => {
      const svg = svgRef.current;
      if (!svg) return;
      const box = svg.getBoundingClientRect();
      if (box.width === 0) return;
      const vx = ((event.clientX - box.left) / box.width) * width;
      setProbe(clamp01((vx - PAD_L) / plotW));
    },
    [plotW, width],
  );

  const marks = useMemo(() => {
    if (!geometry || !milestones) return [];
    const { first } = geometry;
    const stampWidth = shortDate(first.at).length * 10.5 * 0.56;
    return traceMilestones(geometry.vertices, {
      baselineY,
      fontSize: 10.5,
      avoid: [
        // The origin stamp, offset clear of the value extension.
        {
          x0: first.x,
          y0: Math.max(PAD_T + 12, first.y - 11) - 10.5,
          x1: first.x + 6 + stampWidth + 4,
          y1: Math.max(PAD_T + 12, first.y - 11) + 3,
        },
        // The scale label over the value extension.
        { x0: 0, y0: 0, x1: PAD_L + 140, y1: PAD_T },
        // The value column at the right margin.
        { x0: plotR - 4, y0: 0, x1: width, y1: height },
      ],
      duration: STAR_DRAW_MS,
      calendar: true,
    });
  }, [geometry, milestones, baselineY, plotR, width, height]);

  // The reading the sheet is currently stating. Untouched, it states its own
  // latest value; under the pointer it states the reading there, and says so
  // with "≈" when that reading falls between two recorded ones.
  const reading = !geometry
    ? null
    : probe === null || !interactive
      ? { at: geometry.last.at, value: geometry.last.value, approximate: false }
      : sampleAt(parsed, probe, axis);
  const activeX = !geometry
    ? 0
    : probe === null || !interactive
      ? geometry.last.x
      : PAD_L + probe * plotW;
  const activeY =
    !geometry || !reading
      ? 0
      : PAD_T +
        plotH -
        valueFraction(reading.value, geometry.max, logScale) * plotH;

  useLeaderFollow(
    activeX,
    (x, settled) => {
      if (!geometry || !reading) return;
      // Settled, the leader states the reading React rendered. In transit it
      // states the trace where it actually stands, so the terminator stays on
      // the line and the value always matches its own height.
      let y = activeY;
      let value = `${reading.approximate ? "≈" : ""}${formatCompact(reading.value)}`;
      let stamp = shortDate(reading.at).toUpperCase();
      if (!settled) {
        const { vertices } = geometry;
        const { index, fraction } = segmentAtX(vertices, x);
        const a = vertices[index];
        const b = vertices[Math.min(vertices.length - 1, index + 1)];
        y = a.y + (b.y - a.y) * fraction;
        value = `≈${formatCompact(a.value + (b.value - a.value) * fraction)}`;
        stamp = shortDate(a.at + (b.at - a.at) * fraction).toUpperCase();
      }
      dropRef.current?.setAttribute("x1", String(x));
      dropRef.current?.setAttribute("x2", String(x));
      dropRef.current?.setAttribute("y1", String(y));
      leaderRef.current?.setAttribute("x1", String(x));
      leaderRef.current?.setAttribute("y1", String(y));
      leaderRef.current?.setAttribute("y2", String(y));
      headRef.current?.setAttribute("transform", `translate(${x} ${y})`);
      valueRef.current?.setAttribute("y", String(y));
      stampRef.current?.setAttribute("y", String(Math.min(height - 8, y + 15)));
      setTextNode(valueRef.current, value);
      setTextNode(stampRef.current, stamp);
    },
    `${width}:${height}:${axis}:${logScale}:${geometry?.first.at}:${geometry?.last.at}`,
  );

  // The wrapper is rendered either way, so the width measurement is attached
  // before the series has any readings and is already correct on the render
  // where the first two arrive.
  if (!geometry || !reading) {
    return (
      <div ref={rootRef} className={className}>
        <p className="py-6 text-[0.8125rem] text-ink-3">
          Not enough readings to draw this series.
        </p>
      </div>
    );
  }

  const { max, d, length, easing, first, last, span } = geometry;
  const isProbing = probe !== null && interactive;
  /** A new scale, axis or range is a new drawing, so the pen draws it. */
  const drawing = `${axis}:${logScale}:${first.at}:${last.at}`;

  const scaleLabel = `${formatCompact(max)} ${valueLabel.toUpperCase()}${
    logScale ? " · LOG" : ""
  }`;
  const rows = stride(parsed, MAX_ROWS);

  return (
    <div
      ref={rootRef}
      className={className}
      style={{
        ["--duration-draw" as string]: `${milestones ? STAR_DRAW_MS : DRAW_MS}ms`,
      }}
      data-draw
    >
      <svg
        ref={svgRef}
        viewBox={`0 0 ${width} ${height}`}
        width="100%"
        height={height}
        preserveAspectRatio="none"
        role="img"
        aria-labelledby={`${uid}-title`}
        className="block touch-pan-y select-none"
        onPointerMove={interactive ? onPointer : undefined}
        onPointerDown={interactive ? onPointer : undefined}
        onPointerLeave={interactive ? () => setProbe(null) : undefined}
      >
        <title id={`${uid}-title`}>
          {`${valueLabel} from ${fullDate(first.at)} to ${fullDate(last.at)}, peaking at ${valueFormatter(max)}.`}
        </title>

        {/* ── The baseline. It encloses the plot and carries the time span, so
            it measures the drawing's whole x extent. ─────────────────────── */}
        <g stroke="var(--rule-strong)" strokeWidth="1" strokeLinecap="round">
          <line x1={PAD_L} y1={baselineY} x2={plotR} y2={baselineY} />
          {/* Extension lines: the two ticks a dimension springs from. */}
          <line x1={PAD_L} y1={baselineY - 4} x2={PAD_L} y2={baselineY + 9} />
          <line x1={plotR} y1={baselineY - 4} x2={plotR} y2={baselineY + 9} />
          {/* The value extension: it spans the plot's y range, from the
              baseline to the level of the largest reading. */}
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

        {/* The span dimension, drawn below the baseline between the two
            extension ticks, with the measured value lettered on it. */}
        {span && (
          <>
            <g
              className="extends"
              // Measured as the trace finishes, whatever its draw length.
              style={{ ["--draw-delay" as string]: "calc(var(--duration-draw) - 280ms)" }}
            >
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
              fill="var(--ink-3)"
            >
              {/* The value sits ON the dimension line, so the line is cut for
                  it. A paint-order stroke in the paper colour is how a drawing
                  opens a gap in a rule for its own lettering. */}
              <tspan
                stroke="var(--paper)"
                strokeWidth="7"
                paintOrder="stroke"
                strokeLinejoin="round"
              >
                {span}
              </tspan>
            </text>
          </>
        )}

        {/* ── The object: the trace. Emitted complete; the dash offset is the
            only thing that moves. ───────────────────────────────────────── */}
        <g key={drawing}>
          <path
            d={d}
            fill="none"
            stroke="var(--ink)"
            strokeWidth="1.75"
            strokeLinejoin="round"
            strokeLinecap="round"
            className="inks-in"
            style={{
              ["--draw-length" as string]: String(length),
              ["--ease-draw" as string]: easing,
            }}
            vectorEffect="non-scaling-stroke"
          />

          {/* Round-number crossings, projected onto the baseline as the pen
              reaches each one. The figure is lettered at first paint. */}
          {marks.map((mark) => (
            <g
              key={mark.threshold}
              style={{ ["--draw-delay" as string]: `${mark.delay}ms` }}
            >
              <line
                x1={mark.x}
                y1={mark.y}
                x2={mark.x}
                y2={baselineY}
                stroke="var(--ink-3)"
                strokeWidth="0.75"
                strokeDasharray="2 3"
                className="drops"
              />
              <circle
                cx={mark.x}
                cy={mark.y}
                r="2.75"
                fill="var(--paper)"
                stroke="var(--ink)"
                strokeWidth="1"
                className="settles"
              />
              <text
                x={mark.labelX}
                y={mark.labelY}
                textAnchor="end"
                className="font-draft tnum"
                fontSize="10.5"
                letterSpacing="0.08em"
                fill="var(--ink-2)"
              >
                <tspan
                  stroke="var(--paper)"
                  strokeWidth="4"
                  paintOrder="stroke"
                  strokeLinejoin="round"
                >
                  {mark.label}
                </tspan>
              </text>
            </g>
          ))}

          {/* The pen rides the leading edge of the ink and lifts at the end. */}
          <g
            className="pen"
            style={{
              offsetPath: `path("${d}")`,
              ["--ease-draw" as string]: easing,
            }}
            aria-hidden="true"
          >
            <circle r="6" fill="var(--paper)" stroke="var(--ink)" strokeWidth="1" />
            <circle r="2" fill="var(--signal)" />
          </g>
        </g>

        {/* ── The measured value. A leader drops from the datum to the
            baseline and a second runs out to the value at the right margin.
            Both terminate on real points. ───────────────────────────────── */}
        <g>
          {/* The notation waits for the trace: until the pen reaches the
              latest reading there is nothing there to measure. Keyed to the
              drawing, so a redrawn trace is measured again when it lands. */}
          <g
            key={drawing}
            className="measures"
            style={{ ["--draw-delay" as string]: "var(--duration-draw)" }}
          >
            <line
              ref={dropRef}
              x1={activeX}
              y1={activeY}
              x2={activeX}
              y2={baselineY}
              stroke="var(--signal)"
              strokeWidth="1"
              strokeDasharray="3 3"
              strokeLinecap="round"
              opacity={isProbing ? 1 : 0.55}
            />
            <line
              ref={leaderRef}
              x1={activeX}
              y1={activeY}
              x2={plotR + 8}
              y2={activeY}
              stroke="var(--signal)"
              strokeWidth="1"
              strokeLinecap="round"
              opacity={isProbing ? 1 : 0.55}
            />
            {/* The terminator: a filled arrowhead landing on the datum. */}
            <g ref={headRef} transform={`translate(${activeX} ${activeY})`}>
              <path d="M0 0 l-5 -3.2 l0 6.4 z" fill="var(--signal)" />
              <circle r="2.5" fill="var(--signal)" />
            </g>
          </g>

          <text
            ref={valueRef}
            x={plotR + 12}
            y={activeY}
            dy="0.32em"
            className="font-draft tnum"
            fontSize="19"
            fill="var(--signal)"
          >
            {`${reading.approximate ? "≈" : ""}${formatCompact(reading.value)}`}
          </text>
          <text
            ref={stampRef}
            x={plotR + 12}
            y={Math.min(height - 8, activeY + 15)}
            className="font-draft tnum"
            fontSize="10.5"
            letterSpacing="0.08em"
            fill="var(--ink-3)"
          >
            {shortDate(reading.at).toUpperCase()}
          </text>
        </g>

        {/* The origin datum. The drawing states where the object begins. */}
        <g>
          <circle cx={first.x} cy={first.y} r="2" fill="var(--ink-3)" />
          {/* Offset clear of the value extension the origin sits on, so the
              first glyph is not struck through by it. */}
          <text
            x={first.x + 6}
            y={Math.max(PAD_T + 12, first.y - 11)}
            className="font-draft tnum"
            fontSize="10.5"
            letterSpacing="0.08em"
            fill="var(--ink-3)"
          >
            {/* A series that climbs at once runs straight through its own
                origin stamp, so the stamp opens a paper gap in the trace, the
                same cut the span dimension makes in its rule. */}
            <tspan
              stroke="var(--paper)"
              strokeWidth="5"
              paintOrder="stroke"
              strokeLinejoin="round"
            >
              {shortDate(first.at).toUpperCase()}
            </tspan>
          </text>
        </g>
      </svg>

      {/* The reading, announced. The visible statement of it is lettered into
          the drawing above, which a screen reader cannot follow inside an
          image; this says the same words. */}
      <p className="sr-only" aria-live="polite">
        {`${reading.approximate ? "About " : ""}${valueFormatter(reading.value)} ${valueLabel} on ${fullDate(reading.at)}.`}
      </p>

      {/* The reading behind the drawing. A chart that only exists as a picture
          is unreadable to a screen reader and to an agent; the numbers are on
          the page either way, with or without a script. */}
      {/* `sr-only` sits on a wrapper, not on the table: Firefox clips a table's
          inner box but paints its caption, so the caption showed on the sheet. */}
      <div className="sr-only">
        <table>
          <caption>{`${valueLabel} by date`}</caption>
          <thead>
            <tr>
              <th scope="col">Date</th>
              <th scope="col">{valueLabel}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.date}>
                <td>{fullDate(row.at)}</td>
                <td>{valueFormatter(row.value)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
