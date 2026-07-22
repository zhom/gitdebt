import { useEffect, useMemo, useState } from "react";
import { Activity, ArrowUpRight, Eye, Star } from "lucide-react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";

import {
  DURATION,
  EASE_OUT,
  REDUCED_MOTION_DURATION,
  SPRING,
} from "@/lib/motion";
import { DitherAreaChart } from "@/components/DitherAreaChart";

type LiveRepo = {
  repo: string;
  stars: number;
  views: number;
  viewed_at: string;
  history_ready: boolean;
  analysis_ready: boolean;
  gained_7d: number;
  gained_30d: number;
};

type HistoryPoint = { date: string; stars: number };

function formatNumber(value: number): string {
  return new Intl.NumberFormat("en").format(value);
}

function viewedLabel(value: string): string {
  const elapsed = Math.max(0, Date.now() - new Date(value).getTime());
  const minutes = Math.floor(elapsed / 60_000);
  if (minutes < 1) return "opened just now";
  if (minutes < 60) return `opened ${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  return hours < 24 ? `opened ${hours}h ago` : `opened ${Math.floor(hours / 24)}d ago`;
}

export function SignalFlowGraphic({ apiBase }: { apiBase: string }) {
  const [repos, setRepos] = useState<LiveRepo[]>([]);
  const [index, setIndex] = useState(0);
  const [paused, setPaused] = useState(false);
  const [history, setHistory] = useState<HistoryPoint[]>([]);
  const reduceMotion = useReducedMotion();

  useEffect(() => {
    let active = true;
    async function refresh() {
      try {
        const response = await fetch(`${apiBase}/api/activity.json`, {
          cache: "no-store",
          headers: { accept: "application/json" },
        });
        if (!response.ok) return;
        const data = (await response.json()) as { repos?: LiveRepo[] };
        if (active && Array.isArray(data.repos)) setRepos(data.repos.slice(0, 6));
      } catch {
        // The lookup remains the primary action if the live pulse is offline.
      }
    }
    void refresh();
    const timer = window.setInterval(refresh, 30_000);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [apiBase]);

  useEffect(() => {
    if (reduceMotion || paused || repos.length < 2) return;
    const timer = window.setInterval(
      () => setIndex((value) => (value + 1) % repos.length),
      5_000,
    );
    return () => window.clearInterval(timer);
  }, [paused, reduceMotion, repos.length]);

  useEffect(() => {
    if (index >= repos.length) setIndex(0);
  }, [index, repos.length]);

  const selected = repos[index];
  const duration = reduceMotion ? REDUCED_MOTION_DURATION : DURATION.enter + 0.08;
  const readySignals = useMemo(
    () =>
      selected
        ? [selected.history_ready, selected.analysis_ready].filter(Boolean).length
        : 0,
    [selected],
  );

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
      .then((body: { history?: HistoryPoint[] } | null) => {
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
    <figure
      className="w-full min-w-0 overflow-hidden border-y border-foreground bg-card text-card-foreground"
      aria-labelledby="live-repo-caption"
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      onFocusCapture={() => setPaused(true)}
      onBlurCapture={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) setPaused(false);
      }}
    >
      <figcaption
        id="live-repo-caption"
        className="flex min-h-14 items-center justify-between gap-4 border-b border-border px-4 sm:px-5"
      >
        <p className="inline-flex items-center gap-2 font-mono text-xs tracking-wide text-muted-foreground uppercase">
          <span className="relative flex size-2" aria-hidden="true">
            <span className="absolute inline-flex size-full motion-safe:animate-ping rounded-full bg-emerald-500 opacity-35" />
            <span className="relative inline-flex size-2 rounded-full bg-emerald-500" />
          </span>
          Live repository
        </p>
        <p className="font-mono text-xs text-muted-foreground">
          {selected ? viewedLabel(selected.viewed_at) : "connecting…"}
        </p>
      </figcaption>

      <div className="min-h-[27rem] p-4 sm:p-5">
        <AnimatePresence mode="wait" initial={false}>
          {selected ? (
            <motion.div
              key={selected.repo}
              initial={{ opacity: 0, x: reduceMotion ? 0 : 10 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: reduceMotion ? 0 : -7 }}
              transition={{ duration, ease: EASE_OUT }}
            >
              <div className="flex items-start justify-between gap-4 border-b border-foreground pb-4">
                <div className="min-w-0">
                  <p className="font-mono text-xs text-muted-foreground uppercase">Now inspecting</p>
                  <p className="mt-1 truncate font-mono text-base font-medium">{selected.repo}</p>
                </div>
                <a
                  href={`/${selected.repo}`}
                  className="group inline-flex min-h-11 shrink-0 items-center gap-1.5 text-sm text-muted-foreground outline-none hover:text-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring sm:min-h-0"
                >
                  Full report
                  <ArrowUpRight
                    className="size-3.5 transition-transform duration-150 group-hover:-translate-y-0.5 group-hover:translate-x-0.5 motion-reduce:transition-none"
                    aria-hidden="true"
                  />
                </a>
              </div>

              <div className="grid grid-cols-3 border-b border-border">
                <div className="py-5 pr-4">
                  <p className="flex items-center gap-2 font-mono text-xs text-muted-foreground uppercase">
                    <Star className="size-3.5" aria-hidden="true" /> Current stars
                  </p>
                  <motion.p
                    key={`stars-${selected.repo}`}
                    initial={{ opacity: 0, y: reduceMotion ? 0 : 6 }}
                    animate={{ opacity: 1, y: 0 }}
                    transition={{ duration, ease: EASE_OUT }}
                    className="mt-2 text-3xl font-semibold tracking-[-0.035em] tabular-nums"
                  >
                    {formatNumber(selected.stars)}
                  </motion.p>
                </div>
                <div className="border-l border-border py-5 pl-4">
                  <p className="flex items-center gap-2 font-mono text-xs text-muted-foreground uppercase">
                    <Eye className="size-3.5" aria-hidden="true" /> Report views
                  </p>
                  <motion.p
                    key={`views-${selected.repo}`}
                    initial={{ opacity: 0, y: reduceMotion ? 0 : 6 }}
                    animate={{ opacity: 1, y: 0 }}
                    transition={{ duration, delay: reduceMotion ? 0 : 0.04, ease: EASE_OUT }}
                    className="mt-2 text-3xl font-semibold tracking-[-0.035em] tabular-nums"
                  >
                    {formatNumber(selected.views)}
                  </motion.p>
                </div>
                <div className="border-l border-border py-5 pl-4">
                  <p className="font-mono text-xs text-muted-foreground uppercase">30d gain</p>
                  <motion.p
                    key={`gain-${selected.repo}`}
                    initial={{ opacity: 0, y: reduceMotion ? 0 : 6 }}
                    animate={{ opacity: 1, y: 0 }}
                    transition={{ duration, delay: reduceMotion ? 0 : 0.06, ease: EASE_OUT }}
                    className="mt-2 text-3xl font-semibold tracking-[-0.035em] tabular-nums"
                  >
                    +{formatNumber(selected.gained_30d)}
                  </motion.p>
                </div>
              </div>

              <div className="mt-4 overflow-hidden border-y border-border bg-background">
                {history.length > 1 ? (
                  <DitherAreaChart
                    points={history.map((point) => ({ date: point.date, value: point.stars }))}
                    height={190}
                    valueLabel="stars"
                  />
                ) : (
                  <div className="flex min-h-44 flex-col justify-center px-6">
                    <p className="inline-flex items-center gap-2 text-sm font-medium">
                      <Activity className="size-4" aria-hidden="true" />
                      Building star history
                    </p>
                    <div className="mt-4 h-1.5 overflow-hidden rounded-full bg-muted">
                      <motion.span
                        initial={{ x: "-65%" }}
                        animate={{ x: "260%" }}
                        transition={
                          reduceMotion
                            ? { duration: 0 }
                            : { duration: 1.4, repeat: Infinity, ease: EASE_OUT }
                        }
                        className="block h-full w-1/3 rounded-full bg-foreground"
                      />
                    </div>
                    <p className="mt-3 text-sm text-muted-foreground">A measured ETA replaces this as soon as work starts.</p>
                  </div>
                )}
              </div>

              <div className="mt-4 flex items-center gap-3">
                <div className="flex flex-1 gap-1" aria-label={`${readySignals} of 2 report layers ready`}>
                  {[selected.history_ready, selected.analysis_ready].map((ready, signalIndex) => (
                    <span key={signalIndex} className="h-1 flex-1 overflow-hidden rounded-full bg-muted">
                      <motion.span
                        initial={false}
                        animate={{ scaleX: ready ? 1 : 0.08 }}
                        transition={reduceMotion ? { duration: 0.12 } : SPRING.snappy}
                        className="block h-full origin-left rounded-full bg-foreground"
                      />
                    </span>
                  ))}
                </div>
                <p className="font-mono text-xs text-muted-foreground">stars · health</p>
              </div>
            </motion.div>
          ) : (
            <div className="grid min-h-[27rem] place-items-center text-sm text-muted-foreground" aria-live="polite">
              Connecting to live repository activity…
            </div>
          )}
        </AnimatePresence>
      </div>
    </figure>
  );
}
