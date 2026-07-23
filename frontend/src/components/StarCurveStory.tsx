import { useEffect, useMemo, useRef, useState } from "react";

import { BODY } from "@/components/style-tokens";
import {
  BAYER4,
  BRAND,
  OFF_TIER,
  RasterBuffer,
  clamp01,
  type RGB,
} from "@/lib/dither";
import {
  flowAroundBoundary,
  isBrowser,
  metricsFor,
  type FlowedLine,
} from "@/lib/pretext";
import { cn } from "@/lib/utils";

export type StoryPoint = { date: string; value: number };

type Props = {
  /** The prose to flow around the curve. Rendered verbatim for assistive tech. */
  text: string;
  /** The repo's cumulative star series. Fewer than two points → plain prose. */
  points: StoryPoint[];
  height?: number;
  fill?: RGB;
  className?: string;
};

/** 1 CSS px : 0.5 canvas cell, matched to the rest of the dither system. */
const CELL = 2;
/** Insets for the flowed text, in px. `right` keeps lines off the curve edge. */
const PAD = { left: 2, top: 4, right: 14, bottom: 8 };
/** Density at the curve contour; the fill ramps from here to 1 at the floor. */
const RAMP = 0.66;

type Parsed = { at: number; value: number };

function parseSeries(points: StoryPoint[]): Parsed[] {
  return points
    .map((point) => ({ at: Date.parse(point.date), value: point.value }))
    .filter((p) => Number.isFinite(p.at) && Number.isFinite(p.value))
    .sort((a, b) => a.at - b.at);
}

/** Linear-interpolated value at time-fraction `u` across the series range. */
function valueAt(parsed: Parsed[], u: number): number {
  const first = parsed[0];
  const last = parsed[parsed.length - 1];
  const target = first.at + (last.at - first.at) * u;
  let lo = 0;
  let hi = parsed.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (parsed[mid].at < target) lo = mid + 1;
    else hi = mid;
  }
  const high = lo;
  const low = Math.max(0, high - 1);
  const before = parsed[low];
  const after = parsed[high];
  const span = Math.max(1, after.at - before.at);
  const t = high === low ? 0 : (target - before.at) / span;
  return before.value + (after.value - before.value) * t;
}

/**
 * The star curve is cumulative, so its pixel-y is monotonically non-increasing
 * left-to-right. That lets us binary-search, for a text line whose box bottom
 * sits at `yBottom`, the right-most x still fully *above* the curve — the width
 * that line may use before it would collide with the rising area.
 */
function makeBoundary(
  parsed: Parsed[],
  max: number,
  width: number,
  height: number,
) {
  const yTop = 10; // headroom so the peak never touches the top edge
  const yBottom = height;
  const curveY = (x: number) => {
    const frac = clamp01(valueAt(parsed, clamp01(x / width)) / max);
    return yBottom - frac * (yBottom - yTop);
  };
  return (yBand: number): number => {
    if (curveY(0) < yBand) return 0;
    if (curveY(width) >= yBand) return width;
    let lo = 0;
    let hi = width;
    while (hi - lo > 1) {
      const mid = (lo + hi) / 2;
      if (curveY(mid) >= yBand) lo = mid;
      else hi = mid;
    }
    return lo;
  };
}

export function StarCurveStory({
  text,
  points,
  height = 208,
  fill = BRAND,
  className,
}: Props) {
  const rootRef = useRef<HTMLDivElement>(null);
  const probeRef = useRef<HTMLSpanElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [width, setWidth] = useState(0);
  const [flow, setFlow] = useState<FlowedLine[] | null>(null);

  const parsed = useMemo(() => parseSeries(points), [points]);
  const max = useMemo(
    () => Math.max(1, ...parsed.map((p) => p.value)),
    [parsed],
  );

  // Track the available width (one observed read; all wrapping math is off-DOM).
  useEffect(() => {
    const root = rootRef.current;
    if (!root || !isBrowser()) return;
    const measure = () => setWidth(root.clientWidth);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(root);
    return () => observer.disconnect();
  }, []);

  // Flow the prose into the space the curve leaves free.
  useEffect(() => {
    const probe = probeRef.current;
    if (!probe || !isBrowser() || parsed.length < 2 || width <= 0 || !text) {
      setFlow(null);
      return;
    }
    const { font, lineHeight, letterSpacing } = metricsFor(probe);
    const rightEdge = makeBoundary(parsed, max, width, height);
    const lines = flowAroundBoundary(text, font, {
      top: PAD.top,
      bottom: height - PAD.bottom,
      lineHeight,
      letterSpacing,
      left: PAD.left,
      widthAt: (yBottom) => rightEdge(yBottom) - PAD.right,
    });
    setFlow(lines.length > 0 ? lines : null);
  }, [parsed, max, width, height, text]);

  // Paint the dithered area behind the text once the canvas is mounted.
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !flow || parsed.length < 2 || width <= 0) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const cols = Math.max(8, Math.round(width / CELL));
    const rows = Math.max(8, Math.round(height / CELL));
    canvas.width = cols;
    canvas.height = rows;
    const buf = new RasterBuffer(cols, rows);
    const topRow = Math.round((10 / height) * rows);
    for (let x = 0; x < cols; x++) {
      const frac = clamp01(valueAt(parsed, x / cols) / max);
      const lineRow = Math.min(
        rows - 1,
        Math.max(topRow, Math.round(rows - frac * (rows - topRow))),
      );
      const depth = Math.max(1, rows - 1 - lineRow);
      for (let y = lineRow; y < rows; y++) {
        const raw = (y - lineRow) / depth;
        const density = clamp01(1 - RAMP * (1 - raw));
        const lit = density > BAYER4[y & 3][x & 3];
        const k = 0.3 + density * 0.7;
        buf.set(x, y, fill, lit ? k : k * OFF_TIER);
      }
      buf.set(x, lineRow, fill, 0.72);
    }
    ctx.putImageData(buf.image, 0, 0);
  }, [flow, parsed, max, width, height, fill]);

  const enhanced = flow !== null && flow.length > 0 && parsed.length >= 2;

  return (
    <div ref={rootRef} className={cn("relative w-full", className)}>
      {/* Metric probe: pretext reads the flowed-text font off this element's
          computed style, so measurement always matches the real render. */}
      <span
        ref={probeRef}
        aria-hidden="true"
        className={cn(BODY, "pointer-events-none absolute -z-10 opacity-0")}
      />
      {enhanced ? (
        <div className="relative isolate overflow-hidden" style={{ height }}>
          <canvas
            ref={canvasRef}
            aria-hidden="true"
            className="pointer-events-none absolute inset-0 -z-10 size-full opacity-70 [image-rendering:pixelated]"
          />
          <p className="sr-only">{text}</p>
          <div aria-hidden="true" className="absolute inset-0">
            {flow.map((line, index) => (
              <span
                key={index}
                className={cn(BODY, "absolute text-foreground")}
                style={{
                  left: line.x,
                  top: line.y,
                  width: line.width + 1,
                  whiteSpace: "pre",
                  textShadow: "0 1px 6px var(--background)",
                }}
              >
                {line.text}
              </span>
            ))}
          </div>
        </div>
      ) : (
        <p className={cn(BODY, "max-w-[62ch]")}>{text}</p>
      )}
    </div>
  );
}
