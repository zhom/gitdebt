import { useEffect, useMemo, useState } from "react";
import { ExternalLink, Loader2 } from "lucide-react";

import { ButtonLink } from "@/components/ButtonLink";
import { ChartViewer } from "@/components/ChartViewer";
import { ProfileCardPreview } from "@/components/ProfileCardPreview";
import { StatStrip } from "@/components/StatStrip";
import { BODY, CAPTION, HEADING, PANEL, TITLE } from "@/components/style-tokens";
import { cn } from "@/lib/utils";

type UserAnalyze = {
  login: string;
  repos_included: number;
  repos_pending: number;
  repos_analyzed: number;
  repos_analyzing: number;
  total_stars: number;
  history: { date: string; stars: number }[];
};

const LOGIN_RE = /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,37}[A-Za-z0-9])?$/;
const POLL_MS = 8_000;

function selectedLogin(): string | null {
  if (typeof window === "undefined") return null;
  const value = new URLSearchParams(window.location.search)
    .get("login")
    ?.trim()
    .toLowerCase();
  return value && LOGIN_RE.test(value) ? value : null;
}

export function LiveUserProfile({
  apiBase,
  login: requestedLogin,
}: {
  apiBase: string;
  login?: string;
}) {
  const login = useMemo(() => {
    const normalized = requestedLogin?.trim().toLowerCase();
    if (normalized && LOGIN_RE.test(normalized)) return normalized;
    return selectedLogin();
  }, [requestedLogin]);
  const [data, setData] = useState<UserAnalyze | null>(null);
  const [loading, setLoading] = useState(Boolean(login));
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const targetLogin = login ?? "";
    if (!targetLogin) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let warmAttempted = false;

    async function load() {
      try {
        setLoading(true);
        let response: Response | null = null;
        if (!warmAttempted) {
          warmAttempted = true;
          const warm = await fetch(`${apiBase}/api/users/${targetLogin}/warm`, {
            method: "POST",
            cache: "no-store",
            credentials: "include",
            headers: { accept: "application/json" },
          });
          if (warm.ok) response = warm;
        }
        response ??= await fetch(`${apiBase}/api/users/${targetLogin}/analyze`, {
          cache: "no-store",
          credentials: "omit",
          headers: { accept: "application/json" },
        });
        if (response.status === 404) throw new Error("GitHub user not found.");
        if (!response.ok) throw new Error("Profile data is temporarily unavailable.");
        const payload = (await response.json()) as Partial<UserAnalyze>;
        const next: UserAnalyze = {
          login: payload.login ?? targetLogin,
          repos_included: payload.repos_included ?? 0,
          repos_pending: payload.repos_pending ?? 0,
          repos_analyzed: payload.repos_analyzed ?? 0,
          repos_analyzing: payload.repos_analyzing ?? 0,
          total_stars: payload.total_stars ?? 0,
          history: payload.history ?? [],
        };
        if (cancelled) return;
        setData(next);
        setError(null);
        if (next.repos_pending > 0 || next.repos_analyzing > 0) {
          timer = setTimeout(load, POLL_MS);
        }
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
      <section>
        <h1 className={TITLE}>No GitHub profile selected</h1>
        <p className={cn(BODY, "mt-2")}>
          Sign in from the header to open your profile-wide report.
        </p>
      </section>
    );
  }

  const revision = data
    ? `${data.total_stars}-${data.repos_included}-${data.repos_pending}-${data.repos_analyzed}`
    : "pending";
  const hasHistory = (data?.history.length ?? 0) > 0;

  return (
    <div className="space-y-12">
      <header className="flex flex-col gap-5 sm:flex-row sm:items-end sm:justify-between">
        <div className="space-y-2">
          <h1 className={TITLE}>{login}</h1>
          <p className={cn(BODY, "max-w-[65ch]")}>
            A live aggregate of the public repositories gitdebt currently
            tracks for this GitHub account.
          </p>
        </div>
        <ButtonLink
          href={`https://github.com/${login}`}
          target="_blank"
          rel="noreferrer"
          variant="outline"
          className="shrink-0 self-start sm:self-auto"
        >
          Open GitHub profile
          <ExternalLink className="size-3.5" strokeWidth={1.8} aria-hidden="true" />
        </ButtonLink>
      </header>

      <StatStrip
        columns={3}
        items={[
          {
            label: "Tracked stars",
            value: data ? data.total_stars.toLocaleString() : "—",
          },
          {
            label: "Repos included",
            value: data ? data.repos_included.toLocaleString() : "—",
          },
          {
            label: "Code-health reports",
            value: data ? `${data.repos_analyzed.toLocaleString()} ready` : "—",
          },
        ]}
      />

      {(loading || (data?.repos_pending ?? 0) > 0 || (data?.repos_analyzing ?? 0) > 0) && (
        <div
          className={cn(PANEL, "flex items-start gap-3 p-3.5")}
          role="status"
        >
          <Loader2
            className="mt-0.5 size-4 shrink-0 motion-safe:animate-spin"
            aria-hidden="true"
          />
          <p className={CAPTION}>
            {data?.repos_analyzing
              ? `Analyzing ${data.repos_analyzing} repositories with interactive priority. `
              : "Discovering public repositories. "}
            This page updates every few seconds.
          </p>
        </div>
      )}

      {error && (
        <p className={cn(CAPTION, "font-mono")}>{error}</p>
      )}

      {hasHistory ? (
        <ChartViewer
          apiBase={apiBase}
          path={`/api/users/${login}/chart.svg?v=${revision}`}
          alt={`Aggregate star history across ${login}'s public repositories`}
          caption="Aggregate star history"
          priority
          points={data?.history ?? []}
        />
      ) : (
        data &&
        data.repos_pending === 0 && (
          <section>
            <h2 className={HEADING}>No historical curve is available yet</h2>
            <p className={cn(BODY, "mt-2 max-w-[65ch]")}>
              The public profile and current repository totals can still be
              discovered, but GitHub may not expose the stargazer timestamps
              needed to build a historical aggregate.
            </p>
          </section>
        )
      )}

      <section className="space-y-4">
        <header className="space-y-1">
          <h2 className={HEADING}>Profile card</h2>
          <p className={BODY}>
            A compact maintainer footprint with an activity-based profile title.
          </p>
        </header>
        <div className="flex justify-center py-2">
          <ProfileCardPreview
            apiBase={apiBase}
            login={login}
            initialRevision={revision}
            warm={false}
          />
        </div>
      </section>
    </div>
  );
}
