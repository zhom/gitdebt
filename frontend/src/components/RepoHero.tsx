import { useEffect, useMemo, useRef, useState } from "react";
import {
  animate,
  motion,
  useMotionValue,
  useReducedMotion,
  useTransform,
} from "motion/react";
import { ExternalLink, Loader2 } from "lucide-react";

import { BalancedText } from "@/components/BalancedText";
import { ButtonLink } from "@/components/ButtonLink";
import { DitherMeter } from "@/components/DitherMeter";
import { SeriesProvenance } from "@/components/SeriesProvenance";
import { StatStrip } from "@/components/StatStrip";
import { BODY, CAPTION, PANEL, TITLE } from "@/components/style-tokens";
import { BRAND } from "@/lib/dither";
import { historyFreshness, noticeText } from "@/lib/history-freshness";
import { DURATION, EASE_OUT } from "@/lib/motion";
import { formatCountdown, useLiveCountdown } from "@/lib/live-eta";
import { cn } from "@/lib/utils";

export type StarPoint = { date: string; stars: number };
export type HistoryKind =
  "current_stargazers" | "public_star_actions" | "unavailable";
/**
 * `restricted` is a real terminal value the analyzer emits: GitHub serves the
 * repository's stargazer list only to its own admins and collaborators, so
 * there is nothing left to attempt. It was missing from this union, which is
 * why the report used to poll a no-store endpoint forever and offer a retry
 * that was never scheduled.
 */
const HISTORY_STATUSES = [
  "ready",
  "queued",
  "retrying",
  "restricted",
  "not_public",
] as const;

export type HistoryStatus = (typeof HISTORY_STATUSES)[number];

export type AnalyzeResponse = {
  repo: string;
  total_stars: number;
  created_at: string | null;
  queued: number;
  pending?: boolean;
  history_complete: boolean;
  history_kind: HistoryKind;
  history_event_count: number;
  history_coverage_start: string | null;
  history_coverage_end: string | null;
  history_approximate: boolean;
  history_status?: HistoryStatus;
  history_unavailable?: boolean;
  backfilling?: boolean;
  not_found?: boolean;
  history: StarPoint[];
};

/**
 * The payload as it actually arrives — from `fetch` at runtime or from a
 * build-time snapshot. `history_status` is whatever string the backend sent,
 * including a value this build has never heard of, so it is narrowed at the
 * boundary rather than asserted. An unrecognised status classifies as absent,
 * which is the same non-committal reading `historyFreshness()` gives it.
 */
export type AnalyzePayload = Omit<AnalyzeResponse, "history_status"> & {
  history_status?: string;
};

function asHistoryStatus(value: string | undefined): HistoryStatus | undefined {
  return HISTORY_STATUSES.find((status) => status === value);
}

function normalizeAnalyze(payload: AnalyzePayload): AnalyzeResponse;
function normalizeAnalyze(payload: AnalyzePayload | null): AnalyzeResponse | null;
function normalizeAnalyze(payload: AnalyzePayload | null): AnalyzeResponse | null {
  if (!payload) return null;
  return { ...payload, history_status: asHistoryStatus(payload.history_status) };
}

type ProgressPhase =
  | "idle"
  | "pending"
  | "retrying"
  | "fetching"
  | "backfilling"
  | "analyzing"
  | "complete"
  | "not_found"
  | "restricted"
  | "failed";

type ProgressWork = {
  phase: ProgressPhase;
  complete: boolean;
  next_page?: number | null;
  detail?: string;
  queue_position?: number;
  processed_units?: number;
  total_units?: number;
  percent?: number;
  elapsed_seconds?: number;
  eta_seconds?: number;
  retry_at?: string;
  priority?: "interactive";
  blocked_reason?: "provider_quota";
};

export type RepoProgress = {
  repo: string;
  phase: ProgressPhase;
  terminal: boolean;
  stars: ProgressWork;
  analysis: ProgressWork;
};

type Props = {
  owner: string;
  repo: string;
  apiBase: string;
  initialData: AnalyzePayload | null;
};

const POLL_MS = 20_000;
const PROGRESS_POLL_MS = 4_000;

/** The hero's standing description. The actual star curve belongs below. */
const HERO_BLURB =
  "Star momentum, maintenance concentration, contributor health, and codebase change — one report built from public repository data.";

function needsPolling(data: AnalyzeResponse): boolean {
  // Terminal states. Polling them is a request stream that can never change
  // its own answer.
  if (
    data.not_found ||
    data.history_status === "not_public" ||
    data.history_status === "restricted"
  ) {
    return false;
  }
  return (
    data.pending === true ||
    data.backfilling === true ||
    data.history_unavailable === true ||
    data.history_status === "queued" ||
    data.history_status === "retrying" ||
    !data.history_complete
  );
}

function firstStarYear(data: AnalyzeResponse): string | null {
  const first = data.history[0]?.date;
  if (!first) return null;
  const d = new Date(first);
  return Number.isNaN(d.getTime()) ? null : String(d.getUTCFullYear());
}

function formatDate(value: string | null | undefined): string {
  if (!value) return "—";
  const d = new Date(value);
  if (Number.isNaN(d.getTime())) return "—";
  return d.toLocaleDateString("en-US", {
    year: "numeric",
    month: "short",
    day: "numeric",
    timeZone: "UTC",
  });
}

function isArchiveHistory(data: AnalyzeResponse | null): boolean {
  return (
    data?.history_kind === "public_star_actions" ||
    data?.history_approximate === true
  );
}

export function RepoHero({ owner, repo, apiBase, initialData }: Props) {
  const seed = useMemo(() => normalizeAnalyze(initialData), [initialData]);
  const [data, setData] = useState<AnalyzeResponse | null>(seed);
  const [loading, setLoading] = useState(!seed);
  const [progress, setProgress] = useState<RepoProgress | null>(null);
  const [liveProgress, setLiveProgress] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let progressTimer: ReturnType<typeof setTimeout> | null = null;
    let fetching = false;

    async function tick(schedule = true) {
      if (fetching) return;
      fetching = true;
      try {
        setLoading(true);
        const res = await fetch(
          `${apiBase}/api/repos/${owner}/${repo}/analyze`,
          {
            cache: "no-store",
            credentials: "omit",
            headers: { accept: "application/json" },
          },
        );
        if (!res.ok) throw new Error(`API ${res.status}`);
        const next = normalizeAnalyze((await res.json()) as AnalyzePayload);
        if (!cancelled) {
          setData(next);
          window.dispatchEvent(
            new CustomEvent("gitdebt:repo-data", { detail: next }),
          );
          if (schedule && needsPolling(next)) {
            timer = setTimeout(tick, POLL_MS);
          }
        }
      } catch {
        if (!cancelled && schedule) timer = setTimeout(tick, POLL_MS);
      } finally {
        fetching = false;
        if (!cancelled) setLoading(false);
      }
    }

    async function enqueueAnalysis() {
      try {
        await fetch(
          `${apiBase}/api/repos/${owner}/${repo}/analyze-history?view=1`,
          {
            method: "POST",
            credentials: "include",
            signal: AbortSignal.timeout(8_000),
          },
        );
      } catch {
        // Progress polling and the stat cards remain useful if this enqueue
        // attempt is interrupted; a later visit can safely retry it.
      }
    }

    let events: EventSource | null = null;
    let lastProgressKey: string | null = null;
    function applyProgress(next: RepoProgress, isLive: boolean) {
      if (cancelled) return;
      setProgress(next);
      setLiveProgress(isLive);
      window.dispatchEvent(
        new CustomEvent("gitdebt:repo-progress", { detail: next }),
      );
      // Only re-read /analyze when something it reports actually moved.
      // Refetching on every frame turned one open report into a second,
      // uncacheable request stream against the analyze path — the busier the
      // queues, the more frames, the more load.
      const key = `${next.terminal}|${next.stars.phase}|${next.analysis.phase}`;
      if (key !== lastProgressKey) {
        lastProgressKey = key;
        void tick(false);
      }
    }

    let progressFailures = 0;
    function scheduleProgressPoll() {
      if (cancelled) return;
      // Exponential backoff with jitter after a failure: without it every
      // open tab retries on the same fixed interval, so a restarting API tier
      // is met by a synchronized retry wave from every client at once.
      const base =
        progressFailures === 0
          ? PROGRESS_POLL_MS
          : Math.min(PROGRESS_POLL_MS * 2 ** progressFailures, 60_000);
      const delay = base * (0.85 + Math.random() * 0.3);
      progressTimer = setTimeout(() => void pollProgress(), delay);
    }

    async function pollProgress(schedule = true): Promise<RepoProgress | null> {
      if (cancelled) return null;
      // A backgrounded tab has nothing to show; resume on visibilitychange.
      if (document.visibilityState !== "visible") {
        if (schedule) scheduleProgressPoll();
        return null;
      }
      try {
        const response = await fetch(
          `${apiBase}/api/repos/${owner}/${repo}/progress.json`,
          {
            cache: "no-store",
            credentials: "omit",
            headers: { accept: "application/json" },
          },
        );
        if (!response.ok) throw new Error(`progress ${response.status}`);
        const next = (await response.json()) as RepoProgress;
        progressFailures = 0;
        applyProgress(next, false);
        if (next.terminal) return next;
        if (schedule) scheduleProgressPoll();
        return next;
      } catch {
        progressFailures += 1;
        setLiveProgress(false);
      }
      if (schedule) scheduleProgressPoll();
      return null;
    }

    function startProgressPolling() {
      if (progressTimer || cancelled) return;
      void pollProgress();
    }

    const handleProgress = (event: MessageEvent<string>) => {
      try {
        const next = JSON.parse(event.data) as RepoProgress;
        applyProgress(next, true);
        if (next.terminal) events?.close();
      } catch {
        // A malformed progress frame should not interrupt the polling fallback.
      }
    };

    function connectProgress() {
      if (cancelled) return;
      events = new EventSource(
        `${apiBase}/api/repos/${owner}/${repo}/progress`,
      );
      events.addEventListener("progress", handleProgress as EventListener);
      events.addEventListener("open", () => setLiveProgress(true));
      for (const eventName of ["timeout", "unavailable"]) {
        events.addEventListener(eventName, () => {
          setLiveProgress(false);
          events?.close();
          startProgressPolling();
        });
      }
      events.addEventListener("error", () => {
        setLiveProgress(false);
        events?.close();
        startProgressPolling();
      });
    }

    if (!seed || needsPolling(seed)) {
      // Both analyzer reads are enqueue triggers. Wait for them before opening
      // the read-only stream so a cold repo cannot report idle and close before
      // its durable work rows exist.
      void Promise.allSettled([tick(), enqueueAnalysis()]).then(() => {
        void pollProgress(false).then((snapshot) => {
          if (!snapshot?.terminal) connectProgress();
        });
      });
    } else {
      // Already complete: nothing to enqueue and nothing to watch. Crawlers
      // that render JS walk thousands of these pages, and each avoided
      // enqueue+stream is durable queue work and a live connection saved.
      setProgress(null);
    }
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
      if (progressTimer) clearTimeout(progressTimer);
      events?.close();
    };
  }, [owner, repo, apiBase, seed]);

  const slug = data?.repo ?? `${owner}/${repo}`;
  const latest = data?.history[data.history.length - 1]?.date ?? null;
  const year = data ? firstStarYear(data) : null;
  const archiveHistory = isArchiveHistory(data);

  if (data?.not_found || data?.history_status === "not_public") {
    return (
      <section className="space-y-6">
        <div className="space-y-2">
          <BalancedText as="h1" className={TITLE}>
            Repository not public or not found
          </BalancedText>
          <p className={cn(BODY, "max-w-[62ch]")}>
            GitHub did not expose {slug} as a public repository. Check the owner
            and repository name, or open it on GitHub if you have private
            access. gitdebt does not ingest private repository data.
          </p>
        </div>
        <ButtonLink
          href={`https://github.com/${owner}/${repo}`}
          target="_blank"
          rel="noreferrer"
          variant="outline"
          pulse
        >
          Check on GitHub
          <ExternalLink
            className="size-3.5"
            strokeWidth={1.8}
            aria-hidden="true"
          />
        </ButtonLink>
      </section>
    );
  }

  const stats = [
    {
      label: archiveHistory ? "Current GitHub stars" : "GitHub stars",
      value: data ? data.total_stars.toLocaleString() : "—",
    },
    {
      label: archiveHistory ? "Activity since" : "History since",
      value: year ?? formatDate(data?.history[0]?.date),
    },
    {
      label: archiveHistory ? "Latest activity" : "Latest star",
      value: formatDate(latest),
    },
  ];
  const starsWork: ProgressWork = progress?.stars ?? {
    phase: starPhaseFromAnalyze(data),
    complete: data?.history_complete ?? false,
    next_page: null,
  };
  const analysisWork: ProgressWork = progress?.analysis ?? {
    phase: "idle",
    complete: false,
  };
  // The strip is a working indicator, not a checklist. `idle` is terminal
  // backend state, not a synonym for "still loading"; treating it as active
  // left the analysis panel pinned to already-finished reports forever.
  const showProgress =
    (!data && loading) ||
    isActive(starsWork) ||
    isActive(analysisWork) ||
    (data !== null && needsPolling(data));

  return (
    <section className="space-y-6">
      <header className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
        <div className="min-w-0 space-y-3">
          <BalancedText as="h1" className={TITLE}>
            {slug}
          </BalancedText>
          <p className={cn(BODY, "max-w-[62ch]")}>{HERO_BLURB}</p>
        </div>
        <ButtonLink
          href={`https://github.com/${owner}/${repo}`}
          target="_blank"
          rel="noreferrer"
          variant="outline"
          pulse
          className="shrink-0 self-start sm:self-auto"
        >
          Open on GitHub
          <ExternalLink
            className="size-3.5"
            strokeWidth={1.8}
            aria-hidden="true"
          />
        </ButtonLink>
      </header>

      <StatStrip
        columns={3}
        items={stats.map((stat, index) => ({
          label: stat.label,
          value:
            index === 0 && data ? (
              <AnimatedNumber
                value={data.total_stars}
                format={(n) => Math.round(n).toLocaleString()}
              />
            ) : (
              stat.value
            ),
        }))}
      />

      {/* All five states, not just the archive one. An owner arriving at their
          own frozen repository previously saw nothing at all here. */}
      {data && (
        <SeriesProvenance
          snapshot={data}
          slug={slug}
          variant="panel"
          headingId="star-history-provenance"
          signInHref={`${apiBase}/auth/github/start?return_to=${encodeURIComponent(`/${slug}`)}`}
        />
      )}

      {showProgress && (
        <ReportProgress
          stars={starsWork}
          analysis={analysisWork}
          live={liveProgress}
        />
      )}

      {data?.backfilling && (
        <div className={cn(PANEL, "flex items-start gap-3 p-3.5", BODY)}>
          <Loader2
            className="mt-0.5 size-3.5 shrink-0 motion-safe:animate-spin"
            aria-hidden="true"
          />
          <p>
            This is a large repository. Historical windows are being collected
            in the background; the chart appears after a complete snapshot is
            committed.
          </p>
        </div>
      )}
    </section>
  );
}

function starPhaseFromAnalyze(data: AnalyzeResponse | null): ProgressPhase {
  if (!data) return "pending";
  if (data.not_found) return "not_found";
  if (data.history_status === "restricted") return "restricted";
  if (data.history_status === "retrying" || data.history_unavailable)
    return "retrying";
  if (data.backfilling) return "backfilling";
  if (data.history_complete) return "complete";
  return data.pending ? "fetching" : "pending";
}

/**
 * Working indicator for a report that is still being built.
 *
 * One dithered rail carrying the overall fraction plus the phase actually
 * running right now — the point is "what is happening and how far along",
 * not a checklist of finished parts.
 */
function ReportProgress({
  stars,
  analysis,
  live,
}: {
  stars: ProgressWork;
  analysis: ProgressWork;
  live: boolean;
}) {
  const active = isActive(stars) ? stars : analysis;
  // "Collecting" is a claim about work in flight. A restricted read is over,
  // so the heading has to stop saying it is happening.
  const label =
    active.phase === "restricted"
      ? "Star history stopped"
      : isActive(stars)
        ? "Collecting star history"
        : "Reading repository history";
  const remaining = useLiveCountdown(
    active.eta_seconds,
    `${active.phase}:${active.processed_units ?? ""}:${active.queue_position ?? ""}`,
  );
  const detail = progressDetail(active, remaining);
  const done = [stars, analysis].filter(isSettled).length;
  const partial = active.percent !== undefined ? active.percent / 100 : 0;
  const ratio = Math.max(0.02, (done + partial) / 2);

  return (
    <div className={cn(PANEL, "p-3.5")} role="status" aria-live="polite">
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <p className="text-[13px]">{label}</p>
        <p className={cn(CAPTION, "tabular-nums")}>
          {detail}
          {active.priority === "interactive" ? " · priority" : ""}
        </p>
      </div>
      <DitherMeter
        className="mt-3"
        ratio={ratio}
        percent={Math.round(ratio * 100)}
        fill={BRAND}
        label="Report progress"
      />
      <p className={cn(CAPTION, "mt-2")}>
        {live
          ? "Streaming from the backend — sections appear as they land."
          : "Checking progress…"}
      </p>
    </div>
  );
}

/** Settled = nothing more will happen for this half of the report. */
function isSettled(work: ProgressWork): boolean {
  return (
    work.complete || work.phase === "complete" || work.phase === "not_found"
  );
}

function isActive(work: ProgressWork): boolean {
  return (
    work.phase === "pending" ||
    work.phase === "retrying" ||
    work.phase === "fetching" ||
    work.phase === "backfilling" ||
    work.phase === "analyzing" ||
    work.phase === "failed" ||
    work.phase === "restricted"
  );
}

function progressDetail(
  work: ProgressWork,
  remaining: number | undefined,
): string {
  if (
    (work.complete || work.phase === "complete") &&
    work.detail === "recent_window" &&
    work.total_units !== undefined
  ) {
    return `Ready · newest ${work.total_units.toLocaleString()} commits analyzed`;
  }
  if (work.complete || work.phase === "complete") return "Ready";
  if (work.phase === "not_found") return "Not public or not found";
  if (work.blocked_reason === "provider_quota") {
    const retry = work.retry_at ? new Date(work.retry_at) : null;
    const retryLabel =
      retry && !Number.isNaN(retry.getTime())
        ? ` Next attempt ${retry.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}.`
        : "";
    return `Waiting for BigQuery billing or quota.${retryLabel}`;
  }
  const eta =
    remaining !== undefined
      ? ` · about ${formatCountdown(remaining)} left`
      : "";
  if (work.detail === "cloning") return `Cloning the default branch${eta}`;
  if (work.detail === "scanning_history") {
    const units =
      work.processed_units !== undefined && work.total_units !== undefined
        ? `${work.processed_units.toLocaleString()} / ${work.total_units.toLocaleString()} commits`
        : "Walking recent commit history";
    return `${units}${eta}`;
  }
  if (work.detail === "scanning_todos") {
    const units =
      work.processed_units !== undefined && work.total_units !== undefined
        ? `${work.processed_units.toLocaleString()} / ${work.total_units.toLocaleString()} recent patches`
        : "Checking recent TODO/FIXME changes";
    return `${units}${eta}`;
  }
  if (work.detail === "saving_history")
    return `Saving repository signals${eta}`;
  if (work.detail === "finishing")
    return `Counting languages and resolving top contributors${eta}`;
  if (work.phase === "restricted") {
    // Nothing is scheduled and nothing will be. Say what actually happened,
    // in the one place that sentence is written.
    return (
      noticeText(historyFreshness({ history_status: "restricted" })) ??
      "GitHub does not serve this repository's stargazer list to gitdebt."
    );
  }
  if (work.phase === "retrying" || work.phase === "failed") {
    return `Retry scheduled${eta}`;
  }
  if (work.queue_position) {
    // A 1-based rank among pending jobs, not a count of reports ahead: rank 1
    // is the next one to start, so phrasing it as "1 ahead" reads as a wait
    // that does not exist.
    const place =
      work.queue_position === 1
        ? "next up"
        : `queue position ${work.queue_position.toLocaleString()}`;
    return remaining !== undefined
      ? `About ${formatCountdown(remaining)} left · ${place}`
      : `${place} · measuring wait`;
  }
  if (
    work.phase === "backfilling" &&
    work.processed_units !== undefined &&
    work.total_units
  ) {
    return `${work.processed_units} / ${work.total_units} archive months${eta}`;
  }
  if (work.phase === "analyzing") return `Walking recent commit history${eta}`;
  if (remaining !== undefined)
    return `About ${formatCountdown(remaining)} left`;
  return "Measuring work and wait";
}

function AnimatedNumber({
  value,
  format,
}: {
  value: number;
  format: (n: number) => string;
}) {
  const ref = useRef<HTMLSpanElement>(null);
  const previousValue = useRef(value);
  const mv = useMotionValue(value);
  const display = useTransform(mv, (n) => format(n));
  const reduceMotion = useReducedMotion();

  useEffect(() => {
    const previous = previousValue.current;
    previousValue.current = value;
    if (reduceMotion) {
      mv.set(value);
      return;
    }
    if (previous === value) {
      mv.set(value);
      return;
    }
    mv.set(previous);
    const controls = animate(mv, value, {
      duration: DURATION.chart,
      ease: EASE_OUT,
    });
    return () => controls.stop();
  }, [value, mv, reduceMotion]);

  return (
    <motion.span ref={ref} className="inline-block">
      {display}
    </motion.span>
  );
}
