import { useEffect, useId, useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { ArrowRight, Plus, X } from "lucide-react";

import { ButtonLink } from "@/components/ButtonLink";
import { Button } from "@/components/ui/button";
import { DitherComparisonChart } from "@/components/DitherComparisonChart";
import {
  RepoComparisonMatrix,
  type ComparisonInitialRepo,
} from "@/components/RepoComparisonMatrix";
import { BODY, EYEBROW, PANEL, PANEL_PADDED } from "@/components/style-tokens";
import { warmRepos } from "@/components/WarmRepos";
import { CATEGORIES } from "@/data/categories";
import {
  DURATION,
  EASE_IN_OUT,
  EASE_OUT,
  REDUCED_MOTION_DURATION,
} from "@/lib/motion";
import { cn } from "@/lib/utils";

type Props = {
  apiBase: string;
  initialRepos?: string[];
};

const SLUG_RE = /^[A-Za-z0-9._-]+\/[A-Za-z0-9._-]+$/;
const MIN_ROWS = 2;
const MAX_REPOS = 8;
const STATIC_PAIRS = new Set(
  CATEGORIES.flatMap((category) =>
    category.repos.flatMap((first, i) =>
      category.repos.slice(i + 1).map((second) =>
        [first, second].sort().join("/"),
      ),
    ),
  ),
);

type CompareRow = { id: string; value: string };

function seedRows(initial: string[], prefix: string): CompareRow[] {
  const values = initial.slice(0, MAX_REPOS).map((s) => s.toLowerCase());
  while (values.length < MIN_ROWS) values.push("");
  return values.map((value, index) => ({
    id: `${prefix}-${index}`,
    value,
  }));
}

function resolveSeed(initialRepos: string[]): string[] {
  if (initialRepos.length > 0) return initialRepos;
  if (typeof window === "undefined") return [];
  const param = new URLSearchParams(window.location.search).get("repos");
  if (!param) return [];
  return param
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
}

export function CompareBuilder({ apiBase, initialRepos = [] }: Props) {
  const seed = resolveSeed(initialRepos);
  const idPrefix = useId().replaceAll(":", "");
  const nextRowId = useRef(Math.max(seed.length, MIN_ROWS));
  const [rows, setRows] = useState<CompareRow[]>(() =>
    seedRows(seed, idPrefix),
  );
  const [error, setError] = useState<string | null>(null);
  const [active, setActive] = useState<string[] | null>(() => {
    const valid = seed
      .map((s) => s.trim().toLowerCase())
      .filter((s) => SLUG_RE.test(s));
    return valid.length >= MIN_ROWS ? valid.slice(0, MAX_REPOS) : null;
  });
  const pendingFocusId = useRef<string | null>(null);
  const inputRefs = useRef(new Map<string, HTMLInputElement>());
  const reduceMotion = useReducedMotion();

  function setRow(i: number, value: string) {
    setRows((prev) =>
      prev.map((row, idx) => (idx === i ? { ...row, value } : row)),
    );
  }

  function addRow() {
    setRows((prev) => {
      if (prev.length >= MAX_REPOS) return prev;
      const id = `${idPrefix}-${nextRowId.current++}`;
      pendingFocusId.current = id;
      return [...prev, { id, value: "" }];
    });
  }

  function removeRow(i: number) {
    setRows((prev) => {
      if (prev.length <= MIN_ROWS) return prev;
      const remaining = prev.filter((_, idx) => idx !== i);
      pendingFocusId.current =
        remaining[Math.min(i, remaining.length - 1)]?.id ?? null;
      return remaining;
    });
  }

  function compare(e: React.SubmitEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    const cleaned = rows
      .map((row) => row.value.trim().toLowerCase())
      .filter(Boolean);
    if (cleaned.length < MIN_ROWS) {
      setError(`Add at least ${MIN_ROWS} repositories.`);
      return;
    }
    const bad = cleaned.find((r) => !SLUG_RE.test(r));
    if (bad) {
      setError(`“${bad}” is not a valid owner/repo.`);
      return;
    }
    const unique = Array.from(new Set(cleaned)).slice(0, MAX_REPOS);
    warmRepos(apiBase, unique, true);
    setActive(unique);
  }

  useEffect(() => {
    const id = pendingFocusId.current;
    if (!id) return;
    inputRefs.current.get(id)?.focus();
    pendingFocusId.current = null;
  }, [rows]);

  const chartPath = active
    ? `/api/chart.svg?repos=${encodeURIComponent(active.join(","))}`
    : null;
  const overlayLabel = active ? active.join(" vs ") : "";
  const compareHref = active
    ? `/compare?repos=${encodeURIComponent(active.join(","))}`
    : "/compare";
  const pairKey =
    active && active.length === 2 ? [...active].sort().join("/") : null;
  const vsHref =
    active && pairKey && STATIC_PAIRS.has(pairKey)
      ? `/vs/${[...active].sort().join("/")}`
      : null;
  const embedHref = vsHref ?? compareHref;

  return (
    <div className="space-y-8">
      <form onSubmit={compare} className={cn(PANEL, "p-3.5")}>
        <p className={EYEBROW}>Repos to overlay</p>
        <div className="mt-3 space-y-2">
          <AnimatePresence initial={false}>
            {rows.map((row, i) => (
              <motion.div
                key={row.id}
                layout={reduceMotion ? false : "position"}
                initial={{
                  opacity: 0,
                  y: reduceMotion ? 0 : -4,
                  scale: reduceMotion ? 1 : 0.98,
                }}
                animate={{ opacity: 1, y: 0, scale: 1 }}
                exit={{
                  opacity: 0,
                  scale: reduceMotion ? 1 : 0.98,
                  transition: {
                    duration: reduceMotion
                      ? REDUCED_MOTION_DURATION
                      : DURATION.feedback,
                    ease: EASE_OUT,
                  },
                }}
                transition={{
                  duration: reduceMotion
                    ? REDUCED_MOTION_DURATION
                    : DURATION.enter,
                  ease: EASE_OUT,
                  layout: {
                    duration: DURATION.move,
                    ease: EASE_IN_OUT,
                  },
                }}
                className="flex items-center gap-2"
              >
                <div className="flex min-h-10 flex-1 items-center rounded-md border border-border/60 bg-background/60 font-mono text-[13px] transition-[border-color] duration-150 hover:border-foreground/25 focus-within:border-accent/70">
                  <label
                    htmlFor={`compare-repo-${row.id}`}
                    className="pl-3 text-muted-foreground select-none"
                  >
                    github.com/
                  </label>
                  <input
                    ref={(node) => {
                      if (node) inputRefs.current.set(row.id, node);
                      else inputRefs.current.delete(row.id);
                    }}
                    id={`compare-repo-${row.id}`}
                    name={`repo-${i + 1}`}
                    value={row.value}
                    onChange={(e) => setRow(i, e.target.value)}
                    placeholder="owner/repo"
                    autoCapitalize="off"
                    autoCorrect="off"
                    spellCheck={false}
                    aria-label={`Repository ${i + 1}, as owner/repo`}
                    className="w-full flex-1 bg-transparent py-2 pr-3 pl-1 text-foreground placeholder:text-muted-foreground/50 outline-none"
                  />
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  onClick={() => removeRow(i)}
                  disabled={rows.length <= MIN_ROWS}
                  aria-label={`Remove repository ${i + 1}`}
                  className="shrink-0"
                >
                  <X className="size-4" strokeWidth={1.75} aria-hidden="true" />
                </Button>
              </motion.div>
            ))}
          </AnimatePresence>
        </div>

        <div className="mt-4 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <Button
            variant="outline"
            onClick={addRow}
            disabled={rows.length >= MAX_REPOS}
          >
            <Plus className="size-4 shrink-0" strokeWidth={2} aria-hidden="true" />
            Add repo
          </Button>
          <Button type="submit" size="lg" className="w-full sm:w-auto">
            Compare
          </Button>
        </div>

        <AnimatePresence initial={false}>
          {error && (
            <motion.p
              key={error}
              initial={{
                opacity: 0,
                y: reduceMotion ? 0 : -4,
              }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, transition: { duration: 0.12 } }}
              transition={{
                duration: reduceMotion
                  ? REDUCED_MOTION_DURATION
                  : DURATION.enter,
                ease: EASE_OUT,
              }}
              className="mt-3 text-[11px] text-[var(--swatch-red)]"
              role="alert"
            >
              {error}
            </motion.p>
          )}
        </AnimatePresence>
      </form>

      {active && chartPath && (
        <div className="space-y-6">
          {vsHref && (
            <div>
              <ButtonLink href={vsHref} variant="outline">
                Open the {active[0]} vs {active[1]} head-to-head page
                <ArrowRight className="size-4 shrink-0" strokeWidth={2} aria-hidden="true" />
              </ButtonLink>
            </div>
          )}

          <BuilderComparisonResults
            apiBase={apiBase}
            path={chartPath}
            repos={active}
            embedLink={`https://gitdebt.com${embedHref}`}
            label={overlayLabel}
          />
        </div>
      )}
    </div>
  );
}

type AnalyzeResponse = {
  repo: string;
  total_stars: number;
  created_at: string | null;
  pending?: boolean;
  backfilling?: boolean;
  not_found?: boolean;
  history: { date: string; stars: number }[];
};

function BuilderComparisonResults({
  apiBase,
  path,
  repos,
  embedLink,
  label,
}: {
  apiBase: string;
  path: string;
  repos: string[];
  embedLink: string;
  label: string;
}) {
  const [data, setData] = useState<ComparisonInitialRepo[]>([]);
  const [settled, setSettled] = useState(false);
  useEffect(() => {
    let cancelled = false;
    setData([]);
    setSettled(false);
    Promise.all(
      repos.map(async (slug): Promise<ComparisonInitialRepo | null> => {
        try {
          const res = await fetch(`${apiBase}/api/repos/${slug}/analyze`);
          if (!res.ok) return null;
          const json = (await res.json()) as AnalyzeResponse;
          if (json.not_found) return null;
          return {
            slug,
            total_stars: json.total_stars,
            created_at: json.created_at,
            history: json.history,
            pending: json.pending,
            backfilling: json.backfilling,
          };
        } catch {
          return null;
        }
      }),
    ).then((rows) => {
      if (cancelled) return;
      setData(
        rows.filter((row): row is ComparisonInitialRepo => row !== null),
      );
      setSettled(true);
    });
    return () => {
      cancelled = true;
    };
  }, [apiBase, repos]);

  const chartSeries = data
    .filter((repo) => repo.history.length >= 2)
    .map((repo) => ({ slug: repo.slug, points: repo.history }));

  return (
    <>
      {chartSeries.length >= 2 ? (
        <DitherComparisonChart
          apiBase={apiBase}
          path={path}
          caption="Star history overlay"
          embedLink={embedLink}
          label={label}
          series={chartSeries}
        />
      ) : (
        <section className={PANEL_PADDED} aria-live="polite">
          <p className={EYEBROW}>
            {settled ? "Historical coverage incomplete" : "Loading comparison"}
          </p>
          <p className={`mt-2 max-w-[70ch] ${BODY}`}>
            {settled
              ? "At least two complete star series are needed for the interactive overlay. Current metadata and available health analysis remain visible below."
              : "Reading the completed star series for every repository. The animated overlay appears here automatically."}
          </p>
        </section>
      )}
      <RepoComparisonMatrix apiBase={apiBase} repos={repos} initial={data} />
    </>
  );
}
