import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  BODY,
  CAPTION,
  DATUM,
  FIELD,
  HEADING,
} from "@/components/style-tokens";
import { FIELD_CELL, FIELD_ROWS } from "@/components/StatStrip";
import { Segmented } from "@/components/ui/controls";
import { Leader } from "@/components/ui/marks";
import {
  commitMonthPoints,
  healthFacts,
  healthReadings,
  type HealthReading,
  type RepoHealth,
} from "@/lib/repo-health";
import { formatCompact } from "@/lib/star-insights";
import { cn } from "@/lib/utils";

/**
 * Four readings taken off a repository's commit history, lettered as a
 * scorecard.
 *
 * Each reading is a field block — the signal's name, the verdict in words, the
 * measurement drawn against its own full scale, and the numbers behind it — and
 * the four sit on one grid, so the verdict line, the bar and the note line
 * share a baseline across all of them however long any single verdict runs.
 *
 * Severity is carried by the words. There is no coloured status dot: the one
 * reading that is genuinely a warning takes drafting red on its measurement,
 * which is what a revision mark is for, and the verdict beside it says the same
 * thing in English so the colour is never the only carrier.
 */

type ActivityRepo = { repo: string; analysis_ready: boolean };

/** What a repository slot is currently showing. */
type Slot =
  | { state: "loading" }
  | { state: "ready"; health: RepoHealth }
  | { state: "analyzing" }
  | { state: "offline" };

/** Repositories offered at once. Enough to browse, few enough to read. */
const MAX_CHOICES = 5;

/** `owner/repo` → `repo`, the part a visitor recognises on a control. */
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

  // Prefer repositories whose analysis has already landed: the scorecard is
  // the point of the section, so a choice that can only say "still analysing"
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
        setChoices([...new Set([...live, ...curated])].slice(0, MAX_CHOICES));
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
          // re-selecting it asks again instead of showing the same dead card
          // for the rest of the session.
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

  return (
    <div className="border border-rule-strong bg-paper">
      <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-3 border-b border-rule px-4 py-3">
        <p className={FIELD}>Health scorecard</p>
        {choices.length > 0 && (
          <Segmented
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

      <div className="p-4 sm:p-6">
        {slot.state === "ready" ? (
          <Scorecard health={slot.health} />
        ) : (
          <Placeholder slug={selected} state={slot.state} />
        )}
      </div>
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
    <div className="grid min-h-64 place-items-center px-4 text-center">
      <div>
        <p className={cn(BODY, "text-ink")} aria-live="polite">
          {copy}
        </p>
        {slug && (
          <a
            href={`/${slug}`}
            className="mt-3 inline-flex items-baseline gap-1.5 text-[0.8125rem] text-ink-3 outline-none transition-colors duration-[--duration-ui] hover:text-signal focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-signal"
          >
            Open the {slug} report
            <Leader size={12} />
          </a>
        )}
      </div>
    </div>
  );
}

/**
 * One reading, drawn against its own scale.
 *
 * The hairline track is the whole of what could be measured and the heavy line
 * is what was, so a short line reads as a small share rather than as a small
 * bar. The tick at its end is the terminator, and the tick at the origin is the
 * datum the reading is taken from.
 */
function Measure({ ratio, warn }: { ratio: number; warn: boolean }) {
  const extent = Math.min(100, Math.max(0, ratio * 100));
  const ink = warn ? "var(--signal)" : "var(--ink)";
  return (
    <svg
      viewBox="0 0 100 10"
      preserveAspectRatio="none"
      width="100%"
      height="10"
      aria-hidden="true"
      focusable="false"
      className="block"
    >
      <g strokeLinecap="butt">
        <line
          x1="0.5"
          y1="1"
          x2="0.5"
          y2="9"
          stroke="var(--rule-strong)"
          strokeWidth="1"
          vectorEffect="non-scaling-stroke"
        />
        <line
          x1="0.5"
          y1="5"
          x2="100"
          y2="5"
          stroke="var(--rule)"
          strokeWidth="1"
          vectorEffect="non-scaling-stroke"
        />
        {extent > 0.4 && (
          <line
            x1="0.5"
            y1="5"
            x2={extent}
            y2="5"
            stroke={ink}
            strokeWidth="2"
            vectorEffect="non-scaling-stroke"
          />
        )}
        <line
          x1={Math.max(0.5, extent)}
          y1="1.5"
          x2={Math.max(0.5, extent)}
          y2="8.5"
          stroke={ink}
          strokeWidth="1"
          vectorEffect="non-scaling-stroke"
        />
      </g>
    </svg>
  );
}

/**
 * Commits per month, as columns standing on a baseline.
 *
 * Columns rather than a trace: this is a count taken per month, not a level
 * measured continuously, and the sheet already carries the trace grammar for
 * the things that are.
 */
function CommitColumns({
  points,
}: {
  points: { date: string; value: number }[];
}) {
  const max = Math.max(1, ...points.map((point) => point.value));
  const width = Math.max(1, points.length);
  return (
    <svg
      viewBox={`0 0 ${width} 100`}
      preserveAspectRatio="none"
      width="100%"
      height="150"
      aria-hidden="true"
      focusable="false"
      className="block"
    >
      {points.map((point, index) => {
        const height = (point.value / max) * 88;
        if (height <= 0) return null;
        return (
          <line
            key={point.date}
            x1={index + 0.5}
            y1={99}
            x2={index + 0.5}
            y2={99 - height}
            stroke="var(--ink-2)"
            strokeWidth="3"
            strokeLinecap="butt"
            vectorEffect="non-scaling-stroke"
          />
        );
      })}
      <line
        x1="0"
        y1="99.5"
        x2={width}
        y2="99.5"
        stroke="var(--rule-strong)"
        strokeWidth="1"
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}

function Reading({ reading }: { reading: HealthReading }) {
  return (
    <section className={FIELD_CELL}>
      <h4 className={FIELD}>{reading.label}</h4>
      <p className="mt-2.5 font-draft text-[1.0625rem] leading-tight text-ink">
        {reading.verdict}
      </p>
      <div className="mt-3">
        <Measure ratio={reading.ratio} warn={reading.tone === "risk"} />
      </div>
      <div className="mt-2.5">
        <p className={CAPTION}>{reading.detail}</p>
        <p className={cn(CAPTION, "mt-1")}>{reading.question}</p>
      </div>
    </section>
  );
}

function Scorecard({ health }: { health: RepoHealth }) {
  const readings = healthReadings(health);
  const facts = healthFacts(health);
  const points = commitMonthPoints(health);
  const commits = points.reduce((total, point) => total + point.value, 0);
  const peak = Math.max(0, ...points.map((point) => point.value));

  return (
    <div>
      <div className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-2">
        <h3 className={cn(HEADING, "min-w-0 truncate font-mono text-[1rem]")}>
          {health.repo}
        </h3>
        <div className="flex flex-wrap items-baseline gap-x-6 gap-y-2">
          <span className="flex items-baseline gap-2">
            <span className={FIELD}>Stars</span>
            <span className="font-draft text-[1.0625rem] tabular-nums text-ink">
              {formatCompact(health.stars)}
            </span>
          </span>
          <a
            href={`/${health.repo}`}
            className="inline-flex items-baseline gap-1.5 text-[0.8125rem] text-ink-3 outline-none transition-colors duration-[--duration-ui] hover:text-signal focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-signal"
          >
            full report
            <Leader size={12} />
          </a>
        </div>
      </div>

      {/* Four readings, four rows, one grid: no verdict's length can move the
          bar or the note in the cell beside it. */}
      <div className="mt-8 grid grid-rows-[auto_auto_auto_auto] gap-x-10 gap-y-8 sm:grid-cols-2">
        {readings.map((reading) => (
          <Reading key={reading.key} reading={reading} />
        ))}
      </div>

      {commits > 0 && (
        <section className="mt-8 border border-rule">
          <header className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1 border-b border-rule px-4 py-2.5">
            <h4 className={FIELD}>Commits per month</h4>
            <p className={CAPTION}>last {points.length} months</p>
          </header>
          <div className="px-4 py-4">
            <CommitColumns points={points} />
            <p className={cn(CAPTION, "mt-2.5")}>
              {formatCompact(commits)} commits across the window · busiest month{" "}
              {formatCompact(peak)}
            </p>
          </div>
        </section>
      )}

      <dl className={cn("mt-8 grid gap-x-8 gap-y-6 sm:grid-cols-3", FIELD_ROWS)}>
        {facts.map((fact) => (
          <div key={fact.key} className={FIELD_CELL}>
            <dt className={FIELD}>{fact.label}</dt>
            <dd
              className={cn(DATUM, "mt-2.5 truncate text-ink")}
              title={fact.value}
            >
              {fact.value}
            </dd>
            <dd className={cn(CAPTION, "mt-2")}>{fact.detail}</dd>
          </div>
        ))}
      </dl>

      {health.analysis_truncated && (
        <p className={cn(CAPTION, "mt-6 border border-rule bg-table px-4 py-3")}>
          Bounded analysis window: repair load, the hotspot and the debt markers
          describe the commits gitdebt read, not the repository's entire
          history.
        </p>
      )}
    </div>
  );
}
