import { staticApiBase } from "@/lib/static-api-base";

export type BuildAnalyzeResponse = {
  repo: string;
  total_stars: number;
  created_at: string | null;
  queued: number;
  history_complete: boolean;
  history_kind:
    | "current_stargazers"
    | "public_star_actions"
    | "stargazers_then_activity"
    | "unavailable";
  history_event_count: number;
  history_coverage_start: string | null;
  history_coverage_end: string | null;
  /**
   * Where a `stargazers_then_activity` series changes method; null on every
   * other kind. Optional so an older cached snapshot still type-checks — the
   * copy states the change without naming a day when it is absent, and naming
   * the day is the whole point of the state, so it belongs in the build
   * payload rather than only in the live one.
   */
  history_splice_at?: string | null;
  history_approximate: boolean;
  /**
   * Optional and additive: nothing that already reads this type breaks, and it
   * is what lets a prerendered page classify `restricted` and `exact_frozen`
   * through `historyFreshness()` instead of falling to "unknown" on every
   * build-time surface. Widened to `string` deliberately — the build has no
   * business rejecting a status value the backend adds later.
   */
  history_status?: string;
  pending?: boolean;
  backfilling?: boolean;
  not_found?: boolean;
  history: { date: string; stars: number }[];
};

export type BuildRepoSnapshot = {
  data: BuildAnalyzeResponse | null;
  error: string | null;
  notFound: boolean;
};

const snapshots = new Map<string, Promise<BuildRepoSnapshot>>();
let scheduleTail = Promise.resolve();
let nextStartAt = 0;

function requestIntervalMs(): number {
  const configured = Number(import.meta.env.STATIC_ANALYZE_INTERVAL_MS);
  if (Number.isFinite(configured) && configured >= 0) {
    return Math.min(5_000, Math.floor(configured));
  }
  // The public analyze budget is two starts/second. Production static builds
  // share one egress IP, so pacing unique repo snapshots prevents hundreds of
  // parallel pages from baking 429 responses into permanent HTML.
  return import.meta.env.STATIC_CATALOG_REQUIRED === "1" ? 525 : 0;
}

async function waitForStartSlot() {
  const previous = scheduleTail;
  let release!: () => void;
  scheduleTail = new Promise<void>((resolve) => {
    release = resolve;
  });
  await previous;
  const wait = Math.max(0, nextStartAt - Date.now());
  if (wait > 0) {
    await new Promise((resolve) => setTimeout(resolve, wait));
  }
  nextStartAt = Date.now() + requestIntervalMs();
  release();
}

function retryDelay(response: Response, attempt: number): number {
  const retryAfter = Number(response.headers.get("retry-after"));
  if (Number.isFinite(retryAfter) && retryAfter > 0) {
    return Math.min(30_000, retryAfter * 1_000);
  }
  return Math.min(8_000, 500 * 2 ** attempt);
}

async function fetchOne(slug: string): Promise<BuildRepoSnapshot> {
  const apiBase = staticApiBase();
  const endpoint = `${apiBase}/api/repos/${slug}/analyze?enqueue=0`;
  const required = import.meta.env.STATIC_CATALOG_REQUIRED === "1";
  let lastError = "analysis snapshot unavailable";

  for (let attempt = 0; attempt < 6; attempt += 1) {
    await waitForStartSlot();
    try {
      const response = await fetch(endpoint, {
        headers: { accept: "application/json" },
        signal: AbortSignal.timeout(8_000),
      });
      if (response.ok) {
        const data = (await response.json()) as BuildAnalyzeResponse;
        if (data.not_found) {
          if (required) {
            lastError = "required catalog repository is not public";
            break;
          }
          return { data: null, error: null, notFound: true };
        }
        return { data, error: null, notFound: false };
      }
      if (response.status === 404) {
        if (required) {
          lastError = "required catalog repository is not public";
          break;
        }
        return { data: null, error: null, notFound: true };
      }
      lastError = `backend returned ${response.status}`;
      const transient =
        response.status === 408 ||
        response.status === 425 ||
        response.status === 429 ||
        response.status >= 500;
      if (!transient) break;
      await response.arrayBuffer().catch(() => undefined);
      await new Promise((resolve) =>
        setTimeout(resolve, retryDelay(response, attempt)),
      );
    } catch (error) {
      lastError = error instanceof Error ? error.message : String(error);
      if (attempt < 5) {
        await new Promise((resolve) =>
          setTimeout(resolve, Math.min(8_000, 500 * 2 ** attempt)),
        );
      }
    }
  }

  if (required) {
    throw new Error(`required analysis snapshot failed for ${slug}: ${lastError}`);
  }
  return { data: null, error: lastError, notFound: false };
}

/** One shared, rate-aware snapshot request per repository per Astro build. */
export function loadBuildRepoSnapshot(slug: string): Promise<BuildRepoSnapshot> {
  const key = slug.trim().toLowerCase();
  let snapshot = snapshots.get(key);
  if (!snapshot) {
    snapshot = fetchOne(key);
    snapshots.set(key, snapshot);
  }
  return snapshot;
}
