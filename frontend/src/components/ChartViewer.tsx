import { useEffect, useId, useMemo, useState } from "react";

import { EmbedSnippet } from "@/components/EmbedSnippet";
import { MEDIA_RENDER_REVISION } from "@/lib/media";
import { useRenderedTheme } from "@/lib/rendered-theme";

export type ChartType = "date" | "timeline";
export type StarPoint = { date: string; stars: number };

type Props = {
  apiBase: string;
  path: string;
  alt: string;
  caption?: string;
  delay?: number;
  embedLink?: string;
  label?: string;
  priority?: boolean;
  liveRepo?: string;
  points?: StarPoint[];
};

const DATE_RE = /^\d{4}-\d{2}-\d{2}$/;

export function ChartViewer({
  apiBase,
  path,
  alt,
  caption,
  embedLink,
  label,
  priority = false,
  liveRepo,
  points = [],
}: Props) {
  const [type, setType] = useState<ChartType>("date");
  const [logScale, setLogScale] = useState(false);
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [controlsChanged, setControlsChanged] = useState(false);
  const [revision, setRevision] = useState(0);
  const [series, setSeries] = useState<StarPoint[]>(points);
  const theme = useRenderedTheme();
  const id = useId();

  useEffect(() => {
    setSeries(points);
  }, [points]);

  useEffect(() => {
    const targetRepo = liveRepo?.toLowerCase();
    if (!targetRepo) return;
    function refresh(event: Event) {
      if (!targetRepo) return;
      const detail = (event as CustomEvent<{
        repo?: string;
        stars?: { phase?: string };
      }>).detail;
      if (
        detail?.repo?.toLowerCase() === targetRepo &&
        detail.stars?.phase === "complete"
      ) {
        setRevision((value) => value + 1);
      }
    }
    window.addEventListener("gitdebt:repo-progress", refresh);
    function updateSeries(event: Event) {
      const detail = (event as CustomEvent<{
        repo?: string;
        history?: StarPoint[];
      }>).detail;
      if (
        detail?.repo?.toLowerCase() === targetRepo &&
        Array.isArray(detail.history)
      ) {
        setSeries(detail.history);
      }
    }
    window.addEventListener("gitdebt:repo-data", updateSeries);
    return () => {
      window.removeEventListener("gitdebt:repo-progress", refresh);
      window.removeEventListener("gitdebt:repo-data", updateSeries);
    };
  }, [liveRepo]);

  const validFrom = DATE_RE.test(from) ? from : "";
  const validTo = DATE_RE.test(to) ? to : "";
  const visibleSeries = useMemo(() => {
    const fromMs = validFrom ? Date.parse(`${validFrom}T00:00:00Z`) : -Infinity;
    const toMs = validTo ? Date.parse(`${validTo}T23:59:59Z`) : Infinity;
    return series.filter((point) => {
      const at = Date.parse(point.date);
      return Number.isFinite(at) && at >= fromMs && at <= toMs;
    });
  }, [series, validFrom, validTo]);

  const params: string[] = [
    `type=${type}`,
    `animate=${controlsChanged ? "0" : "1"}`,
    `render=${MEDIA_RENDER_REVISION}`,
  ];
  if (logScale) params.push("log=1");
  if (validFrom) params.push(`from=${validFrom}`);
  if (validTo) params.push(`to=${validTo}`);
  if (revision > 0) params.push(`v=${revision}`);

  const src = `${apiBase}${path}`;
  const sep = path.includes("?") ? "&" : "?";
  const withParams = (theme: "light" | "dark") =>
    `${src}${sep}${params.join("&")}&theme=${theme}`;

  const tabClass = (active: boolean) =>
    `min-h-11 rounded-md px-3 py-2 font-mono text-base tracking-wide uppercase sm:min-h-0 sm:px-2.5 sm:py-1 sm:text-xs ${
      active
        ? "bg-accent text-accent-foreground"
        : "text-muted-foreground hover:bg-accent/60 hover:text-accent-foreground"
    }`;

  const dateInputClass =
    "min-h-11 w-full rounded-md border border-input bg-background px-2 py-2 font-mono text-base text-foreground outline-none scheme-light dark:scheme-dark focus-visible:outline-2 focus-visible:-outline-offset-1 focus-visible:outline-ring sm:min-h-0 sm:w-[8.5rem] sm:py-1 sm:text-xs";

  const figure = (
    <figure className="card-panel relative">
      <figcaption className="flex flex-wrap items-center justify-between gap-3 border-b border-border bg-muted/40 px-5 py-3">
        {caption && (
          <div className="inline-flex items-center gap-2 font-mono text-xs tracking-wide text-muted-foreground uppercase">
            <span className="size-1.5 shrink-0 rounded-full bg-signal" aria-hidden="true" />
            {caption}
          </div>
        )}
        <div className="flex w-full flex-wrap items-center gap-3 sm:w-auto">
          <div className="grid w-full grid-cols-[auto_1fr] items-center gap-2 sm:flex sm:w-auto sm:gap-1.5">
            <label
              htmlFor={`${id}-from`}
              className="font-mono text-xs tracking-wide text-muted-foreground uppercase"
            >
              From
              <span className="sr-only"> date (YYYY-MM-DD)</span>
            </label>
            <input
              id={`${id}-from`}
              name="from"
              type="date"
              value={from}
              onChange={(e) => {
                setControlsChanged(true);
                setFrom(e.target.value);
              }}
              className={dateInputClass}
            />
            <label
              htmlFor={`${id}-to`}
              className="font-mono text-xs tracking-wide text-muted-foreground uppercase"
            >
              To
              <span className="sr-only"> date (YYYY-MM-DD)</span>
            </label>
            <input
              id={`${id}-to`}
              name="to"
              type="date"
              value={to}
              onChange={(e) => {
                setControlsChanged(true);
                setTo(e.target.value);
              }}
              className={dateInputClass}
            />
          </div>
          <div className="flex items-center gap-1" role="group" aria-label="Y-axis scale">
            <button
              type="button"
              aria-pressed={logScale}
              onClick={() => {
                setControlsChanged(true);
                setLogScale((v) => !v);
              }}
              className={tabClass(logScale)}
            >
              Log
            </button>
          </div>
          <div className="flex items-center gap-1" role="group" aria-label="Chart axis">
            <button
              type="button"
              aria-pressed={type === "date"}
              onClick={() => {
                setControlsChanged(true);
                setType("date");
              }}
              className={tabClass(type === "date")}
            >
              Date
            </button>
            <button
              type="button"
              aria-pressed={type === "timeline"}
              onClick={() => {
                setControlsChanged(true);
                setType("timeline");
              }}
              className={tabClass(type === "timeline")}
            >
              Timeline
            </button>
          </div>
          {embedLink && label && (
            <EmbedSnippet
              apiBase={apiBase}
              chartPath={path}
              linkHref={embedLink}
              label={label}
              state={{ type, log: logScale, from: validFrom, to: validTo }}
              variant="menu"
            />
          )}
        </div>
      </figcaption>
      <InteractiveChart
        src={withParams(theme)}
        alt={alt}
        points={visibleSeries}
        logScale={logScale}
        axis={type}
        priority={priority}
      />
    </figure>
  );

  return figure;
}

type HoverPoint = {
  date: Date;
  stars: number;
  approximate: boolean;
  x: number;
  y: number;
};

function InteractiveChart({
  src,
  alt,
  points,
  logScale,
  axis,
  priority,
}: {
  src: string;
  alt: string;
  points: StarPoint[];
  logScale: boolean;
  axis: ChartType;
  priority: boolean;
}) {
  const [hover, setHover] = useState<HoverPoint | null>(null);
  const parsed = useMemo(
    () =>
      points
        .map((point) => ({ ...point, at: Date.parse(point.date) }))
        .filter((point) => Number.isFinite(point.at))
        .sort((a, b) => a.at - b.at),
    [points],
  );

  function inspect(clientX: number, bounds: DOMRect) {
    if (parsed.length < 2) return;
    const plotLeft = 56 / 1200;
    const plotRight = 1 - plotLeft;
    const raw = (clientX - bounds.left) / bounds.width;
    const fraction = Math.min(1, Math.max(0, (raw - plotLeft) / (plotRight - plotLeft)));
    const first = parsed[0];
    const last = parsed[parsed.length - 1];
    const timelineIndex = fraction * (parsed.length - 1);
    const low =
      axis === "timeline"
        ? Math.floor(timelineIndex)
        : Math.max(
            0,
            parsed.findIndex(
              (point) =>
                point.at >= first.at + (last.at - first.at) * fraction,
            ) - 1,
          );
    const high =
      axis === "timeline"
        ? Math.ceil(timelineIndex)
        : Math.min(low + 1, parsed.length - 1);
    const before = parsed[low];
    const after = parsed[high];
    const span = Math.max(1, after.at - before.at);
    const target =
      axis === "timeline"
        ? before.at + (after.at - before.at) * (timelineIndex - low)
        : first.at + (last.at - first.at) * fraction;
    const local = high === low ? 0 : (target - before.at) / span;
    const stars = Math.round(before.stars + (after.stars - before.stars) * local);
    const yMax = Math.max(1, last.stars);
    const yFraction = logScale
      ? Math.log(Math.max(0, stars) + 1) / Math.log(yMax + 1)
      : stars / yMax;
    setHover({
      date: new Date(target),
      stars,
      approximate: target !== before.at && target !== after.at,
      x: (plotLeft + fraction * (plotRight - plotLeft)) * 100,
      y: (56 / 600 + (1 - yFraction) * (464 / 600)) * 100,
    });
  }

  return (
    <div
      className="group/chart relative overflow-hidden rounded-b-[inherit] bg-card"
      onPointerMove={(event) => inspect(event.clientX, event.currentTarget.getBoundingClientRect())}
      onPointerDown={(event) => inspect(event.clientX, event.currentTarget.getBoundingClientRect())}
      onPointerLeave={() => setHover(null)}
    >
      <img
        src={src}
        alt={alt}
        loading={priority ? "eager" : "lazy"}
        fetchPriority={priority ? "high" : "auto"}
        decoding="async"
        className="block w-full select-none"
        draggable={false}
      />
      {hover && (
        <>
          <div
            className="pointer-events-none absolute top-[9.3%] bottom-[13.3%] w-px bg-foreground/35"
            style={{ left: `${hover.x}%` }}
            aria-hidden="true"
          />
          <div
            className="pointer-events-none absolute size-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-background bg-foreground"
            style={{ left: `${hover.x}%`, top: `${hover.y}%` }}
            aria-hidden="true"
          />
          <output
            className={`pointer-events-none absolute top-3 z-20 min-w-36 rounded-lg border border-border bg-popover/95 px-3 py-2 text-popover-foreground backdrop-blur-xl ${hover.x > 76 ? "-translate-x-full -ml-3" : "ml-3"}`}
            style={{ left: `${hover.x}%` }}
            aria-live="polite"
          >
            <span className="block font-mono text-[11px] tracking-wide text-muted-foreground uppercase">
              {hover.date.toLocaleDateString(undefined, {
                year: "numeric",
                month: "short",
                day: "numeric",
                timeZone: "UTC",
              })}
            </span>
            <span className="mt-0.5 block text-sm font-semibold tabular-nums">
              {hover.approximate ? "≈ " : ""}{hover.stars.toLocaleString()} stars
            </span>
          </output>
        </>
      )}
      {parsed.length >= 2 && !hover && (
        <span className="pointer-events-none absolute right-3 bottom-3 rounded-md border border-border bg-background/85 px-2 py-1 font-mono text-[10px] tracking-wide text-muted-foreground uppercase opacity-0 backdrop-blur transition-opacity duration-150 group-hover/chart:opacity-100 motion-reduce:transition-none">
          Hover for daily values
        </span>
      )}
    </div>
  );
}
