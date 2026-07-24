import { useEffect, useMemo, useState } from "react";
import { ArrowUpRight, Eye, GitBranch, Star } from "lucide-react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";

import { formatCountdown, useLiveCountdown } from "@/lib/live-eta";
import { DURATION, EASE_OUT, REDUCED_MOTION_DURATION } from "@/lib/motion";
import { DitherAreaChart } from "@/components/DitherAreaChart";
import { DitherMeter } from "@/components/DitherMeter";
import { DitherRail } from "@/components/DitherRail";
import { StatStrip } from "@/components/StatStrip";
import {
  CAPTION,
  EYEBROW,
  HEADING,
  PANEL,
  ROW,
} from "@/components/style-tokens";
import { cn } from "@/lib/utils";

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
    const timer = window.setInterval(() => {
      // A hidden tab renders nothing; polling it forever is pure origin load.
      if (document.visibilityState === "visible") void refresh();
    }, 60_000);
    const onVisible = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      active = false;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisible);
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
    // The rotation changes every 6 seconds; opening a stream per rotation
    // churned connections against a process-wide cap that the report pages
    // actually need. Only stream while the tab is in front.
    if (document.visibilityState !== "visible") return;
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
  const chartPoints = useMemo(
    () => history.map((point) => ({ date: point.date, value: point.stars })),
    [history],
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
      className={cn(
        PANEL,
        "grid overflow-hidden lg:grid-cols-[0.82fr_1.18fr]",
      )}
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      onFocusCapture={() => setPaused(true)}
      onBlurCapture={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) setPaused(false);
      }}
    >
      <div className="border-b border-border/40 lg:border-r lg:border-b-0">
        <div className="flex items-center justify-between gap-3 border-b border-border/40 px-3.5 py-3">
          <span className={EYEBROW}>Recently viewed</span>
          <span className={CAPTION}>Live platform data</span>
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
                  className="relative px-2 first:pt-2 last:pb-2"
                >
                  <a
                    href={`/${entry.repo}`}
                    onMouseEnter={() => setSelectedRepo(entry.repo)}
                    onFocus={() => setSelectedRepo(entry.repo)}
                    className={cn(
                      ROW,
                      "h-auto justify-between py-2.5 pl-3.5",
                      selectedRow && "bg-card text-foreground",
                    )}
                  >
                    {selectedRow && <DitherRail />}
                    <span className="min-w-0">
                      <span className="block truncate text-[13px] text-foreground">{entry.repo}</span>
                      <span className="mt-0.5 block text-[11px] text-muted-foreground">{relativeTime(entry.viewed_at)}</span>
                    </span>
                    <span className="flex shrink-0 items-center gap-3 text-[11px] text-muted-foreground">
                      <span className="inline-flex items-center gap-1.5 tabular-nums">
                        <Star className="size-3.5" strokeWidth={1.75} aria-hidden="true" />
                        {compact(entry.stars)}
                      </span>
                      <ArrowUpRight className="size-4 transition-transform duration-150 group-hover:-translate-y-0.5 group-hover:translate-x-0.5 motion-reduce:transition-none" aria-hidden="true" />
                    </span>
                  </a>
                </motion.li>
              );
            })}
          </motion.ol>
        ) : (
          <div className="flex min-h-72 items-center px-3.5 py-8 text-[13px] text-muted-foreground" aria-live="polite">
            {failed
              ? "Live activity is temporarily unavailable."
              : loaded
                ? "No repository views recorded yet. Repos appear here when their reports are opened."
                : "Loading recently viewed repositories…"}
          </div>
        )}
      </div>

      <div className="min-h-[26rem] p-3.5 sm:p-5">
        <AnimatePresence mode="wait" initial={false}>
          {selected && (
            <motion.a
              key={selected.repo}
              href={`/${selected.repo}`}
              aria-label={`Open the ${selected.repo} repository report`}
              initial={{ opacity: 0, x: reduceMotion ? 0 : 8 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: reduceMotion ? 0 : -5 }}
              transition={{ duration, ease: EASE_OUT }}
              className="flex h-full flex-col"
            >
              <div className="flex flex-wrap items-start justify-between gap-4">
                <h3 className={HEADING}>{selected.repo}</h3>
                <div className="flex flex-wrap gap-1.5">
                  <span
                    className={cn(
                      "dither-chip",
                      historyReady && "border-foreground/30 text-foreground",
                    )}
                  >
                    history {historyStatus}
                  </span>
                  <span
                    className={cn(
                      "dither-chip",
                      selected.analysis_ready &&
                        "border-foreground/30 text-foreground",
                    )}
                  >
                    health {selected.analysis_ready ? "ready" : "analyzing"}
                  </span>
                </div>
              </div>

              <StatStrip
                className="mt-5"
                columns={3}
                items={[
                  {
                    label: (
                      <span className="inline-flex items-center gap-1.5">
                        <Star className="size-3" aria-hidden="true" /> current stars
                      </span>
                    ),
                    key: "stars",
                    value: compact(selected.stars),
                  },
                  {
                    label: (
                      <span className="inline-flex items-center gap-1.5">
                        <Eye className="size-3" aria-hidden="true" /> platform views
                      </span>
                    ),
                    key: "views",
                    value: compact(selected.views),
                  },
                  {
                    label: "30d gain",
                    key: "gain",
                    value: `+${compact(selected.gained_30d)}`,
                  },
                ]}
              />

              <div className={cn(PANEL, "mt-5 flex flex-1 items-center justify-center overflow-hidden")}>
                {history.length > 1 ? (
                  <DitherAreaChart
                    points={chartPoints}
                    height={230}
                    valueLabel="stars"
                  />
                ) : (
                  <div className="max-w-sm px-8 py-12 text-center">
                    <GitBranch className="mx-auto size-6 text-muted-foreground" strokeWidth={1.5} aria-hidden="true" />
                    <p className="mt-4 text-[13px]">
                      {remaining !== undefined
                        ? `Star history in about ${formatCountdown(remaining)}`
                        : "Measuring star-history wait"}
                    </p>
                    <p className={cn(CAPTION, "mt-1")}>
                      {starProgress?.percent !== undefined
                        ? `${starProgress.percent}% complete · updates live`
                        : "The estimate appears as soon as processing starts."}
                    </p>
                    {starProgress?.percent !== undefined && (
                      <DitherMeter
                        className="mx-auto mt-5 h-1.5 max-w-56"
                        ratio={Math.max(0.015, starProgress.percent / 100)}
                        percent={starProgress.percent}
                        label="Star history progress"
                      />
                    )}
                  </div>
                )}
              </div>
            </motion.a>
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}
