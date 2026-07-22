import { useEffect, useMemo, useState } from "react";
import { ArrowUpRight, Eye, GitBranch, Star } from "lucide-react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";

import { formatCountdown, useLiveCountdown } from "@/lib/live-eta";
import { DURATION, EASE_OUT, REDUCED_MOTION_DURATION, SPRING } from "@/lib/motion";
import { DitherAreaChart } from "@/components/DitherAreaChart";

type ActivityRepo = {
  repo: string;
  stars: number;
  views: number;
  viewed_at: string;
  history_ready: boolean;
  analysis_ready: boolean;
  gained_7d: number;
  gained_30d: number;
};

type ActivityResponse = { repos: ActivityRepo[] };

type StarProgress = {
  phase: string;
  complete: boolean;
  processed_units?: number;
  total_units?: number;
  percent?: number;
  eta_seconds?: number;
};

function compact(value: number): string {
  return new Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 }).format(value);
}

function relativeTime(value: string): string {
  const elapsed = Math.max(0, Date.now() - new Date(value).getTime());
  const minutes = Math.floor(elapsed / 60_000);
  if (minutes < 1) return "viewed just now";
  if (minutes < 60) return `viewed ${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `viewed ${hours}h ago`;
  return `viewed ${Math.floor(hours / 24)}d ago`;
}

export function PlatformPulse({ apiBase }: { apiBase: string }) {
  const [repos, setRepos] = useState<ActivityRepo[]>([]);
  const [selectedRepo, setSelectedRepo] = useState("");
  const [failed, setFailed] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [starProgress, setStarProgress] = useState<StarProgress | null>(null);
  const [paused, setPaused] = useState(false);
  const [history, setHistory] = useState<{ date: string; stars: number }[]>([]);
  const reduceMotion = useReducedMotion();

  useEffect(() => {
    let active = true;
    async function refresh() {
      try {
        const response = await fetch(`${apiBase}/api/activity.json`, {
          headers: { accept: "application/json" },
        });
        if (!response.ok) throw new Error(`activity ${response.status}`);
        const data = (await response.json()) as ActivityResponse;
        if (!active) return;
        setRepos(data.repos);
        setSelectedRepo((current) =>
          data.repos.some((entry) => entry.repo === current)
            ? current
            : (data.repos[0]?.repo ?? ""),
        );
        setFailed(false);
        setLoaded(true);
      } catch {
        if (active) {
          setFailed(true);
          setLoaded(true);
        }
      }
    }
    void refresh();
    const timer = window.setInterval(refresh, 60_000);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [apiBase]);

  useEffect(() => {
    if (reduceMotion || paused || repos.length < 2) return;
    const timer = window.setInterval(() => {
      setSelectedRepo((current) => {
        const index = Math.max(0, repos.findIndex((entry) => entry.repo === current));
        return repos[(index + 1) % repos.length]?.repo ?? current;
      });
    }, 6_000);
    return () => window.clearInterval(timer);
  }, [paused, reduceMotion, repos]);

  useEffect(() => {
    setStarProgress(null);
    const selected = repos.find((entry) => entry.repo === selectedRepo);
    if (!selected || selected.history_ready) return;
    const events = new EventSource(
      `${apiBase}/api/repos/${selected.repo}/progress`,
    );
    const handle = (event: MessageEvent<string>) => {
      try {
        const next = JSON.parse(event.data) as { stars?: StarProgress };
        if (next.stars) setStarProgress(next.stars);
      } catch {
        // Keep the last measured estimate when one malformed frame arrives.
      }
    };
    events.addEventListener("progress", handle as EventListener);
    return () => events.close();
  }, [apiBase, repos, selectedRepo]);

  const selected = useMemo(
    () => repos.find((entry) => entry.repo === selectedRepo) ?? repos[0],
    [repos, selectedRepo],
  );
  const duration = reduceMotion ? REDUCED_MOTION_DURATION : DURATION.enter;
  const remaining = useLiveCountdown(
    starProgress?.eta_seconds,
    `${selectedRepo}:${starProgress?.processed_units ?? ""}`,
  );
  const historyReady = selected?.history_ready || starProgress?.complete;
  const historyStatus = historyReady
    ? "ready"
    : remaining !== undefined
      ? `${formatCountdown(remaining)} left`
      : "measuring wait";

  useEffect(() => {
    if (!selected?.history_ready) {
      setHistory([]);
      return;
    }
    let active = true;
    fetch(`${apiBase}/api/repos/${selected.repo}/analyze?enqueue=0`, {
      headers: { accept: "application/json" },
    })
      .then((response) => (response.ok ? response.json() : null))
      .then((body: { history?: { date: string; stars: number }[] } | null) => {
        if (active) setHistory(Array.isArray(body?.history) ? body.history : []);
      })
      .catch(() => {
        if (active) setHistory([]);
      });
    return () => {
      active = false;
    };
  }, [apiBase, selected?.history_ready, selected?.repo]);

  return (
    <div
      className="grid border-y border-foreground lg:grid-cols-[0.82fr_1.18fr]"
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      onFocusCapture={() => setPaused(true)}
      onBlurCapture={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) setPaused(false);
      }}
    >
      <div className="border-b border-border lg:border-r lg:border-b-0">
        <div className="flex min-h-14 items-center justify-between border-b border-border px-5 py-3">
          <span className="font-mono text-xs tracking-[0.12em] text-muted-foreground uppercase">
            Recently viewed
          </span>
          <span className="inline-flex items-center gap-2 font-mono text-xs text-muted-foreground">
            <span className="relative flex size-2" aria-hidden="true">
              <span className="absolute inline-flex size-full motion-safe:animate-ping rounded-full bg-emerald-500 opacity-35" />
              <span className="relative inline-flex size-2 rounded-full bg-emerald-500" />
            </span>
            Live platform data
          </span>
        </div>

        {repos.length > 0 ? (
          <motion.ol
            initial="hidden"
            animate="visible"
            variants={{
              hidden: {},
              visible: { transition: { staggerChildren: reduceMotion ? 0 : 0.045 } },
            }}
          >
            {repos.slice(0, 6).map((entry) => {
              const selectedRow = selected?.repo === entry.repo;
              return (
                <motion.li
                  key={entry.repo}
                  variants={{
                    hidden: { opacity: 0, y: reduceMotion ? 0 : 7 },
                    visible: { opacity: 1, y: 0 },
                  }}
                  transition={{ duration, ease: EASE_OUT }}
                  className="relative border-b border-border last:border-b-0"
                >
                  {selectedRow && (
                    <motion.span
                      layoutId="platform-pulse-selection"
                      className="absolute inset-y-0 left-0 w-0.5 bg-foreground"
                      transition={reduceMotion ? { duration: 0 } : SPRING.snappy}
                      aria-hidden="true"
                    />
                  )}
                  <a
                    href={`/${entry.repo}`}
                    onMouseEnter={() => setSelectedRepo(entry.repo)}
                    onFocus={() => setSelectedRepo(entry.repo)}
                    className={`group flex min-h-[4.5rem] items-center justify-between gap-4 px-5 py-3 outline-none focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring ${selectedRow ? "bg-muted" : "hover:bg-muted/60"}`}
                  >
                    <div className="min-w-0">
                      <p className="truncate font-mono text-sm font-medium text-foreground">{entry.repo}</p>
                      <p className="mt-1 font-mono text-xs text-muted-foreground">{relativeTime(entry.viewed_at)}</p>
                    </div>
                    <div className="flex shrink-0 items-center gap-4 font-mono text-xs text-muted-foreground">
                      <span className="inline-flex items-center gap-1.5">
                        <Star className="size-3.5" strokeWidth={1.75} aria-hidden="true" />
                        {compact(entry.stars)}
                      </span>
                      <ArrowUpRight className="size-4 text-muted-foreground transition-transform duration-150 group-hover:-translate-y-0.5 group-hover:translate-x-0.5 motion-reduce:transition-none" aria-hidden="true" />
                    </div>
                  </a>
                </motion.li>
              );
            })}
          </motion.ol>
        ) : (
          <div className="flex min-h-72 items-center px-5 py-8 text-sm text-muted-foreground" aria-live="polite">
            {failed
              ? "Live activity is temporarily unavailable."
              : loaded
                ? "No public report views yet. Open a report to start the live pulse."
                : "Loading recently viewed repositories…"}
          </div>
        )}
      </div>

      <div className="min-h-[26rem] bg-muted/45 p-5 sm:p-7">
        <AnimatePresence mode="wait" initial={false}>
          {selected && (
            <motion.div
              key={selected.repo}
              initial={{ opacity: 0, x: reduceMotion ? 0 : 8 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: reduceMotion ? 0 : -5 }}
              transition={{ duration, ease: EASE_OUT }}
              className="flex h-full flex-col"
            >
              <div className="flex flex-wrap items-start justify-between gap-4">
                <div>
                  <p className="font-mono text-xs tracking-[0.12em] text-muted-foreground uppercase">Current signal</p>
                  <h3 className="mt-2 text-xl font-semibold tracking-tight">{selected.repo}</h3>
                </div>
                <div className="flex flex-wrap gap-2 font-mono text-[11px] uppercase">
                  <span className={`border px-2 py-1 ${historyReady ? "border-emerald-500/35 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300" : "border-border bg-background text-foreground"}`}>
                    history {historyStatus}
                  </span>
                  <span className={`border px-2 py-1 ${selected.analysis_ready ? "border-emerald-500/35 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300" : "border-border bg-background text-foreground"}`}>
                    health {selected.analysis_ready ? "ready" : "analyzing"}
                  </span>
                </div>
              </div>

              <div className="mt-6 grid grid-cols-3 border-y border-border py-4">
                <div>
                  <p className="flex items-center gap-2 font-mono text-xs text-muted-foreground uppercase">
                    <Star className="size-3.5" aria-hidden="true" /> current stars
                  </p>
                  <p className="mt-2 text-2xl font-semibold tabular-nums">{compact(selected.stars)}</p>
                </div>
                <div className="border-l border-border pl-5">
                  <p className="flex items-center gap-2 font-mono text-xs text-muted-foreground uppercase">
                    <Eye className="size-3.5" aria-hidden="true" /> platform views
                  </p>
                  <p className="mt-2 text-2xl font-semibold tabular-nums">{compact(selected.views)}</p>
                </div>
                <div className="border-l border-border pl-5">
                  <p className="font-mono text-xs text-muted-foreground uppercase">30d gain</p>
                  <p className="mt-2 text-2xl font-semibold tabular-nums">+{compact(selected.gained_30d)}</p>
                </div>
              </div>

              <div className="mt-6 flex flex-1 items-center justify-center overflow-hidden border-y border-border bg-card">
                {history.length > 1 ? (
                  <DitherAreaChart
                    points={history.map((point) => ({ date: point.date, value: point.stars }))}
                    height={230}
                    valueLabel="stars"
                  />
                ) : (
                  <div className="max-w-sm px-8 py-12 text-center">
                    <GitBranch className="mx-auto size-6 text-muted-foreground" strokeWidth={1.5} aria-hidden="true" />
                    <p className="mt-4 text-sm font-medium">
                      {remaining !== undefined
                        ? `Star history in about ${formatCountdown(remaining)}`
                        : "Measuring star-history wait"}
                    </p>
                    <p className="mt-1 text-sm text-muted-foreground">
                      {starProgress?.percent !== undefined
                        ? `${starProgress.percent}% complete · updates live`
                        : "The estimate appears as soon as processing starts."}
                    </p>
                    {starProgress?.percent !== undefined && (
                      <div className="mx-auto mt-5 h-1.5 max-w-56 overflow-hidden rounded-full bg-muted">
                        <motion.div
                          initial={false}
                          animate={{ scaleX: Math.max(0.015, starProgress.percent / 100) }}
                          transition={reduceMotion ? { duration: 0.12 } : SPRING.snappy}
                          className="h-full origin-left rounded-full bg-foreground"
                        />
                      </div>
                    )}
                  </div>
                )}
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}
