import type { UserAnalyze, UserStats } from "@/components/LiveUserProfile";
import { staticApiBase } from "@/lib/static-api-base";

export type BuildProfileSnapshot = {
  analyze: UserAnalyze | null;
  stats: UserStats | null;
  notFound: boolean;
  error: string | null;
};

/**
 * One shared profile snapshot per login per Astro build.
 *
 * The HTML page and the Markdown representation are separate routes rendering
 * the same account, so without this they would each pay for their own
 * `analyze` + `stats.json` pair. Memoizing here means adding the Markdown
 * surface costs the build nothing.
 *
 * Best-effort throughout: a profile that cannot be read renders as a page
 * without live figures rather than failing the build. Only a 404 is a
 * conclusion, and it is the caller's to act on.
 */
const snapshots = new Map<string, Promise<BuildProfileSnapshot>>();

async function fetchJson<T>(path: string): Promise<T | null> {
  try {
    const response = await fetch(`${staticApiBase()}${path}`, {
      headers: { accept: "application/json" },
      signal: AbortSignal.timeout(5_000),
    });
    return response.ok ? ((await response.json()) as T) : null;
  } catch {
    return null;
  }
}

async function fetchOne(login: string): Promise<BuildProfileSnapshot> {
  let analyze: UserAnalyze | null = null;
  let error: string | null = null;
  try {
    const response = await fetch(
      `${staticApiBase()}/api/users/${login}/analyze?enqueue=0`,
      {
        headers: { accept: "application/json" },
        signal: AbortSignal.timeout(5_000),
      },
    );
    if (response.ok) {
      analyze = (await response.json()) as UserAnalyze;
    } else if (response.status === 404) {
      return { analyze: null, stats: null, notFound: true, error: null };
    } else {
      error = `backend returned ${response.status}`;
    }
  } catch (thrown) {
    error = thrown instanceof Error ? thrown.message : String(thrown);
  }

  // Code signals come from the Postgres-only profile aggregate. A miss is
  // never fatal: the star report still renders without it.
  const stats = analyze
    ? await fetchJson<UserStats>(`/api/users/${login}/stats.json`)
    : null;

  return { analyze, stats, notFound: false, error };
}

export function loadBuildProfileSnapshot(
  login: string,
): Promise<BuildProfileSnapshot> {
  const key = login.trim();
  let snapshot = snapshots.get(key);
  if (!snapshot) {
    snapshot = fetchOne(key);
    snapshots.set(key, snapshot);
  }
  return snapshot;
}
