import { useEffect, useMemo, useState } from "react";

import { DitherAreaChart, type DitherPoint } from "@/components/DitherAreaChart";
import { DitherMeter } from "@/components/DitherMeter";
import { EmbedSnippet } from "@/components/EmbedSnippet";
import { FileAgeRings } from "@/components/FileAgeRings";
import { FileCouplingNetwork } from "@/components/FileCouplingNetwork";
import { StatCard } from "@/components/StatCard";
import { languageColor } from "@/components/language-colors";
import {
  CAPTION,
  EYEBROW,
  HEADING,
  KPI,
  PANEL,
} from "@/components/style-tokens";
import { SWATCH } from "@/lib/dither";
import type {
  FileAgeBand,
  FileCoupling,
} from "@/lib/repo-signal-visuals";
import { cn } from "@/lib/utils";

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
  const filesByFrequency = [...stats.files].sort((a, b) => b.commits - a.commits).slice(0, 10);
  const risk = stats.bus_factor <= 1 ? "Solo" : stats.bus_factor === 2 ? "High" : stats.bus_factor === 3 ? "Medium" : "Low";
  const majorAuthors = stats.authors.filter((author) => author.commits / totalAuthored >= 0.01).slice(0, 8);
  const hasFileAges = (stats.file_age_bands?.length ?? 0) > 0;
  const hasCouplings = (stats.file_couplings?.length ?? 0) > 0;
  const singleNewVisual = hasFileAges !== hasCouplings;

  return (
    <div className="space-y-10">
      {stats.analysis_truncated && (
        <p className="border-l-2 border-accent/70 py-1 pl-3 text-base text-muted-foreground sm:text-sm">
          Bounded analysis window: {compact(stats.analysis_scope_commits)} of{" "}
          {compact(stats.total_commits)} repository commits were read. Change
          frequency, fix-labelled changes, contributors, and TODO/FIXME
          movement below describe only that window.
        </p>
      )}
      <div className="grid gap-x-10 gap-y-14 sm:grid-cols-2">
        <FileBars apiBase={apiBase} slug={slug} embedLink={embedLink} name="bug-magnets" label="Fix-labelled changes" rows={filesByFix.map((file) => ({ label: file.path, value: file.fix_commits }))} />
        <FileBars apiBase={apiBase} slug={slug} embedLink={embedLink} name="top-files" label="File change frequency" rows={filesByFrequency.map((file) => ({ label: file.path, value: file.commits }))} />

        {hasFileAges && (
          <section className={cn(PANEL, singleNewVisual && "sm:col-span-2")}>
            <header className="border-b border-border/40 px-3.5 py-3">
              <h3 className={EYEBROW}>File age × change frequency</h3>
            </header>
            <FileAgeRings bands={stats.file_age_bands ?? []} />
          </section>
        )}

        {hasCouplings && (
          <section className={cn(PANEL, singleNewVisual && "sm:col-span-2")}>
            <header className="border-b border-border/40 px-3.5 py-3">
              <h3 className={EYEBROW}>Files that change together</h3>
            </header>
            <FileCouplingNetwork
              couplings={stats.file_couplings ?? []}
              seed={`${slug}:file-couplings`}
            />
          </section>
        )}

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
          <div className="@container p-3.5">
            <ul
              role="list"
              className="grid grid-cols-[repeat(auto-fit,minmax(4.5rem,1fr))] gap-x-3 gap-y-6"
            >
              {stats.authors.map((author) => (
                <li
                  key={`${author.login}-${author.label}`}
                  className="min-w-0 text-center"
                >
                  <ContributorAvatar
                    author={author}
                    index={0}
                    large
                    overlap={false}
                  />
                  <p
                    className="mt-2 truncate font-mono text-[0.6875rem] text-muted-foreground"
                    title={author.label}
                  >
                    {author.label}
                  </p>
                </li>
              ))}
            </ul>
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
          <p className={cn(CAPTION, "mt-2.5 px-1 [text-wrap:pretty]")}>
            Rolling 52-week calendar using the same dithered activity grammar
            as profile reports.
            {stats.analysis_truncated
              ? " Earlier blank days outside the bounded analysis window are labelled unobserved."
              : ""}
          </p>
        </div>

        <section className={cn(PANEL, "sm:col-span-2")}>
          <SignalHeader apiBase={apiBase} slug={slug} embedLink={embedLink} name="todo-trend" label="Recent TODO/FIXME movement" />
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

function ContributorAvatar({ author, index, large = false, overlap = true }: { author: Author; index: number; large?: boolean; overlap?: boolean }) {
  const size = large ? "size-16" : "size-13";
  const offset = overlap
    ? index === 0
      ? "ml-0"
      : large
        ? "-ml-4"
        : "-ml-3"
    : "mx-auto";
  const classes = `${offset} relative block shrink-0 rounded-full border-2 border-background bg-muted outline-none transition-[transform,filter] duration-200 ease-out hover:z-50 hover:-translate-y-2 hover:scale-110 hover:saturate-125 focus-visible:z-50 focus-visible:-translate-y-2 focus-visible:scale-110 focus-visible:ring-2 focus-visible:ring-accent/30 motion-reduce:transition-none ${size}`;
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
