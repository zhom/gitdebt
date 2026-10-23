/**
 * Three small technical drawings of the three things this product renders.
 *
 * What was here before was decoration wearing a chart's clothes: a shaded
 * blob on a looping animation, five motion tracks, an IntersectionObserver to
 * stop them, and nothing on the page that any of it measured.
 *
 * These are drawings instead. Each one is the notation the real asset uses,
 * reduced to the smallest form that still reads:
 *
 *   stars   a trace over a dimensioned baseline, with a leader dropping from
 *           the last datum — the star sheet, miniaturised.
 *   health  three gauges springing from one extension line, each terminating
 *           in an arrowhead at its own measured value.
 *   readme  a sheet with the drawing placed in it, below the prose.
 *
 * They are illustrative and they say so at the call site; no figure here is a
 * repository's value. Each one draws itself the way the real asset does — the
 * trace inks in, the gauges extend, the terminators land — over a drawing that
 * is complete in the markup, so nothing waits for a script or an observer.
 */

type Kind = "stars" | "health" | "readme";

const LABEL: Record<Kind, string> = {
  stars: "A star-history trace measured against its baseline",
  health: "Three repository-health readings, each measured to its own value",
  readme: "A README sheet with the star-history drawing placed in it",
};

/** The three gauge values in the health drawing. Fixed, and not a measurement. */
const GAUGES = [
  { y: 20, to: 132 },
  { y: 39, to: 96 },
  { y: 58, to: 116 },
];

/** The illustrative trace, as points, so the polyline is written once. */
const TRACE = [
  [10, 52],
  [38, 49],
  [62, 44],
  [88, 38],
  [112, 30],
  [140, 21],
  [166, 12],
] as const;

const TRACE_D = TRACE.map(
  ([x, y], i) => `${i === 0 ? "M" : "L"}${x} ${y}`,
).join(" ");

/** Summed from the points, never measured from the DOM. */
const TRACE_LENGTH = Math.ceil(
  TRACE.slice(1).reduce(
    (total, [x, y], i) => total + Math.hypot(x - TRACE[i][0], y - TRACE[i][1]),
    0,
  ),
);

const DOC_TRACE = [
  [27, 49],
  [57, 46],
  [86, 41],
  [116, 40],
  [149, 37],
] as const;
const DOC_TRACE_LENGTH = Math.ceil(
  DOC_TRACE.slice(1).reduce(
    (total, [x, y], i) =>
      total + Math.hypot(x - DOC_TRACE[i][0], y - DOC_TRACE[i][1]),
    0,
  ),
);

const delay = (ms: number | string) => ({
  ["--draw-delay" as string]: typeof ms === "number" ? `${ms}ms` : ms,
});

export function ReportLayerGraphic({ kind }: { kind: Kind }) {
  const last = TRACE[TRACE.length - 1];

  return (
    <svg
      data-draw
      viewBox="0 0 176 72"
      role="img"
      aria-label={LABEL[kind]}
      className="block h-16 w-40 max-w-full"
      fill="none"
    >
      {kind === "stars" && (
        <g>
          {/* The baseline, and the two extension ticks a dimension springs
              from. It encloses the trace, so it measures its whole extent. */}
          <g stroke="var(--rule-strong)" strokeWidth="1" strokeLinecap="round">
            <path d="M10 62H166" />
            <path d="M10 58v8" />
            <path d="M166 58v8" />
          </g>
          <path
            d={TRACE_D}
            stroke="var(--ink)"
            strokeWidth="1.5"
            strokeLinejoin="round"
            strokeLinecap="round"
            className="inks-in"
            style={{ ["--draw-length" as string]: String(TRACE_LENGTH) }}
          />
          <circle cx={TRACE[0][0]} cy={TRACE[0][1]} r="1.6" fill="var(--ink-3)" />
          {/* The measured value: a leader drops from the last real datum to the
              baseline, and the terminator lands on the datum itself — once the
              trace has reached it. */}
          <g className="measures" style={delay("var(--duration-draw)")}>
            <path
              d={`M${last[0]} ${last[1]}V62`}
              stroke="var(--signal)"
              strokeWidth="1"
              strokeDasharray="3 3"
              strokeLinecap="round"
            />
            <path
              d={`M${last[0]} ${last[1]} l-4.5 -2.8 l0 5.6 z`}
              fill="var(--signal)"
            />
          </g>
        </g>
      )}

      {kind === "health" && (
        <g>
          {/* One extension line: every gauge is measured from the same datum,
              which is the whole point of reading four things off one history. */}
          <path
            d="M10 12V66"
            stroke="var(--rule-strong)"
            strokeWidth="1"
            strokeLinecap="round"
          />
          {GAUGES.map((gauge, index) => (
            <g key={gauge.y}>
              <path
                d={`M10 ${gauge.y}H166`}
                stroke="var(--rule)"
                strokeWidth="1"
                strokeLinecap="round"
              />
              <path
                d={`M10 ${gauge.y}H${gauge.to - 5}`}
                stroke={index === 0 ? "var(--signal)" : "var(--ink)"}
                strokeWidth="1.5"
                strokeLinecap="round"
                className="extends"
                style={delay(index * 110)}
              />
              <path
                d={`M${gauge.to} ${gauge.y} l-5 -3 l0 6 z`}
                fill={index === 0 ? "var(--signal)" : "var(--ink)"}
                className="lands"
                style={delay(index * 110 + 200)}
              />
            </g>
          ))}
        </g>
      )}

      {kind === "readme" && (
        <g>
          {/* The sheet. */}
          <rect
            x="10.5"
            y="6.5"
            width="155"
            height="59"
            stroke="var(--rule-strong)"
            strokeWidth="1"
          />
          {/* Prose, at three unequal measures. */}
          <g stroke="var(--ink-3)" strokeWidth="1.5" strokeLinecap="round">
            <path d="M20 17h58" />
            <path d="M20 25h96" />
          </g>
          {/* The drawing, placed below the prose: its own frame, its own
              baseline, its own trace. */}
          <rect
            x="20.5"
            y="33.5"
            width="135"
            height="23"
            stroke="var(--rule)"
            strokeWidth="1"
          />
          <path
            d="M27 52H149"
            stroke="var(--rule-strong)"
            strokeWidth="1"
            strokeLinecap="round"
          />
          <path
            d={DOC_TRACE.map(([x, y], i) => `${i === 0 ? "M" : "L"}${x} ${y}`).join(" ")}
            stroke="var(--ink)"
            strokeWidth="1.5"
            strokeLinejoin="round"
            strokeLinecap="round"
            className="inks-in"
            style={{
              ["--draw-length" as string]: String(DOC_TRACE_LENGTH),
              ...delay(240),
            }}
          />
          <circle
            cx="149"
            cy="37"
            r="1.8"
            fill="var(--signal)"
            className="settles"
            style={delay("calc(240ms + var(--duration-draw))")}
          />
        </g>
      )}
    </svg>
  );
}
