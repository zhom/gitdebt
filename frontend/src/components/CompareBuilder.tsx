import { useEffect, useId, useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { ArrowRight, Plus, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import { ChartViewer } from "@/components/ChartViewer";
import { warmRepos } from "@/components/WarmRepos";
import { CATEGORIES } from "@/data/categories";
import {
  DURATION,
  EASE_IN_OUT,
  EASE_OUT,
  REDUCED_MOTION_DURATION,
} from "@/lib/motion";

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
      <form onSubmit={compare} className="card-panel p-6">
        <p className="mono-label">Repos to overlay</p>
        <div className="mt-4 space-y-3">
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
                <div className="dither-control flex flex-1 items-center rounded-md border font-mono text-base focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-ring sm:text-sm">
                  <label
                    htmlFor={`compare-repo-${row.id}`}
                    className="pl-3.5 text-muted-foreground select-none"
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
                    className="w-full flex-1 bg-transparent py-2.5 pr-3.5 pl-1 text-foreground placeholder:text-muted-foreground/50 outline-none"
                  />
                </div>
                <button
                  type="button"
                  onClick={() => removeRow(i)}
                  disabled={rows.length <= MIN_ROWS}
                  aria-label={`Remove repository ${i + 1}`}
                  className="inline-flex size-12 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-accent-foreground disabled:pointer-events-none disabled:opacity-40 sm:size-9"
                >
                  <X
                    className="size-6"
                    strokeWidth={1.75}
                    aria-hidden="true"
                  />
                </button>
              </motion.div>
            ))}
          </AnimatePresence>
        </div>

        <div className="mt-4 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <button
            type="button"
            onClick={addRow}
            disabled={rows.length >= MAX_REPOS}
            className="dither-control inline-flex min-h-11 items-center justify-center gap-1.5 rounded-md border px-3 py-2 font-mono text-base text-muted-foreground hover:text-accent-foreground disabled:pointer-events-none disabled:opacity-40 sm:min-h-0 sm:py-1.5 sm:text-sm"
          >
            <Plus className="size-4 shrink-0" strokeWidth={2} aria-hidden="true" />
            Add repo
          </button>
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
              className="mt-3 text-base text-destructive sm:text-sm"
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
              <a
                href={vsHref}
                className="dither-control inline-flex min-h-11 items-center gap-2 rounded-md border px-3 py-2 font-mono text-base text-muted-foreground hover:text-accent-foreground sm:min-h-0 sm:py-1.5 sm:text-sm"
              >
                Open the {active[0]} vs {active[1]} head-to-head page
                <ArrowRight className="size-4 shrink-0" strokeWidth={2} aria-hidden="true" />
              </a>
            </div>
          )}

          <ChartViewer
            apiBase={apiBase}
            path={chartPath}
            alt={`Star history overlay of ${overlayLabel}`}
            caption="Star history overlay"
            embedLink={`https://gitdebt.com${embedHref}`}
            label={overlayLabel}
          />

          <RepoTable apiBase={apiBase} repos={active} />
        </div>
      )}
    </div>
  );
}

type RowData = { slug: string; total: number | null; year: string | null };

type AnalyzeResponse = {
  total_stars: number;
  history: { date: string; stars: number }[];
};

function RepoTable({ apiBase, repos }: { apiBase: string; repos: string[] }) {
  const [data, setData] = useState<RowData[]>(() =>
    repos.map((slug) => ({ slug, total: null, year: null })),
  );

  useEffect(() => {
    let cancelled = false;
    setData(repos.map((slug) => ({ slug, total: null, year: null })));
    Promise.all(
      repos.map(async (slug): Promise<RowData> => {
        try {
          const res = await fetch(`${apiBase}/api/repos/${slug}/analyze`);
          if (!res.ok) return { slug, total: null, year: null };
          const json = (await res.json()) as AnalyzeResponse;
          const first = json.history[0]?.date;
          const year = first ? String(new Date(first).getUTCFullYear()) : null;
          return { slug, total: json.total_stars, year };
        } catch {
          return { slug, total: null, year: null };
        }
      }),
    ).then((rows) => {
      if (!cancelled) setData(rows);
    });
    return () => {
      cancelled = true;
    };
  }, [apiBase, repos]);

  return (
    <section className="space-y-4">
      <h3 className="flex items-center gap-2 text-base font-medium">
        <span className="size-1.5 shrink-0 rounded-full bg-(--dither-wave-2)" aria-hidden="true" />
        Summary
      </h3>
      <div className="-mx-6 -my-2 overflow-x-auto">
        <div className="inline-block min-w-full px-6 py-2 align-middle">
          <table className="w-full text-left text-base sm:text-sm">
            <thead>
              <tr className="border-b border-border">
                <th className="mono-label py-3 pr-4 whitespace-nowrap">
                  Repo
                </th>
                <th className="mono-label py-3 pr-4 text-right whitespace-nowrap">
                  Total stars
                </th>
                <th className="mono-label py-3 text-right whitespace-nowrap">
                  First star
                </th>
              </tr>
            </thead>
            <tbody className="tabular-nums">
              {data.map((row) => (
                <tr
                  key={row.slug}
                  className="border-b border-border"
                >
                  <td className="py-3 pr-4 font-mono">{row.slug}</td>
                  <td className="py-3 pr-4 text-right">
                    {row.total === null ? "—" : row.total.toLocaleString()}
                  </td>
                  <td className="py-3 text-right text-muted-foreground">
                    {row.year ?? "—"}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </section>
  );
}
