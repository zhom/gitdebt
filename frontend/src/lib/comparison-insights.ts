export type ComparisonPoint = { date: string; stars: number };
export type ComparisonDay = { date: string; value: number };
export type ComparisonLanguage = {
  language: string;
  files: number;
  code: number;
  blank: number;
  comment: number;
};
export type ComparisonAuthor = { commits: number };
export type ComparisonFile = {
  path: string;
  commits: number;
  fix_commits: number;
};

export type ComparisonStats = {
  ready: boolean;
  total_commits: number;
  attributed_commits?: number;
  analyzed_commits: number;
  analysis_scope_commits: number;
  analysis_truncated: boolean;
  bus_factor: number;
  files: ComparisonFile[];
  authors: ComparisonAuthor[];
  commit_days: ComparisonDay[];
  todo_days: ComparisonDay[];
  languages: ComparisonLanguage[];
};

type RegistryDownloads = {
  total: number;
  series?: { date: string; downloads: number }[];
};

export type ComparisonUsage = {
  forks: number;
  downloads: {
    npm: RegistryDownloads | null;
    crates: RegistryDownloads | null;
    pypi: RegistryDownloads | null;
    docker: { total: number } | null;
  };
};

export type ComparisonRepo = {
  slug: string;
  totalStars: number;
  createdAt: string | null;
  history: ComparisonPoint[];
  stats: ComparisonStats | null;
  usage: ComparisonUsage | null;
  analysisPending?: boolean;
};

export type ComparisonMetricValue = {
  display: string;
  /** Numeric values let the UI highlight the leader without parsing copy. */
  numeric: number | null;
  note?: string;
};

export type ComparisonMetric = {
  group: "Reach" | "Codebase" | "Maintenance" | "Ownership" | "Change signals";
  label: string;
  description: string;
  higherIsBetter?: boolean;
  values: ComparisonMetricValue[];
};

function compact(value: number): string {
  return new Intl.NumberFormat("en", {
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(value);
}

function unavailable(repo: ComparisonRepo): ComparisonMetricValue {
  return {
    display: repo.analysisPending ? "Analysis running" : "Not available",
    numeric: null,
  };
}

function validDays(days: ComparisonDay[]): { at: number; value: number }[] {
  return days
    .map((day) => ({
      at: Date.parse(`${day.date.slice(0, 10)}T00:00:00Z`),
      value: Math.max(0, day.value),
    }))
    .filter((day) => Number.isFinite(day.at))
    .sort((a, b) => a.at - b.at);
}

/** Sum an inclusive trailing window, anchored to the newest recorded day. */
export function trailingActivity(
  days: ComparisonDay[],
  windowDays: number,
): { commits: number; activeDays: number; previousCommits: number } {
  const rows = validDays(days);
  if (rows.length === 0 || windowDays <= 0) {
    return { commits: 0, activeDays: 0, previousCommits: 0 };
  }
  const dayMs = 86_400_000;
  const end = rows.at(-1)!.at;
  const start = end - (windowDays - 1) * dayMs;
  const previousStart = start - windowDays * dayMs;
  let commits = 0;
  let activeDays = 0;
  let previousCommits = 0;
  for (const row of rows) {
    if (row.at >= start && row.at <= end) {
      commits += row.value;
      if (row.value > 0) activeDays += 1;
    } else if (row.at >= previousStart && row.at < start) {
      previousCommits += row.value;
    }
  }
  return { commits, activeDays, previousCommits };
}

export function dominantLanguage(
  languages: ComparisonLanguage[],
): { language: string; code: number; share: number } | null {
  const rows = languages
    .map((language) => ({ ...language, code: Math.max(0, language.code) }))
    .filter((language) => language.code > 0)
    .sort(
      (a, b) =>
        b.code - a.code || a.language.localeCompare(b.language, "en"),
    );
  const total = rows.reduce((sum, language) => sum + language.code, 0);
  if (rows.length === 0 || total === 0) return null;
  return {
    language: rows[0].language,
    code: rows[0].code,
    share: rows[0].code / total,
  };
}

export function contributorConcentration(
  stats: ComparisonStats,
): number | null {
  const total = Math.max(
    0,
    stats.attributed_commits ?? stats.analyzed_commits,
  );
  if (total === 0 || stats.authors.length === 0) return null;
  const top = Math.max(0, ...stats.authors.map((author) => author.commits));
  return Math.min(1, top / total);
}

export function hotFileFixShare(stats: ComparisonStats): number | null {
  const commits = stats.files.reduce(
    (sum, file) => sum + Math.max(0, file.commits),
    0,
  );
  if (commits === 0) return null;
  const fixes = stats.files.reduce(
    (sum, file) => sum + Math.max(0, file.fix_commits),
    0,
  );
  return fixes / commits;
}

function starGain(history: ComparisonPoint[], days: number): number | null {
  const parsed = history
    .map((point) => ({ at: Date.parse(point.date), stars: point.stars }))
    .filter(
      (point) => Number.isFinite(point.at) && Number.isFinite(point.stars),
    )
    .sort((a, b) => a.at - b.at);
  if (parsed.length < 2) return null;
  const end = parsed.at(-1)!;
  const threshold = end.at - days * 86_400_000;
  let baseline = parsed[0];
  for (const point of parsed) {
    if (point.at > threshold) break;
    baseline = point;
  }
  return Math.max(0, end.stars - baseline.stars);
}

function repoAge(createdAt: string | null): number | null {
  if (!createdAt) return null;
  const created = Date.parse(createdAt);
  if (!Number.isFinite(created)) return null;
  return Math.max(0, (Date.now() - created) / (365.25 * 86_400_000));
}

function packageReach(usage: ComparisonUsage | null): {
  total: number;
  label: string;
} | null {
  if (!usage) return null;
  const sources = [
    ["npm", usage.downloads.npm?.total],
    ["crates.io", usage.downloads.crates?.total],
    ["PyPI", usage.downloads.pypi?.total],
    ["Docker", usage.downloads.docker?.total],
  ] as const;
  const available: { label: string; total: number }[] = [];
  for (const [label, total] of sources) {
    if (typeof total === "number" && Number.isFinite(total)) {
      available.push({ label, total });
    }
  }
  available.sort((a, b) => b.total - a.total);
  return available.length > 0
    ? { total: available[0].total, label: available[0].label }
    : null;
}

function todoSignal(days: ComparisonDay[]): {
  total: number;
  trailingDelta: number;
} | null {
  const rows = validDays(days);
  if (rows.length === 0) return null;
  const latest = rows.at(-1)!;
  const threshold = latest.at - 90 * 86_400_000;
  let baseline = rows[0];
  for (const row of rows) {
    if (row.at > threshold) break;
    baseline = row;
  }
  return {
    total: latest.value,
    trailingDelta: latest.value - baseline.value,
  };
}

export function buildComparisonMetrics(
  repos: ComparisonRepo[],
): ComparisonMetric[] {
  const statsValue = (
    repo: ComparisonRepo,
    read: (stats: ComparisonStats) => ComparisonMetricValue,
  ) => (repo.stats?.ready ? read(repo.stats) : unavailable(repo));

  return [
    {
      group: "Reach",
      label: "GitHub stars",
      description: "Current public GitHub metadata total.",
      higherIsBetter: true,
      values: repos.map((repo) => ({
        display: repo.totalStars.toLocaleString("en"),
        numeric: repo.totalStars,
      })),
    },
    {
      group: "Reach",
      label: "Stars gained · trailing 90d",
      description: "Change in the newest 90 days covered by each cached series.",
      higherIsBetter: true,
      values: repos.map((repo) => {
        const value = starGain(repo.history, 90);
        return value === null
          ? { display: "History unavailable", numeric: null }
          : { display: `+${value.toLocaleString("en")}`, numeric: value };
      }),
    },
    {
      group: "Reach",
      label: "Forks",
      description: "Current public GitHub metadata total.",
      higherIsBetter: true,
      values: repos.map((repo) =>
        repo.usage
          ? {
              display: repo.usage.forks.toLocaleString("en"),
              numeric: repo.usage.forks,
            }
          : { display: "Not available", numeric: null },
      ),
    },
    {
      group: "Reach",
      label: "Largest package audience",
      description: "Largest resolved registry total; sources are shown explicitly.",
      higherIsBetter: true,
      values: repos.map((repo) => {
        const reach = packageReach(repo.usage);
        return reach
          ? {
              display: compact(reach.total),
              numeric: reach.total,
              note: reach.label,
            }
          : { display: "No resolved package", numeric: null };
      }),
    },
    {
      group: "Reach",
      label: "Repository age",
      description: "Elapsed years since the GitHub creation timestamp.",
      values: repos.map((repo) => {
        const age = repoAge(repo.createdAt);
        return age === null
          ? { display: "Not available", numeric: null }
          : {
              display: `${age.toFixed(age >= 10 ? 0 : 1)} years`,
              numeric: age,
            };
      }),
    },
    {
      group: "Codebase",
      label: "Dominant language",
      description: "Largest share of analyzed code lines at current HEAD.",
      values: repos.map((repo) =>
        statsValue(repo, (stats) => {
          const dominant = dominantLanguage(stats.languages);
          return dominant
            ? {
                display: dominant.language,
                numeric: dominant.share,
                note: `${Math.round(dominant.share * 100)}% of code`,
              }
            : { display: "No code census", numeric: null };
        }),
      ),
    },
    {
      group: "Codebase",
      label: "Code lines",
      description: "Analyzed source lines at current HEAD, excluding blanks and comments.",
      values: repos.map((repo) =>
        statsValue(repo, (stats) => {
          const value = stats.languages.reduce(
            (sum, language) => sum + Math.max(0, language.code),
            0,
          );
          return value > 0
            ? { display: compact(value), numeric: value }
            : { display: "No line census", numeric: null };
        }),
      ),
    },
    {
      group: "Codebase",
      label: "Languages represented",
      description: "Languages with non-zero files or line counts.",
      values: repos.map((repo) =>
        statsValue(repo, (stats) => ({
          display: stats.languages.length.toLocaleString("en"),
          numeric: stats.languages.length,
        })),
      ),
    },
    {
      group: "Maintenance",
      label: "Repository commits",
      description: "Full repository commit total reported by the analysis.",
      higherIsBetter: true,
      values: repos.map((repo) =>
        statsValue(repo, (stats) => ({
          display: compact(stats.total_commits),
          numeric: stats.total_commits,
          note: stats.analysis_truncated
            ? `${compact(stats.analysis_scope_commits)} analyzed`
            : "complete analysis",
        })),
      ),
    },
    {
      group: "Maintenance",
      label: "Commits · trailing 90d",
      description: "Commit count in the newest 90 days covered by the repository analysis.",
      higherIsBetter: true,
      values: repos.map((repo) =>
        statsValue(repo, (stats) => {
          const activity = trailingActivity(stats.commit_days, 90);
          const trend =
            activity.previousCommits > 0
              ? activity.commits / activity.previousCommits - 1
              : null;
          return {
            display: activity.commits.toLocaleString("en"),
            numeric: activity.commits,
            note:
              trend === null
                ? `${activity.activeDays} active days`
                : `${activity.activeDays} active days · ${trend >= 0 ? "+" : ""}${Math.round(trend * 100)}% vs prior`,
          };
        }),
      ),
    },
    {
      group: "Ownership",
      label: "Bus factor",
      description: "Fewest analyzed contributors carrying at least half the commits.",
      higherIsBetter: true,
      values: repos.map((repo) =>
        statsValue(repo, (stats) => ({
          display: stats.bus_factor.toLocaleString("en"),
          numeric: stats.bus_factor,
          note: stats.analysis_truncated ? "within analyzed scope" : undefined,
        })),
      ),
    },
    {
      group: "Ownership",
      label: "Top contributor concentration",
      description: "Share of attributed commits carried by the most active contributor.",
      higherIsBetter: false,
      values: repos.map((repo) =>
        statsValue(repo, (stats) => {
          const share = contributorConcentration(stats);
          return share === null
            ? { display: "Not available", numeric: null }
            : {
                display: `${Math.round(share * 100)}%`,
                numeric: share,
                note: `${stats.authors.length} contributors represented`,
              };
        }),
      ),
    },
    {
      group: "Change signals",
      label: "Fix-labelled share · frequently changed files",
      description: "Fix-labelled commits divided by commits across the analyzed high-frequency file set.",
      higherIsBetter: false,
      values: repos.map((repo) =>
        statsValue(repo, (stats) => {
          const share = hotFileFixShare(stats);
          return share === null
            ? { display: "Not available", numeric: null }
            : {
                display: `${Math.round(share * 100)}%`,
                numeric: share,
                note: `${stats.files.length} hot files sampled`,
              };
        }),
      ),
    },
    {
      group: "Change signals",
      label: "Recent TODO/FIXME movement",
      description: "Latest cumulative marker count and its change over the trailing 90 days.",
      higherIsBetter: false,
      values: repos.map((repo) =>
        statsValue(repo, (stats) => {
          const signal = todoSignal(stats.todo_days);
          return signal
            ? {
                display: signal.total.toLocaleString("en"),
                numeric: signal.total,
                note: `${signal.trailingDelta >= 0 ? "+" : ""}${signal.trailingDelta} in 90d`,
              }
            : { display: "No marker history", numeric: null };
        }),
      ),
    },
  ];
}
