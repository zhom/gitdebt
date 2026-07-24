import { useEffect, useMemo, useState } from "react";

import { DitherAreaChart, type DitherPoint } from "@/components/DitherAreaChart";
import { DitherMeter } from "@/components/DitherMeter";
import { EmbedSnippet } from "@/components/EmbedSnippet";
import { StatCard } from "@/components/StatCard";
import { languageColor } from "@/components/language-colors";
import {
  CAPTION,
  EYEBROW,
  HEADING,
  KPI,
  PANEL,
} from "@/components/style-tokens";
import { BAYER4, INK, OFF_TIER, SWATCH } from "@/lib/dither";
import { cn } from "@/lib/utils";

/**
 * A heatmap day is one dither cell: the same density-to-alpha rule the charts
 * use, so the section speaks one visual language instead of three.
 */
function heatAlpha(value: number, max: number, index: number): number {
  const density = max > 0 ? value / max : 0;
  const lit = density > BAYER4[index & 3][(index >> 2) & 3];
  const alpha = (0.3 + 0.7 * density) * (lit ? 1 : OFF_TIER);
  return Math.round(alpha * 1000) / 1000;
}

const INK_RGB = `${INK[0]}, ${INK[1]}, ${INK[2]}`;

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
    <header className="flex items-center justify-between gap-3 border-b border-border/40 px-3.5 py-3">
      <h3 className={EYEBROW}>{label}</h3>
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
  const [activeDay, setActiveDay] = useState<Day | null>(null);

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

  const totalAuthored = Math.max(1, stats.attributed_commits ?? stats.analyzed_commits);
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
      <div className="grid gap-x-10 gap-y-14 sm:grid-cols-2">
        <FileBars apiBase={apiBase} slug={slug} embedLink={embedLink} name="bug-magnets" label="Where fixes cluster" rows={filesByFix.map((file) => ({ label: file.path, value: file.fix_commits }))} />
        <FileBars apiBase={apiBase} slug={slug} embedLink={embedLink} name="top-files" label="Files carrying the churn" rows={filesByChurn.map((file) => ({ label: file.path, value: file.commits }))} />

        <section className={PANEL}>
          <SignalHeader apiBase={apiBase} slug={slug} embedLink={embedLink} name="bus-factor" label="Ownership concentration" />
          <div className="grid min-h-44 gap-5 p-3.5 sm:grid-cols-[10rem_1fr] sm:items-center">
            <div>
              <p className={EYEBROW}>Risk · factor {stats.bus_factor}</p>
              <p className={cn(KPI, "mt-2 text-[28px]")}>{risk}</p>
              <p className={cn(CAPTION, "mt-3")}>{compact(stats.total_commits)} total repository commits</p>
            </div>
            <div className="flex items-center overflow-visible pl-3">
              {majorAuthors.map((author, index) => <ContributorAvatar key={`${author.login}-${author.label}`} author={author} index={index} />)}
            </div>
          </div>
        </section>

        <section className={cn(PANEL, "sm:col-span-2")}>
          <SignalHeader apiBase={apiBase} slug={slug} embedLink={embedLink} name="commit-trend" label="Maintenance pulse" />
          <div className="px-2 py-3 sm:px-3.5">
            <DitherAreaChart
              points={maintenance}
              height={360}
              valueLabel="commits / month"
              seed={`${slug}:commit-trend`}
            />
          </div>
        </section>
      </div>

      <h3 className={HEADING}>
        People, language, cadence, and debt markers
      </h3>

      <div className="grid gap-x-10 gap-y-14 sm:grid-cols-2">
        <section className={cn(PANEL, "sm:col-span-2")}>
          <SignalHeader apiBase={apiBase} slug={slug} embedLink={embedLink} name="contributors" label="Contributors" />
          <div className="overflow-x-auto px-3.5 pt-10 pb-7">
            <div className="flex min-w-max items-center pl-3 pr-8">
              {stats.authors.slice(0, 32).map((author, index) => <ContributorAvatar key={`${author.login}-${author.label}`} author={author} index={index} large />)}
            </div>
          </div>
        </section>

        <section className={PANEL}>
          <SignalHeader apiBase={apiBase} slug={slug} embedLink={embedLink} name="lines" label="Language activity" />
          <div className="space-y-4 p-3.5">
            {stats.languages.map((language) => {
              const lines = language.code + language.blank + language.comment;
              const total = lines > 0 ? lines : language.files;
              const max = Math.max(1, ...stats.languages.map((row) => {
                const rowLines = row.code + row.blank + row.comment;
                return rowLines > 0 ? rowLines : row.files;
              }));
              const unit = lines > 0 ? "lines" : language.files === 1 ? "file" : "files";
              return (
                <div key={language.language} title={`${language.files.toLocaleString()} files${lines > 0 ? ` · ${lines.toLocaleString()} lines` : ""}`}>
                  <div className="flex items-center justify-between gap-4 font-mono text-[12px]"><span>{language.language}</span><span className="text-muted-foreground tabular-nums">{compact(total)} {unit}</span></div>
                  <DitherMeter
                    className="mt-1.5"
                    ratio={total / max}
                    fill={languageColor(language.language)}
                  />
                </div>
              );
            })}
          </div>
        </section>

        <section className={PANEL}>
          <SignalHeader apiBase={apiBase} slug={slug} embedLink={embedLink} name="heatmap" label="Commit activity" />
          <div className="p-3.5">
            <p className="mb-4 min-h-5 font-mono text-[11px] text-muted-foreground" aria-live="polite">
              {activeDay ? `${activeDay.date} · ${activeDay.value.toLocaleString()} commit${activeDay.value === 1 ? "" : "s"} · open on GitHub` : "Hover for the exact day. Select a square to inspect its commits."}
            </p>
            <div className="grid grid-flow-col grid-rows-7 gap-1 overflow-x-auto overflow-y-visible py-2" aria-label="Last 52 weeks of commit activity">
              {heatmap.map((day, index) => (
                <a
                  key={day.date}
                  href={`https://github.com/${slug}/commits?since=${day.date}T00%3A00%3A00Z&until=${day.date}T23%3A59%3A59Z`}
                  target="_blank"
                  rel="noopener"
                  aria-label={`${day.date}: ${day.value} commits; open on GitHub`}
                  title={`${day.date}: ${day.value} commits`}
                  onPointerEnter={() => setActiveDay(day)}
                  onFocus={() => setActiveDay(day)}
                  onPointerLeave={() => setActiveDay(null)}
                  onBlur={() => setActiveDay(null)}
                  className="size-2.5 rounded-[1px] outline-none transition-transform duration-200 ease-out hover:z-10 hover:-translate-y-1 hover:scale-150 focus-visible:z-10 focus-visible:-translate-y-1 focus-visible:scale-150 focus-visible:ring-2 focus-visible:ring-accent/30 motion-reduce:transition-none"
                  style={{
                    backgroundColor: `rgba(${INK_RGB}, ${heatAlpha(day.value, heatMax, index)})`,
                  }}
                />
              ))}
            </div>
          </div>
        </section>

        <section className={cn(PANEL, "sm:col-span-2")}>
          <SignalHeader apiBase={apiBase} slug={slug} embedLink={embedLink} name="todo-trend" label="TODO/FIXME trend" />
          <div className="px-2 py-3 sm:px-3.5">
            <DitherAreaChart
              points={stats.todo_days}
              height={280}
              valueLabel="debt markers"
              seed={`${slug}:todo-trend`}
              fill={SWATCH.orange}
            />
          </div>
        </section>
      </div>
    </div>
  );
}

function ContributorAvatar({ author, index, large = false }: { author: Author; index: number; large?: boolean }) {
  const size = large ? "size-16" : "size-13";
  const classes = `${index === 0 ? "ml-0" : large ? "-ml-4" : "-ml-3"} relative block shrink-0 rounded-full border-2 border-background bg-muted outline-none transition-[transform,filter] duration-200 ease-out hover:z-50 hover:-translate-y-2 hover:scale-110 hover:saturate-125 focus-visible:z-50 focus-visible:-translate-y-2 focus-visible:scale-110 focus-visible:ring-2 focus-visible:ring-accent/30 motion-reduce:transition-none ${size}`;
  const content = author.avatar_url
    ? <img src={author.avatar_url} alt="" loading="lazy" className="size-full rounded-full object-cover [image-rendering:auto]" />
    : <span className="grid size-full place-items-center rounded-full bg-muted font-mono font-semibold">{author.label.slice(0, 1).toUpperCase()}</span>;
  return author.login
    ? <a href={`https://github.com/${author.login}`} target="_blank" rel="noopener" className={classes} title={author.label} aria-label={`Open ${author.label} on GitHub`}>{content}</a>
    : <span className={classes} title={author.label}>{content}</span>;
}

function FileBars({ apiBase, slug, embedLink, name, label, rows }: { apiBase: string; slug: string; embedLink: string; name: string; label: string; rows: { label: string; value: number }[] }) {
  const max = Math.max(1, ...rows.map((row) => row.value));
  return (
    <section className={PANEL}>
      <SignalHeader apiBase={apiBase} slug={slug} embedLink={embedLink} name={name} label={label} />
      <div className="space-y-3 p-3.5">
        {rows.map((row) => (
          <div key={row.label} title={`${row.label}: ${row.value}`}>
            <div className="flex items-center justify-between gap-3 font-mono text-[11px]"><span className="min-w-0 truncate text-muted-foreground">{row.label}</span><span className="tabular-nums">{row.value}</span></div>
            <DitherMeter className="mt-1" ratio={row.value / max} />
          </div>
        ))}
      </div>
    </section>
  );
}
