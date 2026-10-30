import type { APIRoute } from "astro";
import { CATEGORIES } from "@/data/categories";
import { postDay, publishedPosts } from "@/lib/blog";
import { allGuides } from "@/lib/guides";
import {
  staticComparisonPaths,
  staticLogins,
} from "@/lib/build-catalog";

// All emitted profile routes enter this build-time sitemap. The strict
// post-build SEO pass removes any profile whose rendered page is `noindex`, so
// the final sitemap exactly matches the indexable output.
export const prerender = true;

function resolveSite(astroSite: URL | undefined): string {
  const fromEnv = import.meta.env.PUBLIC_SITE_URL as string | undefined;
  const base = astroSite?.href ?? fromEnv ?? "https://gitdebt.com";
  return base.replace(/\/$/, "");
}

type SitemapPage = {
  path: string;
  changefreq: string;
  priority: string;
  lastmod?: string;
};

const PAGES: SitemapPage[] = [
  { path: "/", changefreq: "weekly", priority: "1.0" },
  { path: "/compare", changefreq: "weekly", priority: "0.8" },
  { path: "/leaderboard", changefreq: "daily", priority: "0.8" },
  { path: "/badges", changefreq: "weekly", priority: "0.8" },
  // Listed even while the blog is empty: the empty index is `noindex`, and the
  // post-build SEO pass prunes it from the sitemap until a post exists.
  { path: "/blog", changefreq: "weekly", priority: "0.6" },
  { path: "/about", changefreq: "monthly", priority: "0.5" },
  { path: "/privacy", changefreq: "yearly", priority: "0.3" },
  { path: "/terms", changefreq: "yearly", priority: "0.3" },
  ...CATEGORIES.map((c) => ({
    path: `/compare/${c.slug}`,
    changefreq: "weekly",
    priority: "0.7",
  })),
  ...staticComparisonPaths().map(({ params }) => ({
    path: `/vs/${params.owner1}/${params.repo1}/${params.owner2}/${params.repo2}`,
    changefreq: "weekly",
    priority: "0.6",
  })),
];

export const GET: APIRoute = async ({ site }) => {
  const SITE = resolveSite(site);
  const loginPages = (await staticLogins()).map((login) => ({
    path: `/${login}`,
    changefreq: "daily",
    priority: "0.6",
  }));
  // Drafts have no page, so they have no entry either.
  const postPages: SitemapPage[] = (await publishedPosts()).map((post) => ({
    path: `/blog/${post.id}`,
    changefreq: "monthly",
    priority: "0.6",
    lastmod: postDay(post.data.updatedDate ?? post.data.publishDate),
  }));
  const guides = await allGuides();
  const guidePages: SitemapPage[] = guides.map((guide) => ({
    path: `/guides/${guide.id}`,
    changefreq: "monthly",
    priority: "0.7",
    lastmod: postDay(guide.data.updatedDate),
  }));
  const guideIndex: SitemapPage = {
    path: "/guides",
    changefreq: "monthly",
    priority: "0.7",
    lastmod: guidePages.map((page) => page.lastmod ?? "").sort().at(-1),
  };
  const pages: SitemapPage[] = [
    ...PAGES,
    guideIndex,
    ...guidePages,
    ...postPages,
    ...loginPages,
  ];

  const urls = pages.map(
    (p) => `  <url>
    <loc>${SITE}${p.path}</loc>${p.lastmod ? `\n    <lastmod>${p.lastmod}</lastmod>` : ""}
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
