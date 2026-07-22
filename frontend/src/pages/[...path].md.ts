import type { APIRoute } from "astro";
import { CATEGORIES } from "@/data/categories";
import {
  loadBuildCatalog,
  staticComparisonPaths,
} from "@/lib/build-catalog";
import {
  markdownResponse,
  renderAgentMarkdown,
  type AgentPage,
} from "@/lib/agent-markdown";
import { staticApiBase } from "@/lib/static-api-base";

export const prerender = true;

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
  const pages = new Map<string, AgentPage>();

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

  for (const login of new Set(catalog.map(({ slug }) => slug.split("/")[0]))) {
    pages.set(`u/${login}`, { kind: "profile", login });
  }

  for (const category of CATEGORIES) {
    pages.set(`compare/${category.slug}`, {
      kind: "category",
      slug: category.slug,
      name: category.name,
      description: category.short,
    });
  }

  for (const { params } of staticComparisonPaths()) {
    const first = `${params.owner1}/${params.repo1}`;
    const second = `${params.owner2}/${params.repo2}`;
    pages.set(`vs/${first}/${second}`, { kind: "comparison", first, second });
  }

  return [...pages].map(([path, props]) => ({ params: { path }, props }));
}

export const GET: APIRoute = ({ props, site }) => {
  const origin = (site ?? new URL("https://gitdebt.com")).href;
  const page = props as AgentPage;
  const path = page.kind === "repo"
    ? `/${page.slug}`
    : page.kind === "profile"
      ? `/u/${page.login}`
      : page.kind === "category"
        ? `/compare/${page.slug}`
        : page.kind === "comparison"
          ? `/vs/${page.first}/${page.second}`
          : `/${page.path}`;
  const canonical = new URL(path, origin).href;
  return markdownResponse(
    renderAgentMarkdown(page, origin, staticApiBase()),
    canonical,
  );
};
