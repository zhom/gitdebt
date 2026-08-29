import { useEffect, useMemo } from "react";

import { ButtonLink } from "@/components/ButtonLink";
import { ChartViewer } from "@/components/ChartViewer";
import { EarnedBadges } from "@/components/EarnedBadges";
import { InteractiveRepoSignals } from "@/components/InteractiveRepoSignals";
import { LiveUserProfile } from "@/components/LiveUserProfile";
import { RepoHero } from "@/components/RepoHero";
import {
  BODY,
  CAPTION,
  HEADING,
  LEAD,
  MEASURE,
  SECTION_HEADER,
  TITLE,
} from "@/components/style-tokens";
import { UsageSection } from "@/components/UsageSection";
import { publishLiveSubject } from "@/lib/live-subject";
import { restoreServedTitle } from "@/lib/live-title";
import {
  liveReportRepo,
  missingProfileReportTarget,
} from "@/lib/static-routing.mjs";
import { cn } from "@/lib/utils";

/**
 * The report for a repository the build never prerendered.
 *
 * Two routes mount this — `/report`, where the reader asked for a repository
 * by name, and `/404`, where the host had no file for `/{owner}/{repo}` and
 * this island recognises the path and renders the real thing in place. Both
 * arrive with a served `<title>` that describes neither: "Live GitHub
 * repository report" and "Page not found". Correcting that is what
 * `useResolvedTitle` below is for.
 */

function selectedRepo(): { owner: string; repo: string } | null {
  if (typeof window === "undefined") return null;
  return liveReportRepo(window.location.pathname, window.location.search);
}

function selectedProfile(): string | null {
  if (typeof window === "undefined") return null;
  return missingProfileReportTarget(window.location.pathname)?.slice(1) ?? null;
}

/** The fields of `/api/repos/:owner/:repo/analyze` this file actually reads. */
type ResolveProbe = {
  total_stars?: number;
  created_at?: string | null;
  history?: unknown[];
  history_status?: string;
  not_found?: boolean;
};

/**
 * Attempts, in milliseconds from mount. A cold repository has no cached row at
 * all, so the first read cannot confirm it exists; the schedule waits for the
 * metadata fetch that `RepoHero`'s own (enqueueing) read triggers, then gives
 * up. It is bounded on purpose: a tab title is not worth an unbounded poll.
 */
const RESOLVE_SCHEDULE = [0, 2_000, 5_000, 10_000, 20_000, 40_000] as const;

/**
 * Correct the tab once — and only once — the API confirms the repository is
 * real.
 *
 * The probe is the read-only variant of the same endpoint `RepoHero` polls, so
 * it adds no queue work and no GitHub calls; it exists solely to decide whether
 * the subject in the address bar is a repository that actually resolved.
 * Retitling on the slug alone would put "owner/typo" in the tab, the bookmark
 * and the history entry for a repository GitHub has never heard of, which is a
 * worse lie than the generic title it replaced.
 */
function useResolvedTitle(apiBase: string, slug: string | null) {
  useEffect(() => {
    if (!slug) return;
    const target = slug;
    const controller = new AbortController();
    const timers: ReturnType<typeof setTimeout>[] = [];
    let settled = false;
    let inFlight = false;

    const stop = () => {
      settled = true;
      for (const timer of timers) clearTimeout(timer);
      controller.abort();
    };

    async function probe() {
      if (settled || inFlight) return;
      inFlight = true;
      let payload: ResolveProbe | null = null;
      try {
        const response = await fetch(
          `${apiBase}/api/repos/${target}/analyze?enqueue=0`,
          {
            cache: "no-store",
            credentials: "omit",
            headers: { accept: "application/json" },
            signal: controller.signal,
          },
        );
        payload = response.ok ? ((await response.json()) as ResolveProbe) : null;
      } catch {
        return; // A failed probe leaves the served title alone, which is honest.
      } finally {
        inFlight = false;
      }
      if (!payload || settled) return;

      // GitHub told us this is not a public repository. The report itself says
      // so; the tab must stop implying otherwise.
      if (payload.not_found === true || payload.history_status === "not_public") {
        restoreServedTitle();
        stop();
        return;
      }

      // Evidence that GitHub answered for this slug: a creation date, a star
      // count, or a stored series. Absence of all three means "not read yet",
      // not "does not exist", so the schedule tries again.
      const resolved =
        typeof payload.created_at === "string" ||
        (payload.total_stars ?? 0) > 0 ||
        (payload.history?.length ?? 0) > 0;
      if (!resolved) return;

      publishLiveSubject({
        subject: target,
        description: `Star history and repository health for ${target}: commit cadence, ownership concentration, repair load and debt markers, read from its own commit history.`,
        path: `/${target}`,
        image: `${apiBase}/api/repos/${target}/og.png`,
      });
      stop();
    }

    for (const delay of RESOLVE_SCHEDULE) {
      timers.push(setTimeout(() => void probe(), delay));
    }
    return stop;
  }, [apiBase, slug]);
}

export function LiveRepoReport({ apiBase }: { apiBase: string }) {
  const selected = useMemo(selectedRepo, []);
  const profile = useMemo(selectedProfile, []);
  const slug = selected ? `${selected.owner}/${selected.repo}` : null;

  useResolvedTitle(apiBase, slug);

  // A single missing segment is a maintainer, not a repository: the whole
  // point of profiles at the root is that github.com/<name> rewrites here.
  // `LiveUserProfile` corrects the tab for that case itself.
  if (!selected && profile) {
    return <LiveUserProfile apiBase={apiBase} login={profile} />;
  }

  if (!selected || !slug) {
    return (
      <div>
        <h1 className={TITLE}>Name a public repository</h1>
        <p className={cn(LEAD, MEASURE, "mt-4")}>
          Every report is a drawing of one repository. Enter it as owner/repo on
          the home page, or open gitdebt.com followed by the same path you would
          use on GitHub.
        </p>
        <ButtonLink href="/#lookup" variant="primary" className="mt-8">
          Open the lookup
        </ButtonLink>
      </div>
    );
  }

  const { owner, repo } = selected;
  const embedLink = `https://gitdebt.com/${slug}`;

  return (
    <div className="space-y-16">
      <RepoHero owner={owner} repo={repo} apiBase={apiBase} initialData={null} />

      <section>
        <div className={SECTION_HEADER}>
          <h2 className={HEADING}>Earned badges</h2>
          <p className={CAPTION}>Computed from commit and star history</p>
        </div>
        <div className="mt-6">
          <EarnedBadges
            owner={owner}
            repo={repo}
            apiBase={apiBase}
            embedLink={embedLink}
          />
        </div>
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

      <section>
        <h2 className={HEADING}>What deserves attention first</h2>
        <p className={cn(BODY, MEASURE, "mt-3")}>
          Fix-labelled changes, file change frequency, ownership risk and
          maintenance cadence are the clearest starting points for understanding
          this codebase.
        </p>
        <div className="mt-6">
          <InteractiveRepoSignals
            apiBase={apiBase}
            slug={slug}
            embedLink={embedLink}
          />
        </div>
      </section>

      <section>
        <h2 className={HEADING}>Attention versus real usage</h2>
        <div className="mt-6">
          <UsageSection
            owner={owner}
            repo={repo}
            apiBase={apiBase}
            showEmbed={false}
          />
        </div>
      </section>
    </div>
  );
}
