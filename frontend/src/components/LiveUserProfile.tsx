import { useEffect, useMemo, useState } from "react";
import { ExternalLink, Loader2 } from "lucide-react";

import { ChartViewer } from "@/components/ChartViewer";

type UserAnalyze = {
  login: string;
  repos_included: number;
  repos_pending: number;
  total_stars: number;
  history: { date: string; stars: number }[];
};

const LOGIN_RE = /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,37}[A-Za-z0-9])?$/;
const POLL_MS = 20_000;

function selectedLogin(): string | null {
  if (typeof window === "undefined") return null;
  const value = new URLSearchParams(window.location.search)
    .get("login")
    ?.trim()
    .toLowerCase();
  return value && LOGIN_RE.test(value) ? value : null;
}

export function LiveUserProfile({ apiBase }: { apiBase: string }) {
  const login = useMemo(selectedLogin, []);
  const [data, setData] = useState<UserAnalyze | null>(null);
  const [loading, setLoading] = useState(Boolean(login));
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!login) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;

    async function load() {
      try {
        setLoading(true);
        const response = await fetch(
          `${apiBase}/api/users/${login}/analyze`,
          {
            cache: "no-store",
            credentials: "omit",
            headers: { accept: "application/json" },
          },
        );
        if (response.status === 404) throw new Error("GitHub user not found.");
        if (!response.ok) throw new Error("Profile data is temporarily unavailable.");
        const next = (await response.json()) as UserAnalyze;
        if (cancelled) return;
        setData(next);
        setError(null);
        if (next.repos_pending > 0) timer = setTimeout(load, POLL_MS);
      } catch (reason) {
        if (!cancelled) {
          setError(
            reason instanceof Error
              ? reason.message
              : "Profile data is temporarily unavailable.",
          );
          timer = setTimeout(load, POLL_MS);
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    }

    void load();
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [apiBase, login]);

  if (!login) {
    return (
      <section className="border-y border-border py-8">
        <h1 className="text-2xl font-semibold tracking-tight">
          No GitHub profile selected
        </h1>
        <p className="mt-2 text-base text-muted-foreground">
          Sign in from the header to open your profile-wide report.
        </p>
      </section>
    );
  }

  const revision = data
    ? `${data.total_stars}-${data.repos_included}-${data.repos_pending}`
    : "pending";
  const hasHistory = (data?.history.length ?? 0) > 0;

  return (
    <div className="space-y-12">
      <header className="flex flex-col gap-5 sm:flex-row sm:items-end sm:justify-between">
        <div className="space-y-3">
          <p className="font-mono text-xs tracking-wide text-muted-foreground uppercase">
            Your GitHub profile report
          </p>
          <h1 className="text-3xl font-semibold tracking-tight sm:text-4xl">
            {login}
          </h1>
          <p className="max-w-[65ch] text-lg leading-relaxed text-pretty text-muted-foreground">
            A live aggregate of the public repositories gitdebt currently
            tracks for this GitHub account.
          </p>
        </div>
        <a
          href={`https://github.com/${login}`}
          target="_blank"
          rel="noreferrer"
          className="inline-flex min-h-11 items-center gap-2 self-start rounded-md border border-border px-3 py-2 text-sm text-muted-foreground hover:bg-accent hover:text-foreground sm:self-auto"
        >
          Open GitHub profile
          <ExternalLink className="size-3.5" strokeWidth={1.8} aria-hidden="true" />
        </a>
      </header>

      <dl className="grid border-y border-border sm:grid-cols-3 sm:divide-x sm:divide-border">
        {[
          {
            label: "Tracked stars",
            value: data ? data.total_stars.toLocaleString() : "—",
          },
          {
            label: "Repos included",
            value: data ? data.repos_included.toLocaleString() : "—",
          },
          {
            label: "Repos still warming",
            value: data ? data.repos_pending.toLocaleString() : "—",
          },
        ].map((item) => (
          <div
            key={item.label}
            className="border-t border-border py-4 first:border-t-0 sm:border-t-0 sm:px-5 sm:first:pl-0"
          >
            <dt className="font-mono text-xs tracking-wide text-muted-foreground uppercase">
              {item.label}
            </dt>
            <dd className="mt-1.5 text-xl font-semibold tabular-nums">
              {item.value}
            </dd>
          </div>
        ))}
      </dl>

      {(loading || (data?.repos_pending ?? 0) > 0) && (
        <div
          className="flex items-start gap-3 border-y border-border py-4"
          role="status"
        >
          <Loader2
            className="mt-0.5 size-4 shrink-0 motion-safe:animate-spin text-signal"
            aria-hidden="true"
          />
          <p className="text-sm leading-relaxed text-muted-foreground">
            Discovering public repositories and warming their saved history.
            This page updates automatically.
          </p>
        </div>
      )}

      {error && (
        <p className="border-y border-border py-4 text-sm">
          {error}
        </p>
      )}

      {hasHistory ? (
        <ChartViewer
          apiBase={apiBase}
          path={`/api/users/${login}/chart.svg?v=${revision}`}
          alt={`Aggregate star history across ${login}'s public repositories`}
          caption="Aggregate star history"
          priority
        />
      ) : (
        data &&
        data.repos_pending === 0 && (
          <section className="border-y border-border py-8">
            <h2 className="text-xl font-semibold tracking-tight">
              No historical curve is available yet
            </h2>
            <p className="mt-2 max-w-[65ch] text-base leading-relaxed text-pretty text-muted-foreground">
              The public profile and current repository totals can still be
              discovered, but GitHub may not expose the stargazer timestamps
              needed to build a historical aggregate.
            </p>
          </section>
        )
      )}

      <section className="space-y-4">
        <header className="space-y-1">
          <h2 className="text-xl font-semibold tracking-tight">
            Profile card
          </h2>
          <p className="text-base text-muted-foreground">
            A compact lower-bound summary over repositories gitdebt has tracked.
          </p>
        </header>
        <div className="flex justify-center border-y border-border py-5">
          <img
            src={`${apiBase}/api/users/${login}/card.svg?theme=light&v=${revision}`}
            alt={`gitdebt profile statistics for ${login}`}
            loading="lazy"
            decoding="async"
          />
        </div>
      </section>
    </div>
  );
}
