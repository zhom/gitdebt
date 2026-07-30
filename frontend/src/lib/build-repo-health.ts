import type { RepoHealth } from "@/lib/repo-health";
import { staticApiBase } from "@/lib/static-api-base";

/**
 * Build-time repository-health summary, for the Markdown representation.
 *
 * `health.json` is a Postgres-only read on the image-rate-limit class, so this
 * costs the build one cheap request per repository and never touches GitHub.
 * Unlike the analyze snapshot it is strictly best-effort: an unanalyzed
 * repository answers `202 {ready:false}`, and a miss simply means the Markdown
 * page carries star facts without health readings. It must never fail a build,
 * because health analysis is asynchronous by design and a repository being
 * mid-analysis is a normal state, not an error.
 */
const health = new Map<string, Promise<RepoHealth | null>>();

async function fetchOne(slug: string): Promise<RepoHealth | null> {
  try {
    const response = await fetch(`${staticApiBase()}/api/repos/${slug}/health.json`, {
      headers: { accept: "application/json" },
      signal: AbortSignal.timeout(5_000),
    });
    if (!response.ok) return null;
    const body = (await response.json()) as RepoHealth;
    return body.ready ? body : null;
  } catch {
    return null;
  }
}

/** One shared health request per repository per Astro build. */
export function loadBuildRepoHealth(slug: string): Promise<RepoHealth | null> {
  const key = slug.trim().toLowerCase();
  let pending = health.get(key);
  if (!pending) {
    pending = fetchOne(key);
    health.set(key, pending);
  }
  return pending;
}
