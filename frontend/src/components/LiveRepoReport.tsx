import { useMemo } from "react";

import { ButtonLink } from "@/components/ButtonLink";
import { BODY, HEADING, TITLE } from "@/components/style-tokens";
import { ChartViewer } from "@/components/ChartViewer";
import { EarnedBadges } from "@/components/EarnedBadges";
import { InteractiveRepoSignals } from "@/components/InteractiveRepoSignals";
import { RepoHero } from "@/components/RepoHero";
import { UsageSection } from "@/components/UsageSection";
import { cn } from "@/lib/utils";

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
      <div>
        <h1 className={TITLE}>Choose a public GitHub repository</h1>
        <p className={cn(BODY, "mt-2")}>
          Use the homepage lookup and enter a repository as owner/repo.
        </p>
        <ButtonLink href="/" variant="primary" className="mt-5">
          Open repo lookup
        </ButtonLink>
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
        <header className="flex items-baseline justify-between gap-4">
          <h2 className={HEADING}>Earned badges</h2>
          <p className="text-[11px] text-muted-foreground">
            Computed from commit and star history
          </p>
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
          <h2 className={HEADING}>What deserves attention first</h2>
          <p className={BODY}>
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
        <header className="max-w-2xl">
          <h2 className={HEADING}>Attention versus real usage</h2>
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
