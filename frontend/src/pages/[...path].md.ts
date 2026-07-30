import type { APIRoute } from "astro";
import { CATEGORIES } from "@/data/categories";
import {
  loadBuildCatalog,
  staticComparisonPaths,
  staticLogins,
} from "@/lib/build-catalog";
import { loadBuildRepoHealth } from "@/lib/build-repo-health";
import { loadBuildProfileSnapshot } from "@/lib/build-profile-snapshot";
import { loadBuildRepoSnapshot } from "@/lib/build-repo-snapshot";
import { starFacts } from "@/lib/agent-prompt";
import {
  markdownResponse,
  renderAgentMarkdown,
  type AgentPage,
  type RepoFacts,
} from "@/lib/agent-markdown";
import { firstStarYear } from "@/lib/star-insights";
import { staticApiBase } from "@/lib/static-api-base";

export const prerender = true;

/** What `getStaticPaths` hands to `GET`, before any data has been read. */
type PageSeed =
  | { kind: "static"; path: string; title: string; description: string }
  | { kind: "repo"; slug: string; updatedAt: string | null }
  | { kind: "profile"; login: string }
  | { kind: "category"; slug: string; name: string; description: string; repos: string[] }
  | { kind: "comparison"; first: string; second: string };

const STATIC_PAGES = [
  { path: "404", title: "Page not found", description: "Search for a public GitHub repository report on gitdebt." },
  { path: "about", title: "About gitdebt", description: "How gitdebt collects and presents public GitHub repository analytics." },
  { path: "badges", title: "GitHub repository badges", description: "Evidence-backed badges and README media for public GitHub repositories." },
  { path: "compare", title: "Compare GitHub repositories", description: "Compare star history and growth for public GitHub repositories." },
  { path: "leaderboard", title: "GitHub repository leaderboard", description: "Public repositories ranked by stars and recent growth." },
  { path: "privacy", title: "gitdebt privacy policy", description: "The gitdebt privacy and public-data policy." },
  { path: "profile", title: "GitHub profile statistics", description: "Open aggregate statistics for a user's public GitHub repositories." },
  { path: "report", title: "GitHub repository report", description: "Open a live public-repository analysis." },
  { path: "terms", title: "gitdebt terms", description: "Terms for using gitdebt and its public analytics API." },
] as const;

export async function getStaticPaths() {
  const catalog = await loadBuildCatalog();
  const pages = new Map<string, PageSeed>();

  for (const page of STATIC_PAGES) {
    pages.set(page.path, { kind: "static", ...page });
  }

  for (const repo of catalog) {
    if (!pages.has(repo.slug)) {
      pages.set(repo.slug, {
        kind: "repo",
        slug: repo.slug,
        updatedAt: repo.updatedAt,
      });
    }
  }

  // Profiles occupy the root path, so a login can never shadow a static page:
  // `staticLogins` has already dropped every reserved first segment.
  for (const login of await staticLogins()) {
    pages.set(login, { kind: "profile", login });
  }

  for (const category of CATEGORIES) {
    pages.set(`compare/${category.slug}`, {
      kind: "category",
      slug: category.slug,
      name: category.name,
      description: category.short,
      repos: [...category.repos],
    });
  }

  for (const { params } of staticComparisonPaths()) {
    const first = `${params.owner1}/${params.repo1}`;
    const second = `${params.owner2}/${params.repo2}`;
    pages.set(`vs/${first}/${second}`, { kind: "comparison", first, second });
  }

  return [...pages].map(([path, props]) => ({ params: { path }, props }));
}

function routeFor(seed: PageSeed): string {
  switch (seed.kind) {
    case "repo":
      return `/${seed.slug}`;
    case "profile":
      return `/${seed.login}`;
    case "category":
      return `/compare/${seed.slug}`;
    case "comparison":
      return `/vs/${seed.first}/${seed.second}`;
    default:
      return `/${seed.path}`;
  }
}

/**
 * Star facts for one repository, from the analyze snapshot the HTML page has
 * already memoized for this build. The Markdown surface therefore costs the
 * build no extra analyze request.
 */
async function repoFacts(slug: string): Promise<{
  facts: RepoFacts | null;
  notFound: boolean;
}> {
  const snapshot = await loadBuildRepoSnapshot(slug);
  if (snapshot.notFound) return { facts: null, notFound: true };
  const data = snapshot.data;
  if (!data) return { facts: null, notFound: false };
  const approximate =
    data.history_kind === "public_star_actions" || data.history_approximate;
  return {
    notFound: false,
    facts: {
      stars: starFacts(data.history, data.total_stars, approximate),
      history: data.history.map((point) => ({
        date: point.date,
        stars: point.stars,
      })),
      createdAt: data.created_at,
      coverageEnd:
        data.history_coverage_end ??
        data.history[data.history.length - 1]?.date ??
        null,
      eventCount: approximate ? data.history_event_count : null,
    },
  };
}

/** Resolve a seed into the fully-populated page the renderer consumes. */
async function resolve(seed: PageSeed): Promise<AgentPage> {
  if (seed.kind === "repo") {
    const [{ facts, notFound }, health] = await Promise.all([
      repoFacts(seed.slug),
      loadBuildRepoHealth(seed.slug),
    ]);
    return {
      kind: "repo",
      slug: seed.slug,
      updatedAt: seed.updatedAt,
      facts,
      health,
      notFound,
    };
  }

  if (seed.kind === "profile") {
    const { analyze } = await loadBuildProfileSnapshot(seed.login);
    return {
      kind: "profile",
      login: seed.login,
      totalStars: analyze?.total_stars ?? null,
      reposIncluded: analyze?.repos_included ?? null,
      firstYear: analyze ? firstStarYear(analyze.history) : null,
    };
  }

  if (seed.kind === "comparison") {
    const [first, second] = await Promise.all([
      repoFacts(seed.first),
      repoFacts(seed.second),
    ]);
    return {
      kind: "comparison",
      first: seed.first,
      second: seed.second,
      facts: {
        [seed.first]: first.facts,
        [seed.second]: second.facts,
      },
    };
  }

  return seed;
}

export const GET: APIRoute = async ({ props, site }) => {
  const origin = (site ?? new URL("https://gitdebt.com")).href;
  const seed = props as PageSeed;
  const canonical = new URL(routeFor(seed), origin).href;
  return markdownResponse(
    renderAgentMarkdown(await resolve(seed), origin, staticApiBase()),
    canonical,
  );
};
