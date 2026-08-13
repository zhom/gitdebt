import { useEffect, useMemo, useState } from "react";
import { Activity, ArrowUpRight } from "lucide-react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";

import {
  DURATION,
  EASE_OUT,
  REDUCED_MOTION_DURATION,
} from "@/lib/motion";
import { DitherAreaChart } from "@/components/DitherAreaChart";
import { DitherMeter } from "@/components/DitherMeter";
import { StatStrip } from "@/components/StatStrip";
import { CAPTION, EYEBROW, PANEL } from "@/components/style-tokens";
import { useInView } from "@/components/ui/use-in-view";
import {
  healthReadings,
  type HealthReading,
  type RepoHealth,
} from "@/lib/repo-health";
import { cn } from "@/lib/utils";

type LiveRepo = {
  repo: string;
  stars: number;
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

const HEALTH_TONE_WEIGHT = {
  risk: 40,
  watch: 30,
  good: 20,
  steady: 10,
} as const;

const HEALTH_READING_PRIORITY = {
  ownership: 4,
  maintenance: 3,
  debt: 2,
  repair: 1,
} as const;

/**
 * Lead with the most consequential reading. Ownership wins ties because it is
 * gitdebt's clearest compact complement to a repository's popularity curve.
 */
function featuredHealthReading(health: RepoHealth): HealthReading {
  return healthReadings(health).reduce((featured, candidate) => {
    const featuredScore =
      HEALTH_TONE_WEIGHT[featured.tone] +
      HEALTH_READING_PRIORITY[featured.key];
    const candidateScore =
      HEALTH_TONE_WEIGHT[candidate.tone] +
      HEALTH_READING_PRIORITY[candidate.key];
    return candidateScore > featuredScore ? candidate : featured;
  });
}

export function SignalFlowGraphic({ apiBase }: { apiBase: string }) {
  const [repos, setRepos] = useState<LiveRepo[]>([]);
  const [index, setIndex] = useState(0);
  const [paused, setPaused] = useState(false);
  const [history, setHistory] = useState<HistoryPoint[]>([]);
  const [health, setHealth] = useState<RepoHealth | null>(null);
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
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") void refresh();
    }, 30_000);
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
  const chartPoints = useMemo(
    () => history.map((point) => ({ date: point.date, value: point.stars })),
    [history],
  );
  const selectedHealth =
    health && selected && health.repo.toLowerCase() === selected.repo.toLowerCase()
      ? health
      : null;
  const featuredHealth = useMemo(
    () => (selectedHealth ? featuredHealthReading(selectedHealth) : null),
    [selectedHealth],
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

  useEffect(() => {
    if (!selected?.analysis_ready) {
      setHealth(null);
      return;
    }
    let active = true;
    fetch(`${apiBase}/api/repos/${selected.repo}/health.json`, {
      headers: { accept: "application/json" },
    })
      .then((response) => (response.ok ? response.json() : null))
      .then((body: RepoHealth | null) => {
        if (active) setHealth(body?.ready ? body : null);
      })
      .catch(() => {
        if (active) setHealth(null);
      });
    return () => {
      active = false;
    };
  }, [apiBase, selected?.analysis_ready, selected?.repo]);

  return (
    <a
      href={selected ? `/${selected.repo}` : undefined}
      aria-label={selected ? `Open the ${selected.repo} repository report` : "Live repository activity"}
      className="group block rounded-lg outline-none focus-visible:ring-2 focus-visible:ring-accent/30 focus-visible:ring-offset-2 focus-visible:ring-offset-background"
    >
    <figure
      className={cn(PANEL, "w-full min-w-0 overflow-hidden")}
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
        className="flex items-center justify-between gap-4 border-b border-border/40 px-3.5 py-3"
      >
        <p className={EYEBROW}>Live repository</p>
        <p className={CAPTION}>
          {selected ? viewedLabel(selected.viewed_at) : "connecting…"}
        </p>
      </figcaption>

      <div className="min-h-[27rem] p-3.5">
        <AnimatePresence mode="wait" initial={false}>
          {selected ? (
            <motion.div
              key={selected.repo}
              initial={{ opacity: 0, x: reduceMotion ? 0 : 10 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: reduceMotion ? 0 : -7 }}
              transition={{ duration, ease: EASE_OUT }}
              className="group block"
            >
              <div className="flex items-baseline justify-between gap-4">
                <p className="min-w-0 truncate font-mono text-[13px] text-foreground">{selected.repo}</p>
                <span className="inline-flex shrink-0 items-center gap-1.5 text-[11px] text-muted-foreground group-hover:text-foreground">
                  Full report
                  <ArrowUpRight
                    className="size-3.5 transition-transform duration-150 group-hover:-translate-y-0.5 group-hover:translate-x-0.5 motion-reduce:transition-none"
                    aria-hidden="true"
                  />
                </span>
              </div>

              <StatStrip
                className="mt-3"
                columns={3}
                items={[
                  {
                    key: "stars",
                    label: "Current stars",
                    value: formatNumber(selected.stars),
                  },
                  {
                    key: "gain",
                    label: "30d gain",
                    value: `+${formatNumber(selected.gained_30d)}`,
                  },
                  {
                    key: "health",
                    label: featuredHealth?.label ?? "Repo health",
                    value: (
                      <>
                        <div
                          className="truncate text-[15px] leading-none tracking-tight"
                          title={featuredHealth?.detail}
                        >
                          {featuredHealth
                            ? featuredHealth.verdict
                            : selected.analysis_ready
                              ? "Reading insight…"
                              : "Analyzing…"}
                        </div>
                        {featuredHealth && (
                          <span className="sr-only">. {featuredHealth.detail}</span>
                        )}
                      </>
                    ),
                  },
                ]}
              />

              <div className={cn(PANEL, "mt-3 overflow-hidden")}>
                {history.length > 1 ? (
                  <DitherAreaChart
                    points={chartPoints}
                    height={190}
                    valueLabel="stars"
                    seed={selected.repo}
                  />
                ) : (
                  <BuildingHistory />
                )}
              </div>

              <div className="mt-3 flex items-center gap-3">
                <div className="flex flex-1 gap-1" aria-label={`${readySignals} of 2 report layers ready`}>
                  {[selected.history_ready, selected.analysis_ready].map((ready, signalIndex) => (
                    <DitherMeter
                      key={signalIndex}
                      ratio={ready ? 1 : 0.08}
                      className="h-1 flex-1"
                    />
                  ))}
                </div>
                <p className={CAPTION}>stars · health</p>
              </div>
            </motion.div>
          ) : (
            <div className="grid min-h-[27rem] place-items-center text-[13px] text-muted-foreground" aria-live="polite">
              Connecting to live repository activity…
            </div>
          )}
        </AnimatePresence>
      </div>
    </figure>
    </a>
  );
}

/**
 * The indeterminate bar shown while a repository's star history is still being
 * built.
 *
 * It is its own component for one reason: it renders conditionally, and a hook
 * on the parent would attach its IntersectionObserver on the parent's mount,
 * long before this element exists. Mounting the block is what must start the
 * observer, so the observer lives with the block.
 *
 * Off-screen, in a hidden tab, or under reduced motion the sweep parks at its
 * start — one deterministic frame, no scheduled work.
 */
function BuildingHistory() {
  const reduceMotion = useReducedMotion();
  const [ref, inView] = useInView<HTMLDivElement>();
  const active = !reduceMotion && inView;

  return (
    <div className="flex min-h-44 flex-col justify-center px-6">
      <p className="inline-flex items-center gap-2 text-[13px]">
        <Activity className="size-4" aria-hidden="true" />
        Building star history
      </p>
      <div ref={ref} className="mt-4 h-1.5 overflow-hidden rounded-[2px]">
        <motion.span
          initial={{ x: "-65%" }}
          animate={active ? { x: "260%" } : { x: "-65%" }}
          transition={
            active
              ? { duration: 1.4, repeat: Infinity, ease: EASE_OUT }
              : { duration: 0 }
          }
          className="block h-full w-1/3"
        >
          <DitherMeter ratio={1} className="h-full" />
        </motion.span>
      </div>
      <p className={cn(CAPTION, "mt-3")}>A measured ETA replaces this as soon as work starts.</p>
    </div>
  );
}
