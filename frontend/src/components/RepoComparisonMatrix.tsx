import { useEffect, useMemo, useState } from "react";

import {
  BODY,
  CAPTION,
  EYEBROW,
  HEADING,
  SECTION_HEADER,
} from "@/components/style-tokens";
import {
  buildComparisonMetrics,
  type ComparisonMetric,
  type ComparisonRepo,
  type ComparisonStats,
  type ComparisonUsage,
} from "@/lib/comparison-insights";

export type ComparisonInitialRepo = {
  slug: string;
  total_stars: number;
  created_at: string | null;
  history: { date: string; stars: number }[];
  pending?: boolean;
  backfilling?: boolean;
};

type Props = {
  apiBase: string;
  repos: string[];
  initial?: ComparisonInitialRepo[];
};

type AnalyzeResponse = {
  total_stars: number;
  created_at: string | null;
  pending?: boolean;
  backfilling?: boolean;
  history: { date: string; stars: number }[];
};

const GROUP_COPY: Record<ComparisonMetric["group"], string> = {
  Reach: "Popularity and real-world adoption",
  Codebase: "Size and technical center of gravity",
  Maintenance: "Commit volume and recent operating cadence",
  Ownership: "How broadly maintenance responsibility is distributed",
  "Change signals": "Debt markers and fix pressure in frequently changed files",
};

function emptyRepo(
  slug: string,
  initial?: ComparisonInitialRepo,
): ComparisonRepo {
  return {
    slug,
    totalStars: initial?.total_stars ?? 0,
    createdAt: initial?.created_at ?? null,
    history: initial?.history ?? [],
    stats: null,
    usage: null,
    analysisPending: initial?.pending || initial?.backfilling,
  };
}

export function RepoComparisonMatrix({
  apiBase,
  repos,
  initial = [],
}: Props) {
  const [data, setData] = useState<ComparisonRepo[]>(() =>
    repos.map((slug) =>
      emptyRepo(
        slug,
        initial.find((item) => item.slug.toLowerCase() === slug.toLowerCase()),
      ),
    ),
  );

  useEffect(() => {
    let active = true;
    const initialBySlug = new Map(
      initial.map((item) => [item.slug.toLowerCase(), item]),
    );
    setData(
      repos.map((slug) => emptyRepo(slug, initialBySlug.get(slug.toLowerCase()))),
    );

    const update = (slug: string, patch: Partial<ComparisonRepo>) => {
      if (!active) return;
      setData((current) =>
        current.map((repo) => (repo.slug === slug ? { ...repo, ...patch } : repo)),
      );
    };

    for (const slug of repos) {
      const seeded = initialBySlug.get(slug.toLowerCase());
      if (!seeded) {
        void fetch(`${apiBase}/api/repos/${slug}/analyze`, {
          headers: { accept: "application/json" },
        })
          .then(async (response) => {
            if (!response.ok) return;
            const body = (await response.json()) as AnalyzeResponse;
            update(slug, {
              totalStars: body.total_stars,
              createdAt: body.created_at,
              history: body.history,
              analysisPending: body.pending || body.backfilling,
            });
          })
          .catch(() => undefined);
      }

      void fetch(`${apiBase}/api/repos/${slug}/usage`, {
        headers: { accept: "application/json" },
      })
        .then(async (response) => {
          if (!response.ok) return;
          update(slug, { usage: (await response.json()) as ComparisonUsage });
        })
        .catch(() => undefined);
    }

    let statsTimer = 0;
    const readStats = async () => {
      const results = await Promise.all(
        repos.map(async (slug) => {
          try {
            const response = await fetch(
              `${apiBase}/api/repos/${slug}/stats.json`,
              { headers: { accept: "application/json" } },
            );
            const body = (await response.json()) as ComparisonStats;
            return {
              slug,
              stats: response.ok && body.ready ? body : null,
              pending: response.status === 202 || body.ready === false,
            };
          } catch {
            return { slug, stats: null, pending: false };
          }
        }),
      );
      if (!active) return;
      let retry = false;
      for (const result of results) {
        if (result.stats) {
          update(result.slug, {
            stats: result.stats,
            analysisPending: false,
          });
        } else if (result.pending) {
          retry = true;
          update(result.slug, { analysisPending: true });
        }
      }
      if (retry) statsTimer = window.setTimeout(readStats, 4_000);
    };
    void readStats();

    const onProgress = (event: Event) => {
      const detail = (
        event as CustomEvent<{
          repo?: string;
          analysis?: { phase?: string };
        }>
      ).detail;
      if (
        detail?.repo &&
        repos.some(
          (slug) => slug.toLowerCase() === detail.repo?.toLowerCase(),
        ) &&
        detail.analysis?.phase === "complete"
      ) {
        window.clearTimeout(statsTimer);
        void readStats();
      }
    };
    window.addEventListener("gitdebt:repo-progress", onProgress);
    return () => {
      active = false;
      window.clearTimeout(statsTimer);
      window.removeEventListener("gitdebt:repo-progress", onProgress);
    };
  }, [apiBase, initial, repos]);

  const metrics = useMemo(() => buildComparisonMetrics(data), [data]);
  const groups = useMemo(
    () =>
      [...new Set(metrics.map((metric) => metric.group))].map((group) => ({
        group,
        metrics: metrics.filter((metric) => metric.group === group),
      })),
    [metrics],
  );

  return (
    <section className="mt-16 scroll-mt-24 border-t border-border/60 pt-12">
      <div className={SECTION_HEADER}>
        <div>
          <p className={EYEBROW}>Decision matrix</p>
          <h2 className={`mt-2 ${HEADING}`}>Repository health compared</h2>
        </div>
        <p className={CAPTION}>
          Live Postgres analysis · unavailable data stays labeled
        </p>
      </div>
      <p className={`mt-3 max-w-[74ch] ${BODY}`}>
        Popularity is only one axis. Compare technical scale, maintenance
        cadence, ownership resilience, and change pressure using the same
        completed repository analysis behind each report.
      </p>

      <div className="mt-10 grid gap-12">
        {groups.map(({ group, metrics }) => (
          <MetricGroup
            key={group}
            group={group}
            metrics={metrics}
            repos={repos}
          />
        ))}
      </div>
    </section>
  );
}

function MetricGroup({
  group,
  metrics,
  repos,
}: {
  group: ComparisonMetric["group"];
  metrics: ComparisonMetric[];
  repos: string[];
}) {
  return (
    <section>
      <div className="flex flex-wrap items-end justify-between gap-3">
        <h3 className="font-mono text-sm font-medium tracking-wide">
          :: {group}
        </h3>
        <p className={CAPTION}>{GROUP_COPY[group]}</p>
      </div>
      <div className="-mx-6 -my-2 overflow-x-auto whitespace-nowrap">
        <div className="inline-block min-w-full px-6 py-2 align-middle">
          <table className="w-full text-left text-base sm:text-sm">
            <thead>
              <tr className="border-b border-border">
                <th className="py-3 pr-5 font-mono font-medium whitespace-nowrap text-muted-foreground">
                  Signal
                </th>
                {repos.map((repo) => (
                  <th
                    key={repo}
                    className="py-3 pr-5 text-right font-mono font-medium whitespace-nowrap last:pr-0"
                  >
                    <a
                      href={`/${repo}`}
                      className="rounded outline-none hover:underline hover:decoration-border hover:underline-offset-4 focus-visible:ring-2 focus-visible:ring-accent/30"
                    >
                      {repo}
                    </a>
                  </th>
                ))}
              </tr>
            </thead>
            <tbody className="tabular-nums">
              {metrics.map((metric) => {
                const comparable = metric.values
                  .map((value) => value.numeric)
                  .filter((value): value is number => value !== null);
                const leader =
                  comparable.length < 2 || metric.higherIsBetter === undefined
                    ? null
                    : metric.higherIsBetter
                      ? Math.max(...comparable)
                      : Math.min(...comparable);
                return (
                  <tr
                    key={metric.label}
                    className="border-b border-border/40 last:border-0"
                  >
                    <td className="max-w-[20rem] py-3 pr-5 whitespace-normal">
                      <span className="font-medium">{metric.label}</span>
                      <span className="mt-0.5 block text-base text-muted-foreground sm:text-sm">
                        {metric.description}
                      </span>
                    </td>
                    {metric.values.map((value, index) => {
                      const wins =
                        leader !== null && value.numeric === leader;
                      return (
                        <td
                          key={`${metric.label}-${repos[index]}`}
                          className="min-w-36 py-3 pr-5 text-right align-top last:pr-0"
                        >
                          <span
                            className={
                              wins
                                ? "font-semibold text-foreground"
                                : value.numeric === null
                                  ? "text-muted-foreground"
                                  : "text-foreground"
                            }
                          >
                            {wins && (
                              <span
                                className="mr-1 text-accent"
                                aria-label="Best result in this row"
                              >
                                ◆
                              </span>
                            )}
                            {value.display}
                          </span>
                          {value.note && (
                            <span className="mt-0.5 block font-mono text-[0.6875rem] text-muted-foreground">
                              {value.note}
                            </span>
                          )}
                        </td>
                      );
                    })}
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </div>
    </section>
  );
}
