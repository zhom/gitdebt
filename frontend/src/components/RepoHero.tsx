import { useEffect, useRef, useState } from "react";
import {
  animate,
  motion,
  useMotionValue,
  useReducedMotion,
  useTransform,
} from "motion/react";
import { ExternalLink, Loader2 } from "lucide-react";

import { DURATION, EASE_OUT } from "@/lib/motion";

export type StarPoint = { date: string; stars: number };
export type HistoryKind =
  | "current_stargazers"
  | "public_star_actions"
  | "unavailable";
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

export type RepoProgress = {
  repo: string;
  phase: ProgressPhase;
  terminal: boolean;
  stars: {
    phase: ProgressPhase;
    complete: boolean;
    next_page: number | null;
  };
  analysis: {
    phase: ProgressPhase;
    complete: boolean;
  };
};

type Props = {
  owner: string;
  repo: string;
  apiBase: string;
  initialData: AnalyzeResponse | null;
};

const POLL_MS = 20_000;

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
    let fetching = false;

    async function tick(schedule = true) {
      if (fetching) return;
      fetching = true;
      try {
        setLoading(true);
        const res = await fetch(`${apiBase}/api/repos/${owner}/${repo}/analyze`, {
          cache: "no-store",
          credentials: "omit",
          headers: { accept: "application/json" },
        });
        if (!res.ok) throw new Error(`API ${res.status}`);
        const next = (await res.json()) as AnalyzeResponse;
        if (!cancelled) {
          setData(next);
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
          `${apiBase}/api/repos/${owner}/${repo}/analyze-history`,
          {
            method: "POST",
            credentials: "omit",
            signal: AbortSignal.timeout(8_000),
          },
        );
      } catch {
        // Progress polling and the stat cards remain useful if this enqueue
        // attempt is interrupted; a later visit can safely retry it.
      }
    }

    let events: EventSource | null = null;
    const handleProgress = (event: MessageEvent<string>) => {
      try {
        const next = JSON.parse(event.data) as RepoProgress;
        if (cancelled) return;
        setProgress(next);
        setLiveProgress(true);
        window.dispatchEvent(
          new CustomEvent("gitdebt:repo-progress", { detail: next }),
        );
        void tick(false);
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
        });
      }
      events.addEventListener("error", () => {
        setLiveProgress(false);
        events?.close();
      });
    }

    if (!initialData || needsPolling(initialData)) {
      // Both analyzer reads are enqueue triggers. Wait for them before opening
      // the read-only stream so a cold repo cannot report idle and close before
      // its durable work rows exist.
      void Promise.allSettled([tick(), enqueueAnalysis()]).then(connectProgress);
    } else {
      void enqueueAnalysis().finally(connectProgress);
    }
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
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
        <div className="space-y-2 border-y border-border py-8">
          <p className="font-mono text-xs tracking-wide text-muted-foreground uppercase">
            Repository visibility
          </p>
          <h1 className="text-3xl font-semibold tracking-tight text-balance sm:text-4xl">
            Repository not public or not found
          </h1>
          <p className="max-w-[62ch] text-base text-pretty text-muted-foreground">
            GitHub did not expose {slug} as a public repository. Check the
            owner and repository name, or open it on GitHub if you have private
            access. gitdebt does not ingest private repository data.
          </p>
        </div>
        <a
          href={`https://github.com/${owner}/${repo}`}
          target="_blank"
          rel="noreferrer"
          className="inline-flex min-h-11 items-center gap-2 rounded-md border border-border px-3 py-2 text-sm text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
        >
          Check on GitHub
          <ExternalLink className="size-3.5" strokeWidth={1.8} aria-hidden="true" />
        </a>
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
    {
      label: "Code analysis",
      value: analysisLabel(progress?.analysis.phase),
    },
  ];
  const showProgress =
    !data ||
    loading ||
    (progress !== null && !progress.terminal) ||
    (data !== null && needsPolling(data));

  return (
    <section className="space-y-6">
      <header className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
        <div className="space-y-2">
          <p className="font-mono text-xs tracking-wide text-muted-foreground uppercase">
            Repository intelligence
          </p>
          <h1 className="text-3xl font-semibold tracking-tight text-balance sm:text-4xl">
            {slug}
          </h1>
          <p className="max-w-[62ch] text-base leading-relaxed text-pretty text-muted-foreground">
            Star momentum, maintenance concentration, contributor health, and
            codebase change — one report built from public repository data.
          </p>
        </div>
        <a
          href={`https://github.com/${owner}/${repo}`}
          target="_blank"
          rel="noreferrer"
          className="inline-flex min-h-11 shrink-0 items-center gap-2 self-start rounded-md border border-border px-3 py-2 text-sm text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring sm:self-auto"
        >
          Open on GitHub
          <ExternalLink className="size-3.5" strokeWidth={1.8} aria-hidden="true" />
        </a>
      </header>

      <dl className="grid border-y border-border sm:grid-cols-2 lg:grid-cols-4 lg:divide-x lg:divide-border">
        {stats.map((stat, index) => (
          <div
            key={stat.label}
            className={`py-4 lg:px-5 lg:first:pl-0 ${
              index > 0 ? "border-t border-border sm:border-t-0" : ""
            } ${index > 1 ? "sm:border-t sm:border-border lg:border-t-0" : ""}`}
          >
            <dt className="font-mono text-xs tracking-wide text-muted-foreground uppercase">
              {stat.label}
            </dt>
            <dd className="mt-1.5 text-xl font-semibold tabular-nums">
              {index === 0 && data ? (
                <AnimatedNumber
                  value={data.total_stars}
                  format={(n) => Math.round(n).toLocaleString()}
                />
              ) : (
                stat.value
              )}
            </dd>
          </div>
        ))}
      </dl>

      {data && archiveHistory && (
        <HistoryProvenance data={data} slug={slug} />
      )}

      {showProgress && (
        <div
          className="border-y border-border py-5"
          role="status"
          aria-live="polite"
        >
          <div className="flex flex-wrap items-center justify-between gap-2">
            <p className="inline-flex items-center gap-2 text-sm font-medium">
              <Loader2
                className="size-4 shrink-0 motion-safe:animate-spin text-signal"
                aria-hidden="true"
              />
              Building this report
            </p>
            <p className="font-mono text-xs text-muted-foreground">
              {liveProgress ? "live updates" : "checking progress"}
            </p>
          </div>
          <div className="mt-4 grid gap-3 sm:grid-cols-2">
            <ProgressStep
              label="Star history"
              phase={progress?.stars.phase ?? starPhaseFromAnalyze(data)}
            />
            <ProgressStep
              label="Repository health"
              phase={progress?.analysis.phase ?? "pending"}
            />
          </div>
          <p className="mt-3 text-sm leading-relaxed text-muted-foreground">
            You can stay on this page. Finished sections appear as soon as the
            backend commits them; no manual refresh is needed.
          </p>
        </div>
      )}

      {data?.backfilling && (
        <div className="flex items-start gap-3 border-y border-border py-4 text-base text-pretty text-muted-foreground sm:text-sm">
          <Loader2
            className="mt-0.5 size-3.5 shrink-0 motion-safe:animate-spin text-signal"
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
      className="grid gap-4 border-y border-border py-4 lg:grid-cols-[minmax(0,1fr)_auto] lg:items-start"
      aria-labelledby="star-history-provenance"
    >
      <div className="max-w-[68ch] space-y-1.5">
        <h2
          id="star-history-provenance"
          className="text-base font-medium sm:text-sm"
        >
          Approximate public star activity
        </h2>
        <p className="text-base text-pretty text-muted-foreground sm:text-sm">
          This curve uses public GitHub WatchEvents, which record star actions
          but not unstars. The current GitHub star total for {slug} remains the
          headline figure above.
        </p>
      </div>
      <dl className="grid grid-cols-2 gap-x-6 gap-y-3 text-base sm:text-sm">
        <div className="space-y-0.5">
          <dt className="font-medium text-foreground">Observed actions</dt>
          <dd className="text-muted-foreground tabular-nums">
            {data.history_event_count.toLocaleString()}
          </dd>
        </div>
        <div className="space-y-0.5">
          <dt className="font-medium text-foreground">Coverage</dt>
          <dd className="text-muted-foreground">
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

function analysisLabel(phase: ProgressPhase | undefined): string {
  switch (phase) {
    case "complete":
      return "Ready";
    case "analyzing":
    case "fetching":
    case "backfilling":
      return "In progress";
    case "retrying":
    case "failed":
      return "Retrying";
    case "not_found":
      return "Not found";
    case "restricted":
      return "Retrying";
    default:
      return "Queued";
  }
}

function ProgressStep({
  label,
  phase,
}: {
  label: string;
  phase: ProgressPhase;
}) {
  const complete = phase === "complete";
  const stopped = phase === "not_found";
  const detail = complete
    ? "Ready"
    : stopped
      ? analysisLabel(phase)
      : phase === "retrying" || phase === "failed" || phase === "restricted"
        ? "Retry scheduled"
      : phase === "idle" || phase === "pending"
        ? "Queued"
        : phase === "backfilling"
          ? "Filling older data"
          : phase === "analyzing"
            ? "Walking commit history"
            : "Collecting data";

  return (
    <div className="flex items-center gap-3">
      <span
        className={`grid size-6 shrink-0 place-items-center rounded-full border text-xs ${
          complete
            ? "border-signal bg-signal text-signal-foreground"
            : stopped
              ? "border-border text-muted-foreground"
              : "border-signal/40 bg-signal/10 text-signal"
        }`}
        aria-hidden="true"
      >
        {complete ? "✓" : "·"}
      </span>
      <div>
        <p className="text-sm font-medium">{label}</p>
        <p className="font-mono text-xs tracking-wide text-muted-foreground uppercase">
          {detail}
        </p>
      </div>
    </div>
  );
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
    <motion.span
      ref={ref}
      className="inline-block text-xl font-semibold tracking-tight tabular-nums"
    >
      {display}
    </motion.span>
  );
}
