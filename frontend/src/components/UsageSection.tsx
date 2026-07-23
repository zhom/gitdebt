import { type ReactNode, useEffect, useMemo, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { ChevronDown, Loader2 } from "lucide-react";

import { EmbedSnippet } from "@/components/EmbedSnippet";
import { BODY, EYEBROW, KPI, PANEL, ROW } from "@/components/style-tokens";
import { DitherSegmented } from "@/components/ui/dither-segmented";
import { CONTROL } from "@/components/ui/dither-surface";
import type { ChartType } from "@/components/ChartViewer";
import { MEDIA_RENDER_REVISION } from "@/lib/media";
import {
  DURATION,
  EASE_OUT,
  REDUCED_MOTION_DURATION,
} from "@/lib/motion";
import { useRenderedTheme } from "@/lib/rendered-theme";
import { cn } from "@/lib/utils";

type DownloadSeriesPoint = { date: string; downloads: number };
type RegistryDownloads = { total: number; series: DownloadSeriesPoint[] };
type DockerDownloads = { total: number };

export type UsageResponse = {
  repo: string;
  stars_total: number;
  forks: number;
  resolved: {
    npm: string | null;
    crate: string | null;
    pypi: string | null;
    docker: string | null;
  };
  downloads: {
    npm: RegistryDownloads | null;
    crates: RegistryDownloads | null;
    pypi: RegistryDownloads | null;
    docker: DockerDownloads | null;
  };
};

type UsageSource = "auto" | "npm" | "crates" | "pypi";

type Props = {
  owner: string;
  repo: string;
  apiBase: string;
  initialData?: UsageResponse | null;
  showEmbed?: boolean;
  priority?: boolean;
};

function availableSources(data: UsageResponse): UsageSource[] {
  const out: UsageSource[] = ["auto"];
  if (data.resolved.npm && data.downloads.npm) out.push("npm");
  if (data.resolved.crate && data.downloads.crates) out.push("crates");
  if (data.resolved.pypi && data.downloads.pypi) out.push("pypi");
  return out;
}

const AXIS_OPTIONS = [
  { value: "date" as const, label: "Date" },
  { value: "timeline" as const, label: "Timeline" },
];

const SOURCE_LABEL: Record<UsageSource, string> = {
  auto: "Auto",
  npm: "npm",
  crates: "crates",
  pypi: "PyPI",
};

function compact(n: number): string {
  return Intl.NumberFormat(undefined, {
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(n);
}

function registryUrl(source: string, name: string): string {
  const encoded = encodeURIComponent(name);
  switch (source) {
    case "npm":
      return `https://www.npmjs.com/package/${encoded}`;
    case "crates":
      return `https://crates.io/crates/${encoded}`;
    case "PyPI":
      return `https://pypi.org/project/${encoded}`;
    default:
      return `https://hub.docker.com/r/${name.split("/").map(encodeURIComponent).join("/")}`;
  }
}

export function UsageSection({
  owner,
  repo,
  apiBase,
  initialData = null,
  showEmbed = true,
  priority = false,
}: Props) {
  const [data, setData] = useState<UsageResponse | null>(initialData);
  const [loading, setLoading] = useState(!initialData);
  const [errored, setErrored] = useState(false);
  const [type, setType] = useState<ChartType>("date");
  const [source, setSource] = useState<UsageSource>("auto");
  const theme = useRenderedTheme();

  useEffect(() => {
    if (initialData) return;
    let cancelled = false;
    (async () => {
      try {
        setLoading(true);
        const res = await fetch(`${apiBase}/api/repos/${owner}/${repo}/usage`, {
          headers: { accept: "application/json" },
        });
        if (!res.ok) throw new Error(`API ${res.status}`);
        const json = (await res.json()) as UsageResponse;
        if (!cancelled) setData(json);
      } catch {
        if (!cancelled) setErrored(true);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [owner, repo, apiBase, initialData]);

  const sources = useMemo(
    () => (data ? availableSources(data) : (["auto"] as UsageSource[])),
    [data],
  );

  useEffect(() => {
    if (!sources.includes(source)) setSource("auto");
  }, [sources, source]);

  const hasPackage = Boolean(
    data &&
      ((data.resolved.npm && data.downloads.npm) ||
        (data.resolved.crate && data.downloads.crates) ||
        (data.resolved.pypi && data.downloads.pypi) ||
        (data.resolved.docker && data.downloads.docker)),
  );

  const hasDownloadSeries = Boolean(
    data &&
      (data.resolved.npm || data.resolved.crate || data.resolved.pypi),
  );

  const chartPath = `/api/repos/${owner}/${repo}/usage.svg?type=${type}&source=${source}`;

  const resolvedRows = useMemo(() => {
    if (!data) return [] as { label: string; value: string; href: string }[];
    const rows: { label: string; value: string; href: string }[] = [];
    if (data.resolved.npm && data.downloads.npm) rows.push({ label: "npm", value: data.resolved.npm, href: registryUrl("npm", data.resolved.npm) });
    if (data.resolved.crate && data.downloads.crates) rows.push({ label: "crates", value: data.resolved.crate, href: registryUrl("crates", data.resolved.crate) });
    if (data.resolved.pypi && data.downloads.pypi) rows.push({ label: "PyPI", value: data.resolved.pypi, href: registryUrl("PyPI", data.resolved.pypi) });
    if (data.resolved.docker && data.downloads.docker) rows.push({ label: "Docker", value: data.resolved.docker, href: registryUrl("Docker", data.resolved.docker) });
    return rows;
  }, [data]);

  const downloadTotal = useMemo(() => {
    if (!data) return null;
    const parts = [
      data.downloads.npm?.total,
      data.downloads.crates?.total,
      data.downloads.pypi?.total,
      data.downloads.docker?.total,
    ].filter((n): n is number => typeof n === "number");
    if (parts.length === 0) return null;
    return parts.reduce((a, b) => a + b, 0);
  }, [data]);

  if (loading) {
    return (
      <AsyncSwap state="loading">
        <div className={cn(PANEL, "p-3.5")}>
          <p className={cn(EYEBROW, "inline-flex items-center gap-2")}>
            <Loader2
              className="size-3.5 shrink-0 motion-safe:animate-spin"
              aria-hidden="true"
            />
            Resolving packages
          </p>
        </div>
      </AsyncSwap>
    );
  }

  if (errored || !data || !hasPackage) {
    return (
      <AsyncSwap state="empty">
        <figure className={cn(PANEL, "overflow-hidden")}>
          <figcaption className={cn(EYEBROW, "border-b border-border/40 px-4 py-3")}>
            Stars vs. usage
          </figcaption>
          <div className="px-4 py-6">
            <p className={BODY}>
              {errored
                ? "Couldn't load usage data right now."
                : "No published package detected for this repo — nothing to overlay against star growth."}
            </p>
          </div>
        </figure>
      </AsyncSwap>
    );
  }

  const totals: { label: string; value: string }[] = [
    { label: "Stars", value: data.stars_total.toLocaleString() },
    { label: "Forks", value: data.forks.toLocaleString() },
    {
      label: "Downloads",
      value: downloadTotal === null ? "—" : compact(downloadTotal),
    },
  ];

  return (
    <AsyncSwap state="ready">
      <section className="space-y-6">
        <figure className={cn(PANEL, "overflow-hidden")}>
          <figcaption className="flex flex-wrap items-center justify-between gap-3 border-b border-border/40 px-4 py-3">
            <div className={EYEBROW}>Stars vs. usage</div>
            <div className="flex flex-wrap items-center gap-2">
              {sources.length > 1 && (
                <div className="inline-flex items-center gap-2">
                  <label htmlFor="usage-source" className={EYEBROW}>
                    Source
                  </label>
                  <span className="relative grid grid-cols-1 items-center">
                    <select
                      id="usage-source"
                      name="source"
                      value={source}
                      onChange={(e) => setSource(e.target.value as UsageSource)}
                      className={cn(
                        CONTROL,
                        "col-start-1 row-start-1 w-auto appearance-none pr-9 text-foreground",
                      )}
                    >
                      {sources.map((s) => (
                        <option key={s} value={s}>
                          {SOURCE_LABEL[s]}
                        </option>
                      ))}
                    </select>
                    <ChevronDown
                      className="pointer-events-none col-start-1 row-start-1 mr-3 size-3.5 justify-self-end text-muted-foreground"
                      strokeWidth={2}
                      aria-hidden="true"
                    />
                  </span>
                </div>
              )}
              <DitherSegmented
                role="radiogroup"
                aria-label="Chart axis"
                value={type}
                options={AXIS_OPTIONS}
                onValueChange={setType}
              />
            </div>
          </figcaption>

          <div className="grid gap-4 border-b border-border/40 p-3.5 sm:grid-cols-2">
            <dl>
              <dt className={EYEBROW}>Resolved packages</dt>
              <dd className="mt-2 flex flex-col">
                {resolvedRows.map((row) => (
                  <a
                    key={row.label}
                    href={row.href}
                    target="_blank"
                    rel="noreferrer"
                    className={cn(ROW, "-mx-2.5 justify-between")}
                  >
                    <span>{row.label}</span>
                    <span className="truncate text-foreground/90">
                      {row.value} ↗
                    </span>
                  </a>
                ))}
              </dd>
            </dl>
            <dl className="grid grid-cols-3 divide-x divide-border/40">
              {totals.map((t) => (
                <div key={t.label} className="min-w-0 px-3.5 first:pl-0 last:pr-0">
                  <dt className={EYEBROW}>{t.label}</dt>
                  <dd className={cn("mt-2", KPI)}>{t.value}</dd>
                </div>
              ))}
            </dl>
          </div>

          {hasDownloadSeries ? (
            <img
              src={`${apiBase}${chartPath}&theme=${theme}&render=${MEDIA_RENDER_REVISION}`}
              alt={`Star growth versus download volume for ${owner}/${repo}`}
              loading={priority ? "eager" : "lazy"}
              fetchPriority={priority ? "high" : "auto"}
              decoding="async"
              className="block w-full"
            />
          ) : (
            <div className="px-4 py-6">
              <p className={BODY}>
                A package is published, but no download history is available
                to chart against star growth.
              </p>
            </div>
          )}
        </figure>

        {hasDownloadSeries && showEmbed && (
          <EmbedSnippet
            apiBase={apiBase}
            chartPath={`/api/repos/${owner}/${repo}/usage.svg?source=${source}`}
            state={{ type }}
            linkHref={`https://gitdebt.com/${owner}/${repo}`}
            label={`${owner}/${repo} stars vs. usage`}
          />
        )}
      </section>
    </AsyncSwap>
  );
}

function AsyncSwap({
  state,
  children,
}: {
  state: "loading" | "empty" | "ready";
  children: ReactNode;
}) {
  const reduceMotion = useReducedMotion();
  return (
    <div className="relative">
      <AnimatePresence initial={false} mode="wait">
        <motion.div
          key={state}
          initial={{
            opacity: 0,
            y: reduceMotion ? 0 : 4,
          }}
          animate={{ opacity: 1, y: 0 }}
          exit={{
            opacity: 0,
            transition: { duration: 0.1, ease: EASE_OUT },
          }}
          transition={{
            duration: reduceMotion
              ? REDUCED_MOTION_DURATION
              : DURATION.enter,
            ease: EASE_OUT,
          }}
        >
          {children}
        </motion.div>
      </AnimatePresence>
    </div>
  );
}
