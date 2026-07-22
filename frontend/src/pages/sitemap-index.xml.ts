import type { APIRoute } from "astro";
import { loadBuildCatalog } from "@/lib/build-catalog";

const PER = 5_000;

function resolveSite(astroSite: URL | undefined): string {
  const fromEnv = import.meta.env.PUBLIC_SITE_URL as string | undefined;
  const base = astroSite?.href ?? fromEnv ?? "https://gitdebt.com";
  return base.replace(/\/$/, "");
}

export const GET: APIRoute = async ({ site }) => {
  const SITE = resolveSite(site);
  const total = (await loadBuildCatalog()).length;

  const chunkCount = Math.ceil(total / PER);
  const entries: string[] = [];
  for (let i = 0; i < chunkCount; i++) {
    entries.push(`${SITE}/sitemaps/repos-${i}.xml`);
  }
  entries.push(`${SITE}/sitemaps/pages.xml`);

  const body = `<?xml version="1.0" encoding="UTF-8"?>
<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
${entries
  .map(
    (loc) => `  <sitemap>
    <loc>${loc}</loc>
  </sitemap>`,
  )
  .join("\n")}
</sitemapindex>
`;

  return new Response(body, {
    status: 200,
    headers: {
      "Content-Type": "application/xml; charset=utf-8",
    },
  });
};
