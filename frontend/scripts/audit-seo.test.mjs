import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, test } from "node:test";
import {
  auditSeo,
  pruneNoindexSitemapUrls,
} from "./audit-seo.mjs";

const temporaryDirectories = [];
const SITE = "https://example.com";

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

function fixture() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "gitdebt-seo-"));
  temporaryDirectories.push(directory);
  return directory;
}

function write(directory, relative, contents) {
  const filePath = path.join(directory, relative);
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, contents);
}

function page({
  route,
  title,
  description,
  robots = "index,follow",
  imageAlt = "Chart",
}) {
  const canonical = `${SITE}${route === "/" ? "/" : route}`;
  return `<!doctype html>
<html lang="en">
  <head>
    <title>${title}</title>
    <meta name="description" content="${description}">
    <meta name="robots" content="${robots}">
    <link rel="canonical" href="${canonical}">
    <link rel="apple-touch-icon" href="/apple-touch-icon.png">
    <meta property="og:type" content="website">
    <meta property="og:title" content="${title}">
    <meta property="og:description" content="${description}">
    <meta property="og:url" content="${canonical}">
    <meta property="og:image" content="${SITE}/og.png">
    <meta name="twitter:card" content="summary_large_image">
    <meta name="twitter:title" content="${title}">
    <meta name="twitter:description" content="${description}">
    <meta name="twitter:image" content="${SITE}/og.png">
    <script type="application/ld+json">{"@context":"https://schema.org"}</script>
  </head>
  <body><h1>${title}</h1><img src="/chart.svg" alt="${imageAlt}"></body>
</html>`;
}

function sitemaps(directory, routes) {
  write(
    directory,
    "robots.txt",
    `User-agent: *\nAllow: /\nSitemap: ${SITE}/sitemap-index.xml\n`,
  );
  write(
    directory,
    "sitemap-index.xml",
    `<?xml version="1.0"?><sitemapindex><sitemap><loc>${SITE}/sitemaps/pages.xml</loc></sitemap></sitemapindex>`,
  );
  write(
    directory,
    "sitemaps/pages.xml",
    `<?xml version="1.0"?><urlset>${routes
      .map((route) => `<url><loc>${SITE}${route}</loc></url>`)
      .join("")}</urlset>`,
  );
}

test("accepts a healthy static site with a nested sitemap index", () => {
  const directory = fixture();
  write(
    directory,
    "index.html",
    page({
      route: "/",
      title: "Home",
      description: "GitHub repository analytics",
    }),
  );
  write(
    directory,
    "about/index.html",
    page({
      route: "/about",
      title: "About",
      description: "About this repository analytics project",
    }),
  );
  sitemaps(directory, ["/", "/about"]);

  const result = auditSeo({ distDir: directory, site: SITE });
  assert.deepEqual(result.errors, []);
  assert.equal(result.pages, 2);
  assert.equal(result.sitemapUrls, 2);
});

test("rejects noindex URLs listed in the sitemap", () => {
  const directory = fixture();
  write(
    directory,
    "report/index.html",
    page({
      route: "/report",
      title: "Report",
      description: "A live report that should not be indexed",
      robots: "noindex,follow",
    }),
  );
  sitemaps(directory, ["/report"]);

  const result = auditSeo({ distDir: directory, site: SITE });
  assert.ok(
    result.errors.includes("Sitemap URL is noindex: /report"),
    result.errors.join("\n"),
  );
});

test("rejects indexable pages omitted from the sitemap", () => {
  const directory = fixture();
  write(
    directory,
    "index.html",
    page({
      route: "/",
      title: "Home",
      description: "GitHub repository analytics",
    }),
  );
  write(
    directory,
    "repo/index.html",
    page({
      route: "/repo",
      title: "Repository",
      description: "Repository analytics and star history",
    }),
  );
  sitemaps(directory, ["/"]);

  const result = auditSeo({ distDir: directory, site: SITE });
  assert.ok(
    result.errors.includes("Indexable pages outside the sitemap: /repo"),
    result.errors.join("\n"),
  );
});

test("prunes noindex URLs before running the strict audit", () => {
  const directory = fixture();
  write(
    directory,
    "index.html",
    page({
      route: "/",
      title: "Home",
      description: "GitHub repository analytics",
    }),
  );
  write(
    directory,
    "report/index.html",
    page({
      route: "/report",
      title: "Report",
      description: "A live report that should not be indexed",
      robots: "noindex,follow",
    }),
  );
  sitemaps(directory, ["/", "/report"]);

  const pruned = pruneNoindexSitemapUrls({
    distDir: directory,
    site: SITE,
  });
  const result = auditSeo({ distDir: directory, site: SITE });

  assert.equal(pruned.pruned, 1);
  assert.deepEqual(result.errors, []);
  assert.equal(result.sitemapUrls, 1);
  assert.doesNotMatch(
    fs.readFileSync(path.join(directory, "sitemaps/pages.xml"), "utf8"),
    /\/report/,
  );
});

test("detects duplicate metadata and missing image alternatives", () => {
  const directory = fixture();
  const first = page({
    route: "/first",
    title: "Duplicate",
    description: "The same description",
  });
  const second = page({
    route: "/second",
    title: "Duplicate",
    description: "The same description",
  }).replace('alt="Chart"', "");
  write(directory, "first/index.html", first);
  write(directory, "second/index.html", second);
  sitemaps(directory, ["/first", "/second"]);

  const result = auditSeo({ distDir: directory, site: SITE });
  assert.ok(result.errors.some((error) => error.startsWith("Duplicate title:")));
  assert.ok(
    result.errors.some((error) =>
      error.startsWith("Duplicate meta description:"),
    ),
  );
  assert.ok(result.errors.includes("/second: image missing alt"));
});
