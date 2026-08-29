import { useEffect, useMemo, useState, type ReactNode } from "react";

import { EmbedSnippet } from "@/components/EmbedSnippet";
import { FileAgeRings } from "@/components/FileAgeRings";
import { FileCouplingNetwork } from "@/components/FileCouplingNetwork";
import { StatCard } from "@/components/StatCard";
import { FieldBlock, FIELD_ROWS } from "@/components/StatStrip";
import { TraceChart, type TracePoint } from "@/components/TraceChart";
import { BODY, CAPTION, FIELD, HEADING, MEASURE } from "@/components/style-tokens";
import type { FileAgeBand, FileCoupling } from "@/lib/repo-signal-visuals";
import { cn } from "@/lib/utils";

/**
 * The repository's own drawings: where changes concentrate, who carries them,
 * and how the cadence moves.
 *
 * Every plot on this sheet is real SVG in the document, with the numbers
 * present as text beside or beneath it. Nothing here is painted into a canvas,
 * which means nothing here disappears when script is throttled, when a
 * screenshot pass runs, or when the reader is a screen reader. That was the
 * single largest defect in what this replaced: a grid of charts that rendered
 * empty boxes without JavaScript.
 *
 * The plots themselves are the house components — `TraceChart` for a measured
 * series, `FileAgeRings` and `FileCouplingNetwork` for the two repo-health
 * drawings — so this file composes the sheet and does not redraw them. What it
 * owns is the ranked bar, which is the one reading the shared set has no
 * primitive for.
 *
 * Drafting red is spent once per plot, on the reading that plot exists to
 * deliver — the busiest file, the dominant language, the largest band. A sheet
 * where everything is highlighted highlights nothing.
 */

type FileSignal = { path: string; commits: number; fix_commits: number };
type Author = { label: string; login?: string; avatar_url?: string; commits: number };
type Language = { language: string; files: number; code: number; blank: number; comment: number };
type Day = { date: string; value: number };
type Stats = {
  ready: boolean;
  total_commits: number;
  analyzed_commits: number;
  attributed_commits?: number;
  analysis_scope_commits: number;
  analysis_truncated: boolean;
  bus_factor: number;
  files: FileSignal[];
  authors: Author[];
  commit_days: Day[];
  todo_days: Day[];
  languages: Language[];
  file_age_bands?: FileAgeBand[];
  file_couplings?: FileCoupling[];
};

const FALLBACK = [
  ["bug-magnets", "Fix-labelled changes"],
  ["top-files", "File change frequency"],
  ["bus-factor", "Bus factor"],
  ["commit-trend", "Maintenance pulse"],
  ["contributors", "Contributors"],
  ["lines", "Language activity"],
  ["heatmap", "Commit activity"],
  ["todo-trend", "Recent TODO/FIXME movement"],
] as const;

function compact(value: number): string {
  return new Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 }).format(value);
}

function monthly(days: Day[]): TracePoint[] {
  const buckets = new Map<string, number>();
  for (const day of days) {
    const month = day.date.slice(0, 7);
    buckets.set(month, (buckets.get(month) ?? 0) + Math.max(0, day.value));
  }
  return [...buckets]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([month, value]) => ({ date: `${month}-01`, value }));
}

/* -------------------------------------------------------------------------- *
 * Sheet furniture
 * -------------------------------------------------------------------------- */

/**
 * One plot on the sheet: a drawn panel whose head names the measured quantity
 * and, where the plot is embeddable, carries the one action that leaves it.
 */
function Plot({
  label,
  action,
  children,
  className,
}: {
  label: string;
  action?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section
      className={cn(
        "cut-edge p-4 [--pad-x:1rem] [--pad-y:1rem]",
        className,
      )}
    >
      <header className="flex min-h-9 items-center justify-between gap-3 border-b border-rule pb-3">
        <h3 className={FIELD}>{label}</h3>
        {action}
      </header>
      <div className="pt-4">{children}</div>
    </section>
  );
}

function EmbedAction({
  apiBase,
  slug,
  embedLink,
  name,
  label,
}: {
  apiBase: string;
  slug: string;
  embedLink: string;
  name: string;
  label: string;
}) {
  return (
    <EmbedSnippet
      apiBase={apiBase}
      chartPath={`/api/repos/${slug}/stats/${name}.svg`}
      linkHref={embedLink}
      label={label}
      altText={`${label} for ${slug}`}
      variant="menu"
    />
  );
}

type BarRow = { label: string; value: number; formatted?: string; hint?: string };

/**
 * A ranked set of readings, each drawn as a measured extent.
 *
 * The hairline behind every bar runs the full width of the set's largest
 * reading, so a bar's length is a comparison against something real rather than
 * against the width of a box, and the tick at its head is the terminator that
 * lands on that row's own value. The largest row takes the drafting red,
 * because that row is the reading the plot delivers.
 *
 * The list is markup, not a picture: each value is text a screen reader reads
 * straight through, so there is no separate table to keep in step with it.
 */
function BarList({ rows }: { rows: BarRow[] }) {
  const max = Math.max(1, ...rows.map((row) => row.value));
  return (
    <ul role="list" className="space-y-3.5">
      {rows.map((row) => {
        const peak = row.value === max;
        const pct = Math.max(0, Math.min(1, row.value / max)) * 100;
        return (
          <li key={row.label} title={row.hint ?? `${row.label}: ${row.value}`}>
            <div className="flex items-baseline justify-between gap-4">
              <span className="min-w-0 truncate font-mono text-[0.75rem] text-ink-2">
                {row.label}
              </span>
              <span
                className={cn(
                  "shrink-0 font-mono text-[0.75rem] tabular-nums",
                  peak ? "text-signal" : "text-ink",
                )}
              >
                {row.formatted ?? row.value.toLocaleString()}
              </span>
            </div>
            <div className="relative mt-1.5 h-2" aria-hidden="true">
              <span className="absolute inset-x-0 top-1/2 h-px -translate-y-1/2 bg-rule" />
              <span
                className={cn(
                  "absolute top-1/2 left-0 h-[2px] -translate-y-1/2",
                  peak ? "bg-signal" : "bg-ink",
                )}
                style={{ width: `${pct}%` }}
              />
              <span
                className={cn(
                  "absolute inset-y-0 w-px",
                  peak ? "bg-signal" : "bg-ink",
                )}
                style={{ left: `max(0px, calc(${pct}% - 1px))` }}
              />
            </div>
          </li>
        );
      })}
    </ul>
  );
}

/**
 * A contributor, as a square print.
 *
 * Nothing lifts and nothing scales — the template's reflex, and banned here:
 * the pointer changes the drawn edge from hairline to graphite, which is the
 * whole affordance. Every print is at least 44px so it is reachable by a thumb,
 * and they sit in a row rather than fanned into an overlapping stack, because
 * a stack hides most of the people it exists to show.
 */
function Contributor({
  author,
  large = false,
}: {
  author: Author;
  large?: boolean;
}) {
  const classes = cn(
    "relative block shrink-0 border border-rule-strong bg-table outline-none transition-colors duration-[--duration-ui] hover:border-ink focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-signal",
    large ? "size-14" : "size-11",
  );
  const title = `${author.label} · ${author.commits.toLocaleString()} commits`;
  const content = author.avatar_url ? (
    <img
      src={author.avatar_url}
      alt=""
      loading="lazy"
      decoding="async"
      className="size-full object-cover"
    />
  ) : (
    <span className="grid size-full place-items-center font-mono text-[0.875rem] text-ink-2">
      {author.label.slice(0, 1).toUpperCase()}
    </span>
  );

  return author.login ? (
    <a
      href={`https://github.com/${author.login}`}
      target="_blank"
      rel="noopener"
      className={classes}
      title={title}
      aria-label={`Open ${author.label} on GitHub`}
    >
      {content}
    </a>
  ) : (
    <span className={classes} title={title}>
      {content}
    </span>
  );
}

/* -------------------------------------------------------------------------- *
 * The sheet
 * -------------------------------------------------------------------------- */

type Props = { apiBase: string; slug: string; embedLink: string };

/**
 * Before `stats.json` lands, the server-rendered plots are the report. They are
 * real images from the API, so this state is a complete sheet rather than a
 * grid of empty boxes waiting for a fetch.
 */
function FallbackSignals({ apiBase, slug, embedLink }: Props) {
  return (
    <div className="grid gap-6 sm:grid-cols-2">
      {FALLBACK.map(([name, label]) => (
        <div
          key={name}
          className={
            name === "commit-trend" || name === "contributors"
              ? "sm:col-span-2"
              : ""
          }
        >
          <StatCard
            src={`${apiBase}/api/repos/${slug}/stats/${name}.svg`}
            alt={`${label} for ${slug}`}
            caption={label}
            apiBase={apiBase}
            embedLink={embedLink}
            priority={true}
            liveRepo={slug}
          />
        </div>
      ))}
    </div>
  );
}

export function InteractiveRepoSignals(props: Props) {
  const { apiBase, slug, embedLink } = props;
  const [stats, setStats] = useState<Stats | null>(null);

  useEffect(() => {
    let active = true;
    let timer = 0;
    const refresh = async () => {
      try {
        const response = await fetch(`${apiBase}/api/repos/${slug}/stats.json`, {
          headers: { accept: "application/json" },
        });
        const body = (await response.json()) as Stats;
        if (!active) return;
        if (response.ok && body.ready) setStats(body);
        else timer = window.setTimeout(refresh, 3_000);
      } catch {
        if (active) timer = window.setTimeout(refresh, 8_000);
      }
    };
    void refresh();
    const onProgress = (event: Event) => {
      const detail = (event as CustomEvent<{ repo?: string; analysis?: { phase?: string } }>).detail;
      if (detail?.repo?.toLowerCase() === slug.toLowerCase() && detail.analysis?.phase === "complete") {
        window.clearTimeout(timer);
        void refresh();
      }
    };
    window.addEventListener("gitdebt:repo-progress", onProgress);
    return () => {
      active = false;
      window.clearTimeout(timer);
      window.removeEventListener("gitdebt:repo-progress", onProgress);
    };
  }, [apiBase, slug]);

  const maintenance = useMemo(() => monthly(stats?.commit_days ?? []), [stats?.commit_days]);
  const todo = useMemo(() => monthly(stats?.todo_days ?? []), [stats?.todo_days]);
  if (!stats) return <FallbackSignals {...props} />;

  const totalAuthored = Math.max(1, stats.attributed_commits ?? stats.analyzed_commits);
  const filesByFix = [...stats.files]
    .filter((file) => file.fix_commits > 0)
    .sort((a, b) => b.fix_commits - a.fix_commits)
    .slice(0, 10);
  const filesByFrequency = [...stats.files].sort((a, b) => b.commits - a.commits).slice(0, 10);
  const risk =
    stats.bus_factor <= 1
      ? "Solo"
      : stats.bus_factor === 2
        ? "High"
        : stats.bus_factor === 3
          ? "Medium"
          : "Low";
  const majorAuthors = stats.authors
    .filter((author) => author.commits / totalAuthored >= 0.01)
    .slice(0, 8);
  const ageBands = stats.file_age_bands ?? [];
  const couplings = stats.file_couplings ?? [];
  const singleNewVisual = (ageBands.length > 0) !== (couplings.length > 0);

  const languageRows: BarRow[] = stats.languages.map((language) => {
    const lines = language.code + language.blank + language.comment;
    const total = lines > 0 ? lines : language.files;
    const unit = lines > 0 ? "lines" : language.files === 1 ? "file" : "files";
    return {
      label: language.language,
      value: total,
      formatted: `${compact(total)} ${unit}`,
      hint: `${language.files.toLocaleString()} files${lines > 0 ? ` · ${lines.toLocaleString()} lines` : ""}`,
    };
  });

  return (
    <div className="space-y-10">
      {stats.analysis_truncated && (
        <p className={cn(BODY, MEASURE)}>
          Bounded analysis window:{" "}
          <span className="measured">
            {compact(stats.analysis_scope_commits)}
          </span>{" "}
          of {compact(stats.total_commits)} repository commits were read. Change
          frequency, fix-labelled changes, contributors, and TODO/FIXME movement
          below describe only that window.
        </p>
      )}

      <div className="grid gap-x-8 gap-y-10 sm:grid-cols-2">
        <Plot
          label="Fix-labelled changes"
          action={
            <EmbedAction
              apiBase={apiBase}
              slug={slug}
              embedLink={embedLink}
              name="bug-magnets"
              label="Fix-labelled changes"
            />
          }
        >
          {filesByFix.length > 0 ? (
            <BarList
              rows={filesByFix.map((file) => ({
                label: file.path,
                value: file.fix_commits,
              }))}
            />
          ) : (
            <p className={CAPTION}>
              No fix-labelled commits in the analyzed window.
            </p>
          )}
        </Plot>

        <Plot
          label="File change frequency"
          action={
            <EmbedAction
              apiBase={apiBase}
              slug={slug}
              embedLink={embedLink}
              name="top-files"
              label="File change frequency"
            />
          }
        >
          <BarList
            rows={filesByFrequency.map((file) => ({
              label: file.path,
              value: file.commits,
            }))}
          />
        </Plot>

        {/* These two draw their own inset, because they are plates rather than
            lists: the negative margin hands them the panel's full width so
            their `p-4` is the one that lands, instead of stacking on top of
            this panel's and pushing a wide plot into a narrow column. It
            cancels exactly, so the drawing still clears the chamfer. */}
        {ageBands.length > 0 && (
          <Plot
            label="File age × change frequency"
            className={singleNewVisual ? "sm:col-span-2" : undefined}
          >
            <FileAgeRings bands={ageBands} className="-m-4" />
          </Plot>
        )}

        {couplings.length > 0 && (
          <Plot
            label="Files that change together"
            className={singleNewVisual ? "sm:col-span-2" : undefined}
          >
            <FileCouplingNetwork
              couplings={couplings}
              seed={`${slug}:file-couplings`}
              className="-m-4"
            />
          </Plot>
        )}

        <Plot
          label="Ownership concentration"
          className="sm:col-span-2"
          action={
            <EmbedAction
              apiBase={apiBase}
              slug={slug}
              embedLink={embedLink}
              name="bus-factor"
              label="Ownership concentration"
            />
          }
        >
          <div className={cn("grid gap-6 sm:grid-cols-[9rem_1fr]", FIELD_ROWS)}>
            <FieldBlock
              label="Risk"
              value={risk}
              caption={
                <>
                  Bus factor{" "}
                  <span className="measured">{stats.bus_factor}</span> ·{" "}
                  {compact(stats.total_commits)} total commits
                </>
              }
            />
            <ul
              role="list"
              className="row-span-3 flex flex-wrap content-start items-center gap-1.5"
            >
              {majorAuthors.map((author) => (
                <li key={`${author.login}-${author.label}`}>
                  <Contributor author={author} />
                </li>
              ))}
            </ul>
          </div>
        </Plot>

        <Plot
          label="Maintenance pulse"
          className="sm:col-span-2"
          action={
            <EmbedAction
              apiBase={apiBase}
              slug={slug}
              embedLink={embedLink}
              name="commit-trend"
              label="Maintenance pulse"
            />
          }
        >
          <TraceChart
            points={maintenance}
            height={300}
            valueLabel="commits / month"
          />
        </Plot>
      </div>

      <h3 className={HEADING}>People, language, cadence, and debt markers</h3>

      <div className="grid gap-x-8 gap-y-10 sm:grid-cols-2">
        <Plot
          label="Contributors"
          className="sm:col-span-2"
          action={
            <EmbedAction
              apiBase={apiBase}
              slug={slug}
              embedLink={embedLink}
              name="contributors"
              label="Contributors"
            />
          }
        >
          <ul
            role="list"
            className="grid grid-cols-[repeat(auto-fill,minmax(5rem,1fr))] gap-x-3 gap-y-5"
          >
            {stats.authors.map((author) => (
              <li
                key={`${author.login}-${author.label}`}
                className="flex min-w-0 flex-col items-center gap-2 text-center"
              >
                <Contributor author={author} large />
                <span
                  className="w-full truncate font-mono text-[0.6875rem] text-ink-3"
                  title={author.label}
                >
                  {author.label}
                </span>
              </li>
            ))}
          </ul>
        </Plot>

        <Plot
          label="Language activity"
          action={
            <EmbedAction
              apiBase={apiBase}
              slug={slug}
              embedLink={embedLink}
              name="lines"
              label="Language activity"
            />
          }
        >
          {languageRows.length > 0 ? (
            <BarList rows={languageRows} />
          ) : (
            <p className={CAPTION}>No languages resolved for this checkout.</p>
          )}
        </Plot>

        <Plot
          label="Recent TODO/FIXME movement"
          action={
            <EmbedAction
              apiBase={apiBase}
              slug={slug}
              embedLink={embedLink}
              name="todo-trend"
              label="Recent TODO/FIXME movement"
            />
          }
        >
          <TraceChart
            points={todo}
            height={260}
            valueLabel="debt markers / month"
          />
        </Plot>

        <div className="sm:col-span-2">
          <StatCard
            src={`${apiBase}/api/repos/${slug}/stats/heatmap.svg`}
            alt={`Commit activity for ${slug}`}
            caption="Commit activity"
            apiBase={apiBase}
            embedLink={embedLink}
            priority={true}
            liveRepo={slug}
          />
          <p className={cn(CAPTION, MEASURE, "mt-2.5")}>
            A rolling 52-week calendar, rendered by the same service that draws
            the README assets.
            {stats.analysis_truncated
              ? " Earlier blank days outside the bounded analysis window are labelled unobserved."
              : ""}
          </p>
        </div>
      </div>
    </div>
  );
}
