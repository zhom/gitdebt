import { CATEGORIES } from "@/data/categories";
import { staticApiBase } from "@/lib/static-api-base";
import { profileLogin } from "@/lib/static-routing.mjs";

export type CatalogRepo = {
  slug: string;
  updatedAt: string | null;
};

type SitemapResponse = {
  total?: number;
  repos?: { slug?: string; updated_at?: string }[];
};

const SLUG_RE = /^[a-z0-9._-]+\/[a-z0-9._-]+$/;
const DEFAULT_LIMIT = 1_000;
const MAX_LIMIT = 8_000;

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

async function fetchCatalog(): Promise<CatalogRepo[]> {
  const limit = catalogLimit();
  const apiBase = staticApiBase();
  const endpoint = `${apiBase}/api/sitemap/repos?page=0&per=${limit}`;
  const curated = curatedRepos();
  const bySlug = new Map(curated.map((repo) => [repo.slug, repo]));

  try {
    const response = await fetch(endpoint, {
      headers: { accept: "application/json" },
      signal: AbortSignal.timeout(10_000),
    });
    if (!response.ok) {
      throw new Error(`backend returned ${response.status}`);
    }
    const body = (await response.json()) as SitemapResponse;
    if (!Array.isArray(body.repos)) {
      throw new Error("backend returned an invalid static catalog");
    }
    for (const row of body.repos) {
      const slug = row.slug?.toLowerCase();
      if (!slug || !SLUG_RE.test(slug)) continue;
      bySlug.set(slug, {
        slug,
        updatedAt:
          typeof row.updated_at === "string" ? row.updated_at : null,
      });
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

  return [...bySlug.values()].sort((a, b) =>
    a.slug.localeCompare(b.slug),
  );
}

export function loadBuildCatalog(): Promise<CatalogRepo[]> {
  catalogPromise ??= fetchCatalog();
  return catalogPromise;
}

export async function staticRepoPaths() {
  const repos = await loadBuildCatalog();
  return repos.map(({ slug, updatedAt }) => {
    const [owner, repo] = slug.split("/");
    return {
      params: { owner, repo },
      props: { updatedAt },
    };
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
