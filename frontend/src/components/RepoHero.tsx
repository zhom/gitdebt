import { useEffect, useRef, useState } from "react";
import {
  animate,
  motion,
  useMotionValue,
  useReducedMotion,
  useTransform,
} from "motion/react";
import { ExternalLink, Loader2 } from "lucide-react";

import { ButtonLink } from "@/components/ButtonLink";
import { DitherMeter } from "@/components/DitherMeter";
import { StatStrip } from "@/components/StatStrip";
import { BODY, CAPTION, EYEBROW, HEADING, PANEL, TITLE } from "@/components/style-tokens";
import { BRAND } from "@/lib/dither";
import { DURATION, EASE_OUT } from "@/lib/motion";
import { formatCountdown, useLiveCountdown } from "@/lib/live-eta";
import { cn } from "@/lib/utils";

export type StarPoint = { date: string; stars: number };
export type HistoryKind =
  "current_stargazers" | "public_star_actions" | "unavailable";
export type HistoryStatus = "ready" | "queued" | "retrying" | "not_public";

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
  initialData: AnalyzeResponse | null;
};

const POLL_MS = 20_000;
const PROGRESS_POLL_MS = 4_000;

function needsPolling(data: AnalyzeResponse): boolean {
  if (data.not_found || data.history_status === "not_public") return false;
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
  return d.toLocaleDateString(undefined, {
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
  const [data, setData] = useState<AnalyzeResponse | null>(initialData);
  const [loading, setLoading] = useState(!initialData);
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
        const next = (await res.json()) as AnalyzeResponse;
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
    function applyProgress(next: RepoProgress, isLive: boolean) {
      if (cancelled) return;
      setProgress(next);
      setLiveProgress(isLive);
      window.dispatchEvent(
        new CustomEvent("gitdebt:repo-progress", { detail: next }),
      );
      void tick(false);
    }

    async function pollProgress(schedule = true): Promise<RepoProgress | null> {
      if (cancelled) return null;
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
        applyProgress(next, false);
        if (next.terminal) return next;
        if (schedule) progressTimer = setTimeout(pollProgress, PROGRESS_POLL_MS);
        return next;
      } catch {
        setLiveProgress(false);
      }
      if (schedule) progressTimer = setTimeout(pollProgress, PROGRESS_POLL_MS);
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

    if (!initialData || needsPolling(initialData)) {
      // Both analyzer reads are enqueue triggers. Wait for them before opening
      // the read-only stream so a cold repo cannot report idle and close before
      // its durable work rows exist.
      void Promise.allSettled([tick(), enqueueAnalysis()]).then(() => {
        void pollProgress(false).then((snapshot) => {
          if (!snapshot?.terminal) connectProgress();
        });
      });
    } else {
      void enqueueAnalysis().finally(() => {
        void pollProgress(false).then((snapshot) => {
          if (!snapshot?.terminal) connectProgress();
        });
      });
    }
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
      if (progressTimer) clearTimeout(progressTimer);
      events?.close();
    };
  }, [owner, repo, apiBase, initialData]);

  const slug = data?.repo ?? `${owner}/${repo}`;
  const latest = data?.history[data.history.length - 1]?.date ?? null;
  const year = data ? firstStarYear(data) : null;
  const archiveHistory = isArchiveHistory(data);

  if (data?.not_found || data?.history_status === "not_public") {
    return (
      <section className="space-y-6">
        <div className="space-y-2">
          <h1 className={TITLE}>Repository not public or not found</h1>
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
    phase: "pending",
    complete: false,
  };
  // The strip is a working indicator, not a checklist: once both halves have
  // settled there is nothing to report, so it disappears instead of showing
  // rows of ticks.
  const showProgress =
    (!data && loading) ||
    !isSettled(starsWork) ||
    !isSettled(analysisWork) ||
    (data !== null && needsPolling(data));

  return (
    <section className="space-y-6">
      <header className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
        <div className="space-y-2">
          <h1 className={TITLE}>{slug}</h1>
          <p className={cn(BODY, "max-w-[62ch]")}>
            Star momentum, maintenance concentration, contributor health, and
            codebase change — one report built from public repository data.
          </p>
        </div>
        <ButtonLink
          href={`https://github.com/${owner}/${repo}`}
          target="_blank"
          rel="noreferrer"
          variant="outline"
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
        columns={4}
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

      {data && archiveHistory && <HistoryProvenance data={data} slug={slug} />}

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

function HistoryProvenance({
  data,
  slug,
}: {
  data: AnalyzeResponse;
  slug: string;
}) {
  const coverageStart =
    data.history_coverage_start ?? data.history[0]?.date ?? null;
  const coverageEnd =
    data.history_coverage_end ??
    data.history[data.history.length - 1]?.date ??
    null;

  return (
    <aside
      className={cn(
        PANEL,
        "grid gap-4 p-3.5 lg:grid-cols-[minmax(0,1fr)_auto] lg:items-start",
      )}
      aria-labelledby="star-history-provenance"
    >
      <div className="max-w-[68ch] space-y-1.5">
        <h2 id="star-history-provenance" className={HEADING}>
          Approximate public star activity
        </h2>
        <p className={BODY}>
          This curve uses public GitHub WatchEvents, which record star actions
          but not unstars. The current GitHub star total for {slug} remains the
          headline figure above.
        </p>
      </div>
      <dl className="grid grid-cols-2 gap-x-6 gap-y-3">
        <div className="space-y-1">
          <dt className={EYEBROW}>Observed actions</dt>
          <dd className="text-[13px] tabular-nums">
            {data.history_event_count.toLocaleString()}
          </dd>
        </div>
        <div className="space-y-1">
          <dt className={EYEBROW}>Coverage</dt>
          <dd className="text-[13px]">
            {formatDate(coverageStart)} {"–"} {formatDate(coverageEnd)}
          </dd>
        </div>
      </dl>
    </aside>
  );
}

function starPhaseFromAnalyze(data: AnalyzeResponse | null): ProgressPhase {
  if (!data) return "pending";
  if (data.not_found) return "not_found";
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
  const active = !isSettled(stars) ? stars : analysis;
  const label = !isSettled(stars)
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
  if (
    work.phase === "retrying" ||
    work.phase === "failed" ||
    work.phase === "restricted"
  ) {
    return `Retry scheduled${eta}`;
  }
  if (work.queue_position) {
    return remaining !== undefined
      ? `About ${formatCountdown(remaining)} left · ${work.queue_position.toLocaleString()} ahead`
      : `${work.queue_position.toLocaleString()} reports ahead · measuring wait`;
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
