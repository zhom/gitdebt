#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const DEFAULT_SITE = "https://gitdebt.com";

function walkFiles(directory, predicate) {
  if (!fs.existsSync(directory)) return [];

  const files = [];
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...walkFiles(entryPath, predicate));
    } else if (predicate(entryPath)) {
      files.push(entryPath);
    }
  }
  return files;
}

function decodeXml(value) {
  return value
    .replaceAll("&amp;", "&")
    .replaceAll("&lt;", "<")
    .replaceAll("&gt;", ">")
    .replaceAll("&quot;", '"')
    .replaceAll("&apos;", "'");
}

function attribute(tag, name) {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = tag.match(
    new RegExp(
      `(?:^|\\s)${escaped}\\s*=\\s*(?:"([^"]*)"|'([^']*)'|([^\\s>]+))`,
      "i",
    ),
  );
  return match ? decodeXml(match[1] ?? match[2] ?? match[3] ?? "") : null;
}

function tags(html, tagName) {
  const escaped = tagName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return [...html.matchAll(new RegExp(`<${escaped}\\b[^>]*>`, "gi"))].map(
    (match) => match[0],
  );
}

function metaContent(html, key, value) {
  const target = value.toLowerCase();
  for (const tag of tags(html, "meta")) {
    if (attribute(tag, key)?.toLowerCase() === target) {
      return attribute(tag, "content");
    }
  }
  return null;
}

function linkHref(html, relation) {
  const target = relation.toLowerCase();
  for (const tag of tags(html, "link")) {
    const relations = (attribute(tag, "rel") ?? "")
      .toLowerCase()
      .split(/\s+/);
    if (relations.includes(target)) return attribute(tag, "href");
  }
  return null;
}

function elementText(html, tagName) {
  const escaped = tagName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return (
    html
      .match(new RegExp(`<${escaped}\\b[^>]*>([\\s\\S]*?)</${escaped}>`, "i"))
      ?.[1]?.replace(/<[^>]+>/g, "")
      .trim() ?? null
  );
}

function routeForHtml(distDir, filePath) {
  const relative = path.relative(distDir, filePath).split(path.sep).join("/");
  if (relative === "index.html") return "/";
  if (relative.endsWith("/index.html")) {
    return `/${relative.slice(0, -"/index.html".length)}`;
  }
  return `/${relative.slice(0, -".html".length)}`;
}

function normalizePathname(value) {
  if (value === "/") return "/";
  return value.replace(/\/+$/, "");
}

function htmlFileForUrl(distDir, url) {
  const decodedPath = decodeURIComponent(url.pathname);
  const relative = decodedPath.replace(/^\/+/, "").replace(/\/+$/, "");
  if (
    relative
      .split("/")
      .some((segment) => segment === ".." || segment === ".")
  ) {
    return null;
  }

  const candidates =
    relative.length === 0
      ? [path.join(distDir, "index.html")]
      : [
          path.join(distDir, relative, "index.html"),
          path.join(distDir, `${relative}.html`),
        ];
  return candidates.find((candidate) => fs.existsSync(candidate)) ?? null;
}

function xmlLocs(xml) {
  return [...xml.matchAll(/<loc>\s*([\s\S]*?)\s*<\/loc>/gi)].map((match) =>
    decodeXml(match[1].trim()),
  );
}

function isNoindex(robots) {
  const directives = (robots ?? "")
    .toLowerCase()
    .split(",")
    .map((directive) => directive.trim());
  return directives.includes("noindex") || directives.includes("none");
}

function pageData(distDir, filePath) {
  const html = fs.readFileSync(filePath, "utf8");
  const route = routeForHtml(distDir, filePath);
  const canonical = linkHref(html, "canonical");
  const robots = metaContent(html, "name", "robots");
  const jsonLd = [
    ...html.matchAll(
      /<script\b[^>]*\btype\s*=\s*(?:"application\/ld\+json"|'application\/ld\+json')[^>]*>([\s\S]*?)<\/script>/gi,
    ),
  ].map((match) => match[1].trim());

  return {
    filePath,
    route,
    html,
    title: elementText(html, "title"),
    description: metaContent(html, "name", "description"),
    canonical,
    robots,
    noindex: isNoindex(robots),
    h1Count: tags(html, "h1").length,
    jsonLd,
  };
}

function pushDuplicateErrors(pages, property, label, errors) {
  const seen = new Map();
  for (const page of pages) {
    const value = page[property];
    if (!value) continue;
    const routes = seen.get(value) ?? [];
    routes.push(page.route);
    seen.set(value, routes);
  }

  for (const routes of seen.values()) {
    if (routes.length > 1) {
      errors.push(`Duplicate ${label}: ${routes.join(", ")}`);
    }
  }
}

function readSitemapGraph(distDir, sitemapUrls, siteOrigin, errors) {
  const pageUrls = [];
  const visitedSitemaps = new Set();
  const sitemapFiles = [];

  function visit(sitemapUrl) {
    let url;
    try {
      url = new URL(sitemapUrl);
    } catch {
      errors.push(`Invalid sitemap URL: ${sitemapUrl}`);
      return;
    }

    if (url.origin !== siteOrigin) {
      errors.push(`Sitemap points to a different origin: ${url.href}`);
      return;
    }

    if (visitedSitemaps.has(url.href)) return;
    visitedSitemaps.add(url.href);

    let relative;
    try {
      relative = decodeURIComponent(url.pathname).replace(/^\/+/, "");
    } catch {
      errors.push(`Invalid sitemap path encoding: ${url.pathname}`);
      return;
    }
    if (
      relative
        .split("/")
        .some((segment) => segment === ".." || segment === ".")
    ) {
      errors.push(`Invalid sitemap path: ${url.pathname}`);
      return;
    }

    const filePath = path.join(distDir, relative);
    if (!fs.existsSync(filePath)) {
      errors.push(`Referenced sitemap is missing: ${url.pathname}`);
      return;
    }

    const xml = fs.readFileSync(filePath, "utf8");
    sitemapFiles.push({ filePath, url, xml });
    const locs = xmlLocs(xml);
    if (/<sitemapindex\b/i.test(xml)) {
      for (const child of locs) visit(child);
    } else if (/<urlset\b/i.test(xml)) {
      pageUrls.push(...locs);
    } else {
      errors.push(`Invalid sitemap document: ${url.pathname}`);
    }
  }

  for (const sitemapUrl of sitemapUrls) visit(sitemapUrl);
  return { pageUrls, sitemapFiles };
}

export function pruneNoindexSitemapUrls({
  distDir,
  site = process.env.PUBLIC_SITE_URL ?? DEFAULT_SITE,
}) {
  const absoluteDist = path.resolve(distDir);
  const siteOrigin = new URL(site).origin;
  const robotsPath = path.join(absoluteDist, "robots.txt");
  if (!fs.existsSync(robotsPath)) return { pruned: 0, filesChanged: 0 };

  const sitemapUrls = [
    ...fs
      .readFileSync(robotsPath, "utf8")
      .matchAll(/^\s*Sitemap:\s*(\S+)\s*$/gim),
  ].map((match) => match[1]);
  const errors = [];
  const graph = readSitemapGraph(
    absoluteDist,
    sitemapUrls,
    siteOrigin,
    errors,
  );
  if (errors.length > 0) {
    throw new Error(errors.join("\n"));
  }

  let pruned = 0;
  let filesChanged = 0;
  for (const sitemap of graph.sitemapFiles) {
    if (!/<urlset\b/i.test(sitemap.xml)) continue;

    const nextXml = sitemap.xml.replace(
      /\s*<url>\s*[\s\S]*?<\/url>/gi,
      (block) => {
        const [loc] = xmlLocs(block);
        if (!loc) return block;

        let url;
        try {
          url = new URL(loc);
        } catch {
          return block;
        }
        const htmlPath = htmlFileForUrl(absoluteDist, url);
        if (!htmlPath) return block;

        const page = pageData(absoluteDist, htmlPath);
        if (!page.noindex) return block;
        pruned += 1;
        return "";
      },
    );

    if (nextXml !== sitemap.xml) {
      fs.writeFileSync(sitemap.filePath, `${nextXml.trimEnd()}\n`);
      filesChanged += 1;
    }
  }

  return { pruned, filesChanged };
}

export function auditSeo({
  distDir,
  site = process.env.PUBLIC_SITE_URL ?? DEFAULT_SITE,
}) {
  const absoluteDist = path.resolve(distDir);
  const errors = [];
  const warnings = [];
  const siteUrl = new URL(site);
  const siteOrigin = siteUrl.origin;

  if (!fs.existsSync(absoluteDist)) {
    return {
      pages: 0,
      sitemapUrls: 0,
      errors: [`Build output does not exist: ${absoluteDist}`],
      warnings,
    };
  }

  const htmlFiles = walkFiles(absoluteDist, (file) =>
    file.endsWith(".html"),
  );
  if (htmlFiles.length === 0) {
    errors.push("No generated HTML pages found");
  }
  const pages = htmlFiles.map((file) => pageData(absoluteDist, file));
  const byRoute = new Map(pages.map((page) => [page.route, page]));

  for (const page of pages) {
    const prefix = page.route;
    const htmlTag = tags(page.html, "html")[0];
    if (!htmlTag || !attribute(htmlTag, "lang")) {
      errors.push(`${prefix}: missing html lang`);
    }
    if (!page.title) errors.push(`${prefix}: missing title`);
    if (!page.description) errors.push(`${prefix}: missing meta description`);
    if (!page.canonical) errors.push(`${prefix}: missing canonical`);
    if (!page.robots) errors.push(`${prefix}: missing robots metadata`);
    if (!linkHref(page.html, "apple-touch-icon")) {
      errors.push(`${prefix}: missing apple touch icon`);
    }

    for (const [key, value, label] of [
      ["property", "og:type", "Open Graph type"],
      ["property", "og:title", "Open Graph title"],
      ["property", "og:description", "Open Graph description"],
      ["property", "og:url", "Open Graph URL"],
      ["property", "og:image", "Open Graph image"],
      ["name", "twitter:card", "Twitter card"],
      ["name", "twitter:title", "Twitter title"],
      ["name", "twitter:description", "Twitter description"],
      ["name", "twitter:image", "Twitter image"],
    ]) {
      if (!metaContent(page.html, key, value)) {
        errors.push(`${prefix}: missing ${label}`);
      }
    }

    for (const image of tags(page.html, "img")) {
      if (attribute(image, "alt") === null) {
        errors.push(`${prefix}: image missing alt`);
      }
    }

    if (page.canonical) {
      try {
        const canonical = new URL(page.canonical);
        if (canonical.origin !== siteOrigin) {
          errors.push(
            `${prefix}: canonical origin is ${canonical.origin}, expected ${siteOrigin}`,
          );
        }
        if (
          normalizePathname(canonical.pathname) !==
          normalizePathname(page.route)
        ) {
          errors.push(
            `${prefix}: canonical path is ${canonical.pathname}, expected ${page.route}`,
          );
        }
        if (canonical.search || canonical.hash) {
          errors.push(`${prefix}: canonical contains a query or fragment`);
        }

        const ogUrl = metaContent(page.html, "property", "og:url");
        if (ogUrl && ogUrl !== canonical.href) {
          errors.push(`${prefix}: Open Graph URL does not match canonical`);
        }
      } catch {
        errors.push(`${prefix}: invalid canonical URL`);
      }
    }

    for (const [key, value, label] of [
      ["property", "og:image", "Open Graph image"],
      ["name", "twitter:image", "Twitter image"],
    ]) {
      const image = metaContent(page.html, key, value);
      if (image) {
        try {
          const imageUrl = new URL(image);
          if (imageUrl.protocol !== "https:") {
            errors.push(`${prefix}: ${label} must use HTTPS`);
          }
        } catch {
          errors.push(`${prefix}: ${label} is not an absolute URL`);
        }
      }
    }

    if (!page.noindex) {
      if (page.h1Count !== 1) {
        errors.push(
          `${prefix}: indexable page must have exactly one h1 (found ${page.h1Count})`,
        );
      }
      if (page.jsonLd.length === 0) {
        errors.push(`${prefix}: indexable page is missing JSON-LD`);
      }
    }

    for (const json of page.jsonLd) {
      try {
        JSON.parse(json);
      } catch {
        errors.push(`${prefix}: invalid JSON-LD`);
      }
    }
  }

  pushDuplicateErrors(pages, "title", "title", errors);
  pushDuplicateErrors(pages, "description", "meta description", errors);
  pushDuplicateErrors(pages, "canonical", "canonical", errors);

  const robotsPath = path.join(absoluteDist, "robots.txt");
  let sitemapUrls = [];
  if (!fs.existsSync(robotsPath)) {
    errors.push("Missing robots.txt");
  } else {
    sitemapUrls = [
      ...fs
        .readFileSync(robotsPath, "utf8")
        .matchAll(/^\s*Sitemap:\s*(\S+)\s*$/gim),
    ].map((match) => match[1]);
    if (sitemapUrls.length === 0) {
      errors.push("robots.txt does not reference a sitemap");
    }
  }

  const graph = readSitemapGraph(
    absoluteDist,
    sitemapUrls,
    siteOrigin,
    errors,
  );
  const seenSitemapUrls = new Set();
  for (const loc of graph.pageUrls) {
    if (seenSitemapUrls.has(loc)) {
      errors.push(`Duplicate sitemap URL: ${loc}`);
      continue;
    }
    seenSitemapUrls.add(loc);

    let url;
    try {
      url = new URL(loc);
    } catch {
      errors.push(`Invalid page URL in sitemap: ${loc}`);
      continue;
    }
    if (url.origin !== siteOrigin) {
      errors.push(`Sitemap page points to a different origin: ${loc}`);
      continue;
    }

    const htmlPath = htmlFileForUrl(absoluteDist, url);
    if (!htmlPath) {
      errors.push(`Sitemap URL has no generated HTML: ${url.pathname}`);
      continue;
    }
    const route = routeForHtml(absoluteDist, htmlPath);
    const page = byRoute.get(route);
    if (!page) {
      errors.push(`Sitemap URL was not audited: ${url.pathname}`);
    } else if (page.noindex) {
      errors.push(`Sitemap URL is noindex: ${url.pathname}`);
    }
  }

  const indexablePages = pages.filter((page) => !page.noindex);
  const sitemapRoutes = new Set(
    graph.pageUrls.flatMap((loc) => {
      try {
        return [normalizePathname(new URL(loc).pathname)];
      } catch {
        return [];
      }
    }),
  );
  const indexableOutsideSitemap = indexablePages.filter(
    (page) => !sitemapRoutes.has(normalizePathname(page.route)),
  );
  if (indexableOutsideSitemap.length > 0) {
    errors.push(
      `Indexable pages outside the sitemap: ${indexableOutsideSitemap
        .map((page) => page.route)
        .join(", ")}`,
    );
  }

  return {
    pages: pages.length,
    indexablePages: indexablePages.length,
    sitemapUrls: seenSitemapUrls.size,
    errors: [...new Set(errors)],
    warnings: [...new Set(warnings)],
  };
}

function parseArgs(argv) {
  let distDir = "dist";
  let site = process.env.PUBLIC_SITE_URL ?? DEFAULT_SITE;
  let prune = false;
  let json = false;

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--prune-noindex-sitemap") {
      prune = true;
    } else if (arg === "--json") {
      json = true;
    } else if (arg === "--dist") {
      distDir = argv[++index];
    } else if (arg.startsWith("--dist=")) {
      distDir = arg.slice("--dist=".length);
    } else if (arg === "--site") {
      site = argv[++index];
    } else if (arg.startsWith("--site=")) {
      site = arg.slice("--site=".length);
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }
  return { distDir, site, prune, json };
}

function runCli() {
  const options = parseArgs(process.argv.slice(2));
  let pruned = { pruned: 0, filesChanged: 0 };
  if (options.prune) {
    pruned = pruneNoindexSitemapUrls(options);
  }
  const result = auditSeo(options);
  const report = { ...result, prunedSitemapUrls: pruned.pruned };

  if (options.json) {
    console.log(JSON.stringify(report, null, 2));
  } else {
    console.log(
      `SEO audit: ${result.pages} pages, ${result.indexablePages} indexable, ${result.sitemapUrls} sitemap URLs`,
    );
    if (pruned.pruned > 0) {
      console.log(
        `Pruned ${pruned.pruned} noindex sitemap URLs across ${pruned.filesChanged} files`,
      );
    }
    for (const warning of result.warnings) console.warn(`warning: ${warning}`);
    for (const error of result.errors) console.error(`error: ${error}`);
  }

  if (result.errors.length > 0) process.exitCode = 1;
}

const isCli =
  process.argv[1] &&
  import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href;
if (isCli) runCli();
