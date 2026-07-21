import { useMemo, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";

import { ChartViewer } from "@/components/ChartViewer";
import { RepoHero } from "@/components/RepoHero";
import { StatCard } from "@/components/StatCard";
import { UsageSection } from "@/components/UsageSection";
import { DURATION, EASE_OUT, REDUCED_MOTION_DURATION } from "@/lib/motion";

const SLUG_RE = /^([A-Za-z0-9._-]+)\/([A-Za-z0-9._-]+)$/;

const PRIMARY_STATS = [
  ["bug-magnets", "Where fixes cluster"],
  ["top-files", "Files carrying the churn"],
  ["bus-factor", "Knowledge concentration"],
  ["commit-trend", "Maintenance pulse"],
] as const;

const SECONDARY_STATS = [
  ["contributors", "Contributor distribution"],
  ["heatmap", "Commit activity"],
  ["lines", "Language footprint"],
  ["todo-trend", "TODO and FIXME trend"],
] as const;

function selectedRepo(): { owner: string; repo: string } | null {
  if (typeof window === "undefined") return null;
  const raw = new URLSearchParams(window.location.search).get("repo") ?? "";
  const match = raw.trim().match(SLUG_RE);
  if (!match) return null;
  return { owner: match[1].toLowerCase(), repo: match[2].toLowerCase() };
}

export function SecondarySignals({
  apiBase,
  repoBase,
  slug,
  embedLink,
}: {
  apiBase: string;
  repoBase: string;
  slug: string;
  embedLink: string;
}) {
  const [open, setOpen] = useState(false);
  const reduceMotion = useReducedMotion();

  return (
    <div className="border-y border-border">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        className="flex min-h-14 w-full items-center justify-between gap-4 px-5 py-3 text-left text-sm font-medium focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring"
      >
        Four more repository signals
        <span
          className={`font-mono text-lg text-muted-foreground transition-transform duration-150 motion-reduce:transition-none ${open ? "rotate-45" : ""}`}
          aria-hidden="true"
        >
          +
        </span>
      </button>
      <AnimatePresence initial={false}>
        {open && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{
              duration: reduceMotion ? REDUCED_MOTION_DURATION : DURATION.enter,
              ease: EASE_OUT,
            }}
            className="overflow-hidden border-t border-border"
          >
            <div className="grid gap-5 py-5 lg:grid-cols-2">
              {SECONDARY_STATS.map(([name, label]) => (
                <StatCard
                  key={name}
                  src={`${repoBase}/stats/${name}.svg`}
                  alt={`${label} for ${slug}`}
                  caption={label}
                  apiBase={apiBase}
                  embedLink={embedLink}
                  liveRepo={slug}
                />
              ))}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
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
  const repoBase = `${apiBase}/api/repos/${owner}/${repo}`;
  const embedLink = `https://gitdebt.com/${slug}`;

  return (
    <div className="space-y-14">
      <RepoHero
        owner={owner}
        repo={repo}
        apiBase={apiBase}
        initialData={null}
      />

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
        <div className="grid gap-5 lg:grid-cols-2">
          {PRIMARY_STATS.map(([name, label]) => (
            <StatCard
              key={name}
              src={`${repoBase}/stats/${name}.svg`}
              alt={`${label} for ${slug}`}
              caption={label}
              apiBase={apiBase}
              embedLink={embedLink}
              priority
              liveRepo={slug}
            />
          ))}
        </div>
        <SecondarySignals
          apiBase={apiBase}
          repoBase={repoBase}
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
