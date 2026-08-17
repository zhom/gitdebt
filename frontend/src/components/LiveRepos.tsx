import { useEffect, useState } from "react";

import { BODY, CAPTION, ROW } from "@/components/style-tokens";

type LiveRepo = { repo: string; owner_login: string };

/**
 * What the list is currently showing. `signed_out` and `empty` are different
 * facts and must read differently: one means "we do not know your
 * repositories", the other means "we do, and there are none".
 */
type Phase =
  | { kind: "loading" }
  | { kind: "signed_out" }
  | { kind: "offline" }
  | { kind: "ready"; repos: LiveRepo[] };

const SLUG_RE = /^[A-Za-z0-9._-]+\/[A-Za-z0-9._-]+$/;

/**
 * The signed-in user's live repositories — the only repositories the landing
 * page lists.
 *
 * Reads `/api/me/repos` and renders exactly what it returns. Nothing writes
 * `repo_star_grants` yet, so today this is an empty list for every account;
 * the empty state says so plainly rather than advertising a way to fill it,
 * because no such flow exists to point at.
 */
export function LiveRepos({ apiBase }: { apiBase: string }) {
  const [phase, setPhase] = useState<Phase>({ kind: "loading" });

  useEffect(() => {
    const controller = new AbortController();
    void (async () => {
      try {
        const response = await fetch(`${apiBase}/api/me/repos`, {
          credentials: "include",
          cache: "no-store",
          signal: controller.signal,
        });
        if (response.status === 401) {
          setPhase({ kind: "signed_out" });
          return;
        }
        if (!response.ok) {
          setPhase({ kind: "offline" });
          return;
        }
        const body: unknown = await response.json();
        const raw =
          body && typeof body === "object" && Array.isArray((body as { repos?: unknown }).repos)
            ? ((body as { repos: unknown[] }).repos as LiveRepo[])
            : [];
        // The slug is interpolated into an href, so it is validated here and
        // not merely trusted because it arrived from our own API.
        setPhase({
          kind: "ready",
          repos: raw.filter(
            (entry) => typeof entry?.repo === "string" && SLUG_RE.test(entry.repo),
          ),
        });
      } catch {
        if (!controller.signal.aborted) setPhase({ kind: "offline" });
      }
    })();
    return () => controller.abort();
  }, [apiBase]);

  if (phase.kind === "loading") {
    return <p className={CAPTION}>Loading your repositories…</p>;
  }

  if (phase.kind === "offline") {
    return <p className={CAPTION}>Your repositories are unavailable right now.</p>;
  }

  if (phase.kind === "signed_out") {
    return (
      <p className={BODY}>
        <a
          className="underline underline-offset-4 hover:text-foreground"
          href={`${apiBase}/auth/github/start?return_to=${encodeURIComponent("/")}`}
        >
          Sign in with GitHub
        </a>{" "}
        to see your live repositories.
      </p>
    );
  }

  if (phase.repos.length === 0) {
    return <p className={BODY}>No live repositories on this account.</p>;
  }

  return (
    <ul role="list" className="mt-3">
      {phase.repos.map((entry) => (
        <li key={entry.repo}>
          <a href={`/${entry.repo}`} className={ROW}>
            <span className="min-w-0 flex-1 truncate">{entry.repo}</span>
          </a>
        </li>
      ))}
    </ul>
  );
}
