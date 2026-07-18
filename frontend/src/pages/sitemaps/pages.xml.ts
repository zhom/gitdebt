import type { APIRoute } from "astro";
import { CATEGORIES } from "@/data/categories";

// User pages are discovered organically because cold profiles are noindex.
export const prerender = true;

function resolveSite(astroSite: URL | undefined): string {
  const fromEnv = import.meta.env.PUBLIC_SITE_URL as string | undefined;
  const base = astroSite?.href ?? fromEnv ?? "https://gitdebt.com";
  return base.replace(/\/$/, "");
}

const PAGES: { path: string; changefreq: string; priority: string }[] = [
  { path: "/", changefreq: "weekly", priority: "1.0" },
  { path: "/compare", changefreq: "weekly", priority: "0.8" },
  { path: "/leaderboard", changefreq: "daily", priority: "0.8" },
  { path: "/badges", changefreq: "weekly", priority: "0.8" },
  { path: "/about", changefreq: "monthly", priority: "0.5" },
  { path: "/privacy", changefreq: "yearly", priority: "0.3" },
  { path: "/terms", changefreq: "yearly", priority: "0.3" },
  ...CATEGORIES.map((c) => ({
    path: `/compare/${c.slug}`,
    changefreq: "weekly",
    priority: "0.7",
  })),
];

export const GET: APIRoute = async ({ site }) => {
  const SITE = resolveSite(site);

  const urls = PAGES.map(
    (p) => `  <url>
    <loc>${SITE}${p.path}</loc>
    <changefreq>${p.changefreq}</changefreq>
    <priority>${p.priority}</priority>
  </url>`,
  ).join("\n");

  const body = `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
${urls}
</urlset>
`;

  return new Response(body, {
    status: 200,
    headers: {
      "Content-Type": "application/xml; charset=utf-8",
    },
  });
};
