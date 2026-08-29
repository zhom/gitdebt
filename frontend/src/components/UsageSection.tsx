import { useEffect, useMemo, useState, type ReactNode } from "react";

import { EmbedSnippet } from "@/components/EmbedSnippet";
import { StatStrip } from "@/components/StatStrip";
import { BODY, CAPTION, FIELD, MEASURE } from "@/components/style-tokens";
import { Segmented } from "@/components/ui/controls";
import { Leader } from "@/components/ui/marks";
import type { ChartType } from "@/components/ChartViewer";
import { MEDIA_RENDER_REVISION } from "@/lib/media";
import { useRenderedTheme } from "@/lib/rendered-theme";
import { cn } from "@/lib/utils";

/**
 * Attention against use: the star curve laid over download volume.
 *
 * Every state of this section renders something a reader can act on, and it
 * renders it as markup. Nothing fades in, nothing starts transparent, and
 * nothing is swapped behind an exit animation — the previous version wrapped
 * all three states in an `initial={{ opacity: 0 }}` presence, so a stalled
 * hydration or a throttled tab left an empty rectangle where the section is.
 */

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

/** The section's own frame: a drawn panel with the cut corner it shares with
 *  every other panel on the sheet, and a head that names what is measured. */
function UsageFrame({
  head,
  children,
}: {
  head: ReactNode;
  children: ReactNode;
}) {
  return (
    <figure className="m-0 cut-edge p-4 [--pad-x:1rem] [--pad-y:1rem]">
      <figcaption className="flex min-h-11 flex-wrap items-center justify-between gap-x-6 gap-y-3 border-b border-rule pb-3">
        {head}
      </figcaption>
      {children}
    </figure>
  );
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
    data && (data.resolved.npm || data.resolved.crate || data.resolved.pypi),
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
      <UsageFrame head={<h3 className={FIELD}>Stars against usage</h3>}>
        <p className={cn(BODY, MEASURE, "pt-4")}>
          Resolving the packages this repository publishes.
        </p>
      </UsageFrame>
    );
  }

  if (errored || !data || !hasPackage) {
    return (
      <UsageFrame head={<h3 className={FIELD}>Stars against usage</h3>}>
        <p className={cn(BODY, MEASURE, "pt-4")}>
          {errored
            ? "Usage data could not be read right now."
            : "No published package was detected for this repository, so there is nothing to lay over star growth."}
        </p>
      </UsageFrame>
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
    <section className="space-y-5">
      <UsageFrame
        head={
          <>
            <h3 className={FIELD}>Stars against usage</h3>
            <div className="flex flex-wrap items-center gap-3">
              {sources.length > 1 && (
                <Segmented
                  aria-label="Download source"
                  value={source}
                  options={sources.map((s) => ({
                    value: s,
                    label: SOURCE_LABEL[s],
                  }))}
                  onValueChange={setSource}
                />
              )}
              <Segmented
                role="radiogroup"
                aria-label="Chart axis"
                value={type}
                options={AXIS_OPTIONS}
                onValueChange={setType}
              />
            </div>
          </>
        }
      >
        <div className="grid gap-x-8 gap-y-6 border-b border-rule py-4 sm:grid-cols-2">
          <div>
            <p className={FIELD}>Resolved packages</p>
            <ul role="list" className="mt-2.5 space-y-1">
              {resolvedRows.map((row) => (
                // The rule belongs to the row, not to the link inside it: a
                // `last:` on the link matches every link, because each one is
                // the only child of its own item.
                <li key={row.label} className="border-b border-rule last:border-b-0">
                  <a
                    href={row.href}
                    target="_blank"
                    rel="noreferrer"
                    className="group flex min-h-11 items-baseline justify-between gap-4 text-ink-2 outline-none transition-colors duration-[--duration-ui] hover:text-ink focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-signal"
                  >
                    <span className={FIELD}>{row.label}</span>
                    <span className="flex min-w-0 items-baseline gap-1.5 font-mono text-[0.75rem]">
                      <span className="truncate">{row.value}</span>
                      <Leader
                        size={12}
                        className="shrink-0 self-center transition-transform duration-[--duration-ui] group-hover:-translate-y-px group-hover:translate-x-px motion-reduce:transition-none"
                      />
                    </span>
                  </a>
                </li>
              ))}
            </ul>
          </div>

          {/* Three readings on one strip, each label on one baseline and each
              figure on another, whatever the figures happen to say. */}
          <StatStrip
            columns={3}
            aria-label={`Reach for ${owner}/${repo}`}
            items={totals}
            className="self-start"
          />
        </div>

        {hasDownloadSeries ? (
          <img
            src={`${apiBase}${chartPath}&theme=${theme}&render=${MEDIA_RENDER_REVISION}`}
            alt={`Star growth versus download volume for ${owner}/${repo}`}
            loading={priority ? "eager" : "lazy"}
            fetchPriority={priority ? "high" : "auto"}
            decoding="async"
            className="mt-4 block w-full"
          />
        ) : (
          <p className={cn(BODY, MEASURE, "pt-4")}>
            A package is published, but no download history is available to
            chart against star growth.
          </p>
        )}
      </UsageFrame>

      {hasDownloadSeries && (
        <p className={cn(CAPTION, MEASURE)}>
          Downloads are a registry's own count of installs, not a count of
          people. Read the two curves as attention against use, never as one
          measuring the other.
        </p>
      )}

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
  );
}
