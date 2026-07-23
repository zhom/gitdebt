import type { APIRoute } from "astro";
import { markdownResponse } from "@/lib/agent-markdown";

export const prerender = true;

export const GET: APIRoute = ({ site }) => {
  const origin = (site ?? new URL("https://gitdebt.com")).href;
  const canonical = new URL("/", origin).href;
  const body = `# gitdebt\n\nCanonical HTML: ${canonical}\n\n> Star history and repository-health analytics for public GitHub repositories.\n\nGitdebt shows star history, growth, contributors, ownership risk, language activity, code churn, bug magnets, maintenance cadence, and README-ready media. Private repositories are never analyzed or counted.\n\n- [Analyze a repository](${new URL("/report", origin).href})\n- [Repository leaderboard](${new URL("/leaderboard", origin).href})\n- [Compare repositories](${new URL("/compare", origin).href})\n- [Badge catalog](${new URL("/badges", origin).href})\n- [About and API behavior](${new URL("/about", origin).href})\n`;
  return markdownResponse(body, canonical);
};
