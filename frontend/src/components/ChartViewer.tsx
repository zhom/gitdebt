import { useEffect, useId, useState } from "react";

import { EmbedSnippet } from "@/components/EmbedSnippet";
import { MEDIA_RENDER_REVISION } from "@/lib/media";

export type ChartType = "date" | "timeline";

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
}: Props) {
  const [type, setType] = useState<ChartType>("date");
  const [logScale, setLogScale] = useState(false);
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [controlsChanged, setControlsChanged] = useState(false);
  const [revision, setRevision] = useState(0);
  const id = useId();

  useEffect(() => {
    const targetRepo = liveRepo;
    if (!targetRepo) return;
    function refresh(event: Event) {
      if (!targetRepo) return;
      const detail = (event as CustomEvent<{
        repo?: string;
        stars?: { phase?: string };
      }>).detail;
      if (
        detail?.repo?.toLowerCase() === targetRepo.toLowerCase() &&
        detail.stars?.phase === "complete"
      ) {
        setRevision((value) => value + 1);
      }
    }
    window.addEventListener("gitdebt:repo-progress", refresh);
    return () => window.removeEventListener("gitdebt:repo-progress", refresh);
  }, [liveRepo]);

  const validFrom = DATE_RE.test(from) ? from : "";
  const validTo = DATE_RE.test(to) ? to : "";

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
    "min-h-11 w-full rounded-md border border-input bg-background px-2 py-2 font-mono text-base text-foreground outline-none scheme-light focus-visible:outline-2 focus-visible:-outline-offset-1 focus-visible:outline-ring sm:min-h-0 sm:w-[8.5rem] sm:py-1 sm:text-xs";

  const figure = (
    <figure className="card-panel overflow-hidden">
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
      <img
        src={withParams("light")}
        alt={alt}
        loading={priority ? "eager" : "lazy"}
        fetchPriority={priority ? "high" : "auto"}
        decoding="async"
        className="block w-full"
      />
    </figure>
  );

  return figure;
}
