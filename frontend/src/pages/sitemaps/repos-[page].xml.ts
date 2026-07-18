import type { APIRoute } from "astro";
import {
  loadTrackedCatalog,
  type CatalogRepo,
} from "@/lib/build-catalog";

const PER = 5_000;

export async function getStaticPaths() {
  const repos = await loadTrackedCatalog();
  const paths = [];
  for (let page = 0; page * PER < repos.length; page += 1) {
    paths.push({
      params: { page: String(page) },
      props: { repos: repos.slice(page * PER, (page + 1) * PER) },
    });
  }
  return paths;
}

function resolveSite(astroSite: URL | undefined): string {
  const fromEnv = import.meta.env.PUBLIC_SITE_URL as string | undefined;
  const base = astroSite?.href ?? fromEnv ?? "https://gitdebt.com";
  return base.replace(/\/$/, "");
}

function xmlEscape(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&apos;");
}

const REPO_SLUG_RE = /^[A-Za-z0-9._-]+\/[A-Za-z0-9._-]+$/;
const LASTMOD_RE = /^\d{4}-\d{2}-\d{2}(?:T[\d:.+-]+Z?)?$/;

export const GET: APIRoute = async ({ params, props, site }) => {
  const SITE = resolveSite(site);

  const raw = params.page ?? "";
  const page = Number(raw);
  if (!/^\d+$/.test(raw) || !Number.isInteger(page) || page < 0) {
    return new Response("Not found", { status: 404 });
  }

  const repos = (props as { repos?: CatalogRepo[] }).repos ?? [];

  const urls = repos
    .filter((r) => typeof r?.slug === "string" && REPO_SLUG_RE.test(r.slug))
    .map((r) => {
      const loc = `${SITE}/${xmlEscape(r.slug)}`;
      const lastmod =
        typeof r.updatedAt === "string" && LASTMOD_RE.test(r.updatedAt)
          ? xmlEscape(r.updatedAt)
          : null;
      return `  <url>
    <loc>${loc}</loc>${lastmod ? `\n    <lastmod>${lastmod}</lastmod>` : ""}
    <changefreq>daily</changefreq>
  </url>`;
    })
    .join("\n");

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
