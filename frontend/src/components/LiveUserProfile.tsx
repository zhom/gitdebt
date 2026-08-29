import { useEffect, useMemo, useState, type ReactNode } from "react";

import { AgentReadmePrompt } from "@/components/AgentReadmePrompt";
import { ButtonLink } from "@/components/ButtonLink";
import { ChartViewer } from "@/components/ChartViewer";
import { EmbedSnippet } from "@/components/EmbedSnippet";
import { ProfileCardPreview } from "@/components/ProfileCardPreview";
import { StatCard } from "@/components/StatCard";
import {
  BODY,
  CAPTION,
  DATUM,
  FIELD,
  FIGURE,
  HEADING,
  LEAD,
  MEASURE,
  ROW,
  SECTION_ACTION,
  SECTION_HEADER,
  TITLE,
} from "@/components/style-tokens";
import { publishLiveSubject } from "@/lib/live-subject";
import { restoreServedTitle } from "@/lib/live-title";
import {
  firstStarYear,
  formatCompact,
  gainedInTrailingDays,
  growthTrend,
} from "@/lib/star-insights";
import { profileLogin } from "@/lib/static-routing.mjs";
import { cn } from "@/lib/utils";

/**
 * A maintainer's sheet.
 *
 * The subject is one login, and the drawing is the sum of what they have
 * published: one star trace, then the readings taken from their commit
 * history. Every figure on it is lettered as a field and a value, every list is
 * a real list, and every chart carries its own numbers as text.
 *
 * This island also owns one half of the tab-title defect. It mounts on two very
 * different routes: the prerendered `/{login}` page, whose title the build
 * already wrote for this exact subject, and `/404`, where `github.com/<name>`
 * was rewritten here and the served title still says "Page not found" over a
 * complete, correct report. `publishLiveSubject` tells those apart by the
 * served canonical, and it is called only once the API has answered for the
 * login — the aggregate endpoint 404s for an account GitHub does not have, so a
 * 200 is GitHub confirming the subject is real.
 */

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
  /** Known login. When absent it is read from the URL. */
  login?: string;
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
 * Deterministic sparkline path over a cumulative series, or null when there is
 * not enough history to draw an honest shape.
 *
 * The geometry is computed here rather than measured from the DOM, so the
 * server and the client emit identical bytes and the mark is on screen at first
 * paint instead of after hydration.
 */
function sparkPath(values: number[], w = 132, h = 28): string | null {
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

/* ── Notation ────────────────────────────────────────────────────────────── */

/**
 * One reading: the field's name, its value, and what the value means.
 *
 * The label and the value are each held to a single line, so every cell in a
 * row of these lands on the same two baselines however long its note runs. The
 * note is last precisely because it is the only variable-length part, and a
 * variable-length string at the end of a cell cannot push a neighbour's figure
 * out of step.
 */
function Reading({
  label,
  value,
  note,
}: {
  label: string;
  value: string;
  note: string;
}) {
  return (
    <div className="min-w-0 bg-paper p-4">
      <dt className={cn(FIELD, "truncate")}>{label}</dt>
      <dd>
        <p className={cn(FIGURE, "mt-2.5 truncate text-ink")}>{value}</p>
        <p className={cn(CAPTION, "mt-2")}>{note}</p>
      </dd>
    </div>
  );
}

/**
 * A row of readings on one drawn grid.
 *
 * The hairlines between cells are the grid's own gap showing the rule colour
 * through from behind, not `divide-*` borders: `divide-y` walks children in
 * flow order, so in a two-column grid it draws a line above the second cell of
 * the FIRST row — a rule that separates nothing, which is exactly the mark this
 * drawing does not allow. A one-pixel gap is right at every breakpoint without
 * being told how many columns there are.
 */
function ReadingGrid({
  columns,
  children,
}: {
  columns: 4 | 5 | 6;
  children: ReactNode;
}) {
  return (
    <dl
      className={cn(
        "grid grid-cols-2 gap-px border border-rule-strong bg-rule",
        columns === 4 && "sm:grid-cols-4",
        columns === 5 && "lg:grid-cols-5",
        columns === 6 && "sm:grid-cols-3 lg:grid-cols-6",
      )}
    >
      {children}
    </dl>
  );
}

/** A section head: the heading and its one note or action share a baseline. */
function SectionHead({
  id,
  title,
  note,
  action,
}: {
  id: string;
  title: string;
  note?: string;
  action?: { href: string; label: string; external?: boolean };
}) {
  return (
    <div className={SECTION_HEADER}>
      <h2 id={id} className={HEADING}>
        {title}
      </h2>
      {action ? (
        <a
          href={action.href}
          className={SECTION_ACTION}
          {...(action.external
            ? { target: "_blank", rel: "noopener noreferrer" }
            : {})}
        >
          {action.label}
        </a>
      ) : note ? (
        <p className={CAPTION}>{note}</p>
      ) : null}
    </div>
  );
}

/* ── The component ───────────────────────────────────────────────────────── */

export function LiveUserProfile({
  apiBase,
  login: requestedLogin,
  siteOrigin = "https://gitdebt.com",
  analyze: seedAnalyze = null,
  stats: seedStats = null,
  repos: seedRepos = [],
}: Props) {
  const login = useMemo(
    () => profileLogin(requestedLogin ?? "") ?? urlLogin(),
    [requestedLogin],
  );
  const [sessionLogin, setSessionLogin] = useState<string | null>(null);

  const seededAnalyze =
    seedAnalyze && seedAnalyze.login.toLowerCase() === login ? seedAnalyze : null;
  const seededStats =
    seedStats && seedStats.login.toLowerCase() === login ? seedStats : null;

  const [data, setData] = useState<UserAnalyze | null>(seededAnalyze);
  const [stats, setStats] = useState<UserStats | null>(seededStats);
  const [loading, setLoading] = useState(Boolean(login) && !seededAnalyze);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // Resolve the viewer even on a public `/{login}` page. The route's login
    // still wins as the report target; this identity is used only to reveal
    // that account's own achievement roadmap.
    if (!login) return;
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
      .catch(() => undefined);
    return () => controller.abort();
  }, [apiBase, login]);

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

        // The subject is real now, so the tab may name it. This is the only
        // place that is true: the endpoint answers 404 for a login GitHub does
        // not have, so reaching here means GitHub confirmed the account.
        publishLiveSubject({
          subject: next.login,
          description:
            next.repos_included > 0
              ? `${next.login}: ${next.total_stars.toLocaleString()} GitHub stars across ${next.repos_included} public repositories, with commit activity, language footprint and README-ready charts.`
              : `Aggregate GitHub star history for ${next.login}'s public repositories, with commit activity and README-ready charts.`,
          path: `/${next.login}`,
          image: `${apiBase}/api/users/${next.login}/og.png`,
        });

        if (next.repos_pending > 0 || (next.repos_analyzing ?? 0) > 0) {
          connectProgress(target);
        } else {
          clearTimer();
          closeEvents();
        }
      } catch (reason) {
        if (cancelled) return;
        const message =
          reason instanceof Error
            ? reason.message
            : "Profile data is temporarily unavailable.";
        setError(message);
        // A login that does not exist must never leave a corrected title in the
        // tab: put back whatever the server sent for this document.
        if (message === "GitHub user not found.") restoreServedTitle();
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
    data !== null && data.repos_pending === 0 && (data.repos_analyzing ?? 0) === 0;

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
        <h1 className={TITLE}>No maintainer named</h1>
        <p className={cn(LEAD, MEASURE, "mt-4")}>
          Open any maintainer at gitdebt.com followed by their GitHub login, or
          sign in from the header to open your own.
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
    contributionTotal > 0 ? externalAuthoredCommits / contributionTotal : 0;
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

  const readings = [
    {
      label: "Stars",
      value: hasData ? formatCompact(data!.total_stars) : "—",
      note: cappedAccount
        ? `top-${num(reposCap)} slice · ${num(data?.repos_included)} complete`
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
      note: trend ? `${trend} against the lifetime pace` : "new stars",
    },
    {
      label: "Commits",
      value:
        stats && stats.authored_commits > 0
          ? formatCompact(stats.authored_commits)
          : "—",
      note:
        stats && stats.contributed_repos > 0
          ? `authored across ${num(stats.contributed_repos)} repos`
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
        <div className="flex flex-col gap-6 sm:flex-row sm:items-end sm:justify-between">
          <div className="min-w-0">
            <h1 className={TITLE}>{login}</h1>
            <p className={cn(LEAD, MEASURE, "mt-4")}>
              {hasData ? (
                <>
                  {login}'s public repositories have earned{" "}
                  <span className="measured">
                    {data!.total_stars.toLocaleString()}
                  </span>{" "}
                  GitHub stars{firstYear ? ` since ${firstYear}` : ""}
                  {gained90 !== null && gained90 > 0
                    ? `, ${gained90.toLocaleString()} of them in the last 90 days`
                    : ""}
                  {cappedAccount ? (
                    <>
                      , measured across {data!.repos_included} complete histories
                      in gitdebt's top-{reposCap} slice. GitHub reports{" "}
                      {reposTotal!.toLocaleString()} public repositories for this{" "}
                      {accountNoun}.
                    </>
                  ) : (
                    <>
                      , across {data!.repos_included} tracked{" "}
                      {data!.repos_included === 1 ? "repository" : "repositories"}
                      .
                    </>
                  )}
                </>
              ) : (
                <>
                  Star history for {login}'s public repositories is being read.
                  Totals appear here as each repository's history completes.
                </>
              )}
            </p>
          </div>
          <ButtonLink
            href={`https://github.com/${login}`}
            target="_blank"
            rel="noopener noreferrer"
            variant="quiet"
            leader
            className="group shrink-0 self-start sm:self-auto"
          >
            Open on GitHub
          </ButtonLink>
        </div>

        {/* A note on the sheet, in the drawing's own margin: a leader rule that
            terminates on the paragraph it points at. Not a spinner, and not a
            pulsing dot. */}
        {pending && (
          <p
            role="status"
            className={cn(CAPTION, MEASURE, "mt-8 border-l-2 border-signal pl-4")}
          >
            {(data?.repos_analyzing ?? 0) > 0
              ? `Reading code history for ${data!.repos_analyzing} repositories. `
              : "Discovering public repositories. "}
            This sheet fills in as each job finishes; nothing here waits on you.
          </p>
        )}

        {error && (
          <p role="alert" className={cn(CAPTION, MEASURE, "mt-8 text-signal")}>
            {error}
          </p>
        )}
      </header>

      {data && (
        <section className="mt-12" aria-label="Profile summary">
          <ReadingGrid columns={6}>
            {readings.map((reading) => (
              <Reading key={reading.label} {...reading} />
            ))}
          </ReadingGrid>
        </section>
      )}

      {data && (
        <section
          className="mt-16 scroll-mt-24"
          id="star-history"
          aria-labelledby="star-history-title"
        >
          <SectionHead
            id="star-history-title"
            title="Star history"
            note={
              cappedAccount
                ? `Summed across ${num(data.repos_included)} complete histories`
                : `Summed across ${num(data.repos_included)} public ${
                    data.repos_included === 1 ? "repository" : "repositories"
                  }`
            }
          />
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
        <section
          className="mt-16 scroll-mt-24"
          id="top-repositories"
          aria-labelledby="top-repositories-title"
        >
          <SectionHead
            id="top-repositories-title"
            title="Top repositories"
            action={
              cappedAccount
                ? {
                    href: githubReposHref,
                    label: `all ${reposTotal!.toLocaleString()} on GitHub`,
                    external: true,
                  }
                : { href: "/leaderboard", label: "leaderboard" }
            }
          />
          {cappedAccount && (
            <p className={cn(BODY, MEASURE, "mt-3")}>
              These are the strongest repositories inside gitdebt's bounded top-
              {reposCap} slice. It is not the {accountNoun}'s full repository
              list, and the aggregate above does not claim to be account-wide.
            </p>
          )}
          <ul
            role="list"
            className="mt-6 divide-y divide-rule border border-rule-strong bg-paper"
          >
            {topRepos.map((row) => {
              const path = sparkPath(row.spark);
              return (
                <li key={row.repo}>
                  <a href={`/${row.repo}`} className={cn(ROW, "min-h-16 gap-5")}>
                    <span className="min-w-0 flex-1">
                      <span className={cn(DATUM, "block truncate")}>
                        {row.repo}
                      </span>
                      <span className={cn(CAPTION, "mt-1 block")}>
                        {num(row.stars)} stars
                        {row.forks > 0 ? ` · ${num(row.forks)} forks` : ""}
                        {row.commits > 0 ? ` · ${num(row.commits)} commits` : ""}
                      </span>
                    </span>
                    {path ? (
                      <svg
                        className="hidden h-7 w-[132px] shrink-0 text-ink-3 transition-colors duration-[--duration-ui] group-hover:text-ink sm:block"
                        viewBox="0 0 132 28"
                        fill="none"
                        aria-hidden="true"
                        preserveAspectRatio="none"
                      >
                        <path
                          d={path}
                          stroke="currentColor"
                          strokeWidth="1.25"
                          strokeLinejoin="round"
                          strokeLinecap="round"
                          vectorEffect="non-scaling-stroke"
                        />
                      </svg>
                    ) : null}
                  </a>
                </li>
              );
            })}
          </ul>
          <p className={cn(CAPTION, MEASURE, "mt-3")}>
            Each trace uses complete cumulative history only. A repository still
            being read shows its current GitHub totals without one.
          </p>
        </section>
      )}

      {hasContributionSignals && (
        <section
          className="mt-16 scroll-mt-24"
          id="contribution-footprint"
          aria-labelledby="contribution-footprint-title"
        >
          <SectionHead
            id="contribution-footprint-title"
            title="Contribution story"
            note="Attributed public commits"
          />
          <p className={cn(BODY, MEASURE, "mt-3")}>{contributionStory}</p>

          <div className="mt-6">
            <ReadingGrid columns={4}>
              <Reading
                label="Owned repos"
                value={num(ownedContributedRepos)}
                note={`${formatCompact(ownedAuthoredCommits)} authored commits`}
              />
              <Reading
                label="Outside repos"
                value={num(externalContributedRepos)}
                note={`${formatCompact(externalAuthoredCommits)} authored commits`}
              />
              <Reading
                label="Outside share"
                value={`${Math.round(externalShare * 100)}%`}
                note="of attributed commit volume"
              />
              <Reading
                label="Visionary"
                value={num(visionaryRepos.length)}
                note="breakout projects spotted early"
              />
            </ReadingGrid>
          </div>

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
        <section
          className="mt-16 scroll-mt-24"
          id="achievements"
          aria-labelledby="achievements-title"
        >
          <SectionHead
            id="achievements-title"
            title="Earned achievements"
            note="Proven by cached history"
          />
          <ul
            role="list"
            className="mt-6 grid gap-px border border-rule-strong bg-rule sm:grid-cols-2"
          >
            {earnedStreakTiers.map((tier) => (
              <li key={tier.key} className="bg-paper">
                <a
                  href="#maintenance"
                  className={cn(ROW, "min-h-28 items-start p-4")}
                >
                  <span className="min-w-0">
                    <span className={cn(FIELD, "block")}>
                      Streak · {tier.days} days
                    </span>
                    <span className={cn(DATUM, "mt-2.5 block text-ink")}>
                      {tier.label}
                    </span>
                    <span className={cn(CAPTION, "mt-2 block")}>
                      {tier.description} Personal best:{" "}
                      {num(commitStreak?.longest_days ?? 0)} days.
                    </span>
                  </span>
                </a>
              </li>
            ))}
            {visionaryRepos.map((achievement) => {
              const early = achievement.stars_at_first_contribution;
              const growth =
                early > 0
                  ? `${(achievement.current_stars / early).toFixed(1)}× since`
                  : "before the first recorded star";
              return (
                <li key={achievement.repo} className="bg-paper">
                  <a
                    href={`/${achievement.repo}`}
                    className={cn(ROW, "min-h-28 items-start p-4")}
                  >
                    <span className="min-w-0">
                      <span className={cn(FIELD, "block")}>
                        Visionary · early signal
                      </span>
                      <span
                        className={cn(DATUM, "mt-2.5 block truncate text-ink")}
                      >
                        {achievement.repo}
                      </span>
                      <span className={cn(CAPTION, "mt-2 block")}>
                        Contributed at {num(early)} stars · now{" "}
                        {num(achievement.current_stars)} · {growth}
                      </span>
                    </span>
                  </a>
                </li>
              );
            })}
          </ul>
          <p className={cn(CAPTION, MEASURE, "mt-3")}>
            Streaks count consecutive calendar days authored by this resolved
            GitHub login across analyzed public repositories. Visionary uses
            complete star history to prove a contribution landed before a project
            grew past five times that star count and crossed 512 stars.
          </p>
        </section>
      )}

      {isOwnProfile && lockedStreakTiers.length > 0 && (
        <section
          className="mt-16 scroll-mt-24"
          id="locked-achievements"
          aria-labelledby="locked-achievements-title"
        >
          <SectionHead
            id="locked-achievements-title"
            title="Still to unlock"
            note="Only visible to you"
          />
          <p className={cn(BODY, MEASURE, "mt-3")}>
            Progress comes from cached activity in your analyzed public
            repositories. No contributor profiles are stored to compute it.
          </p>
          <ul
            role="list"
            className="mt-6 grid gap-px border border-rule-strong bg-rule sm:grid-cols-2"
          >
            {lockedStreakTiers.map((tier) => (
              <LockedTier
                key={tier.key}
                tier={tier}
                currentDays={commitStreak?.current_days ?? 0}
                longestDays={commitStreak?.longest_days ?? 0}
              />
            ))}
          </ul>
        </section>
      )}

      {hasCodeSignals && (
        <section
          className="mt-16 scroll-mt-24"
          id="code-signals"
          aria-labelledby="code-signals-title"
        >
          <SectionHead
            id="code-signals-title"
            title="Code signals"
            note={
              cappedAccount
                ? `${num(stats!.repos_analyzed)} analyzed · top ${num(stats!.repos_scanned)} of ${num(reposTotal)}`
                : `${num(stats!.repos_analyzed)} analyzed of ${num(stats!.repos_tracked)} repos`
            }
          />
          <p className={cn(BODY, MEASURE, "mt-3")}>
            {cappedAccount
              ? `Aggregated from cached git history inside the bounded ${stats!.repos_scanned}-repository slice.`
              : `Aggregated from cached git history across the public repositories ${login} owns.`}{" "}
            Every drawing below is embeddable.
          </p>
          <div className="mt-6 grid gap-10">
            {OWNED_CODE_CHARTS.map((chart) => (
              <div key={chart.name}>
                <StatCard
                  src={`${apiBase}/api/users/${login}/stats/${chart.name}.svg`}
                  alt={`${chart.label} for ${login}`}
                  caption={chart.label}
                  apiBase={apiBase}
                  embedLink={canonical}
                />
                <p className={cn(CAPTION, MEASURE, "mt-2.5")}>{chart.blurb}</p>
              </div>
            ))}
          </div>
        </section>
      )}

      {hasCodeSignals && (
        <section
          className="mt-16 scroll-mt-24"
          id="maintenance"
          aria-labelledby="maintenance-title"
        >
          <SectionHead
            id="maintenance-title"
            title="Maintenance footprint"
            note="Bot authors excluded"
          />
          <div className="mt-6">
            <ReadingGrid columns={5}>
              <Reading
                label="Solo-carried"
                value={num(stats!.solo_maintained)}
                note={
                  scoredRepos > 0
                    ? `of ${num(scoredRepos)} scored repos, one person holds over half the commits`
                    : "not scored yet"
                }
              />
              <Reading
                label="Shared"
                value={num(stats!.shared_maintained)}
                note="more than one author needed to reach half the commits"
              />
              <Reading
                label="Commits 52w"
                value={commitDaysTotal > 0 ? formatCompact(commitDaysTotal) : "—"}
                note="landed across owned repos in the last 52 weeks"
              />
              <Reading
                label="Active streak"
                value={commitStreak ? `${num(commitStreak.current_days)}d` : "—"}
                note={
                  commitStreak
                    ? `${num(commitStreak.longest_days)} day best across resolved public contributions`
                    : "not scored yet"
                }
              />
              <Reading
                label="Read commits"
                value={
                  stats!.analyzed_commits > 0
                    ? formatCompact(stats!.analyzed_commits)
                    : "—"
                }
                note="total commit history gitdebt has read"
              />
            </ReadingGrid>
          </div>

          {activeRepos.length > 0 && (
            <div className="mt-10">
              <h3 className={FIELD}>Most active · last 90 days</h3>
              <ul
                role="list"
                className="mt-3 divide-y divide-rule border border-rule-strong bg-paper"
              >
                {activeRepos.map((row) => (
                  <li key={row.repo}>
                    <a href={`/${row.repo}`} className={cn(ROW, "min-h-12")}>
                      <span className={cn(DATUM, "min-w-0 flex-1 truncate")}>
                        {row.repo}
                      </span>
                      {/* A bar measuring this repository's recent commits
                          against the busiest one in the list. It never carries
                          the value alone: the figure is beside it. */}
                      <span
                        className="hidden h-[2px] shrink-0 bg-rule-strong transition-colors duration-[--duration-ui] group-hover:bg-ink sm:block"
                        style={{
                          width: `${Math.max(
                            6,
                            Math.round((row.commits_recent / activeMax) * 120),
                          )}px`,
                        }}
                        aria-hidden="true"
                      />
                      <span className="shrink-0 font-mono text-[0.75rem] tabular-nums text-ink-2">
                        {num(row.commits_recent)}
                      </span>
                    </a>
                  </li>
                ))}
              </ul>
            </div>
          )}

          {topLanguages.length > 0 && (
            <div className="mt-10">
              <h3 className={FIELD}>Languages across owned repos</h3>
              <dl className="mt-3 divide-y divide-rule border border-rule-strong bg-paper">
                {topLanguages.map((row) => {
                  const total = row.code + row.blank + row.comment;
                  const share =
                    languageTotal > 0
                      ? Math.round((total / languageTotal) * 100)
                      : 0;
                  return (
                    <div
                      key={row.language}
                      className="flex min-h-11 items-center justify-between gap-4 px-3"
                    >
                      <dt className="min-w-0 truncate text-[0.8125rem] text-ink-2">
                        {row.language}
                      </dt>
                      <dd className="shrink-0 font-mono text-[0.75rem] tabular-nums text-ink">
                        {share > 0 ? `${share}%` : "—"}
                      </dd>
                    </div>
                  );
                })}
              </dl>
            </div>
          )}
        </section>
      )}

      {data && (
        <section
          className="mt-16 scroll-mt-24"
          id="readme-assets"
          aria-labelledby="readme-assets-title"
        >
          <SectionHead
            id="readme-assets-title"
            title="Add to your README"
            note="svg · gif · png · webp"
          />
          <p className={cn(BODY, MEASURE, "mt-3")}>
            Every asset defaults to a static frame; motion is yours to turn on,
            and it plays inside a GitHub README. Each snippet carries a light and
            a dark drawing behind one{" "}
            <code className="font-mono text-[0.8125rem] text-ink">
              &lt;picture&gt;
            </code>{" "}
            element, so it follows the reader's own GitHub theme.
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

          <div className="mt-10 grid gap-10">
            {/* The card is generated live rather than fetched as an image, so
                it gets the same frame written out here. */}
            <figure className="border border-rule-strong bg-paper">
              <figcaption className="flex min-h-11 items-center justify-between gap-3 border-b border-rule px-4 py-2">
                <span className={FIELD}>Profile card</span>
                <EmbedSnippet
                  apiBase={apiBase}
                  chartPath={`/api/users/${login}/card.svg`}
                  linkHref={canonical}
                  label="Profile card"
                  altText={`gitdebt profile statistics for ${login}`}
                  variant="menu"
                />
              </figcaption>
              <div className="flex justify-center p-4">
                <ProfileCardPreview
                  apiBase={apiBase}
                  login={login}
                  initialRevision={revision}
                  warm={false}
                />
              </div>
            </figure>

            <StatCard
              src={`${apiBase}/api/users/${login}/chart.svg`}
              alt={`Aggregate star history across ${login}'s public repos`}
              caption="Aggregate star history"
              apiBase={apiBase}
              embedLink={canonical}
            />

            {PROFILE_CHARTS.map((chart) => (
              <StatCard
                key={chart.name}
                src={`${apiBase}/api/users/${login}/stats/${chart.name}.svg`}
                alt={`${chart.label} for ${login}`}
                caption={chart.label}
                apiBase={apiBase}
                embedLink={canonical}
              />
            ))}
          </div>
        </section>
      )}

      {data && trackedRepos.length > 0 && (
        <section
          className="mt-16 scroll-mt-24"
          id="tracked-repos"
          aria-labelledby="tracked-repos-title"
        >
          <SectionHead
            id="tracked-repos-title"
            title="Tracked repositories"
            action={{ href: "/compare", label: "comparison builder" }}
          />
          <ul
            role="list"
            className="mt-6 grid gap-px border border-rule-strong bg-rule sm:grid-cols-2 lg:grid-cols-3"
          >
            {trackedRepos.map((slug) => (
              <li key={slug} className="bg-paper">
                <a href={`/${slug}`} className={cn(ROW, "min-h-12")}>
                  <span className={cn(DATUM, "min-w-0 flex-1 truncate")}>
                    {slug}
                  </span>
                </a>
              </li>
            ))}
          </ul>
          <p className={cn(CAPTION, MEASURE, "mt-3")}>
            Each repository sheet carries the full star trace plus its
            code-health readings — change frequency, repair load, ownership
            concentration and more.
          </p>
        </section>
      )}

      <section className="mt-16 scroll-mt-24 border-t border-rule pt-10">
        <h2 className={HEADING}>How this sheet is drawn</h2>
        <p className={cn(BODY, MEASURE, "mt-3")}>
          gitdebt sums the cumulative star history of {login}'s top public
          repositories (up to 50, by stars) from cached public star timestamps,
          and derives the code readings from cached git history — commits, commit
          days, language line counts and author concentration. Everything here is
          read from gitdebt's own database; nothing on this page queries GitHub
          while you wait. Repositories without cached history are read in the
          background and join the totals as they complete. Explore further on the{" "}
          <a
            href="/leaderboard"
            className="text-ink underline decoration-rule-strong underline-offset-4 outline-none transition-colors duration-[--duration-ui] hover:text-signal hover:decoration-signal focus-visible:outline-2 focus-visible:outline-signal"
          >
            leaderboard
          </a>{" "}
          or by{" "}
          <a
            href="/compare"
            className="text-ink underline decoration-rule-strong underline-offset-4 outline-none transition-colors duration-[--duration-ui] hover:text-signal hover:decoration-signal focus-visible:outline-2 focus-visible:outline-signal"
          >
            comparing star histories
          </a>{" "}
          across repositories.
        </p>
      </section>
    </div>
  );
}

/**
 * A tier this account has not reached, drawn as what it is: a measurement
 * against a target.
 *
 * The bar is a dimension line growing from its own datum, and the figure above
 * it states the same measurement in numbers, so the meaning never rests on the
 * bar alone. Nothing here starts invisible — the row, the figures and the track
 * are painted before any animation runs, and `extends` only scales a shape that
 * already has its final width.
 */
function LockedTier({
  tier,
  currentDays,
  longestDays,
}: {
  tier: CommitStreakTier;
  currentDays: number;
  longestDays: number;
}) {
  const progress = Math.min(100, Math.round((currentDays / tier.days) * 100));
  const remaining = Math.max(0, tier.days - currentDays);
  const next =
    currentDays > 0
      ? `${num(remaining)} more consecutive ${remaining === 1 ? "day" : "days"} to go. Historical best: ${num(longestDays)} days.`
      : `Land work in a tracked repository today to start a run. Historical best: ${num(longestDays)} days.`;

  return (
    <li className="bg-paper p-4">
      <div className="flex items-baseline justify-between gap-4">
        <p className={FIELD}>Locked · {tier.days}d</p>
        <p className="shrink-0 font-mono text-[0.75rem] tabular-nums text-ink-2">
          {num(currentDays)} / {num(tier.days)}
        </p>
      </div>
      <p className={cn(DATUM, "mt-2.5 text-ink")}>{tier.label}</p>
      <div
        className="mt-4 h-[2px] w-full bg-rule"
        role="progressbar"
        aria-label={`Progress toward ${tier.label}`}
        aria-valuemin={0}
        aria-valuemax={tier.days}
        aria-valuenow={currentDays}
        aria-valuetext={`${num(currentDays)} of ${num(tier.days)} days`}
      >
        <div
          className="extends h-full bg-signal"
          style={{ width: `${progress}%` }}
        />
      </div>
      <p className={cn(CAPTION, "mt-3")}>{next}</p>
    </li>
  );
}
