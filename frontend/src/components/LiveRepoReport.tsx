import { useEffect, useMemo } from "react";

import { BadgeStudio } from "@/components/BadgeStudio";
import { ChartViewer } from "@/components/ChartViewer";
import { RepoHero } from "@/components/RepoHero";
import { StatCard } from "@/components/StatCard";
import { UsageSection } from "@/components/UsageSection";

const SLUG_RE = /^([A-Za-z0-9._-]+)\/([A-Za-z0-9._-]+)$/;

const STATS = [
  ["bug-magnets", "Bug-magnet files"],
  ["top-files", "Most-changed files"],
  ["contributors", "Contributors"],
  ["heatmap", "Commit activity"],
] as const;

function selectedRepo(): { owner: string; repo: string } | null {
  if (typeof window === "undefined") return null;
  const raw = new URLSearchParams(window.location.search).get("repo") ?? "";
  const match = raw.trim().match(SLUG_RE);
  if (!match) return null;
  return { owner: match[1].toLowerCase(), repo: match[2].toLowerCase() };
}

export function LiveRepoReport({ apiBase }: { apiBase: string }) {
  const selected = useMemo(selectedRepo, []);

  useEffect(() => {
    if (!selected) return;
    const controller = new AbortController();
    void fetch(
      `${apiBase}/api/repos/${selected.owner}/${selected.repo}/analyze-history`,
      {
        method: "POST",
        credentials: "omit",
        signal: controller.signal,
      },
    ).catch(() => undefined);
    return () => controller.abort();
  }, [apiBase, selected]);

  if (!selected) {
    return (
      <div className="card-panel p-6">
        <h1 className="text-2xl font-semibold tracking-tight">
          Choose a public GitHub repository
        </h1>
        <p className="mt-2 text-muted-foreground">
          Use the homepage lookup and enter a repository as owner/repo.
        </p>
        <a
          href="/"
          className="mt-5 inline-flex min-h-11 items-center rounded-md bg-primary px-4 py-2 font-medium text-primary-foreground"
        >
          Open repo lookup
        </a>
      </div>
    );
  }

  const { owner, repo } = selected;
  const slug = `${owner}/${repo}`;
  const repoBase = `${apiBase}/api/repos/${owner}/${repo}`;

  return (
    <div className="space-y-12">
      <RepoHero
        owner={owner}
        repo={repo}
        apiBase={apiBase}
        initialData={null}
      />

      <ChartViewer
        apiBase={apiBase}
        path={`/api/repos/${owner}/${repo}/chart.svg`}
        alt={`Star history of ${slug}`}
        caption="Cumulative star history"
        embedLink={`https://gitdebt.com/${slug}`}
        label={slug}
      />

      <section className="space-y-5">
        <header className="space-y-1">
          <p className="font-mono text-xs tracking-wide text-muted-foreground uppercase">
            Repository health
          </p>
          <h2 className="text-2xl font-semibold tracking-tight">
            Where the maintenance cost lives
          </h2>
        </header>
        <div className="grid gap-5 lg:grid-cols-2">
          {STATS.map(([name, label]) => (
            <StatCard
              key={name}
              src={`${repoBase}/stats/${name}.svg`}
              alt={`${label} for ${slug}`}
              caption={label}
              embedLink={`https://gitdebt.com/${slug}`}
            />
          ))}
        </div>
      </section>

      <UsageSection owner={owner} repo={repo} apiBase={apiBase} />

      <section className="space-y-5">
        <header className="space-y-1">
          <p className="font-mono text-xs tracking-wide text-muted-foreground uppercase">
            README badge
          </p>
          <h2 className="text-2xl font-semibold tracking-tight">
            Make the signal shareable
          </h2>
        </header>
        <BadgeStudio owner={owner} repo={repo} apiBase={apiBase} />
      </section>
    </div>
  );
}
