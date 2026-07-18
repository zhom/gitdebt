import { useEffect, useRef, useState } from "react";
import {
  animate,
  motion,
  useMotionValue,
  useReducedMotion,
  useTransform,
} from "motion/react";
import { Loader2 } from "lucide-react";

import { DURATION, EASE_OUT } from "@/lib/motion";

export type StarPoint = { date: string; stars: number };
export type AnalyzeResponse = {
  repo: string;
  total_stars: number;
  created_at: string | null;
  queued: number;
  pending?: boolean;
  history_complete: boolean;
  history_unavailable?: boolean;
  backfilling?: boolean;
  not_found?: boolean;
  history: StarPoint[];
};

type Props = {
  owner: string;
  repo: string;
  apiBase: string;
  initialData: AnalyzeResponse | null;
};

const POLL_MS = 8_000;

function needsPolling(data: AnalyzeResponse): boolean {
  if (data.history_unavailable || data.not_found) return false;
  return data.pending === true || data.backfilling === true || !data.history_complete;
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

export function RepoHero({ owner, repo, apiBase, initialData }: Props) {
  const [data, setData] = useState<AnalyzeResponse | null>(initialData);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;

    async function tick() {
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
          if (needsPolling(next)) {
            timer = setTimeout(tick, POLL_MS);
          }
        }
      } catch {
        if (!cancelled) timer = setTimeout(tick, POLL_MS * 2);
      } finally {
        if (!cancelled) setLoading(false);
      }
    }
    if (!initialData || needsPolling(initialData)) {
      void tick();
    }
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [owner, repo, apiBase, initialData]);

  if (!data) {
    return (
      <div className="card-panel p-6">
        <p className="text-base text-muted-foreground sm:text-sm">no data yet</p>
      </div>
    );
  }

  const year = firstStarYear(data);
  const subtitle = year
    ? `${data.total_stars.toLocaleString()} stars · since ${year}`
    : `${data.total_stars.toLocaleString()} stars`;

  const latest = data.history[data.history.length - 1]?.date ?? null;

  const stats = [
    { label: "Total stars", value: data.total_stars.toLocaleString() },
    { label: "First star", value: formatDate(data.history[0]?.date) },
    { label: "Latest", value: formatDate(latest) },
  ];

  return (
    <section className="space-y-6">
      <header className="space-y-2">
        <p className="font-mono text-xs tracking-wide text-muted-foreground uppercase">
          Star history
        </p>
        <h1 className="text-3xl font-semibold tracking-tight text-balance sm:text-4xl">
          {data.repo}
        </h1>
      </header>

      <div className="card-panel relative overflow-hidden p-6 sm:p-8">
        <div
          className="absolute inset-x-0 top-0 h-px bg-linear-to-r from-transparent via-signal to-transparent"
          aria-hidden="true"
        />

        <p className="font-mono text-xs tracking-wide text-muted-foreground uppercase">
          Total stars
        </p>
        <div className="mt-3">
          <AnimatedNumber
            value={data.total_stars}
            format={(n) => Math.round(n).toLocaleString()}
          />
        </div>
        <p className="mt-2 text-base text-pretty text-muted-foreground sm:text-sm">
          {subtitle}
        </p>

        <dl className="mt-8 grid grid-cols-1 divide-y divide-border sm:grid-cols-3 sm:divide-x sm:divide-y-0">
          {stats.map((stat, i) => (
            <div
              key={stat.label}
              className={
                i === 0
                  ? "pb-4 sm:pb-0 sm:pr-4"
                  : i === stats.length - 1
                    ? "pt-4 sm:pt-0 sm:pl-4"
                    : "py-4 sm:px-4 sm:py-0"
              }
            >
              <dt className="font-mono text-xs tracking-wide text-muted-foreground uppercase">
                {stat.label}
              </dt>
              <dd className="mt-1.5 text-lg font-semibold tabular-nums sm:text-xl">
                {stat.value}
              </dd>
            </div>
          ))}
        </dl>

        {(needsPolling(data) || loading) && (
          <p className="mt-6 inline-flex items-center gap-2 font-mono text-xs tracking-wide text-muted-foreground">
            <Loader2
              className="size-3.5 shrink-0 motion-safe:animate-spin text-signal"
              aria-hidden="true"
            />
            {needsPolling(data) ? "collecting history" : "checking for updates"}
          </p>
        )}
      </div>

      {data.backfilling && (
        <div className="flex items-start gap-3 rounded-xl border border-border bg-muted/30 p-4 text-base text-pretty text-muted-foreground sm:text-sm">
          <Loader2
            className="mt-0.5 size-3.5 shrink-0 motion-safe:animate-spin text-signal"
            aria-hidden="true"
          />
          <p>
            This repo is very large — still backfilling its full history.
            Numbers will keep climbing until it settles.
          </p>
        </div>
      )}

      {data.history_unavailable && (
        <div
          className="rounded-lg border border-border bg-muted/30 p-4 text-base text-pretty text-muted-foreground sm:text-sm"
          role="status"
        >
          Star history is unavailable for this repository. Other repository
          health reports may still be available.
        </div>
      )}
    </section>
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
      className="inline-block text-5xl font-semibold tracking-tight tabular-nums sm:text-6xl"
    >
      {display}
    </motion.span>
  );
}
