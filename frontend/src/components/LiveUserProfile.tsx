import {
  useEffect,
  useMemo,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { ExternalLink, Loader2 } from "lucide-react";
import { motion, useReducedMotion } from "motion/react";

import { AgentReadmePrompt } from "@/components/AgentReadmePrompt";
import { ButtonLink } from "@/components/ButtonLink";
import { ChartViewer } from "@/components/ChartViewer";
import { DitherAreaChart } from "@/components/DitherAreaChart";
import { EmbedSnippet } from "@/components/EmbedSnippet";
import { ProfileCardPreview } from "@/components/ProfileCardPreview";
import { StatCard } from "@/components/StatCard";
import {
  DitherSurface,
  useDitherSurface,
} from "@/components/ui/dither-surface";
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
import { BRAND, INK } from "@/lib/dither";
import { SPRING } from "@/lib/motion";
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
  repos_total?: number | null;
  repos_cap?: number;
  account_type?: "User" | "Organization" | null;
  list_truncated?: boolean;
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

export type CommitStreakTier = {
  key: string;
  label: string;
  days: number;
  description: string;
  earned: boolean;
};

export type CommitStreak = {
  current_days: number;
  longest_days: number;
  latest_active_date: string | null;
  tiers: CommitStreakTier[];
};

/** Mirrors the backend `GET /api/users/:login/stats.json` contract. */
export type UserStats = {
  login: string;
  ready: boolean;
  repos_tracked: number;
  repos_scanned: number;
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
  commit_streak?: CommitStreak;
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

function AchievementCard({ achievement }: { achievement: VisionaryRepo }) {
  const reducedMotion = useReducedMotion();
  const { surface, handlers } = useDitherSurface({
    fill: INK,
    variant: "gradient",
    edge: 0.76,
    alpha: 0.3,
    pulse: true,
  });
  const early = achievement.stars_at_first_contribution;
  const growth =
    early > 0
      ? `${(achievement.current_stars / early).toFixed(1)}× growth`
      : "before the first recorded star";

  return (
    <motion.a
      href={`/${achievement.repo}`}
      initial="rest"
      whileHover={reducedMotion ? undefined : "hover"}
      whileTap={reducedMotion ? undefined : { scale: 0.992 }}
      variants={{
        rest: { y: 0 },
        hover: { y: -3 },
      }}
      transition={SPRING.snappy}
      className={cn(
        PANEL,
        "dither-fallback group relative isolate overflow-hidden p-4 outline-none focus-visible:ring-2 focus-visible:ring-accent/30",
      )}
      {...handlers}
    >
      {surface}
      <motion.p
        aria-hidden="true"
        variants={{
          rest: { x: 0, opacity: 0.13 },
          hover: { x: -10, opacity: 0.28 },
        }}
        transition={SPRING.snappy}
        className="pointer-events-none absolute top-3 right-3 max-w-32 text-right font-mono text-[0.625rem] tracking-[0.18em] text-foreground"
      >
        56 49 53 49
        <br />
        4F 4E 41 52 59
      </motion.p>
      <div className="relative pr-20">
        <p className="font-mono text-[0.625rem] font-semibold tracking-[0.16em] text-[var(--swatch-purple)] uppercase">
          [*] Visionary // early signal
        </p>
        <p className="mt-2 truncate font-mono text-[0.8125rem] text-foreground">
          {achievement.repo}
        </p>
        <p className={cn(CAPTION, "mt-2")}>
          Contributed at {num(early)} stars · now{" "}
          {num(achievement.current_stars)} · {growth}
        </p>
      </div>
    </motion.a>
  );
}

function StreakAchievementCard({
  tier,
  longestDays,
}: {
  tier: CommitStreakTier;
  longestDays: number;
}) {
  const reducedMotion = useReducedMotion();
  const { surface, handlers } = useDitherSurface({
    fill: BRAND,
    variant: "hatched",
    edge: 0.7,
    alpha: 0.3,
    pulse: true,
  });

  return (
    <motion.a
      href="#code-signals"
      initial="rest"
      whileHover={reducedMotion ? undefined : "hover"}
      whileTap={reducedMotion ? undefined : { scale: 0.992 }}
      variants={{
        rest: { y: 0 },
        hover: { y: -3 },
      }}
      transition={SPRING.snappy}
      className={cn(
        PANEL,
        "dither-fallback group relative isolate overflow-hidden p-4 outline-none focus-visible:ring-2 focus-visible:ring-accent/30",
      )}
      {...handlers}
    >
      {surface}
      <motion.p
        aria-hidden="true"
        variants={{
          rest: { x: 0, opacity: 0.15 },
          hover: { x: -8, opacity: 0.32 },
        }}
        transition={SPRING.snappy}
        className="pointer-events-none absolute top-3 right-3 font-mono text-[0.625rem] tracking-[0.16em] text-foreground"
      >
        {tier.days.toString(16).toUpperCase().padStart(3, "0")}D
        <br />
        + + + +
      </motion.p>
      <div className="relative pr-16">
        <p className="font-mono text-[0.625rem] font-semibold tracking-[0.16em] text-[var(--swatch-blue)] uppercase">
          [+] Streak // {tier.days} days
        </p>
        <p className="mt-2 font-mono text-[0.8125rem] text-foreground">
          {tier.label}
        </p>
        <p className={cn(CAPTION, "mt-2")}>
          {tier.description} Personal best: {num(longestDays)} days.
        </p>
      </div>
    </motion.a>
  );
}

function LockedStreakCard({
  tier,
  currentDays,
  longestDays,
}: {
  tier: CommitStreakTier;
  currentDays: number;
  longestDays: number;
}) {
  const reducedMotion = useReducedMotion();
  const progress = Math.min(100, Math.round((currentDays / tier.days) * 100));
  const remaining = Math.max(0, tier.days - currentDays);
  const nextAction =
    currentDays > 0
      ? `Keep the run alive for ${num(remaining)} more consecutive ${remaining === 1 ? "day" : "days"}.`
      : "Land activity in a tracked project today to begin a new run.";

  return (
    <motion.div
      initial={reducedMotion ? false : { opacity: 0, y: 6 }}
      whileInView={reducedMotion ? undefined : { opacity: 1, y: 0 }}
      viewport={{ once: true, amount: 0.35 }}
      transition={SPRING.snappy}
      className={cn(
        PANEL,
        "dither-fallback relative isolate overflow-hidden p-4",
      )}
    >
      <DitherSurface
        fill={INK}
        variant="hatched"
        edge={0.34}
        alpha={0.12}
      />
      <div className="relative">
        <div className="flex items-start justify-between gap-4">
          <div className="min-w-0">
            <p className="font-mono text-[0.625rem] font-semibold tracking-[0.16em] text-muted-foreground uppercase">
              [ ] Locked // {tier.days}D
            </p>
            <p className="mt-2 font-mono text-[0.8125rem] text-foreground">
              {tier.label}
            </p>
          </div>
          <p className="shrink-0 font-mono text-[0.75rem] tabular-nums text-muted-foreground">
            {num(currentDays)} / {num(tier.days)}
          </p>
        </div>
        <div
          className="mt-4 h-2 overflow-hidden rounded-[1px] border border-border/50 bg-background/70"
          aria-label={`${progress}% progress toward ${tier.label}`}
          aria-valuemax={tier.days}
          aria-valuemin={0}
          aria-valuenow={currentDays}
          role="progressbar"
        >
          <div
            className="relative isolate h-full w-(--streak-progress) overflow-hidden"
            style={
              {
                "--streak-progress": `${progress}%`,
              } as CSSProperties
            }
          >
            <DitherSurface
              fill={BRAND}
              variant="gradient"
              edge={null}
              alpha={0.72}
            />
          </div>
        </div>
        <p className={cn(CAPTION, "mt-3")}>
          {nextAction} Historical best: {num(longestDays)} days.
        </p>
      </div>
    </motion.div>
  );
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
    // Resolve the viewer even on a public `/{login}` page. The known route
    // login still wins as the report target; this identity is used only to
    // reveal that account's private achievement roadmap.
    if (!session && !knownLogin) return;
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
    let events: EventSource | null = null;
    let fetching = false;
    let warmAttempted = false;

    const clearTimer = () => {
      if (timer) clearTimeout(timer);
      timer = null;
    };

    const closeEvents = () => {
      events?.close();
      events = null;
    };

    const scheduleFallback = (target: string) => {
      if (timer || cancelled) return;
      timer = setTimeout(() => {
        timer = null;
        void load(target);
      }, POLL_MS);
    };

    const connectProgress = (target: string) => {
      if (events || cancelled) return;
      events = new EventSource(
        `${apiBase}/api/users/${encodeURIComponent(target)}/progress`,
      );
      events.addEventListener("open", clearTimer);
      events.addEventListener("progress", (event) => {
        try {
          const update = JSON.parse((event as MessageEvent<string>).data) as {
            terminal?: boolean;
          };
          if (update.terminal) closeEvents();
        } catch {
          // A malformed event should not prevent the authoritative refetch.
        }
        void load(target);
      });
      for (const eventName of ["timeout", "unavailable"]) {
        events.addEventListener(eventName, () => {
          closeEvents();
          scheduleFallback(target);
        });
      }
      events.addEventListener("error", () => {
        closeEvents();
        scheduleFallback(target);
      });
    };

    async function load(target: string) {
      if (fetching || cancelled) return;
      fetching = true;
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
          repos_total: payload.repos_total ?? null,
          repos_cap: payload.repos_cap ?? 50,
          account_type: payload.account_type ?? null,
          list_truncated: payload.list_truncated ?? false,
          total_stars: payload.total_stars ?? 0,
          history: payload.history ?? [],
        };
        setData(next);
        setError(null);
        if (next.repos_pending > 0 || (next.repos_analyzing ?? 0) > 0) {
          connectProgress(target);
        } else {
          clearTimer();
          closeEvents();
        }
      } catch (reason) {
        if (cancelled) return;
        setError(
          reason instanceof Error
            ? reason.message
            : "Profile data is temporarily unavailable.",
        );
        scheduleFallback(target);
      } finally {
        fetching = false;
        if (!cancelled) setLoading(false);
      }
    }

    void load(login);
    return () => {
      cancelled = true;
      clearTimer();
      closeEvents();
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
  const reposTotal = data?.repos_total ?? null;
  const reposCap = data?.repos_cap ?? 50;
  const cappedAccount = reposTotal !== null && reposTotal > reposCap;
  const accountNoun =
    data?.account_type === "Organization" ? "organization" : "account";
  const githubReposHref =
    data?.account_type === "Organization"
      ? `https://github.com/orgs/${login}/repositories`
      : `https://github.com/${login}?tab=repositories`;
  const aggregateCoverage = cappedAccount
    ? `${num(data?.repos_included)} complete histories · top ${num(reposCap)} of ${num(reposTotal)} public repos`
    : `${num(data?.repos_included)} public ${data?.repos_included === 1 ? "repo" : "repos"}`;
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
  const commitStreak = stats?.commit_streak;
  const streakTiers = commitStreak?.tiers ?? [];
  const earnedStreakTiers = streakTiers.filter((tier) => tier.earned);
  const lockedStreakTiers = streakTiers.filter((tier) => !tier.earned);
  const isOwnProfile = sessionLogin?.toLowerCase() === login.toLowerCase();
  const hasEarnedAchievements =
    visionaryRepos.length > 0 || earnedStreakTiers.length > 0;

  const kpis = [
    {
      label: "Stars",
      value: hasData ? formatCompact(data!.total_stars) : "—",
      note: cappedAccount
        ? `top-${num(reposCap)} analysis slice · ${num(data?.repos_included)} ready`
        : `across ${num(data?.repos_included ?? null)} tracked repos`,
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
                  {cappedAccount ? (
                    <>
                      , measured across {data!.repos_included} completed
                      histories in gitdebt's top-{reposCap} slice. GitHub
                      reports {reposTotal!.toLocaleString()} public
                      repositories for this {accountNoun}.
                    </>
                  ) : (
                    <>
                      , across {data!.repos_included} tracked{" "}
                      {data!.repos_included === 1 ? "repo" : "repos"}.
                    </>
                  )}
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
            pulse
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
                ? `Reading code history for ${data!.repos_analyzing} repositories. `
                : "Discovering public repositories. "}
              Live backend events update this page as each job finishes.
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
              summed across {aggregateCoverage}
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
            <a
              href={
                cappedAccount
                  ? githubReposHref
                  : "/leaderboard"
              }
              target={cappedAccount ? "_blank" : undefined}
              rel={cappedAccount ? "noopener noreferrer" : undefined}
              className={SECTION_ACTION}
            >
              {cappedAccount
                ? `all ${reposTotal!.toLocaleString()} on GitHub ↗`
                : "leaderboard →"}
            </a>
          </div>
          {cappedAccount && (
            <p className={cn(BODY, "mt-2 max-w-[72ch]")}>
              Showing the strongest repositories in gitdebt's bounded top-
              {reposCap} analysis slice. This is not the {accountNoun}'s full
              repository list, and the aggregate above does not claim to be
              account-wide.
            </p>
          )}
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
                    ) : null}
                  </a>
                </li>
              );
            })}
          </ul>
          <p className={cn(BODY, "mt-3")}>
            Sparklines use complete cumulative monthly history only. Current
            GitHub totals remain visible while a full series is being collected.
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

        </section>
      )}

      {hasEarnedAchievements && (
        <section className="mt-16 scroll-mt-24" id="achievements">
          <div className={SECTION_HEADER}>
            <h2 className={HEADING}>Earned achievements</h2>
            <p className="font-mono text-[11px] text-muted-foreground">
              proven by cached history
            </p>
          </div>
          <div className="mt-6 grid gap-3 sm:grid-cols-2">
            {earnedStreakTiers.map((tier) => (
              <StreakAchievementCard
                key={tier.key}
                tier={tier}
                longestDays={commitStreak?.longest_days ?? 0}
              />
            ))}
            {visionaryRepos.map((achievement) => (
              <AchievementCard
                key={achievement.repo}
                achievement={achievement}
              />
            ))}
          </div>
          <p className={cn(BODY, "mt-3 max-w-[70ch]")}>
            Streak awards use consecutive calendar days authored by this
            resolved GitHub login across analyzed public repositories.
            Visionary uses complete star history to prove a contribution landed
            before a project grew beyond five times that star count and crossed
            512 stars.
          </p>
        </section>
      )}

      {isOwnProfile && lockedStreakTiers.length > 0 && (
        <section className="mt-16 scroll-mt-24" id="locked-achievements">
          <div className={SECTION_HEADER}>
            <h2 className={HEADING}>Locked achievements</h2>
            <p className="font-mono text-[11px] text-muted-foreground">
              only visible to you
            </p>
          </div>
          <p className={cn(BODY, "mt-2 max-w-[70ch]")}>
            Keep contributing on consecutive calendar days to unlock more
            profile decorations. Progress comes from cached activity in your
            analyzed public repositories; no contributor profiles are stored.
          </p>
          <div className="@container mt-6">
            <div className="grid gap-3 @xl:grid-cols-2">
              {lockedStreakTiers.map((tier) => (
                <LockedStreakCard
                  key={tier.key}
                  tier={tier}
                  currentDays={commitStreak?.current_days ?? 0}
                  longestDays={commitStreak?.longest_days ?? 0}
                />
              ))}
            </div>
          </div>
        </section>
      )}

      {hasCodeSignals && (
        <section className="mt-16 scroll-mt-24" id="code-signals">
          <div className={SECTION_HEADER}>
            <h2 className={HEADING}>Code signals</h2>
            <p className="font-mono text-[11px] text-muted-foreground">
              {num(stats!.repos_analyzed)} analyzed
              {cappedAccount
                ? ` · top ${num(stats!.repos_scanned)} of ${num(reposTotal)}`
                : ` of ${num(stats!.repos_tracked)} repos`}
            </p>
          </div>
          <p className={cn(BODY, "mt-2 max-w-[70ch]")}>
            {cappedAccount
              ? `Aggregated from cached git history within the bounded ${stats!.repos_scanned}-repository code-analysis slice.`
              : `Aggregated from cached git history across the public repositories ${login} owns.`}{" "}
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
              "mt-6 grid grid-cols-2 divide-border/40 p-3.5 lg:grid-cols-5 lg:divide-x",
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
              <dt className={EYEBROW}>Active streak</dt>
              <dd className={cn("mt-2", KPI, "text-foreground")}>
                {commitStreak
                  ? `${num(commitStreak.current_days)}d`
                  : "—"}
              </dd>
              <p className={cn(CAPTION, "mt-2")}>
                {commitStreak
                  ? `${num(commitStreak.longest_days)} day best across resolved public contributions`
                  : "not scored yet"}
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
            <p className="font-mono text-[11px] text-muted-foreground">
              svg · gif · png · webp
            </p>
          </div>
          <p className={cn(BODY, "mt-2 max-w-[70ch]")}>
            Every profile asset defaults to a static SVG frame — motion is
            yours to turn on, and it plays in a GitHub README. GIF is for the
            surfaces that show an SVG as a single frame, and PNG/WebP are
            static raster. Each snippet ships light and dark behind a{" "}
            <code className="font-mono text-[12px] text-foreground">&lt;picture&gt;</code>{" "}
            element, so they follow the reader's GitHub theme. Copy a snippet and
            paste it into your profile README.
          </p>

          <div className="mt-6">
            <AgentReadmePrompt
              apiBase={apiBase}
              siteOrigin={siteOrigin}
              target={{
                kind: "profile",
                login,
                totalStars: data.total_stars,
                reposIncluded: data.repos_included,
              }}
            />
          </div>

          <div className="mt-10 grid gap-8">
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
            >
              <DitherAreaChart
                points={data.history.map((point) => ({
                  date: point.date,
                  value: point.stars,
                }))}
                height={360}
                valueLabel="stars"
                seed={`user:${login}`}
                className="rounded-t-[inherit]"
              />
            </AssetPanel>

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
            code-health signals — file change frequency, fix-labelled changes,
            bus factor and more.
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
