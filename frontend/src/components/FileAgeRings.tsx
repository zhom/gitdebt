import { useEffect, useId, useMemo, useState } from "react";

import { CAPTION, FIELD, FIGURE } from "@/components/style-tokens";
import { FIELD_CELL, FIELD_ROWS } from "@/components/StatStrip";
import {
  AGE_LABEL,
  layoutAgeRings,
  type AgeRing,
  type FileAgeBand,
} from "@/lib/repo-signal-visuals";
import { cn } from "@/lib/utils";

/**
 * File age against change frequency, drawn as an angular dimension stack.
 *
 * Four concentric bands, one per age range, all swept clockwise from a single
 * datum at twelve o'clock. How far a band sweeps is its share of the tracked
 * files; how heavily it is drawn is how often those files change. Each band
 * carries a leader out to its own label and value, and the leader terminates on
 * the arc's end — the measured extent itself, never a point in space.
 *
 * The radii, shares and intensities are `layoutAgeRings()`'s, unchanged. What
 * changed is the drawing: this was a 2D canvas painting four rings pixel by
 * pixel at sixty frames a second, which rendered a blank square until the
 * script ran, offered a screen reader nothing, and hit-tested the pointer by
 * hand. It is now SVG that is complete in the markup, and the bands are real
 * shapes the pointer can simply land on.
 */

export type FileAgeRingsProps = {
  bands: readonly FileAgeBand[];
  className?: string;
};

const VIEW_W = 520;
const VIEW_H = 330;
const CX = 148;
const CY = 165;
/** Pixels per unit radius. `layoutAgeRings` works in 0..0.96. */
const SCALE = 138;
/** Where every leader's shoulder ends and its lettering begins. */
const ELBOW_X = 288;
const SHOULDER_X = 312;
const LABEL_X = 322;
/** Row pitch of the label stack. Four rows, all on one grid. */
const ROW_Y = [52, 122, 192, 262];

const compact = (value: number) =>
  new Intl.NumberFormat("en", {
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(value);

type Point = { x: number; y: number };

/** A point on a ring, `turn` clockwise from the twelve o'clock datum. */
function onRing(radius: number, turn: number): Point {
  const angle = -Math.PI / 2 + turn * Math.PI * 2;
  return {
    x: CX + Math.cos(angle) * radius,
    y: CY + Math.sin(angle) * radius,
  };
}

/** The swept arc, as a path. A full turn is drawn as two half arcs. */
function arcPath(radius: number, turn: number): string {
  const clamped = Math.min(1, Math.max(0, turn));
  if (clamped <= 0) return "";
  const start = onRing(radius, 0);
  if (clamped >= 0.999) {
    const half = onRing(radius, 0.5);
    return `M${start.x.toFixed(2)} ${start.y.toFixed(2)} A${radius} ${radius} 0 1 1 ${half.x.toFixed(2)} ${half.y.toFixed(2)} A${radius} ${radius} 0 1 1 ${start.x.toFixed(2)} ${start.y.toFixed(2)}`;
  }
  const end = onRing(radius, clamped);
  const large = clamped > 0.5 ? 1 : 0;
  return `M${start.x.toFixed(2)} ${start.y.toFixed(2)} A${radius} ${radius} 0 ${large} 1 ${end.x.toFixed(2)} ${end.y.toFixed(2)}`;
}

/** A filled arrowhead landing on `at`, pointing away from `from`. */
function terminator(at: Point, from: Point): string {
  const dx = at.x - from.x;
  const dy = at.y - from.y;
  const length = Math.hypot(dx, dy) || 1;
  const ux = dx / length;
  const uy = dy / length;
  const baseX = at.x - ux * 7.5;
  const baseY = at.y - uy * 7.5;
  return `M${at.x.toFixed(2)} ${at.y.toFixed(2)} L${(baseX - uy * 2.6).toFixed(2)} ${(baseY + ux * 2.6).toFixed(2)} L${(baseX + uy * 2.6).toFixed(2)} ${(baseY - ux * 2.6).toFixed(2)} z`;
}

type Geometry = {
  ring: AgeRing;
  index: number;
  mid: number;
  band: number;
  /** Weight of the swept arc: thin construction to a heavy object line. */
  weight: number;
  end: Point;
  elbow: Point;
  rowY: number;
  length: number;
};

export function FileAgeRings({ bands, className }: FileAgeRingsProps) {
  const uid = useId();
  const rings = useMemo(() => layoutAgeRings(bands), [bands]);
  const hottest = useMemo(() => {
    let index = 0;
    for (let next = 1; next < rings.length; next += 1) {
      if (rings[next].changeIntensity > rings[index].changeIntensity) {
        index = next;
      }
    }
    return index;
  }, [rings]);
  const [activeIndex, setActiveIndex] = useState(hottest);

  useEffect(() => {
    setActiveIndex(hottest);
  }, [hottest]);

  const totalFiles = rings.reduce((sum, ring) => sum + ring.files, 0);
  const active = rings[activeIndex] ?? rings[0];

  const geometry = useMemo<Geometry[]>(() => {
    const ends = rings.map((ring) => {
      const mid = ((ring.innerRadius + ring.outerRadius) / 2) * SCALE;
      return { mid, end: onRing(mid, ring.fileShare) };
    });
    // Rows are handed out down the sheet in the order the arcs end down the
    // plate, so no leader ever has to cross another one to reach its label.
    // Which row a band lands in therefore follows the drawing, not the list.
    const order = rings
      .map((_, index) => index)
      .sort((a, b) => ends[a].end.y - ends[b].end.y || a - b);
    const rowFor = new Map(order.map((index, row) => [index, ROW_Y[row]]));

    return rings.map((ring, index) => {
      const { mid, end } = ends[index];
      const rowY = rowFor.get(index) ?? ROW_Y[ROW_Y.length - 1];
      return {
        ring,
        index,
        mid,
        band: (ring.outerRadius - ring.innerRadius) * SCALE,
        weight: 3 + ring.changeIntensity * 6,
        end,
        elbow: { x: ELBOW_X, y: rowY },
        rowY,
        length: Math.ceil(2 * Math.PI * mid * ring.fileShare) || 1,
      };
    });
  }, [rings]);

  if (totalFiles === 0) {
    return (
      <p className={cn(CAPTION, "p-4", className)}>
        No file ages have been recorded for this repository yet.
      </p>
    );
  }

  const datumTop = CY - rings[rings.length - 1].outerRadius * SCALE;
  const datumBottom = CY - rings[0].innerRadius * SCALE;
  const ordered = [...geometry].sort(
    (a, b) =>
      Number(a.index === activeIndex) - Number(b.index === activeIndex),
  );

  return (
    <figure className={cn("min-w-0", className)}>
      <div className="overflow-x-auto p-4">
        <svg
          viewBox={`0 0 ${VIEW_W} ${VIEW_H}`}
          width="100%"
          height={VIEW_H}
          preserveAspectRatio="xMidYMid meet"
          role="img"
          aria-labelledby={`${uid}-title`}
          className="block min-w-[420px] touch-pan-y select-none"
        >
          <title id={`${uid}-title`}>
            {`How ${compact(totalFiles)} tracked files divide across four age ranges, and how often each range changes.`}
          </title>

          {/* The datum. Every sweep on this plate starts on this line, which is
              what makes four arcs at four radii one comparable measurement. */}
          <line
            x1={CX}
            y1={datumTop - 8}
            x2={CX}
            y2={datumBottom}
            stroke="var(--rule-strong)"
            strokeWidth="1"
          />

          {/* The band being read is drawn last, so nothing quiet is ever laid
              across the reading. */}
          {ordered.map(({ ring, index, mid, weight, end, length }) => {
            const selected = index === activeIndex;
            const inner = onRing(ring.innerRadius * SCALE, ring.fileShare);
            const outer = onRing(ring.outerRadius * SCALE, ring.fileShare);
            return (
              <g key={ring.range}>
                {/* The band's full extent: what the share is a share OF. */}
                <circle
                  cx={CX}
                  cy={CY}
                  r={mid}
                  fill="none"
                  stroke="var(--rule)"
                  strokeWidth="1"
                />
                {ring.fileShare > 0 && (
                  <path
                    d={arcPath(mid, ring.fileShare)}
                    fill="none"
                    stroke={selected ? "var(--signal)" : "var(--ink-2)"}
                    strokeWidth={weight}
                    strokeLinecap="butt"
                    className="inks-in"
                    style={{
                      ["--draw-length" as string]: String(length),
                      ["--draw-delay" as string]: `${index * 90}ms`,
                    }}
                  />
                )}
                {/* The extension tick at the end of the sweep: the second of
                    the two points this angular dimension spans. */}
                <line
                  x1={inner.x}
                  y1={inner.y}
                  x2={outer.x}
                  y2={outer.y}
                  stroke={selected ? "var(--signal)" : "var(--rule-strong)"}
                  strokeWidth="1"
                />
                <circle
                  cx={end.x}
                  cy={end.y}
                  r={selected ? 0 : 1.8}
                  fill="var(--ink-3)"
                />
              </g>
            );
          })}

          {/* The leaders. Each runs from the end of its own arc out to the row
              that letters that band, and the active one is the measured
              reading: drafting red, with an arrowhead landing on the arc. */}
          {ordered.map(({ ring, index, end, elbow, rowY }) => {
            const selected = index === activeIndex;
            return (
              <g key={`${ring.range}-leader`}>
                <path
                  d={`M${end.x.toFixed(2)} ${end.y.toFixed(2)} L${elbow.x} ${elbow.y} L${SHOULDER_X} ${rowY}`}
                  fill="none"
                  stroke={selected ? "var(--signal)" : "var(--rule-strong)"}
                  strokeWidth="1"
                  strokeLinejoin="round"
                />
                {selected && (
                  <path d={terminator(end, elbow)} fill="var(--signal)" />
                )}
              </g>
            );
          })}

          {/* The lettering. Four rows, one grid: the name, the figure and the
              note sit on the same three baselines in every row. */}
          {geometry.map(({ ring, index, rowY }) => {
            const selected = index === activeIndex;
            return (
              <g key={`${ring.range}-label`}>
                <text
                  x={LABEL_X}
                  y={rowY}
                  className="font-draft"
                  fontSize="12.5"
                  letterSpacing="0.08em"
                  fill="var(--ink-3)"
                >
                  {AGE_LABEL[ring.range].toUpperCase()}
                </text>
                <text
                  x={LABEL_X}
                  y={rowY + 27}
                  className="font-draft"
                  fontSize="21"
                  fill={selected ? "var(--signal)" : "var(--ink)"}
                  style={{ fontVariantNumeric: "tabular-nums" }}
                >
                  {compact(ring.files)}
                  <tspan fontSize="12.5" fill="var(--ink-3)" dx="6">
                    FILES
                  </tspan>
                </text>
                <text
                  x={LABEL_X}
                  y={rowY + 45}
                  className="font-draft"
                  fontSize="10.5"
                  letterSpacing="0.04em"
                  fill="var(--ink-3)"
                  style={{ fontVariantNumeric: "tabular-nums" }}
                >
                  {/* Kept short on the plate so it cannot run past the sheet
                      edge in a fallback face. The cell below spells it out. */}
                  {`${Math.round(ring.fileShare * 100)}% · ${ring.changeRate.toFixed(1)}/FILE`}
                </text>
              </g>
            );
          })}

          {/* The total, dead centre of the plate the four bands divide. */}
          <text
            x={CX}
            y={CY - 7}
            textAnchor="middle"
            dominantBaseline="central"
            className="font-draft"
            fontSize="22"
            fill="var(--ink)"
            style={{ fontVariantNumeric: "tabular-nums" }}
          >
            {compact(totalFiles)}
          </text>
          <text
            x={CX}
            y={CY + 12}
            textAnchor="middle"
            dominantBaseline="central"
            className="font-draft"
            fontSize="10.5"
            letterSpacing="0.12em"
            fill="var(--ink-3)"
          >
            FILES
          </text>

          {/* Hit targets last, so they sit over the drawing. The band itself is
              the target — there is no hand-rolled nearest-point search any
              more, because a stroked arc is a shape the pointer can land on. */}
          {geometry.map(({ ring, index, mid, band }) => (
            <circle
              key={`${ring.range}-hit`}
              cx={CX}
              cy={CY}
              r={mid}
              fill="none"
              stroke="transparent"
              strokeWidth={Math.max(14, band)}
              pointerEvents="stroke"
              onPointerEnter={() => setActiveIndex(index)}
            />
          ))}
        </svg>
      </div>

      {/* The reading, in text, on a shared grid. These are the touch targets
          and the focus targets; the plate above is the pointer's version of the
          same four cells. */}
      <figcaption
        className={cn("grid border-t border-rule sm:grid-cols-2", FIELD_ROWS)}
      >
        {rings.map((ring, index) => (
          <button
            key={ring.range}
            type="button"
            aria-pressed={index === activeIndex}
            onPointerEnter={() => setActiveIndex(index)}
            onFocus={() => setActiveIndex(index)}
            onClick={() => setActiveIndex(index)}
            className={cn(
              FIELD_CELL,
              "min-h-11 p-4 text-left outline-none transition-colors duration-[--duration-ui] hover:bg-table focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-signal aria-pressed:bg-table",
              // Each rule separates two real cells and no rule lands on the
              // outside edge, which is the frame's job rather than a cell's.
              index > 0 && "border-t border-rule",
              index === 1 && "sm:border-t-0",
              index % 2 === 1 && "sm:border-l sm:border-rule",
            )}
          >
            <span className={FIELD}>{AGE_LABEL[ring.range]}</span>
            <span
              className={cn(
                FIGURE,
                "mt-2.5",
                index === activeIndex ? "text-signal" : "text-ink",
              )}
            >
              {compact(ring.files)}
            </span>
            <span className={cn(CAPTION, "mt-2")}>
              {`${Math.round(ring.fileShare * 100)}% of files · ${ring.changeRate.toFixed(1)} changes per file`}
            </span>
          </button>
        ))}
      </figcaption>

      <p className={cn(CAPTION, "px-4 pt-3 pb-4")} aria-live="polite">
        {active
          ? `${AGE_LABEL[active.range]}: ${active.files.toLocaleString()} files and ${active.changes.toLocaleString()} recorded changes. A heavier arc is a range whose files change more often.`
          : "File ages are unavailable."}
      </p>
    </figure>
  );
}
