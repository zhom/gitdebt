#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const DEFAULT_SITE = "https://gitdebt.com";
// `astro build` refuses to run in production without PUBLIC_API_BASE, so a
// dist directory always had one; this default only covers auditing an existing
// build from a shell that no longer carries the variable.
const DEFAULT_API = "https://api.gitdebt.com";

function walk(directory, extension) {
  if (!fs.existsSync(directory)) return [];
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const file = path.join(directory, entry.name);
    return entry.isDirectory()
      ? walk(file, extension)
      : file.endsWith(extension)
        ? [file]
        : [];
  });
}

function routeForHtml(distDir, file) {
  const relative = path.relative(distDir, file).split(path.sep).join("/");
  if (relative === "index.html") return "/";
  if (relative.endsWith("/index.html")) {
    return `/${relative.slice(0, -"/index.html".length)}`;
  }
  return `/${relative.slice(0, -".html".length)}`;
}

function markdownAlternate(html) {
  for (const match of html.matchAll(/<link\b[^>]*>/gi)) {
    const tag = match[0];
    if (!/\brel=["'][^"']*\balternate\b[^"']*["']/i.test(tag)) continue;
    if (!/\btype=["']text\/markdown["']/i.test(tag)) continue;
    return tag.match(/\bhref=["']([^"']+)["']/i)?.[1] ?? null;
  }
  return null;
}

/**
 * Every page must advertise its Markdown representation, and every one of
 * those now lives on the API at `/api/md/{path}` — the site prerenders none.
 * The two halves are one invariant: an emitted `.md` file would be shadowed by
 * the `/*.md` redirect and served as HTML by the 404 fallback instead, so a
 * page pointing at itself is the failure this audit exists to catch.
 */
export function auditAgentSurfaces({
  distDir,
  site = process.env.PUBLIC_SITE_URL ?? DEFAULT_SITE,
  apiBase = process.env.PUBLIC_API_BASE ?? DEFAULT_API,
}) {
  const absoluteDist = path.resolve(distDir);
  const errors = [];
  const htmlFiles = walk(absoluteDist, ".html");
  const apiOrigin = new URL(apiBase).origin;

  for (const file of htmlFiles) {
    const route = routeForHtml(absoluteDist, file);
    // The home page is the empty path, not a segment named `index`.
    const expectedPath = route === "/" ? "/api/md/" : `/api/md${route}`;
    const html = fs.readFileSync(file, "utf8");
    const alternate = markdownAlternate(html);
    if (!alternate) {
      errors.push(`${route}: missing text/markdown alternate`);
      continue;
    }
    let url;
    try {
      url = new URL(alternate, site);
    } catch {
      errors.push(`${route}: invalid Markdown alternate`);
      continue;
    }
    if (url.origin !== apiOrigin || url.pathname !== expectedPath) {
      errors.push(
        `${route}: Markdown alternate is ${url.href}, expected ${apiOrigin}${expectedPath}`,
      );
    }
  }

  for (const file of walk(absoluteDist, ".md")) {
    errors.push(
      `${path.relative(absoluteDist, file)}: Markdown is served by the API, not prerendered`,
    );
  }

  for (const required of ["llms.txt", "llms-full.txt"]) {
    if (!fs.existsSync(path.join(absoluteDist, required))) {
      errors.push(`missing ${required}`);
    }
  }
  const robots = fs.existsSync(path.join(absoluteDist, "robots.txt"))
    ? fs.readFileSync(path.join(absoluteDist, "robots.txt"), "utf8")
    : "";
  for (const bot of ["GPTBot", "ClaudeBot", "PerplexityBot", "Google-Extended"]) {
    if (!new RegExp(`User-agent:\\s*${bot}[\\s\\S]*?Allow:\\s*/`, "i").test(robots)) {
      errors.push(`robots.txt does not explicitly allow ${bot}`);
    }
  }

  return { pages: htmlFiles.length, errors };
}

function main() {
  const distDir = process.argv[2] ?? new URL("../dist", import.meta.url).pathname;
  const result = auditAgentSurfaces({ distDir });
  if (result.errors.length > 0) {
    for (const error of result.errors) console.error(`error: ${error}`);
    process.exitCode = 1;
    return;
  }
  console.log(
    `Agent surfaces audit: ${result.pages} HTML pages point at /api/md, none prerender Markdown`,
  );
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) main();
