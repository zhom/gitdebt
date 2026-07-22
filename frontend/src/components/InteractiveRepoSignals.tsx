import { useEffect, useMemo, useState } from "react";
import { motion, useReducedMotion } from "motion/react";

import { DitherAreaChart, type DitherPoint } from "@/components/DitherAreaChart";
import { EmbedSnippet } from "@/components/EmbedSnippet";
import { StatCard } from "@/components/StatCard";
import { SPRING } from "@/lib/motion";

type FileSignal = { path: string; commits: number; fix_commits: number };
type Author = { label: string; login?: string; avatar_url?: string; commits: number };
type Language = { language: string; files: number; code: number; blank: number; comment: number };
type Day = { date: string; value: number };
type Stats = {
  ready: boolean;
  total_commits: number;
  analyzed_commits: number;
  analysis_scope_commits: number;
  analysis_truncated: boolean;
  bus_factor: number;
  files: FileSignal[];
  authors: Author[];
  commit_days: Day[];
  todo_days: Day[];
  languages: Language[];
};

const FALLBACK = [
  ["bug-magnets", "Bug-magnet files"],
  ["top-files", "Most-changed files"],
  ["bus-factor", "Bus factor"],
  ["commit-trend", "Maintenance pulse"],
  ["contributors", "Contributors"],
  ["lines", "Language activity"],
  ["heatmap", "Commit activity"],
  ["todo-trend", "TODO/FIXME trend"],
] as const;

function compact(value: number): string {
  return new Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 }).format(value);
}

function monthly(days: Day[]): DitherPoint[] {
  const buckets = new Map<string, number>();
  for (const day of days) {
    const month = day.date.slice(0, 7);
    buckets.set(month, (buckets.get(month) ?? 0) + Math.max(0, day.value));
  }
  return [...buckets].map(([month, value]) => ({ date: `${month}-01`, value }));
}

function SignalHeader({
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
    <header className="flex min-h-14 items-center justify-between gap-3 border-b border-border px-5 py-3">
      <h3 className="inline-flex items-center gap-2 font-mono text-xs tracking-wide text-muted-foreground uppercase">
        <span className="size-1.5 bg-foreground" aria-hidden="true" />
        {label}
      </h3>
      <EmbedSnippet
        apiBase={apiBase}
        chartPath={`/api/repos/${slug}/stats/${name}.svg`}
        linkHref={embedLink}
        label={label}
        altText={`${label} for ${slug}`}
        variant="menu"
      />
    </header>
  );
}

function FallbackSignals({ apiBase, slug, embedLink }: Props) {
  return (
    <div className="grid gap-6 sm:grid-cols-2">
      {FALLBACK.map(([name, label]) => (
        <div key={name} className={name === "commit-trend" || name === "contributors" ? "sm:col-span-2" : ""}>
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

type Props = { apiBase: string; slug: string; embedLink: string };

export function InteractiveRepoSignals(props: Props) {
  const { apiBase, slug, embedLink } = props;
  const [stats, setStats] = useState<Stats | null>(null);
  const reducedMotion = useReducedMotion();

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
  if (!stats) return <FallbackSignals {...props} />;

  const totalAuthored = Math.max(1, stats.analyzed_commits);
  const filesByFix = [...stats.files]
    .filter((file) => file.fix_commits > 0)
    .sort((a, b) => b.fix_commits - a.fix_commits)
    .slice(0, 10);
  const filesByChurn = [...stats.files].sort((a, b) => b.commits - a.commits).slice(0, 10);
  const risk = stats.bus_factor <= 1 ? "Solo" : stats.bus_factor === 2 ? "High" : stats.bus_factor === 3 ? "Medium" : "Low";
  const majorAuthors = stats.authors.filter((author) => author.commits / totalAuthored >= 0.01).slice(0, 8);
  const heatmap = stats.commit_days.slice(-364);
  const heatMax = Math.max(1, ...heatmap.map((day) => day.value));

  return (
    <div className="space-y-10">
      <div className="grid gap-6 sm:grid-cols-2">
        <FileBars apiBase={apiBase} slug={slug} embedLink={embedLink} name="bug-magnets" label="Where fixes cluster" rows={filesByFix.map((file) => ({ label: file.path, value: file.fix_commits }))} />
        <FileBars apiBase={apiBase} slug={slug} embedLink={embedLink} name="top-files" label="Files carrying the churn" rows={filesByChurn.map((file) => ({ label: file.path, value: file.commits }))} />

        <section className="border-y border-border bg-card sm:border-x">
          <SignalHeader apiBase={apiBase} slug={slug} embedLink={embedLink} name="bus-factor" label="Ownership concentration" />
          <div className="grid min-h-44 gap-5 px-5 py-5 sm:grid-cols-[10rem_1fr] sm:items-center">
            <div>
              <p className="font-mono text-xs tracking-wide text-muted-foreground uppercase">Risk · factor {stats.bus_factor}</p>
              <p className="mt-1 text-4xl font-semibold tracking-tight">{risk}</p>
              <p className="mt-3 text-xs text-muted-foreground">{compact(stats.total_commits)} total commits · {compact(stats.analyzed_commits)} non-merge commits mapped</p>
            </div>
            <div className="flex flex-wrap items-start gap-2">
              {majorAuthors.map((author) => <AuthorAvatar key={`${author.login}-${author.label}`} author={author} total={totalAuthored} />)}
            </div>
          </div>
        </section>

        <section className="border-y border-border bg-card sm:col-span-2 sm:border-x">
          <SignalHeader apiBase={apiBase} slug={slug} embedLink={embedLink} name="commit-trend" label="Maintenance pulse" />
          <div className="px-2 py-3 sm:px-4">
            <DitherAreaChart points={maintenance} height={360} valueLabel="commits / month" />
          </div>
        </section>
      </div>

      <div>
        <p className="font-mono text-xs tracking-wide text-muted-foreground uppercase">More repository signals</p>
        <h3 className="mt-2 text-xl font-semibold tracking-tight">People, language, cadence, and debt markers</h3>
      </div>

      <div className="grid gap-6 sm:grid-cols-2">
        <section className="border-y border-border bg-card sm:col-span-2 sm:border-x">
          <SignalHeader apiBase={apiBase} slug={slug} embedLink={embedLink} name="contributors" label="Contributors" />
          <div className="flex flex-wrap gap-2 px-5 py-6">
            {stats.authors.slice(0, 24).map((author) => <AuthorAvatar key={`${author.login}-${author.label}`} author={author} total={totalAuthored} large />)}
          </div>
        </section>

        <section className="border-y border-border bg-card sm:border-x">
          <SignalHeader apiBase={apiBase} slug={slug} embedLink={embedLink} name="lines" label="Language activity" />
          <div className="space-y-4 px-5 py-5">
            {stats.languages.map((language) => {
              const total = language.code + language.blank + language.comment;
              const max = Math.max(1, ...stats.languages.map((row) => row.code + row.blank + row.comment));
              return (
                <div key={language.language} title={`${language.files} files · ${total.toLocaleString()} lines`}>
                  <div className="flex items-center justify-between gap-4 font-mono text-xs"><span>{language.language}</span><span className="text-muted-foreground">{compact(total)} lines</span></div>
                  <div className="mt-1.5 h-2 bg-muted"><motion.div initial={false} animate={{ scaleX: total / max }} transition={reducedMotion ? { duration: 0 } : SPRING.snappy} className="signal-dither-fill h-full origin-left bg-foreground" /></div>
                </div>
              );
            })}
          </div>
        </section>

        <section className="border-y border-border bg-card sm:border-x">
          <SignalHeader apiBase={apiBase} slug={slug} embedLink={embedLink} name="heatmap" label="Commit activity" />
          <div className="grid grid-flow-col grid-rows-7 gap-1 overflow-hidden px-5 py-6" aria-label="Last 52 weeks of commit activity">
            {heatmap.map((day) => (
              <span key={day.date} title={`${day.date}: ${day.value} commits`} className="size-2.5 bg-foreground transition-transform hover:scale-150 motion-reduce:transition-none" style={{ opacity: 0.08 + (day.value / heatMax) * 0.92 }} />
            ))}
          </div>
        </section>

        <section className="border-y border-border bg-card sm:col-span-2 sm:border-x">
          <SignalHeader apiBase={apiBase} slug={slug} embedLink={embedLink} name="todo-trend" label="TODO/FIXME trend" />
          <div className="px-2 py-3 sm:px-4">
            <DitherAreaChart points={stats.todo_days} height={280} valueLabel="debt markers" />
          </div>
        </section>
      </div>
    </div>
  );
}

function AuthorAvatar({ author, total, large = false }: { author: Author; total: number; large?: boolean }) {
  const content = (
    <>
      {author.avatar_url ? <img src={author.avatar_url} alt="" loading="lazy" className={`${large ? "size-14" : "size-11"} bg-muted object-cover [image-rendering:auto]`} /> : <span className={`${large ? "size-14" : "size-11"} grid place-items-center bg-muted font-mono font-semibold`}>{author.label.slice(0, 1).toUpperCase()}</span>}
      <span className="min-w-0">
        <span className="block max-w-28 truncate text-xs font-medium">{author.label}</span>
        <span className="block font-mono text-[10px] text-muted-foreground">{author.commits.toLocaleString()} · {(author.commits / total * 100).toFixed(1)}%</span>
      </span>
    </>
  );
  const classes = "group flex items-center gap-2 border border-border bg-background p-1.5 outline-none transition-colors hover:border-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring motion-reduce:transition-none";
  return author.login ? <a href={`https://github.com/${author.login}`} target="_blank" rel="noopener" className={classes}>{content}</a> : <div className={classes}>{content}</div>;
}

function FileBars({ apiBase, slug, embedLink, name, label, rows }: { apiBase: string; slug: string; embedLink: string; name: string; label: string; rows: { label: string; value: number }[] }) {
  const max = Math.max(1, ...rows.map((row) => row.value));
  const reducedMotion = useReducedMotion();
  return (
    <section className="border-y border-border bg-card sm:border-x">
      <SignalHeader apiBase={apiBase} slug={slug} embedLink={embedLink} name={name} label={label} />
      <div className="space-y-3 px-5 py-5">
        {rows.map((row) => (
          <div key={row.label} title={`${row.label}: ${row.value}`}>
            <div className="flex items-center justify-between gap-3 font-mono text-[11px]"><span className="min-w-0 truncate text-muted-foreground">{row.label}</span><span>{row.value}</span></div>
            <div className="mt-1 h-2 bg-muted"><motion.div initial={false} animate={{ scaleX: row.value / max }} transition={reducedMotion ? { duration: 0 } : SPRING.snappy} className="signal-dither-fill h-full origin-left bg-foreground" /></div>
          </div>
        ))}
      </div>
    </section>
  );
}
