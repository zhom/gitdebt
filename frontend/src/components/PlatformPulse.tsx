import { useEffect, useMemo, useState } from "react";
import { ArrowUpRight, Eye, GitBranch, Star } from "lucide-react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";

import { DURATION, EASE_OUT, REDUCED_MOTION_DURATION, SPRING } from "@/lib/motion";
import { MEDIA_RENDER_REVISION } from "@/lib/media";

type ActivityRepo = {
  repo: string;
  stars: number;
  views: number;
  viewed_at: string;
  history_ready: boolean;
  analysis_ready: boolean;
};

type ActivityResponse = { repos: ActivityRepo[] };

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

  const selected = useMemo(
    () => repos.find((entry) => entry.repo === selectedRepo) ?? repos[0],
    [repos, selectedRepo],
  );
  const duration = reduceMotion ? REDUCED_MOTION_DURATION : DURATION.enter;

  return (
    <div className="grid border-y border-black lg:grid-cols-[0.82fr_1.18fr]">
      <div className="border-b border-zinc-300 lg:border-r lg:border-b-0">
        <div className="flex min-h-14 items-center justify-between border-b border-zinc-300 px-5 py-3">
          <span className="font-mono text-xs tracking-[0.12em] text-zinc-500 uppercase">
            Recently viewed
          </span>
          <span className="inline-flex items-center gap-2 font-mono text-xs text-zinc-500">
            <span className="size-1.5 rounded-full bg-emerald-500" aria-hidden="true" />
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
                  className="relative border-b border-zinc-200 last:border-b-0"
                >
                  {selectedRow && (
                    <motion.span
                      layoutId="platform-pulse-selection"
                      className="absolute inset-y-0 left-0 w-0.5 bg-black"
                      transition={reduceMotion ? { duration: 0 } : SPRING.snappy}
                      aria-hidden="true"
                    />
                  )}
                  <a
                    href={`/report?repo=${entry.repo}`}
                    onMouseEnter={() => setSelectedRepo(entry.repo)}
                    onFocus={() => setSelectedRepo(entry.repo)}
                    className={`group flex min-h-[4.5rem] items-center justify-between gap-4 px-5 py-3 outline-none focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-black ${selectedRow ? "bg-zinc-100" : "hover:bg-zinc-50"}`}
                  >
                    <div className="min-w-0">
                      <p className="truncate font-mono text-sm font-medium text-black">{entry.repo}</p>
                      <p className="mt-1 font-mono text-xs text-zinc-500">{relativeTime(entry.viewed_at)}</p>
                    </div>
                    <div className="flex shrink-0 items-center gap-4 font-mono text-xs text-zinc-600">
                      <span className="inline-flex items-center gap-1.5">
                        <Star className="size-3.5" strokeWidth={1.75} aria-hidden="true" />
                        {compact(entry.stars)}
                      </span>
                      <ArrowUpRight className="size-4 text-zinc-400 transition-transform duration-150 group-hover:-translate-y-0.5 group-hover:translate-x-0.5 motion-reduce:transition-none" aria-hidden="true" />
                    </div>
                  </a>
                </motion.li>
              );
            })}
          </motion.ol>
        ) : (
          <div className="flex min-h-72 items-center px-5 py-8 text-sm text-zinc-500" aria-live="polite">
            {failed
              ? "Live activity is temporarily unavailable."
              : loaded
                ? "No public report views yet. Open a report to start the live pulse."
                : "Loading recently viewed repositories…"}
          </div>
        )}
      </div>

      <div className="min-h-[26rem] bg-zinc-50 p-5 sm:p-7">
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
                  <p className="font-mono text-xs tracking-[0.12em] text-zinc-500 uppercase">Current signal</p>
                  <h3 className="mt-2 text-xl font-semibold tracking-tight">{selected.repo}</h3>
                </div>
                <div className="flex flex-wrap gap-2 font-mono text-[11px] uppercase">
                  <span className={`border px-2 py-1 ${selected.history_ready ? "border-emerald-300 bg-emerald-50 text-emerald-800" : "border-amber-300 bg-amber-50 text-amber-800"}`}>
                    stars {selected.history_ready ? "ready" : "queued"}
                  </span>
                  <span className={`border px-2 py-1 ${selected.analysis_ready ? "border-emerald-300 bg-emerald-50 text-emerald-800" : "border-amber-300 bg-amber-50 text-amber-800"}`}>
                    health {selected.analysis_ready ? "ready" : "analyzing"}
                  </span>
                </div>
              </div>

              <div className="mt-6 grid grid-cols-2 border-y border-zinc-300 py-4">
                <div>
                  <p className="flex items-center gap-2 font-mono text-xs text-zinc-500 uppercase">
                    <Star className="size-3.5" aria-hidden="true" /> stars stored
                  </p>
                  <p className="mt-2 text-2xl font-semibold tabular-nums">{compact(selected.stars)}</p>
                </div>
                <div className="border-l border-zinc-300 pl-5">
                  <p className="flex items-center gap-2 font-mono text-xs text-zinc-500 uppercase">
                    <Eye className="size-3.5" aria-hidden="true" /> platform views
                  </p>
                  <p className="mt-2 text-2xl font-semibold tabular-nums">{compact(selected.views)}</p>
                </div>
              </div>

              <div className="mt-6 flex flex-1 items-center justify-center overflow-hidden border border-zinc-300 bg-white">
                {selected.history_ready ? (
                  <img
                    src={`${apiBase}/api/repos/${selected.repo}/chart.svg?theme=light&animate=0&render=${MEDIA_RENDER_REVISION}`}
                    alt={`Star history for ${selected.repo}`}
                    loading="lazy"
                    className="block w-full"
                  />
                ) : (
                  <div className="max-w-sm px-8 py-12 text-center">
                    <GitBranch className="mx-auto size-6 text-zinc-400" strokeWidth={1.5} aria-hidden="true" />
                    <p className="mt-4 text-sm font-medium">Historical events are in the durable queue.</p>
                    <p className="mt-1 text-sm text-zinc-500">Open the report to watch measured progress instead of a fake loading bar.</p>
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
