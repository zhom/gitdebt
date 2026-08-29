import { useEffect, useMemo, useState } from "react";

import {
  BODY,
  CAPTION,
  DATUM,
  FIELD,
  HEADING,
  MEASURE,
  SECTION_HEADER,
} from "@/components/style-tokens";
import {
  buildComparisonMetrics,
  type ComparisonMetric,
  type ComparisonRepo,
  type ComparisonStats,
  type ComparisonUsage,
} from "@/lib/comparison-insights";
import { cn } from "@/lib/utils";

/**
 * The comparison, as a schedule of measurements.
 *
 * A drawing compares parts in a table, and so does this: one row per signal,
 * one column per repository, every figure tabular and every column the same
 * width, so the eye can run down a column or across a row without the layout
 * moving under it. Content length never decides where anything lands — a table
 * is the one layout that guarantees that, which is why it is still a table.
 *
 * The leading figure in a row is lettered in drafting red. It is the measured
 * best of that row, which is the only thing red is ever spent on here, and the
 * same fact is stated in words to a screen reader so colour is never carrying
 * it alone.
 */

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
        repos.some((slug) => slug.toLowerCase() === detail.repo?.toLowerCase()) &&
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
    <section className="mt-16 scroll-mt-24 border-t border-rule pt-12">
      <div className={SECTION_HEADER}>
        <h2 className={HEADING}>Repository health compared</h2>
        <p className={CAPTION}>
          Live analysis · unavailable data stays labelled
        </p>
      </div>
      <p className={cn("mt-3", BODY, MEASURE)}>
        Popularity is only one axis. Compare technical scale, maintenance
        cadence, ownership resilience and change pressure against the same
        completed analysis that sits behind each report.
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
      <div className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1">
        <h3 className="font-draft text-[1.125rem] leading-tight text-ink">
          {group}
        </h3>
        <p className={CAPTION}>{GROUP_COPY[group]}</p>
      </div>

      <div className="mt-4 overflow-x-auto border border-rule-strong bg-paper">
        <table className="w-full min-w-[42rem] border-collapse text-left">
          <thead>
            <tr className="border-b border-rule-strong">
              <th scope="col" className={cn(FIELD, "px-4 py-3 align-bottom")}>
                Signal
              </th>
              {repos.map((repo) => (
                <th
                  key={repo}
                  scope="col"
                  className="w-40 px-4 py-3 text-right align-bottom"
                >
                  <a
                    href={`/${repo}`}
                    className={cn(
                      DATUM,
                      "text-ink outline-none transition-colors duration-[--duration-ui] hover:text-signal focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-signal",
                    )}
                  >
                    {repo}
                  </a>
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {metrics.map((metric) => {
              const comparable = metric.values
                .map((value) => value.numeric)
                .filter((value): value is number => value !== null);
              const best =
                comparable.length < 2 || metric.higherIsBetter === undefined
                  ? null
                  : metric.higherIsBetter
                    ? Math.max(...comparable)
                    : Math.min(...comparable);
              // A tie has no leader. Marking every column red when two repos
              // read the same — which is what an unanalysed pair does, since
              // both sit at zero — states a result the data does not contain.
              const leader =
                best !== null &&
                comparable.filter((value) => value === best).length === 1
                  ? best
                  : null;
              return (
                <tr key={metric.label} className="border-b border-rule last:border-0">
                  <th
                    scope="row"
                    className="max-w-[22rem] px-4 py-3 text-left align-top font-normal"
                  >
                    <span className="text-[0.875rem] text-ink">
                      {metric.label}
                    </span>
                    <span className={cn(CAPTION, "mt-1 block")}>
                      {metric.description}
                    </span>
                  </th>
                  {metric.values.map((value, index) => {
                    const wins = leader !== null && value.numeric === leader;
                    return (
                      <td
                        key={`${metric.label}-${repos[index]}`}
                        className="px-4 py-3 text-right align-top"
                      >
                        <span
                          className={cn(
                            DATUM,
                            wins
                              ? "text-signal"
                              : value.numeric === null
                                ? "text-ink-3"
                                : "text-ink",
                          )}
                        >
                          {wins && (
                            <span className="sr-only">
                              Best result in this row:{" "}
                            </span>
                          )}
                          {value.display}
                        </span>
                        {value.note && (
                          <span className={cn(CAPTION, "mt-1 block")}>
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
    </section>
  );
}
