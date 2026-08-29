import { useId, useMemo, useState } from "react";

import { CAPTION, DATUM, FIELD } from "@/components/style-tokens";
import { FIELD_CELL, FIELD_ROWS } from "@/components/StatStrip";
import {
  layoutFileCouplings,
  type CouplingEdge,
  type FileCoupling,
} from "@/lib/repo-signal-visuals";
import { cn } from "@/lib/utils";

/**
 * Files that change together, drawn as an assembly.
 *
 * Nodes are files, edges are the commits that changed two of them at once, and
 * the relationship being read is dimensioned: extension lines spring from both
 * files, a dimension line spans them, and it carries the number of co-changes.
 * Only one relationship is dimensioned at a time, because a drawing that
 * dimensions everything at once measures nothing.
 *
 * The positions are `layoutFileCouplings()`'s, unchanged, so a repository keeps
 * the same assembly between renders. What changed is the drawing: this was a
 * canvas painting textured nodes, travelling packets and a shimmer at sixty
 * frames a second, with a hand-rolled nearest-node search standing in for hit
 * testing, and it rendered nothing at all without JavaScript. Nodes and edges
 * are now real shapes with real pointer targets, present in the markup.
 */

export type FileCouplingNetworkProps = {
  couplings: readonly FileCoupling[];
  /** Retained for call-site compatibility; the drawing is deterministic. */
  seed?: string;
  className?: string;
};

const VIEW_W = 640;
const VIEW_H = 340;
const PAD_X = 56;
const PAD_Y = 34;

const compact = (value: number) =>
  new Intl.NumberFormat("en", {
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(value);

function tail(path: string): string {
  return path.split("/").at(-1) || path;
}

function edgeLabel(edge: CouplingEdge): string {
  return `${tail(edge.source)} ↔ ${tail(edge.target)}`;
}

export function FileCouplingNetwork({
  couplings,
  className,
}: FileCouplingNetworkProps) {
  const uid = useId();
  const layout = useMemo(() => layoutFileCouplings(couplings), [couplings]);
  // Edges arrive sorted by fix pressure then strength, so index 0 is the
  // relationship the drawing should be dimensioning when nobody has touched it.
  // Nothing cycles: an automatic highlight that moves on its own is a drawing
  // that changes its mind while you are reading it.
  const [activeIndex, setActiveIndex] = useState(0);

  const placed = useMemo(() => {
    const maxWeight = Math.max(1, ...layout.nodes.map((node) => node.weight));
    const points = new Map<string, { x: number; y: number; size: number }>();
    if (layout.nodes.length === 0) return points;

    // The layout works in 0..1 but only ever uses the part of it its clusters
    // need — a repository whose couplings all live in one directory occupies a
    // patch in the middle. The assembly is fitted to the sheet so it is drawn
    // at a readable size, scaled equally on both axes so the shape the layout
    // computed is the shape that gets drawn.
    const xs = layout.nodes.map((node) => node.x);
    const ys = layout.nodes.map((node) => node.y);
    const minX = Math.min(...xs);
    const minY = Math.min(...ys);
    const spanX = Math.max(0.02, Math.max(...xs) - minX);
    const spanY = Math.max(0.02, Math.max(...ys) - minY);
    const plotW = VIEW_W - PAD_X * 2;
    const plotH = VIEW_H - PAD_Y * 2;
    const scale = Math.min(plotW / spanX, plotH / spanY);
    const offsetX = PAD_X + (plotW - spanX * scale) / 2;
    const offsetY = PAD_Y + (plotH - spanY * scale) / 2;

    for (const node of layout.nodes) {
      points.set(node.id, {
        x: offsetX + (node.x - minX) * scale,
        y: offsetY + (node.y - minY) * scale,
        size: 5 + Math.sqrt(node.weight / maxWeight) * 5,
      });
    }
    return points;
  }, [layout]);

  /** The strongest relationship a given file takes part in. */
  const strongestFor = useMemo(() => {
    const byFile = new Map<string, number>();
    layout.edges.forEach((edge, index) => {
      if (!byFile.has(edge.source)) byFile.set(edge.source, index);
      if (!byFile.has(edge.target)) byFile.set(edge.target, index);
    });
    return byFile;
  }, [layout]);

  if (layout.edges.length === 0) {
    return (
      <p className={cn(CAPTION, "p-4", className)}>
        No repeated file relationships were strong enough to draw.
      </p>
    );
  }

  const index = Math.min(activeIndex, layout.edges.length - 1);
  const active = layout.edges[index];
  const a = placed.get(active.source);
  const b = placed.get(active.target);

  /** The dimension across the relationship being read. */
  const dimension = (() => {
    if (!a || !b) return null;
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const length = Math.hypot(dx, dy) || 1;
    const ux = dx / length;
    const uy = dy / length;
    // Normal, always chosen to point up the sheet, so the value never lands
    // underneath its own dimension line.
    let nx = -uy;
    let ny = ux;
    if (ny > 0) {
      nx = -nx;
      ny = -ny;
    }
    const off = 17;
    const from = { x: a.x + nx * off, y: a.y + ny * off };
    const to = { x: b.x + nx * off, y: b.y + ny * off };
    const mid = { x: (from.x + to.x) / 2, y: (from.y + to.y) / 2 };
    const head = (at: { x: number; y: number }, sx: number, sy: number) =>
      `M${at.x.toFixed(2)} ${at.y.toFixed(2)} L${(at.x + sx * 8 - nx * 2.6).toFixed(2)} ${(at.y + sy * 8 - ny * 2.6).toFixed(2)} L${(at.x + sx * 8 + nx * 2.6).toFixed(2)} ${(at.y + sy * 8 + ny * 2.6).toFixed(2)} z`;
    // Where the value is lettered. A dimension offset upward carries it
    // centred above the line; one offset sideways — which is what a near
    // vertical relationship gives — carries it beside the line instead, so the
    // lettering never sits on top of the line it belongs to.
    // Past 0.72 the relationship is within about 45° of vertical, which is
    // where a centred value starts to sit on its own dimension line.
    const sideways = Math.abs(nx) > 0.72;
    const anchor: "start" | "middle" | "end" = sideways
      ? nx < 0
        ? "end"
        : "start"
      : "middle";
    const reach = sideways ? 8 : 11;
    const rawX = mid.x + nx * reach;
    const label = {
      x:
        anchor === "middle"
          ? Math.min(VIEW_W - 110, Math.max(110, rawX))
          : anchor === "end"
            ? Math.min(VIEW_W - 8, Math.max(132, rawX))
            : Math.min(VIEW_W - 132, Math.max(8, rawX)),
      y: mid.y + ny * reach,
      anchor,
    };
    return {
      from,
      to,
      mid,
      nx,
      ny,
      label,
      // Arrowheads point inward along the dimension line, the way a short
      // dimension is terminated when the value will not fit between them.
      heads: [head(from, ux, uy), head(to, -ux, -uy)],
    };
  })();

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
          className="block min-w-[460px] touch-pan-y select-none"
        >
          <title id={`${uid}-title`}>
            {`${layout.nodes.length} files joined by ${layout.edges.length} repeated co-change relationships.`}
          </title>

          {/* The relationship being read is drawn last, so no quiet edge is
              ever laid across it. */}
          {layout.edges
            .map((edge, edgeIndex) => ({ edge, edgeIndex }))
            .sort(
              (a, b) =>
                Number(a.edgeIndex === index) - Number(b.edgeIndex === index),
            )
            .map(({ edge, edgeIndex }) => {
              const from = placed.get(edge.source);
              const to = placed.get(edge.target);
              if (!from || !to) return null;
              const selected = edgeIndex === index;
              const span = Math.ceil(Math.hypot(to.x - from.x, to.y - from.y));
              return (
                <line
                  key={`${edge.source}\0${edge.target}`}
                  x1={from.x}
                  y1={from.y}
                  x2={to.x}
                  y2={to.y}
                  stroke={selected ? "var(--signal)" : "var(--rule-strong)"}
                  strokeWidth={selected ? 1.75 : 0.75 + edge.strength * 0.85}
                  strokeLinecap="round"
                  className="inks-in"
                  style={{
                    ["--draw-length" as string]: String(span),
                    ["--draw-delay" as string]: `${Math.min(edgeIndex, 8) * 45}ms`,
                  }}
                />
              );
            })}

          {/* The measured relationship. Extension lines spring from both files,
              the dimension line spans them, and it carries the count. */}
          {dimension && a && b && (
            <g>
              <line
                x1={a.x}
                y1={a.y}
                x2={dimension.from.x + dimension.nx * 5}
                y2={dimension.from.y + dimension.ny * 5}
                stroke="var(--signal)"
                strokeWidth="1"
              />
              <line
                x1={b.x}
                y1={b.y}
                x2={dimension.to.x + dimension.nx * 5}
                y2={dimension.to.y + dimension.ny * 5}
                stroke="var(--signal)"
                strokeWidth="1"
              />
              <line
                x1={dimension.from.x}
                y1={dimension.from.y}
                x2={dimension.to.x}
                y2={dimension.to.y}
                stroke="var(--signal)"
                strokeWidth="1"
              />
              {dimension.heads.map((head) => (
                <path key={head} d={head} fill="var(--signal)" />
              ))}
              <text
                x={dimension.label.x}
                y={dimension.label.y}
                textAnchor={dimension.label.anchor}
                className="font-draft"
                fontSize="17"
                fill="var(--signal)"
                style={{ fontVariantNumeric: "tabular-nums" }}
              >
                <tspan
                  stroke="var(--paper)"
                  strokeWidth="6"
                  paintOrder="stroke"
                  strokeLinejoin="round"
                >
                  {`${compact(active.cochanges)} CO-CHANGES`}
                </tspan>
              </text>
              <text
                x={dimension.label.x}
                y={dimension.label.y + 15}
                textAnchor={dimension.label.anchor}
                className="font-draft"
                fontSize="11"
                letterSpacing="0.06em"
                fill="var(--ink-3)"
                style={{ fontVariantNumeric: "tabular-nums" }}
              >
                <tspan
                  stroke="var(--paper)"
                  strokeWidth="5"
                  paintOrder="stroke"
                  strokeLinejoin="round"
                >
                  {`${compact(active.fixCommits)} FIX-LABELLED`}
                </tspan>
              </text>
            </g>
          )}

          {layout.nodes.map((node) => {
            const point = placed.get(node.id);
            if (!point) return null;
            const selected =
              node.id === active.source || node.id === active.target;
            return (
              <rect
                key={node.id}
                x={point.x - point.size / 2}
                y={point.y - point.size / 2}
                width={point.size}
                height={point.size}
                fill={selected ? "var(--signal)" : "var(--ink-2)"}
              />
            );
          })}

          {/* Only the two files being measured are named. Naming all fourteen
              at once is how a drawing turns into a word cloud. */}
          {[active.source, active.target].map((id) => {
            const point = placed.get(id);
            if (!point) return null;
            const node = layout.nodes.find((item) => item.id === id);
            if (!node) return null;
            // Clear of both sheet edges by half a full-length filename, so a
            // long name at an extreme node cannot run off the drawing.
            const x = Math.min(VIEW_W - 80, Math.max(80, point.x));
            // Under the file wherever there is room, because the dimension is
            // carried above or beside its line and never below it.
            const below = point.y < VIEW_H - 26;
            return (
              <text
                key={`${id}-label`}
                x={x}
                y={below ? point.y + point.size / 2 + 17 : point.y - point.size / 2 - 9}
                textAnchor="middle"
                className="font-mono"
                fontSize="11.5"
                fill="var(--ink)"
              >
                <tspan
                  stroke="var(--paper)"
                  strokeWidth="5"
                  paintOrder="stroke"
                  strokeLinejoin="round"
                >
                  {node.label}
                </tspan>
              </text>
            );
          })}

          {/* Hit targets, over the drawing. An edge is a line the pointer can
              land on and a file is a square it can land on; neither needs a
              nearest-point search written by hand. */}
          {layout.edges.map((edge, edgeIndex) => {
            const from = placed.get(edge.source);
            const to = placed.get(edge.target);
            if (!from || !to) return null;
            return (
              <line
                key={`${edge.source}\0${edge.target}-hit`}
                x1={from.x}
                y1={from.y}
                x2={to.x}
                y2={to.y}
                stroke="transparent"
                strokeWidth="14"
                pointerEvents="stroke"
                onPointerEnter={() => setActiveIndex(edgeIndex)}
              />
            );
          })}
          {layout.nodes.map((node) => {
            const point = placed.get(node.id);
            const target = strongestFor.get(node.id);
            if (!point || target === undefined) return null;
            return (
              <circle
                key={`${node.id}-hit`}
                cx={point.x}
                cy={point.y}
                r="15"
                fill="transparent"
                onPointerEnter={() => setActiveIndex(target)}
              />
            );
          })}
        </svg>
      </div>

      {/* The four strongest relationships, on one grid. Each cell is the same
          three rows, so the pair, the count and the fix share line up across
          all four however long a filename runs. */}
      <figcaption
        className={cn("grid border-t border-rule sm:grid-cols-2", FIELD_ROWS)}
      >
        {layout.edges.slice(0, 4).map((edge, cellIndex) => (
          <button
            key={`${edge.source}\0${edge.target}`}
            type="button"
            aria-label={`${edge.source} and ${edge.target}: ${edge.cochanges} co-changes and ${edge.fixCommits} fix commits`}
            aria-pressed={cellIndex === index}
            title={`${edge.source} ↔ ${edge.target}`}
            onClick={() => setActiveIndex(cellIndex)}
            onPointerEnter={() => setActiveIndex(cellIndex)}
            onFocus={() => setActiveIndex(cellIndex)}
            className={cn(
              FIELD_CELL,
              "min-h-11 p-4 text-left outline-none transition-colors duration-[--duration-ui] hover:bg-table focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-signal aria-pressed:bg-table",
              cellIndex > 0 && "border-t border-rule",
              cellIndex === 1 && "sm:border-t-0",
              cellIndex % 2 === 1 && "sm:border-l sm:border-rule",
            )}
          >
            <span className={FIELD}>Co-change</span>
            {/* The cell being read is marked by its ground stepping to the
                table, not by red ink: a pair of filenames is the subject of a
                measurement, never the measurement itself. */}
            <span className={cn(DATUM, "mt-2.5 block truncate text-[0.875rem] text-ink")}>
              {edgeLabel(edge)}
            </span>
            <span className={cn(CAPTION, "mt-2")}>
              {`${compact(edge.cochanges)} co-changes · ${compact(edge.fixCommits)} fix-labelled`}
            </span>
          </button>
        ))}
      </figcaption>

      {/* Every relationship in the drawing, as text. A picture of a graph is
          unreadable to a screen reader and to an agent; these are not. */}
      <ul role="list" className="sr-only">
        {layout.edges.map((edge) => (
          <li key={`${edge.source}\0${edge.target}-accessible`}>
            {edge.source} and {edge.target}: {edge.cochanges} co-changes,{" "}
            {edge.fixCommits} fix commits.
          </li>
        ))}
      </ul>

      <p className={cn(CAPTION, "px-4 pt-3 pb-4")} aria-live="polite">
        {`${active.source} and ${active.target} changed together ${active.cochanges.toLocaleString()} times; ${active.fixCommits.toLocaleString()} of the coupled changes were fix commits.`}
      </p>
    </figure>
  );
}
