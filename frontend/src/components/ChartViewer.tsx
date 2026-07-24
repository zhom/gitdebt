import { useEffect, useId, useMemo, useState } from "react";

import { DitherAreaChart } from "@/components/DitherAreaChart";
import { EmbedSnippet } from "@/components/EmbedSnippet";
import { EYEBROW, PANEL } from "@/components/style-tokens";
import { Button } from "@/components/ui/button";
import { DitherSegmented } from "@/components/ui/dither-segmented";
import { CONTROL } from "@/components/ui/dither-surface";
import { MEDIA_RENDER_REVISION } from "@/lib/media";
import { useRenderedTheme } from "@/lib/rendered-theme";
import { cn } from "@/lib/utils";

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

const AXIS_OPTIONS = [
  { value: "date" as const, label: "Date" },
  { value: "timeline" as const, label: "Timeline" },
];

const DATE_FIELD = "w-[8.5rem] tabular-nums scheme-dark";

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
  // An inverted range is reported rather than silently applied: emptying the
  // series without saying why reads as a broken chart.
  const invertedRange = Boolean(validFrom && validTo && validFrom > validTo);
  const appliedFrom = invertedRange ? "" : validFrom;
  const appliedTo = invertedRange ? "" : validTo;
  const visibleSeries = useMemo(() => {
    const fromMs = appliedFrom
      ? Date.parse(`${appliedFrom}T00:00:00Z`)
      : -Infinity;
    const toMs = appliedTo ? Date.parse(`${appliedTo}T23:59:59Z`) : Infinity;
    return series.filter((point) => {
      const at = Date.parse(point.date);
      return Number.isFinite(at) && at >= fromMs && at <= toMs;
    });
  }, [series, appliedFrom, appliedTo]);

  const params: string[] = [
    `type=${type}`,
    `animate=${controlsChanged ? "0" : "1"}`,
    `render=${MEDIA_RENDER_REVISION}`,
  ];
  if (logScale) params.push("log=1");
  if (appliedFrom) params.push(`from=${appliedFrom}`);
  if (appliedTo) params.push(`to=${appliedTo}`);
  if (revision > 0) params.push(`v=${revision}`);

  const src = `${apiBase}${path}`;
  const sep = path.includes("?") ? "&" : "?";
  const withParams = (theme: "light" | "dark") =>
    `${src}${sep}${params.join("&")}&theme=${theme}`;

  const hasRange = Boolean(from || to);

  const figure = (
    <figure className={cn(PANEL, "relative overflow-hidden")}>
      <figcaption className="flex flex-wrap items-center justify-between gap-3 border-b border-border/40 px-4 py-3">
        {caption && <div className={EYEBROW}>{caption}</div>}
        <div className="flex w-full flex-wrap items-center gap-2 sm:w-auto">
          <div className="flex flex-wrap items-center gap-2">
            <label htmlFor={`${id}-from`} className={EYEBROW}>
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
              className={cn(CONTROL, DATE_FIELD)}
            />
            <label htmlFor={`${id}-to`} className={EYEBROW}>
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
              className={cn(CONTROL, DATE_FIELD)}
            />
            {hasRange && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  setControlsChanged(true);
                  setFrom("");
                  setTo("");
                }}
              >
                Reset
              </Button>
            )}
          </div>
          <Button
            variant={logScale ? "primary" : "outline"}
            size="sm"
            pulse={false}
            aria-pressed={logScale}
            onClick={() => {
              setControlsChanged(true);
              setLogScale((v) => !v);
            }}
          >
            Log
          </Button>
          <DitherSegmented
            role="radiogroup"
            aria-label="Chart axis"
            value={type}
            options={AXIS_OPTIONS}
            onValueChange={(next) => {
              setControlsChanged(true);
              setType(next);
            }}
          />
          {embedLink && label && (
            <EmbedSnippet
              apiBase={apiBase}
              chartPath={path}
              linkHref={embedLink}
              label={label}
              state={{
                type,
                log: logScale,
                from: appliedFrom,
                to: appliedTo,
              }}
              variant="menu"
            />
          )}
        </div>
      </figcaption>
      {invertedRange && (
        <p
          role="alert"
          className="border-b border-border/40 px-4 py-2 text-[11px] text-[var(--swatch-red)]"
        >
          The end date is before the start date. Showing the full range.
        </p>
      )}
      <InteractiveChart
        src={withParams(theme)}
        alt={alt}
        points={visibleSeries}
        logScale={logScale}
        axis={type}
        priority={priority}
        seed={liveRepo ?? label ?? path}
      />
    </figure>
  );

  return figure;
}

function InteractiveChart({
  src,
  alt,
  points,
  logScale,
  axis,
  priority,
  seed,
}: {
  src: string;
  alt: string;
  points: StarPoint[];
  logScale: boolean;
  axis: ChartType;
  priority: boolean;
  seed: string;
}) {
  const parsed = useMemo(
    () =>
      points
        .map((point) => ({ ...point, at: Date.parse(point.date) }))
        .filter((point) => Number.isFinite(point.at))
        .sort((a, b) => a.at - b.at),
    [points],
  );

  if (parsed.length >= 2) {
    return (
      <DitherAreaChart
        points={parsed.map((point) => ({
          date: point.date,
          value: point.stars,
        }))}
        axis={axis}
        logScale={logScale}
        height={500}
        valueLabel="stars"
        seed={seed}
        className="rounded-b-[inherit]"
      />
    );
  }

  return (
    <div className="relative overflow-hidden rounded-b-[inherit]">
      <img
        src={src}
        alt={alt}
        loading={priority ? "eager" : "lazy"}
        fetchPriority={priority ? "high" : "auto"}
        decoding="async"
        className="block w-full select-none"
        draggable={false}
      />
    </div>
  );
}
