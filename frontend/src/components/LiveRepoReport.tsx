import { useMemo } from "react";

import { ChartViewer } from "@/components/ChartViewer";
import { EarnedBadges } from "@/components/EarnedBadges";
import { InteractiveRepoSignals } from "@/components/InteractiveRepoSignals";
import { RepoHero } from "@/components/RepoHero";
import { UsageSection } from "@/components/UsageSection";

const SLUG_RE = /^([A-Za-z0-9._-]+)\/([A-Za-z0-9._-]+)$/;
const RESERVED_FIRST_SEGMENTS = new Set([
  "_astro",
  "404",
  "about",
  "api",
  "badges",
  "compare",
  "leaderboard",
  "privacy",
  "profile",
  "report",
  "sitemaps",
  "terms",
  "u",
  "vs",
]);

function selectedRepo(): { owner: string; repo: string } | null {
  if (typeof window === "undefined") return null;
  const queryRepo = new URLSearchParams(window.location.search).get("repo");
  const pathRepo = window.location.pathname.replace(/^\/+|\/+$/g, "");
  const raw = queryRepo ?? pathRepo;
  const match = raw.trim().match(SLUG_RE);
  if (!match || RESERVED_FIRST_SEGMENTS.has(match[1].toLowerCase()))
    return null;
  return { owner: match[1].toLowerCase(), repo: match[2].toLowerCase() };
}

export function LiveRepoReport({ apiBase }: { apiBase: string }) {
  const selected = useMemo(selectedRepo, []);

  if (!selected) {
    return (
      <div className="border-y border-border py-8">
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
  const embedLink = `https://gitdebt.com/${slug}`;

  return (
    <div className="space-y-14">
      <RepoHero
        owner={owner}
        repo={repo}
        apiBase={apiBase}
        initialData={null}
      />

      <section className="space-y-4">
        <header className="flex flex-wrap items-end justify-between gap-3">
          <div>
            <p className="font-mono text-xs tracking-wide text-muted-foreground uppercase">Earned badges</p>
            <h2 className="mt-1 text-xl font-semibold tracking-tight">Claims backed by repository data</h2>
          </div>
          <p className="max-w-md text-sm text-muted-foreground">Embeddable maintenance, ownership, and star-momentum signals.</p>
        </header>
        <EarnedBadges owner={owner} repo={repo} apiBase={apiBase} embedLink={embedLink} />
      </section>

      <ChartViewer
        apiBase={apiBase}
        path={`/api/repos/${owner}/${repo}/chart.svg`}
        alt={`Star activity history of ${slug}`}
        caption="Cumulative star activity"
        priority
        liveRepo={slug}
        embedLink={embedLink}
        label={slug}
      />

      <section className="space-y-6">
        <header className="max-w-2xl space-y-2">
          <p className="font-mono text-xs tracking-wide text-muted-foreground uppercase">
            Maintenance signals
          </p>
          <h2 className="text-2xl font-semibold tracking-tight text-balance sm:text-3xl">
            What deserves attention first
          </h2>
          <p className="text-base leading-relaxed text-pretty text-muted-foreground">
            Fix concentration, file churn, ownership risk, and maintenance
            cadence are the clearest starting points for understanding this
            codebase.
          </p>
        </header>
        <InteractiveRepoSignals
          apiBase={apiBase}
          slug={slug}
          embedLink={embedLink}
        />
      </section>

      <section className="space-y-6">
        <header className="max-w-2xl space-y-2">
          <p className="font-mono text-xs tracking-wide text-muted-foreground uppercase">
            Adoption
          </p>
          <h2 className="text-2xl font-semibold tracking-tight text-balance sm:text-3xl">
            Attention versus real usage
          </h2>
        </header>
        <UsageSection
          owner={owner}
          repo={repo}
          apiBase={apiBase}
          showEmbed={false}
        />
      </section>

    </div>
  );
}
