export type AgentPage =
  | { kind: "static"; path: string; title: string; description: string }
  | { kind: "repo"; slug: string; updatedAt: string | null }
  | { kind: "profile"; login: string }
  | { kind: "category"; slug: string; name: string; description: string }
  | { kind: "comparison"; first: string; second: string };

function absolute(site: string, path: string): string {
  return new URL(path, site).href;
}

function documentHeader(title: string, canonical: string): string {
  return `# ${title}\n\nCanonical HTML: ${canonical}\n\n`;
}

export function renderAgentMarkdown(
  page: AgentPage,
  site: string,
  apiBase: string,
): string {
  if (page.kind === "repo") {
    const [owner, repo] = page.slug.split("/");
    const canonical = absolute(site, `/${page.slug}`);
    return `${documentHeader(`${page.slug} GitHub repository statistics`, canonical)}> Public-repository analytics from gitdebt. Private repositories are never analyzed or counted.\n\n## Live data\n\n- [Repository on GitHub](https://github.com/${page.slug})\n- [Star history JSON](${apiBase}/api/repos/${owner}/${repo}/stars.json)\n- [Repository-health JSON](${apiBase}/api/repos/${owner}/${repo}/stats.json)\n- [Queue and ETA stream](${apiBase}/api/repos/${owner}/${repo}/progress)\n- [Queue and ETA snapshot](${apiBase}/api/repos/${owner}/${repo}/progress.json)\n\n## Shareable media\n\n- [Star-history SVG](${apiBase}/api/repos/${owner}/${repo}/chart.svg)\n- [Contributors SVG](${apiBase}/api/repos/${owner}/${repo}/stats/contributors.svg)\n- [Maintenance pulse SVG](${apiBase}/api/repos/${owner}/${repo}/stats/commit-trend.svg)\n- [Language activity SVG](${apiBase}/api/repos/${owner}/${repo}/stats/lines.svg)\n- [Commit calendar SVG](${apiBase}/api/repos/${owner}/${repo}/stats/heatmap.svg)\n\n${page.updatedAt ? `Catalog snapshot: ${page.updatedAt}\n\n` : ""}Star history is served from gitdebt's Postgres cache. Code-health reports are calculated asynchronously from the public Git repository.\n`;
  }

  if (page.kind === "profile") {
    const canonical = absolute(site, `/${page.login}`);
    return `${documentHeader(`${page.login} public GitHub profile statistics`, canonical)}> Aggregate statistics for public repositories owned by ${page.login}. Private repositories are ignored.\n\n- [GitHub profile](https://github.com/${page.login})\n- [Live profile analysis](${apiBase}/api/users/${page.login}/analyze)\n- [Aggregate star-history SVG](${apiBase}/api/users/${page.login}/chart.svg)\n- [Maintainer profile card](${apiBase}/api/users/${page.login}/card.svg)\n`;
  }

  if (page.kind === "comparison") {
    const path = `/vs/${page.first}/${page.second}`;
    return `${documentHeader(`${page.first} versus ${page.second}`, absolute(site, path))}> Compare public GitHub star history, growth, and repository-health signals.\n\n- [${page.first} on gitdebt](${absolute(site, `/${page.first}`)})\n- [${page.second} on gitdebt](${absolute(site, `/${page.second}`)})\n- [Live comparison SVG](${apiBase}/api/chart.svg?repos=${encodeURIComponent(`${page.first},${page.second}`)})\n`;
  }

  if (page.kind === "category") {
    return `${documentHeader(`${page.name} GitHub repository comparison`, absolute(site, `/compare/${page.slug}`))}> ${page.description}\n\nThis category compares public repositories only. Open the canonical HTML page for interactive charts and links to each repository report.\n`;
  }

  return `${documentHeader(page.title, absolute(site, `/${page.path}`))}> ${page.description}\n\nGitdebt provides fast GitHub star history, growth, contributor, ownership, language, churn, and maintenance statistics for public repositories.\n\n- [Repository leaderboard](${absolute(site, "/leaderboard")})\n- [Compare repositories](${absolute(site, "/compare")})\n- [API and project information](${absolute(site, "/about")})\n`;
}

export function markdownResponse(body: string, canonical: string): Response {
  return new Response(body, {
    headers: {
      "Cache-Control": "public, max-age=3600, s-maxage=86400",
      "Content-Type": "text/markdown; charset=utf-8",
      Link: `<${canonical}>; rel=\"canonical\"`,
      "X-Robots-Tag": "noindex, follow",
    },
  });
}
