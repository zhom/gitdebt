import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ArrowUpRight, Star } from "lucide-react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";

import { DitherAreaChart } from "@/components/DitherAreaChart";
import { DitherMeter } from "@/components/DitherMeter";
import { DitherSegmented } from "@/components/ui/dither-segmented";
import {
  BODY,
  CAPTION,
  EYEBROW,
  PANEL,
  PANEL_PADDED,
} from "@/components/style-tokens";
import { INK, SWATCH, type RGB } from "@/lib/dither";
import { DURATION, EASE_OUT, REDUCED_MOTION_DURATION } from "@/lib/motion";
import {
  commitMonthPoints,
  healthFacts,
  healthReadings,
  type HealthTone,
  type RepoHealth,
} from "@/lib/repo-health";
import { formatCompact } from "@/lib/star-insights";
import { cn } from "@/lib/utils";

type ActivityRepo = { repo: string; analysis_ready: boolean };

/** What a repository slot is currently showing. */
type Slot =
  | { state: "loading" }
  | { state: "ready"; health: RepoHealth }
  | { state: "analyzing" }
  | { state: "offline" };

/** Repositories offered at once. Enough to browse, few enough to read. */
const MAX_CHOICES = 5;

const TONE_FILL: Record<HealthTone, RGB> = {
  good: SWATCH.green,
  steady: INK,
  watch: SWATCH.orange,
  risk: SWATCH.red,
};

const TONE_DOT: Record<HealthTone, string> = {
  good: "var(--swatch-green)",
  steady: "var(--muted-foreground)",
  watch: "var(--swatch-orange)",
  risk: "var(--swatch-red)",
};

/** `owner/repo` → `repo`, the part a visitor recognises on a chip. */
function shortName(slug: string): string {
  return slug.split("/")[1] ?? slug;
}

export function RepoHealthScorecard({
  apiBase,
  repos,
}: {
  apiBase: string;
  /** Curated fallback, used until (and unless) live activity offers better. */
  repos: string[];
}) {
  const curated = useMemo(
    () => repos.map((slug) => slug.toLowerCase()).slice(0, MAX_CHOICES),
    [repos],
  );
  const [choices, setChoices] = useState<string[]>(curated);
  const [selected, setSelected] = useState<string>(curated[0] ?? "");
  const [slots, setSlots] = useState<Record<string, Slot>>({});
  const requested = useRef(new Set<string>());
  const reduceMotion = useReducedMotion();

  // Prefer repositories whose analysis has already landed: the scorecard is
  // the point of the section, so a chip that can only say "still analysing"
  // is a worse first impression than a slightly less topical repository.
  useEffect(() => {
    let active = true;
    fetch(`${apiBase}/api/activity.json`, {
      headers: { accept: "application/json" },
    })
      .then((response) => (response.ok ? response.json() : null))
      .then((body: { repos?: ActivityRepo[] } | null) => {
        if (!active || !Array.isArray(body?.repos)) return;
        const live = body.repos
          .filter((entry) => entry.analysis_ready)
          .map((entry) => entry.repo.toLowerCase());
        if (live.length === 0) return;
        setChoices(
          [...new Set([...live, ...curated])].slice(0, MAX_CHOICES),
        );
      })
      .catch(() => {
        // The curated set already renders; live activity only reorders it.
      });
    return () => {
      active = false;
    };
  }, [apiBase, curated]);

  useEffect(() => {
    setSelected((current) =>
      choices.includes(current) ? current : (choices[0] ?? ""),
    );
  }, [choices]);

  const load = useCallback(
    (slug: string) => {
      if (!slug || requested.current.has(slug)) return;
      requested.current.add(slug);
      setSlots((current) => ({ ...current, [slug]: { state: "loading" } }));
      fetch(`${apiBase}/api/repos/${slug}/health.json`, {
        headers: { accept: "application/json" },
      })
        .then(async (response) => {
          if (!response.ok) return { state: "offline" } as Slot;
          const body = (await response.json()) as RepoHealth;
          return body.ready
            ? ({ state: "ready", health: body } as Slot)
            : ({ state: "analyzing" } as Slot);
        })
        .catch(() => ({ state: "offline" }) as Slot)
        .then((slot) => {
          // Only a finished scorecard is worth keeping. A repository that was
          // mid-analysis (or a request that failed) stays retryable, so
          // re-selecting its chip asks again instead of showing the same
          // dead card for the rest of the session.
          if (slot.state !== "ready") requested.current.delete(slug);
          setSlots((current) => ({ ...current, [slug]: slot }));
        });
    },
    [apiBase],
  );

  useEffect(() => {
    load(selected);
  }, [load, selected]);

  const slot = slots[selected] ?? { state: "loading" };
  const duration = reduceMotion ? REDUCED_MOTION_DURATION : DURATION.enter;

  return (
    <div className={cn(PANEL, "overflow-hidden")}>
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border/40 px-3.5 py-3">
        <p className={EYEBROW}>Health scorecard</p>
        {choices.length > 0 && (
          <DitherSegmented
            role="tablist"
            aria-label="Choose a repository to score"
            value={selected}
            options={choices.map((slug) => ({
              value: slug,
              label: shortName(slug),
            }))}
            onValueChange={(slug) => {
              setSelected(slug);
              load(slug);
            }}
          />
        )}
      </div>

      <AnimatePresence mode="wait" initial={false}>
        <motion.div
          key={`${selected}:${slot.state}`}
          initial={{ opacity: 0, y: reduceMotion ? 0 : 6 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0 }}
          transition={{ duration, ease: EASE_OUT }}
          className="p-3.5 sm:p-5"
        >
          {slot.state === "ready" ? (
            <Scorecard health={slot.health} />
          ) : (
            <Placeholder slug={selected} state={slot.state} />
          )}
        </motion.div>
      </AnimatePresence>
    </div>
  );
}

function Placeholder({
  slug,
  state,
}: {
  slug: string;
  state: "loading" | "analyzing" | "offline";
}) {
  const copy = {
    loading: "Reading the commit history…",
    analyzing:
      "This repository is still being analysed. Its scorecard appears as soon as the commit walk finishes.",
    offline: "Health data is temporarily unavailable.",
  }[state];
  return (
    <div className="grid min-h-72 place-items-center px-6 text-center">
      <div>
        <p className={cn(BODY, "text-foreground")} aria-live="polite">
          {copy}
        </p>
        {slug && (
          <a
            href={`/${slug}`}
            className="mt-3 inline-flex items-center gap-1.5 text-[11px] text-muted-foreground transition-colors duration-150 hover:text-foreground motion-reduce:transition-none"
          >
            Open the {slug} report
            <ArrowUpRight className="size-3.5" aria-hidden="true" />
          </a>
        )}
      </div>
    </div>
  );
}

function Scorecard({ health }: { health: RepoHealth }) {
  const readings = healthReadings(health);
  const facts = healthFacts(health);
  const points = commitMonthPoints(health);
  const commits = points.reduce((total, point) => total + point.value, 0);

  return (
    <div>
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-2">
        <h3 className="min-w-0 truncate font-mono text-[15px] text-foreground">
          {health.repo}
        </h3>
        <div className="flex items-center gap-3">
          <span className="inline-flex items-center gap-1.5 text-[12px] text-muted-foreground tabular-nums">
            <Star className="size-3.5" strokeWidth={1.75} aria-hidden="true" />
            {formatCompact(health.stars)}
          </span>
          <a
            href={`/${health.repo}`}
            className="group inline-flex items-center gap-1.5 text-[11px] text-muted-foreground outline-none transition-colors duration-150 hover:text-foreground focus-visible:ring-2 focus-visible:ring-accent/30 motion-reduce:transition-none"
          >
            full report
            <ArrowUpRight
              className="size-3.5 transition-transform duration-150 group-hover:-translate-y-0.5 group-hover:translate-x-0.5 motion-reduce:transition-none"
              aria-hidden="true"
            />
          </a>
        </div>
      </div>

      {/* No caption here. The section that mounts this card already states
          what these readings are, and a second sentence saying it again is the
          duplication the density pass removes. */}
      <div className="mt-6 grid gap-x-12 gap-y-8 sm:grid-cols-2">
        {readings.map((reading) => (
          <section key={reading.key} className={PANEL_PADDED}>
            <div className="flex items-baseline justify-between gap-3">
              <h4 className={EYEBROW}>{reading.label}</h4>
              <span
                className="size-1.5 shrink-0 rounded-full"
                style={{ backgroundColor: TONE_DOT[reading.tone] }}
                aria-hidden="true"
              />
            </div>
            <p className="mt-2 text-[15px] leading-tight text-foreground">
              {reading.verdict}
            </p>
            <DitherMeter
              className="mt-3"
              ratio={reading.ratio}
              fill={TONE_FILL[reading.tone]}
            />
            <p className={cn(CAPTION, "mt-2.5")}>{reading.detail}</p>
            <p className="mt-1 text-[11px] leading-relaxed text-muted-foreground/60">
              {reading.question}
            </p>
          </section>
        ))}
      </div>

      {commits > 0 && (
        <section className={cn(PANEL, "mt-6 overflow-hidden")}>
          <header className="flex items-baseline justify-between gap-3 border-b border-border/40 px-3.5 py-3">
            <h4 className={EYEBROW}>Commits per month</h4>
            <span className={CAPTION}>last {points.length} months</span>
          </header>
          <div className="px-2 py-3 sm:px-3.5">
            <DitherAreaChart
              points={points}
              height={170}
              valueLabel="commits / month"
              seed={`${health.repo}:health-months`}
            />
          </div>
        </section>
      )}

      <dl className="mt-6 grid gap-x-6 gap-y-4 sm:grid-cols-3">
        {facts.map((fact) => (
          <div key={fact.key} className="min-w-0">
            <dt className={EYEBROW}>{fact.label}</dt>
            <dd
              className="mt-1.5 truncate font-mono text-[12px] text-foreground"
              title={fact.value}
            >
              {fact.value}
            </dd>
            <dd className={cn(CAPTION, "mt-0.5")}>{fact.detail}</dd>
          </div>
        ))}
      </dl>

      {health.analysis_truncated && (
        <p className={cn(CAPTION, "mt-5 border-l-2 border-accent/70 py-1 pl-3")}>
          Bounded analysis window: repair load, the hotspot and debt markers
          describe the commits gitdebt read, not the repository's entire
          history.
        </p>
      )}
    </div>
  );
}
