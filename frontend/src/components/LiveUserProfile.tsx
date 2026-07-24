import { useEffect, useMemo, useState, type ReactNode } from "react";
import { ExternalLink, Loader2 } from "lucide-react";

import { ButtonLink } from "@/components/ButtonLink";
import { ChartViewer } from "@/components/ChartViewer";
import { EmbedSnippet } from "@/components/EmbedSnippet";
import { ProfileCardPreview } from "@/components/ProfileCardPreview";
import { StatCard } from "@/components/StatCard";
import {
  BODY,
  CAPTION,
  EYEBROW,
  HEADING,
  KPI,
  PANEL,
  ROW,
  ROW_BADGE,
  SECTION_ACTION,
  SECTION_HEADER,
  TITLE,
} from "@/components/style-tokens";
import { MEDIA_RENDER_REVISION } from "@/lib/media";
import { useRenderedTheme } from "@/lib/rendered-theme";
import {
  firstStarYear,
  formatCompact,
  gainedInTrailingDays,
  growthTrend,
} from "@/lib/star-insights";
import { profileLogin } from "@/lib/static-routing.mjs";
import { cn } from "@/lib/utils";

/** Mirrors the backend `GET /api/users/:login/analyze` contract. */
export type UserAnalyze = {
  login: string;
  repos_included: number;
  repos_pending: number;
  repos_analyzed?: number;
  repos_analyzing?: number;
  total_stars: number;
  history: { date: string; stars: number }[];
};

export type UserRepoRow = {
  repo: string;
  stars: number;
  forks: number;
  commits: number;
  commits_recent: number;
  spark: number[];
};

export type VisionaryRepo = {
  repo: string;
  current_stars: number;
  stars_at_first_contribution: number;
  first_contribution_at: string;
  owned: boolean;
};

/** Mirrors the backend `GET /api/users/:login/stats.json` contract. */
export type UserStats = {
  login: string;
  ready: boolean;
  repos_tracked: number;
  repos_analyzed: number;
  total_stars: number;
  total_forks: number;
  authored_commits: number;
  contributed_repos: number;
  owned_contributed_repos: number;
  external_contributed_repos: number;
  owned_authored_commits: number;
  external_authored_commits: number;
  visionary_repos: VisionaryRepo[];
  analyzed_commits: number;
  since_year: number | null;
  solo_maintained: number;
  shared_maintained: number;
  languages: {
    language: string;
    files: number;
    code: number;
    blank: number;
    comment: number;
  }[];
  top_repos: UserRepoRow[];
  active_repos: UserRepoRow[];
  commit_days: { date: string; value: number }[];
};

type Props = {
  apiBase: string;
  /** Known login. When absent it is read from the URL, then the session. */
  login?: string;
  /** Resolve the signed-in login when neither prop nor URL carries one. */
  session?: boolean;
  /** Absolute origin used for README embed links. */
  siteOrigin?: string;
  /** Build-time seeds. Present only on the prerendered `/{login}` page. */
  analyze?: UserAnalyze | null;
  stats?: UserStats | null;
  repos?: string[];
};

const POLL_MS = 8_000;

/**
 * The profile's embeddable code signals. `commit-trend` is deliberately absent:
 * a monthly commit-volume curve said nothing the 52-week calendar does not.
 */
const PROFILE_CHARTS = [
  {
    name: "contributions",
    label: "Contribution footprint",
    blurb: "Authored work in owned projects versus other people's projects.",
  },
  {
    name: "languages",
    label: "Language footprint",
    blurb: "Lines of code by language across every analyzed repo you own.",
  },
  {
    name: "commit-activity",
    label: "Commit activity",
    blurb: "Every commit landed in the last 52 weeks, summed across owned repos.",
  },
] as const;

const OWNED_CODE_CHARTS = PROFILE_CHARTS.filter(
  (chart) => chart.name !== "contributions",
);

const nf = new Intl.NumberFormat("en-US");
const num = (value: number | null | undefined) =>
  value === null || value === undefined ? "—" : nf.format(value);
const signed = (value: number | null | undefined) =>
  value === null || value === undefined ? "—" : `+${nf.format(value)}`;

function urlLogin(): string | null {
  if (typeof window === "undefined") return null;
  const query = new URLSearchParams(window.location.search).get("login");
  const fromQuery = profileLogin(query ?? "");
  if (fromQuery) return fromQuery;
  const segments = window.location.pathname.replace(/^\/+|\/+$/g, "").split("/");
  if (segments.length !== 1) return null;
  try {
    return profileLogin(decodeURIComponent(segments[0]));
  } catch {
    return null;
  }
}

/**
 * Deterministic sparkline path over a cumulative series. Returns null when
 * there is not enough history to draw an honest shape.
 */
function sparkPath(values: number[], w = 132, h = 30): string | null {
  if (!Array.isArray(values) || values.length < 3) return null;
  const min = Math.min(...values);
  const max = Math.max(...values);
  const span = max - min || 1;
  const step = w / (values.length - 1);
  return values
    .map((value, index) => {
      const x = (index * step).toFixed(1);
      const y = (h - ((value - min) / span) * h).toFixed(1);
      return `${index === 0 ? "M" : "L"}${x} ${y}`;
    })
    .join(" ");
}

async function fetchJson<T>(url: string): Promise<T | null> {
  try {
    const response = await fetch(url, {
      cache: "no-store",
      credentials: "omit",
      headers: { accept: "application/json" },
    });
    if (!response.ok) return null;
    return (await response.json()) as T;
  } catch {
    return null;
  }
}

export function LiveUserProfile({
  apiBase,
  login: requestedLogin,
  session = false,
  siteOrigin = "https://gitdebt.com",
  analyze: seedAnalyze = null,
  stats: seedStats = null,
  repos: seedRepos = [],
}: Props) {
  const knownLogin = useMemo(
    () => profileLogin(requestedLogin ?? "") ?? urlLogin(),
    [requestedLogin],
  );
  const [sessionLogin, setSessionLogin] = useState<string | null>(null);
  const [sessionChecked, setSessionChecked] = useState(!session);
  const login = knownLogin ?? sessionLogin;

  const seededAnalyze =
    seedAnalyze && seedAnalyze.login.toLowerCase() === login
      ? seedAnalyze
      : null;
  const seededStats =
    seedStats && seedStats.login.toLowerCase() === login ? seedStats : null;

  const [data, setData] = useState<UserAnalyze | null>(seededAnalyze);
  const [stats, setStats] = useState<UserStats | null>(seededStats);
  const [loading, setLoading] = useState(Boolean(login) && !seededAnalyze);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!session || knownLogin) return;
    const controller = new AbortController();
    void fetch(`${apiBase}/api/me`, {
      credentials: "include",
      headers: { accept: "application/json" },
      signal: controller.signal,
    })
      .then(async (response) =>
        response.ok ? ((await response.json()) as { login?: string }) : null,
      )
      .then((me) => setSessionLogin(profileLogin(me?.login ?? "")))
      .catch(() => undefined)
      .finally(() => setSessionChecked(true));
    return () => controller.abort();
  }, [apiBase, session, knownLogin]);

  const seedIsSettled =
    seededAnalyze !== null &&
    seededAnalyze.repos_pending === 0 &&
    (seededAnalyze.repos_analyzing ?? 0) === 0;

  useEffect(() => {
    if (!login || seedIsSettled) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let warmAttempted = false;

    async function load(target: string) {
      try {
        setLoading(true);
        let response: Response | null = null;
        if (!warmAttempted) {
          warmAttempted = true;
          const warm = await fetch(`${apiBase}/api/users/${target}/warm`, {
            method: "POST",
            cache: "no-store",
            credentials: "include",
            headers: { accept: "application/json" },
          });
          if (warm.ok) response = warm;
        }
        response ??= await fetch(`${apiBase}/api/users/${target}/analyze`, {
          cache: "no-store",
          credentials: "omit",
          headers: { accept: "application/json" },
        });
        if (response.status === 404) throw new Error("GitHub user not found.");
        if (!response.ok) {
          throw new Error("Profile data is temporarily unavailable.");
        }
        const payload = (await response.json()) as Partial<UserAnalyze>;
        if (cancelled) return;
        const next: UserAnalyze = {
          login: payload.login ?? target,
          repos_included: payload.repos_included ?? 0,
          repos_pending: payload.repos_pending ?? 0,
          repos_analyzed: payload.repos_analyzed ?? 0,
          repos_analyzing: payload.repos_analyzing ?? 0,
          total_stars: payload.total_stars ?? 0,
          history: payload.history ?? [],
        };
        setData(next);
        setError(null);
        if (next.repos_pending > 0 || (next.repos_analyzing ?? 0) > 0) {
          timer = setTimeout(() => void load(target), POLL_MS);
        }
      } catch (reason) {
        if (cancelled) return;
        setError(
          reason instanceof Error
            ? reason.message
            : "Profile data is temporarily unavailable.",
        );
        timer = setTimeout(() => void load(target), POLL_MS);
      } finally {
        if (!cancelled) setLoading(false);
      }
    }

    void load(login);
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [apiBase, login, seedIsSettled]);

  // Code signals are a Postgres-only aggregate. A miss is never fatal: the
  // star report still renders without it.
  const analysisSettled =
    data !== null &&
    data.repos_pending === 0 &&
    (data.repos_analyzing ?? 0) === 0;

  useEffect(() => {
    if (!login || (seededStats && analysisSettled)) return;
    let cancelled = false;
    void fetchJson<UserStats>(`${apiBase}/api/users/${login}/stats.json`).then(
      (payload) => {
        if (!cancelled && payload) setStats(payload);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [apiBase, login, seededStats, analysisSettled]);

  if (!login) {
    return (
      <section>
        <h1 className={TITLE}>
          {session && !sessionChecked
            ? "Opening your profile report"
            : "No GitHub profile selected"}
        </h1>
        <p className={cn(BODY, "mt-2 max-w-[65ch]")}>
          {session && !sessionChecked
            ? "Reading your signed-in account."
            : "Sign in from the header to open the aggregate report for your own public repositories, or open any maintainer at gitdebt.com/their-login."}
        </p>
      </section>
    );
  }

  const canonical = `${siteOrigin.replace(/\/$/, "")}/${login}`;
  const hasData =
    data !== null && data.repos_included > 0 && data.history.length > 0;
  const gained30 = data ? gainedInTrailingDays(data.history, 30) : null;
  const gained90 = data ? gainedInTrailingDays(data.history, 90) : null;
  const trend = data ? growthTrend(data.history) : null;
  const firstYear = stats?.since_year ?? (data ? firstStarYear(data.history) : null);

  const revision = data
    ? `${data.total_stars}-${data.repos_included}-${data.repos_pending}`
    : "pending";

  const topRepos = (stats?.top_repos ?? []).filter(
    (row) => row.stars > 0 || row.commits > 0,
  );
  const activeRepos = stats?.active_repos ?? [];
  const activeMax = Math.max(1, ...activeRepos.map((row) => row.commits_recent));
  const topLanguages = (stats?.languages ?? []).slice(0, 6);
  const languageTotal = topLanguages.reduce(
    (sum, row) => sum + row.code + row.blank + row.comment,
    0,
  );
  const scoredRepos =
    (stats?.solo_maintained ?? 0) + (stats?.shared_maintained ?? 0);
  const commitDaysTotal = (stats?.commit_days ?? []).reduce(
    (sum, day) => sum + day.value,
    0,
  );
  const hasCodeSignals = Boolean(stats && stats.repos_analyzed > 0);
  const hasContributionSignals = Boolean(
    stats &&
      stats.contributed_repos > 0 &&
      typeof stats.owned_contributed_repos === "number" &&
      typeof stats.external_contributed_repos === "number" &&
      Array.isArray(stats.visionary_repos),
  );
  const ownedContributedRepos = stats?.owned_contributed_repos ?? 0;
  const externalContributedRepos = stats?.external_contributed_repos ?? 0;
  const ownedAuthoredCommits = stats?.owned_authored_commits ?? 0;
  const externalAuthoredCommits = stats?.external_authored_commits ?? 0;
  const visionaryRepos = stats?.visionary_repos ?? [];
  const contributionTotal = stats?.authored_commits ?? 0;
  const externalShare =
    contributionTotal > 0
      ? externalAuthoredCommits / contributionTotal
      : 0;
  const contributionStory =
    externalShare >= 0.68
      ? `${login} is ecosystem-led: most attributed commits land in projects owned by other people.`
      : externalShare <= 0.32
        ? `${login} is builder-led: most attributed commits land in projects they own.`
        : `${login} has a balanced footprint across owned projects and the wider open-source ecosystem.`;
  const trackedRepos =
    seedRepos.length > 0
      ? seedRepos
      : [
          ...new Set([
            ...topRepos.map((row) => row.repo),
            ...activeRepos.map((row) => row.repo),
          ]),
        ];

  const kpis = [
    {
      label: "Stars",
      value: hasData ? formatCompact(data!.total_stars) : "—",
      note: `across ${num(data?.repos_included ?? null)} tracked repos`,
    },
    {
      label: "Last 30 days",
      value: gained30 === null ? "—" : signed(gained30),
      note: "new stars",
    },
    {
      label: "Last 90 days",
      value: gained90 === null ? "—" : signed(gained90),
      note: trend ? `${trend} vs lifetime pace` : "new stars",
    },
    {
      label: "Commits authored",
      value:
        stats && stats.authored_commits > 0
          ? formatCompact(stats.authored_commits)
          : "—",
      note:
        stats && stats.contributed_repos > 0
          ? `in ${num(stats.contributed_repos)} repos`
          : "awaiting analysis",
    },
    {
      label: "Forks",
      value:
        stats && stats.total_forks > 0 ? formatCompact(stats.total_forks) : "—",
      note: "across owned repos",
    },
    {
      label: "First commit",
      value: firstYear ? String(firstYear) : "—",
      note: firstYear ? "earliest tracked year" : "not observed yet",
    },
  ];

  const pending =
    loading || (data?.repos_pending ?? 0) > 0 || (data?.repos_analyzing ?? 0) > 0;

  return (
    <div>
      <header>
        <div className="flex flex-col gap-5 sm:flex-row sm:items-end sm:justify-between">
          <div className="min-w-0">
            <h1 className={TITLE}>{login}</h1>
            <p className={cn(BODY, "mt-3 max-w-[68ch] text-[15px]")}>
              {hasData ? (
                <>
                  {login}'s public repos have earned{" "}
                  <span className="tabular-nums text-foreground">
                    {data!.total_stars.toLocaleString()}
                  </span>{" "}
                  GitHub stars{firstYear ? ` since ${firstYear}` : ""}
                  {gained90 !== null && gained90 > 0
                    ? `, ${gained90.toLocaleString()} of them in the last 90 days`
                    : ""}
                  , across {data!.repos_included} tracked{" "}
                  {data!.repos_included === 1 ? "repo" : "repos"}.
                </>
              ) : (
                <>
                  Star history for {login}'s public repos is being gathered.
                  Totals appear as each repository's history completes.
                </>
              )}
            </p>
          </div>
          <ButtonLink
            href={`https://github.com/${login}`}
            target="_blank"
            rel="noopener noreferrer"
            variant="outline"
            className="shrink-0 self-start sm:self-auto"
          >
            Open GitHub profile
            <ExternalLink className="size-3.5" strokeWidth={1.8} aria-hidden="true" />
          </ButtonLink>
        </div>

        {pending && (
          <div className={cn(PANEL, "mt-6 flex items-start gap-3 p-3.5")} role="status">
            <Loader2
              className="mt-0.5 size-4 shrink-0 motion-safe:animate-spin"
              aria-hidden="true"
            />
            <p className={CAPTION}>
              {(data?.repos_analyzing ?? 0) > 0
                ? `Analyzing ${data!.repos_analyzing} repositories with interactive priority. `
                : "Discovering public repositories. "}
              This page updates every few seconds.
            </p>
          </div>
        )}

        {error && <p className={cn(CAPTION, "mt-6 font-mono")}>{error}</p>}
      </header>

      {data && (
        <section className="mt-12" aria-label="Profile summary">
          <dl
            className={cn(
              PANEL,
              "grid grid-cols-2 divide-border/40 p-3.5 sm:grid-cols-3 sm:divide-x lg:grid-cols-6",
            )}
          >
            {kpis.map((kpi) => (
              <div key={kpi.label} className="min-w-0 px-3.5 py-2">
                <dt className={EYEBROW}>{kpi.label}</dt>
                <dd className={cn("mt-2", KPI, "text-foreground")}>{kpi.value}</dd>
                <p className={cn(CAPTION, "mt-2")}>{kpi.note}</p>
              </div>
            ))}
          </dl>
        </section>
      )}

      {data && (
        <section className="mt-16 scroll-mt-24" id="star-history">
          <div className={SECTION_HEADER}>
            <h2 className={HEADING}>Star history</h2>
            <p className="font-mono text-[11px] text-muted-foreground">
              summed across {num(data.repos_included)} repos
            </p>
          </div>
          <div className="mt-6">
            <ChartViewer
              apiBase={apiBase}
              path={`/api/users/${login}/chart.svg`}
              alt={`Aggregate star history across ${login}'s public repos`}
              caption="Aggregate star history"
              embedLink={canonical}
              label={login}
              points={data.history}
            />
          </div>
        </section>
      )}

      {topRepos.length > 0 && (
        <section className="mt-16 scroll-mt-24" id="top-repositories">
          <div className={SECTION_HEADER}>
            <h2 className={HEADING}>Top repositories</h2>
            <a href="/leaderboard" className={SECTION_ACTION}>
              leaderboard →
            </a>
          </div>
          <ul className="mt-6">
            {topRepos.map((row) => {
              const path = sparkPath(row.spark);
              return (
                <li key={row.repo} className="border-b border-border/40 last:border-0">
                  <a
                    href={`/${row.repo}`}
                    className="group flex items-center gap-4 rounded-md px-2.5 py-3 outline-none transition-colors duration-150 hover:bg-card/60 focus-visible:ring-2 focus-visible:ring-accent/30"
                  >
                    <span className="min-w-0 flex-1">
                      <span className="block truncate font-mono text-[12px] text-muted-foreground transition-colors group-hover:text-foreground">
                        {row.repo}
                      </span>
                      <span className="mt-1 block font-mono text-[11px] tabular-nums text-muted-foreground/80">
                        {num(row.stars)} stars
                        {row.forks > 0 ? ` · ${num(row.forks)} forks` : ""}
                        {row.commits > 0 ? ` · ${num(row.commits)} commits` : ""}
                      </span>
                    </span>
                    {path ? (
                      <svg
                        className="h-[30px] w-[132px] shrink-0 text-foreground/45 transition-colors duration-150 group-hover:text-foreground/80"
                        viewBox="0 0 132 30"
                        fill="none"
                        aria-hidden="true"
                        preserveAspectRatio="none"
                      >
                        <path
                          d={path}
                          stroke="currentColor"
                          strokeWidth="1.25"
                          strokeLinejoin="round"
                          vectorEffect="non-scaling-stroke"
                        />
                      </svg>
                    ) : (
                      <span className="hidden w-[132px] shrink-0 text-right font-mono text-[10px] tracking-[0.08em] text-muted-foreground/70 uppercase sm:block">
                        history pending
                      </span>
                    )}
                  </a>
                </li>
              );
            })}
          </ul>
          <p className={cn(BODY, "mt-3")}>
            Sparklines plot cumulative stars by month, and only render once a
            repository's full star history is cached — a partial series would
            draw a shape that isn't real.
          </p>
        </section>
      )}

      {hasContributionSignals && (
        <section className="mt-16 scroll-mt-24" id="contribution-footprint">
          <div className={SECTION_HEADER}>
            <h2 className={HEADING}>Contribution story</h2>
            <p className="font-mono text-[11px] text-muted-foreground">
              attributed public commits
            </p>
          </div>
          <p className={cn(BODY, "mt-2 max-w-[70ch]")}>{contributionStory}</p>

          <dl
            className={cn(
              PANEL,
              "@container mt-6 grid grid-cols-2 divide-border/40 p-3.5 sm:grid-cols-4 sm:divide-x",
            )}
          >
            <div className="min-w-0 px-3.5 py-2">
              <dt className={cn(EYEBROW, "whitespace-nowrap")}>Owned repos</dt>
              <dd className={cn("mt-2", KPI, "text-foreground")}>
                {num(ownedContributedRepos)}
              </dd>
              <p className={cn(CAPTION, "mt-2")}>
                {formatCompact(ownedAuthoredCommits)} authored commits
              </p>
            </div>
            <div className="min-w-0 px-3.5 py-2">
              <dt className={cn(EYEBROW, "whitespace-nowrap")}>Outside repos</dt>
              <dd className={cn("mt-2", KPI, "text-foreground")}>
                {num(externalContributedRepos)}
              </dd>
              <p className={cn(CAPTION, "mt-2")}>
                {formatCompact(externalAuthoredCommits)} authored commits
              </p>
            </div>
            <div className="min-w-0 px-3.5 py-2">
              <dt className={cn(EYEBROW, "whitespace-nowrap")}>Outside share</dt>
              <dd className={cn("mt-2", KPI, "text-foreground")}>
                {Math.round(externalShare * 100)}%
              </dd>
              <p className={cn(CAPTION, "mt-2")}>of attributed commit volume</p>
            </div>
            <div className="min-w-0 px-3.5 py-2">
              <dt className={cn(EYEBROW, "whitespace-nowrap")}>Visionary</dt>
              <dd className={cn("mt-2", KPI, "text-foreground")}>
                {num(visionaryRepos.length)}
              </dd>
              <p className={cn(CAPTION, "mt-2")}>breakout projects spotted early</p>
            </div>
          </dl>

          <div className="mt-8">
            <StatCard
              src={`${apiBase}/api/users/${login}/stats/contributions.svg`}
              alt={`Contribution footprint for ${login}`}
              caption="Contribution footprint"
              apiBase={apiBase}
              embedLink={canonical}
            />
          </div>

          {visionaryRepos.length > 0 && (
            <div className="mt-8">
              <h3 className={EYEBROW}>Earned achievements</h3>
              <div className="mt-3 grid gap-3 sm:grid-cols-2">
                {visionaryRepos.map((achievement) => {
                  const early = achievement.stars_at_first_contribution;
                  const growth =
                    early > 0
                      ? `${(achievement.current_stars / early).toFixed(1)}× growth`
                      : "before the first recorded star";
                  return (
                    <a
                      key={achievement.repo}
                      href={`/${achievement.repo}`}
                      className={cn(
                        PANEL,
                        "group relative overflow-hidden p-4 outline-none focus-visible:ring-2 focus-visible:ring-accent/30",
                      )}
                    >
                      <span className="font-mono text-[10px] font-semibold tracking-[0.16em] text-[var(--swatch-purple)] uppercase">
                        Visionary
                      </span>
                      <span className="mt-2 block truncate font-mono text-[13px] text-foreground">
                        {achievement.repo}
                      </span>
                      <span className={cn(CAPTION, "mt-2 block")}>
                        Contributed at {num(early)} stars · now{" "}
                        {num(achievement.current_stars)} · {growth}
                      </span>
                      <span
                        className="absolute inset-x-0 bottom-0 h-1 origin-left scale-x-50 bg-[linear-gradient(90deg,var(--swatch-purple),var(--swatch-blue),var(--swatch-pink))] transition-transform duration-200 group-hover:scale-x-100 motion-reduce:transition-none"
                        aria-hidden="true"
                      />
                    </a>
                  );
                })}
              </div>
              <p className={cn(BODY, "mt-3 max-w-[70ch]")}>
                Visionary is earned when a complete star history proves a
                contribution happened before a project grew beyond five times
                that star count and crossed 512 stars.
              </p>
            </div>
          )}
        </section>
      )}

      {hasCodeSignals && (
        <section className="mt-16 scroll-mt-24" id="code-signals">
          <div className={SECTION_HEADER}>
            <h2 className={HEADING}>Code signals</h2>
            <p className="font-mono text-[11px] text-muted-foreground">
              {num(stats!.repos_analyzed)} of {num(stats!.repos_tracked)} repos analyzed
            </p>
          </div>
          <p className={cn(BODY, "mt-2 max-w-[70ch]")}>
            Aggregated from cached git history across every repo {login} owns.
            Each chart is embeddable — use its “Add to README”.
          </p>
          <div className="mt-6 grid gap-8">
            {OWNED_CODE_CHARTS.map((chart) => (
              <div key={chart.name}>
                <StatCard
                  src={`${apiBase}/api/users/${login}/stats/${chart.name}.svg`}
                  alt={`${chart.label} for ${login}`}
                  caption={chart.label}
                  apiBase={apiBase}
                  embedLink={canonical}
                />
                <p className={cn(CAPTION, "mt-2.5 px-1 [text-wrap:pretty]")}>
                  {chart.blurb}
                </p>
              </div>
            ))}
          </div>
        </section>
      )}

      {hasCodeSignals && (
        <section className="mt-16 scroll-mt-24" id="maintenance">
          <div className={SECTION_HEADER}>
            <h2 className={HEADING}>Maintenance footprint</h2>
            <p className="font-mono text-[11px] text-muted-foreground">bots excluded</p>
          </div>
          <dl
            className={cn(
              PANEL,
              "mt-6 grid grid-cols-2 divide-border/40 p-3.5 sm:grid-cols-4 sm:divide-x",
            )}
          >
            <div className="min-w-0 px-3.5 py-2">
              <dt className={EYEBROW}>Solo-carried</dt>
              <dd className={cn("mt-2", KPI, "text-foreground")}>
                {num(stats!.solo_maintained)}
              </dd>
              <p className={cn(CAPTION, "mt-2")}>
                {scoredRepos > 0
                  ? `of ${num(scoredRepos)} scored repos, one person holds over half the commits`
                  : "not scored yet"}
              </p>
            </div>
            <div className="min-w-0 px-3.5 py-2">
              <dt className={EYEBROW}>Shared</dt>
              <dd className={cn("mt-2", KPI, "text-foreground")}>
                {num(stats!.shared_maintained)}
              </dd>
              <p className={cn(CAPTION, "mt-2")}>
                more than one author needed to reach half the commits
              </p>
            </div>
            <div className="min-w-0 px-3.5 py-2">
              <dt className={EYEBROW}>Commits (52w)</dt>
              <dd className={cn("mt-2", KPI, "text-foreground")}>
                {commitDaysTotal > 0 ? formatCompact(commitDaysTotal) : "—"}
              </dd>
              <p className={cn(CAPTION, "mt-2")}>
                landed across owned repos in the last 52 weeks
              </p>
            </div>
            <div className="min-w-0 px-3.5 py-2">
              <dt className={EYEBROW}>Analyzed commits</dt>
              <dd className={cn("mt-2", KPI, "text-foreground")}>
                {stats!.analyzed_commits > 0
                  ? formatCompact(stats!.analyzed_commits)
                  : "—"}
              </dd>
              <p className={cn(CAPTION, "mt-2")}>
                total commit history gitdebt has read
              </p>
            </div>
          </dl>

          {activeRepos.length > 0 && (
            <div className="mt-8">
              <h3 className={EYEBROW}>Most active · last 90 days</h3>
              <ul className="mt-3">
                {activeRepos.map((row) => (
                  <li key={row.repo} className="border-b border-border/40 last:border-0">
                    <a href={`/${row.repo}`} className={ROW}>
                      <span className="min-w-0 flex-1 truncate">{row.repo}</span>
                      <span
                        className="hidden h-1.5 shrink-0 rounded-[1px] bg-foreground/30 transition-colors duration-150 group-hover:bg-foreground/60 sm:block"
                        style={{
                          width: `${Math.max(
                            6,
                            Math.round((row.commits_recent / activeMax) * 120),
                          )}px`,
                        }}
                        aria-hidden="true"
                      />
                      <span className={ROW_BADGE}>{num(row.commits_recent)}</span>
                    </a>
                  </li>
                ))}
              </ul>
            </div>
          )}

          {topLanguages.length > 0 && (
            <div className="mt-8">
              <h3 className={EYEBROW}>Languages across owned repos</h3>
              <div className="mt-3 flex flex-wrap gap-2">
                {topLanguages.map((row) => {
                  const total = row.code + row.blank + row.comment;
                  const share =
                    languageTotal > 0 ? Math.round((total / languageTotal) * 100) : 0;
                  return (
                    <span key={row.language} className="dither-chip">
                      {row.language}
                      <span className="text-foreground/70">
                        {share > 0 ? `${share}%` : "—"}
                      </span>
                    </span>
                  );
                })}
              </div>
            </div>
          )}
        </section>
      )}

      {data && (
        <section className="mt-16 scroll-mt-24" id="readme-assets">
          <div className={SECTION_HEADER}>
            <h2 className={HEADING}>Add to your README</h2>
            <p className="font-mono text-[11px] text-muted-foreground">svg · png · webp</p>
          </div>
          <p className={cn(BODY, "mt-2 max-w-[70ch]")}>
            Every profile asset ships light and dark variants behind a{" "}
            <code className="font-mono text-[12px] text-foreground">&lt;picture&gt;</code>{" "}
            element, so they follow the reader's GitHub theme. Copy a snippet and
            paste it into your profile README.
          </p>

          <div className="mt-6 grid gap-8">
            <AssetPanel
              apiBase={apiBase}
              chartPath={`/api/users/${login}/card.svg`}
              label="Profile card"
              altText={`gitdebt profile statistics for ${login}`}
              linkHref={canonical}
            >
              <div className="flex justify-center p-3.5">
                <ProfileCardPreview
                  apiBase={apiBase}
                  login={login}
                  initialRevision={revision}
                  warm={false}
                />
              </div>
            </AssetPanel>

            <AssetPanel
              apiBase={apiBase}
              chartPath={`/api/users/${login}/chart.svg`}
              label="Aggregate star history"
              altText={`Aggregate star history across ${login}'s public repos`}
              linkHref={canonical}
            />

            {PROFILE_CHARTS.map((chart) => (
              <AssetPanel
                key={chart.name}
                apiBase={apiBase}
                chartPath={`/api/users/${login}/stats/${chart.name}.svg`}
                label={chart.label}
                altText={`${chart.label} for ${login}`}
                linkHref={canonical}
              />
            ))}
          </div>
        </section>
      )}

      {data && trackedRepos.length > 0 && (
        <section className="mt-16 scroll-mt-24" id="tracked-repos">
          <div className={SECTION_HEADER}>
            <h2 className={HEADING}>Tracked repos</h2>
            <a href="/compare" className={SECTION_ACTION}>
              compare →
            </a>
          </div>
          <div className="mt-6 flex flex-wrap gap-2">
            {trackedRepos.map((slug) => (
              <a
                key={slug}
                href={`/${slug}`}
                className="dither-chip min-h-9 rounded-md px-2.5 text-[11px] normal-case outline-none transition-colors duration-150 hover:bg-card/60 hover:text-foreground focus-visible:ring-2 focus-visible:ring-accent/30"
              >
                {slug}
              </a>
            ))}
          </div>
          <p className={cn(BODY, "mt-3")}>
            Each repository page carries the full star-history chart plus
            code-health signals — churn, bug magnets, bus factor and more.
          </p>
        </section>
      )}

      <section className="mt-16 scroll-mt-24 border-t border-border/60 pt-8">
        <h2 className={HEADING}>How this report is built</h2>
        <p className={cn(BODY, "mt-3 max-w-[70ch]")}>
          gitdebt sums the cumulative star history of {login}'s top public repos
          (up to 50, by stars) from cached public star timestamps, and derives the
          code signals from cached git history — commits, commit days, language
          line counts and author concentration. Everything on this page is read
          from gitdebt's own database; nothing here queries GitHub while you wait.
          Repos without cached history are fetched in the background and join the
          totals as they complete. Explore more on the{" "}
          <a
            href="/leaderboard"
            className="rounded underline decoration-border underline-offset-4 outline-none transition-colors duration-150 hover:decoration-foreground/60 focus-visible:ring-2 focus-visible:ring-accent/30"
          >
            repo leaderboard
          </a>{" "}
          or{" "}
          <a
            href="/compare"
            className="rounded underline decoration-border underline-offset-4 outline-none transition-colors duration-150 hover:decoration-foreground/60 focus-visible:ring-2 focus-visible:ring-accent/30"
          >
            compare star history
          </a>{" "}
          across repos.
        </p>
      </section>
    </div>
  );
}

/**
 * An embeddable asset always shows its rendered output above the control that
 * copies it: nobody should paste a snippet they have not seen.
 */
function AssetPanel({
  apiBase,
  chartPath,
  label,
  altText,
  linkHref,
  children,
}: {
  apiBase: string;
  chartPath: string;
  label: string;
  altText: string;
  linkHref: string;
  children?: ReactNode;
}) {
  const theme = useRenderedTheme();
  const [failed, setFailed] = useState(false);
  const src = `${apiBase}${chartPath}?theme=${theme}&context=app&animate=1&render=${MEDIA_RENDER_REVISION}`;

  return (
    <figure className={cn(PANEL, "overflow-hidden")}>
      {children ?? (
        failed ? (
          <p className={cn(CAPTION, "px-3.5 py-10 text-center")}>
            This asset is still rendering. It appears here once the analysis
            finishes.
          </p>
        ) : (
          <img
            src={src}
            alt={altText}
            loading="lazy"
            decoding="async"
            onError={() => setFailed(true)}
            className="block w-full"
          />
        )
      )}
      <figcaption className="flex items-center justify-between gap-3 border-t border-border/40 px-3.5 py-3">
        <span className={EYEBROW}>{label}</span>
        <EmbedSnippet
          apiBase={apiBase}
          chartPath={chartPath}
          linkHref={linkHref}
          label={label}
          altText={altText}
          variant="menu"
        />
      </figcaption>
    </figure>
  );
}
