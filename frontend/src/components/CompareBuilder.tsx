import { useEffect, useId, useRef, useState } from "react";

import { ButtonLink } from "@/components/ButtonLink";
import { Button } from "@/components/ui/button";
import { Cut } from "@/components/ui/marks";
import { ComparisonSheet } from "@/components/ComparisonSheet";
import {
  RepoComparisonMatrix,
  type ComparisonInitialRepo,
} from "@/components/RepoComparisonMatrix";
import { BODY, CAPTION, FIELD, MEASURE, PANEL } from "@/components/style-tokens";
import { warmRepos } from "@/components/WarmRepos";
import { CATEGORIES } from "@/data/categories";
import { cn } from "@/lib/utils";

/**
 * The overlay builder: a list of subjects, and one action that draws them.
 *
 * The rows are a schedule, not a form: each one is numbered, the origin
 * `github.com/` is lettered into the field rather than repeated as a hint, and
 * the field is a drawn box that takes ink under the pointer. Nothing slides in
 * or fades up — a row that exists is on the page from the first paint, and a
 * row that is removed is simply gone.
 *
 * There is one action, and it is the primary one. `Add repository` is a text
 * action beside it rather than the outlined half of a filled/outlined pair.
 */

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
      category.repos.slice(i + 1).map((second) => [first, second].sort().join("/")),
    ),
  ),
);

type CompareRow = { id: string; value: string };

function seedRows(initial: string[], prefix: string): CompareRow[] {
  const values = initial.slice(0, MAX_REPOS).map((s) => s.toLowerCase());
  while (values.length < MIN_ROWS) values.push("");
  return values.map((value, index) => ({ id: `${prefix}-${index}`, value }));
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
  const [rows, setRows] = useState<CompareRow[]>(() => seedRows(seed, idPrefix));
  const [error, setError] = useState<string | null>(null);
  const [active, setActive] = useState<string[] | null>(() => {
    const valid = seed
      .map((s) => s.trim().toLowerCase())
      .filter((s) => SLUG_RE.test(s));
    return valid.length >= MIN_ROWS ? valid.slice(0, MAX_REPOS) : null;
  });
  const pendingFocusId = useRef<string | null>(null);
  const inputRefs = useRef(new Map<string, HTMLInputElement>());

  function setRow(i: number, value: string) {
    setRows((prev) => prev.map((row, idx) => (idx === i ? { ...row, value } : row)));
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
    const cleaned = rows.map((row) => row.value.trim().toLowerCase()).filter(Boolean);
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
  const pairKey = active && active.length === 2 ? [...active].sort().join("/") : null;
  const vsHref =
    active && pairKey && STATIC_PAIRS.has(pairKey)
      ? `/vs/${[...active].sort().join("/")}`
      : null;
  const embedHref = vsHref ?? compareHref;
  const errorId = `${idPrefix}-error`;

  return (
    <div className="space-y-10">
      <form onSubmit={compare} className={cn(PANEL, "space-y-4")}>
        <p className={FIELD}>Repositories to overlay</p>

        <ul role="list" className="space-y-2">
          {rows.map((row, i) => (
            <li key={row.id} className="flex items-center gap-2">
              {/* The row number is the drawing's item mark: it says which
                  subject this is, and it is the only lettering the row needs. */}
              <span className="w-5 shrink-0 text-right font-mono text-[0.75rem] tabular-nums text-ink-3">
                {i + 1}
              </span>
              <div className="flex min-h-11 flex-1 items-center border border-rule-strong bg-paper font-mono text-[0.8125rem] transition-colors duration-[--duration-ui] focus-within:border-ink-3 hover:border-ink-3">
                <label
                  htmlFor={`compare-repo-${row.id}`}
                  className="pl-3 text-ink-3 select-none"
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
                  aria-invalid={error ? true : undefined}
                  aria-describedby={error ? errorId : undefined}
                  className="w-full min-w-0 flex-1 bg-transparent py-2 pr-3 pl-1 text-ink outline-none placeholder:text-ink-3"
                />
              </div>
              <Button
                variant="quiet"
                size="icon"
                onClick={() => removeRow(i)}
                disabled={rows.length <= MIN_ROWS}
                aria-label={`Remove repository ${i + 1}`}
                className="shrink-0 border-0 hover:bg-table"
              >
                <Cut size={15} />
              </Button>
            </li>
          ))}
        </ul>

        {error && (
          <p id={errorId} role="alert" className="text-[0.8125rem] text-signal">
            {error}
          </p>
        )}

        <div className="flex flex-wrap items-center justify-between gap-4">
          <Button
            variant="link"
            onClick={addRow}
            disabled={rows.length >= MAX_REPOS}
          >
            Add repository
          </Button>
          <Button type="submit" variant="primary">
            Draw the overlay
          </Button>
        </div>
      </form>

      {active && chartPath && (
        <div className="space-y-8">
          {vsHref && (
            <ButtonLink href={vsHref} variant="link">
              Open the {active[0]} vs {active[1]} head-to-head sheet
            </ButtonLink>
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
      setData(rows.filter((row): row is ComparisonInitialRepo => row !== null));
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
        <ComparisonSheet
          apiBase={apiBase}
          path={path}
          caption="Star history overlay"
          embedLink={embedLink}
          label={label}
          series={chartSeries}
        />
      ) : (
        <section className={cn(PANEL, "space-y-2")} aria-live="polite">
          <p className="font-draft text-[1.0625rem] leading-[1.2] text-ink">
            {settled ? "Coverage incomplete" : "Reading the series"}
          </p>
          <p className={cn(BODY, MEASURE)}>
            {settled
              ? "Two complete star series are needed to draw the overlay. The metadata and the health readings below are unaffected."
              : "Reading the completed star series for every repository. The overlay is drawn here as soon as they land."}
          </p>
          {!settled && (
            <p className={CAPTION}>
              {repos.length} repositories requested.
            </p>
          )}
        </section>
      )}
      <RepoComparisonMatrix apiBase={apiBase} repos={repos} initial={data} />
    </>
  );
}
