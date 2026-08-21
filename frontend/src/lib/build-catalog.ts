import { CATEGORIES } from "@/data/categories";
import { staticApiBase } from "@/lib/static-api-base";
import { profileLogin } from "@/lib/static-routing.mjs";

export type CatalogRepo = {
  slug: string;
  /**
   * Freshness of the *catalog row*, and the sitemap's `<lastmod>` — nothing
   * more. It is null for every curated repository the API catalog does not
   * list, and null again for any catalog row whose `updated_at` is not a
   * string, so it must never gate build-time correctness: doing that quietly
   * exempts exactly the repositories the landing, category and comparison
   * pages link to. `[owner]/[repo].astro` gates on `staticCatalogRequired()`
   * alone for that reason, and is handed no props at all.
   */
  updatedAt: string | null;
};

type SitemapResponse = {
  total?: number;
  repos?: { slug?: string; updated_at?: string }[];
};

const SLUG_RE = /^[a-z0-9._-]+\/[a-z0-9._-]+$/;
// Cloudflare applies `_redirects` before it looks for an asset, and `/*.md`
// hands every path ending in `.md` to the API's Markdown renderer. A slug is
// allowed to contain dots, so `owner/manual.md` would get a prerendered page
// and a sitemap entry that nobody can ever reach: the URL 302s to
// `/api/md/owner/manual`, a *different* repository. Such a slug is dropped
// from the catalog rather than published behind a redirect that shadows it.
const REDIRECT_SHADOWED_SLUG = /\.md$/;
const DEFAULT_LIMIT = 3_000;
const MAX_LIMIT = 8_000;
// This build runs on push to main, which is the same event that redeploys the
// backend, so the catalog fetch regularly lands inside a restart window. The
// catalog is still *required* — publishing an accidentally empty one is worse
// than failing — but it should fail because the backend is genuinely gone, not
// because one request caught it mid-rollout.
const CATALOG_ATTEMPTS = 5;
const CATALOG_RETRY_BASE_MS = 3_000;
const CATALOG_TIMEOUT_MS = 15_000;

let catalogPromise: Promise<CatalogRepo[]> | undefined;

function catalogLimit(): number {
  const raw = Number(import.meta.env.STATIC_REPO_LIMIT ?? DEFAULT_LIMIT);
  if (!Number.isFinite(raw)) return DEFAULT_LIMIT;
  return Math.min(MAX_LIMIT, Math.max(1, Math.floor(raw)));
}

export function staticCatalogRequired(): boolean {
  return import.meta.env.STATIC_CATALOG_REQUIRED === "1";
}

function curatedRepos(): CatalogRepo[] {
  const seen = new Set<string>();
  const repos: CatalogRepo[] = [];
  for (const category of CATEGORIES) {
    for (const slug of category.repos) {
      if (!seen.has(slug)) {
        seen.add(slug);
        repos.push({ slug, updatedAt: null });
      }
    }
  }
  return repos;
}

const sleep = (ms: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, ms));

/** A gateway/timeout answer means "not right now"; a 4xx means "never". */
function isTransient(error: unknown): boolean {
  if (error instanceof CatalogHttpError) {
    return error.status === 408 || error.status === 429 || error.status >= 500;
  }
  // Network failures and AbortSignal timeouts land here.
  return true;
}

class CatalogHttpError extends Error {
  constructor(readonly status: number) {
    super(`backend returned ${status}`);
  }
}

async function fetchCatalogOnce(endpoint: string): Promise<SitemapResponse> {
  const response = await fetch(endpoint, {
    headers: { accept: "application/json" },
    signal: AbortSignal.timeout(CATALOG_TIMEOUT_MS),
  });
  if (!response.ok) {
    throw new CatalogHttpError(response.status);
  }
  const body = (await response.json()) as SitemapResponse;
  if (!Array.isArray(body.repos)) {
    throw new Error("backend returned an invalid static catalog");
  }
  return body;
}

async function fetchCatalog(): Promise<CatalogRepo[]> {
  const limit = catalogLimit();
  const apiBase = staticApiBase();
  const endpoint = `${apiBase}/api/sitemap/repos?page=0&per=${limit}`;
  const curated = curatedRepos();
  const bySlug = new Map(curated.map((repo) => [repo.slug, repo]));

  try {
    let body: SitemapResponse | undefined;
    for (let attempt = 0; ; attempt += 1) {
      try {
        body = await fetchCatalogOnce(endpoint);
        break;
      } catch (error) {
        const last = attempt >= CATALOG_ATTEMPTS - 1;
        if (last || !isTransient(error)) throw error;
        const delay = CATALOG_RETRY_BASE_MS * 2 ** attempt;
        console.warn(
          `Static catalog attempt ${attempt + 1}/${CATALOG_ATTEMPTS} failed ` +
            `(${error instanceof Error ? error.message : String(error)}); ` +
            `retrying in ${Math.round(delay / 1000)}s`,
        );
        await sleep(delay);
      }
    }
    for (const row of body.repos ?? []) {
      const slug = row.slug?.toLowerCase();
      if (!slug || !SLUG_RE.test(slug)) continue;
      bySlug.set(slug, {
        slug,
        updatedAt:
          typeof row.updated_at === "string" ? row.updated_at : null,
      });
    }

    const total = typeof body.total === "number" ? body.total : null;
    console.log(
      `Static catalog: ${(body.repos?.length ?? 0).toLocaleString()} of ` +
        `${total?.toLocaleString() ?? "an unreported number of"} published ` +
        `repositories, limit ${limit.toLocaleString()}`,
    );
    // Truncation is silent otherwise: the build succeeds and the missing
    // repositories simply never get a static page or a sitemap entry.
    if (staticCatalogRequired() && total !== null && total > limit) {
      console.warn(
        `Static catalog truncated: ${(total - limit).toLocaleString()} of ` +
          `${total.toLocaleString()} published repositories are not being ` +
          `built as static pages. Raise STATIC_REPO_LIMIT (max ` +
          `${MAX_LIMIT.toLocaleString()}) to include them.`,
      );
    }
  } catch (error) {
    if (staticCatalogRequired()) {
      const detail = error instanceof Error ? error.message : String(error);
      const cause =
        error instanceof Error && error.cause
          ? ` (${String(error.cause)})`
          : "";
      throw new Error(
        `Static catalog refresh failed for ${endpoint}: ${detail}${cause}`,
      );
    }
  }

  const publishable: CatalogRepo[] = [];
  let shadowed = 0;
  for (const repo of bySlug.values()) {
    if (REDIRECT_SHADOWED_SLUG.test(repo.slug)) {
      shadowed += 1;
      continue;
    }
    publishable.push(repo);
  }
  if (shadowed > 0) {
    console.warn(
      `Static catalog: dropped ${shadowed.toLocaleString()} ` +
        `${shadowed === 1 ? "repository" : "repositories"} whose slug ends in ` +
        `".md"; the /*.md redirect shadows their pages, so neither a page nor ` +
        `a sitemap entry is emitted for them`,
    );
  }

  return publishable.sort((a, b) => a.slug.localeCompare(b.slug));
}

export function loadBuildCatalog(): Promise<CatalogRepo[]> {
  catalogPromise ??= fetchCatalog();
  return catalogPromise;
}

export async function staticRepoPaths() {
  const repos = await loadBuildCatalog();
  return repos.map(({ slug }) => {
    const [owner, repo] = slug.split("/");
    // No props on purpose. `updatedAt` used to ride along and became the
    // required-snapshot gate, which meant a curated repository absent from the
    // API catalog was exempt from the one check that keeps an unreadable page
    // out of production. The sitemap reads `updatedAt` from the catalog
    // directly; the page has no business seeing it.
    return { params: { owner, repo } };
  });
}

/**
 * Publishable maintainer logins, sorted. Profiles live at the root path, so a
 * login that collides with a route the application owns is dropped rather
 * than published: `profileLogin` is the single arbiter of that collision.
 */
export async function staticLogins(): Promise<string[]> {
  const repos = await loadBuildCatalog();
  const logins = new Set<string>();
  for (const { slug } of repos) {
    const login = profileLogin(slug.split("/")[0]);
    if (login) logins.add(login);
  }
  return [...logins].sort();
}

export async function staticLoginPaths() {
  const logins = await staticLogins();
  return logins.map((login) => ({ params: { login } }));
}

/** Catalog slugs owned by `login`, capped for display. */
export async function catalogReposFor(
  login: string,
  limit = 24,
): Promise<string[]> {
  const repos = await loadBuildCatalog();
  const prefix = `${login.toLowerCase()}/`;
  return repos
    .map(({ slug }) => slug)
    .filter((slug) => slug.startsWith(prefix))
    .slice(0, limit);
}

export function staticCategoryPaths() {
  return CATEGORIES.map(({ slug }) => ({ params: { category: slug } }));
}

export function staticComparisonPaths() {
  const paths = new Map<
    string,
    {
      params: {
        owner1: string;
        repo1: string;
        owner2: string;
        repo2: string;
      };
    }
  >();

  for (const category of CATEGORIES) {
    for (let i = 0; i < category.repos.length; i += 1) {
      for (let j = i + 1; j < category.repos.length; j += 1) {
        const [first, second] = [category.repos[i], category.repos[j]].sort();
        const [owner1, repo1] = first.split("/");
        const [owner2, repo2] = second.split("/");
        paths.set(`${first}/${second}`, {
          params: { owner1, repo1, owner2, repo2 },
        });
      }
    }
  }

  return [...paths.values()];
}
