import { useEffect, useMemo, useState } from "react";

import { ChartFrame, DATE_RE, type ChartAxis } from "@/components/ChartFrame";
import { EmbedSnippet } from "@/components/EmbedSnippet";
import { TraceChart } from "@/components/TraceChart";
import { MEDIA_RENDER_REVISION } from "@/lib/media";
import { useRenderedTheme } from "@/lib/rendered-theme";

/**
 * One repository's star history, on a sheet with its controls.
 *
 * The chrome, the range fields, the scale and axis switches, the embed menu and
 * the inverted-range note all live in `ChartFrame`, which the comparison sheet
 * uses too — those two components carried the same caption row twice, down to
 * the same sentence about an inverted range.
 *
 * What stays here is everything only this component knows: the live listeners
 * that pick up a finished analysis, the cache-busting revision, the flag that
 * stops the server renderer from replaying its animation once a control has
 * been touched, and the fallback to the server-rendered SVG when there are
 * fewer than two readings to plot.
 */

export type ChartType = ChartAxis;
export type StarPoint = { date: string; stars: number };

/**
 * One shared empty series, not a fresh `[]` per render.
 *
 * The default used to be written inline, so every render of a caller that
 * passes no points produced a new array, which changed the effect's dependency,
 * which set state, which rendered again — a loop that started the moment any
 * control was touched on the pages that let the live listeners supply the data.
 */
const NO_POINTS: StarPoint[] = [];

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

export function ChartViewer({
  apiBase,
  path,
  alt,
  caption,
  embedLink,
  label,
  priority = false,
  liveRepo,
  points = NO_POINTS,
}: Props) {
  const [type, setType] = useState<ChartType>("date");
  const [logScale, setLogScale] = useState(false);
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [controlsChanged, setControlsChanged] = useState(false);
  const [revision, setRevision] = useState(0);
  const [series, setSeries] = useState<StarPoint[]>(points);
  const theme = useRenderedTheme();

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
  const withParams = (renderedTheme: "light" | "dark") =>
    `${src}${sep}${params.join("&")}&theme=${renderedTheme}`;

  return (
    <ChartFrame
      title={caption}
      axis={type}
      onAxisChange={(next) => {
        setControlsChanged(true);
        setType(next);
      }}
      logScale={logScale}
      onLogScaleChange={(next) => {
        setControlsChanged(true);
        setLogScale(next);
      }}
      from={from}
      to={to}
      onFromChange={(value) => {
        setControlsChanged(true);
        setFrom(value);
      }}
      onToChange={(value) => {
        setControlsChanged(true);
        setTo(value);
      }}
      onReset={() => {
        setControlsChanged(true);
        setFrom("");
        setTo("");
      }}
      inverted={invertedRange}
      embed={
        embedLink && label ? (
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
        ) : undefined
      }
    >
      <InteractiveChart
        src={withParams(theme)}
        alt={alt}
        points={visibleSeries}
        logScale={logScale}
        axis={type}
        priority={priority}
      />
    </ChartFrame>
  );
}

/**
 * Two readings are the minimum a trace can be drawn from. Below that the sheet
 * shows the server-rendered SVG, which is the same drawing produced by the
 * renderer that also serves README embeds.
 */
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
      <TraceChart
        points={parsed.map((point) => ({
          date: point.date,
          value: point.stars,
        }))}
        axis={axis}
        logScale={logScale}
        height={440}
        valueLabel="stars"
      />
    );
  }

  return (
    <img
      src={src}
      alt={alt}
      loading={priority ? "eager" : "lazy"}
      fetchPriority={priority ? "high" : "auto"}
      decoding="async"
      className="block w-full select-none"
      draggable={false}
    />
  );
}
