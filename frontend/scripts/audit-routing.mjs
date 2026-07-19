#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const REQUIRED_FILE_ROUTES = [
  "about.html",
  "badges.html",
  "compare.html",
  "leaderboard.html",
  "privacy.html",
  "profile.html",
  "report.html",
  "terms.html",
];

const CANONICAL_SLASH_ROUTES = [
  ["/about/", "/about"],
  ["/badges/", "/badges"],
  ["/compare/", "/compare"],
  ["/leaderboard/", "/leaderboard"],
  ["/privacy/", "/privacy"],
  ["/profile/", "/profile"],
  ["/report/", "/report"],
  ["/terms/", "/terms"],
];

function redirectRules(contents) {
  return contents
    .split(/\r?\n/)
    .map((line) => line.replace(/(?:^|\s+)#.*$/, "").trim())
    .filter(Boolean)
    .map((line) => {
      const [source, destination, status = "302", ...extra] = line.split(/\s+/);
      return { source, destination, status, extra };
    });
}

function decodeHtml(value) {
  return value
    .replaceAll("&amp;", "&")
    .replaceAll("&lt;", "<")
    .replaceAll("&gt;", ">")
    .replaceAll("&quot;", '"')
    .replaceAll("&#39;", "'");
}

function tags(html, tagName) {
  const escaped = tagName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return [...html.matchAll(new RegExp(`<${escaped}\\b[^>]*>`, "gi"))].map(
    (match) => match[0],
  );
}

function attribute(tag, name) {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = tag.match(
    new RegExp(
      `(?:^|\\s)${escaped}\\s*=\\s*(?:"([^"]*)"|'([^']*)'|([^\\s>]+))`,
      "i",
    ),
  );
  return match ? decodeHtml(match[1] ?? match[2] ?? match[3] ?? "") : null;
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

function canonicalHrefs(html) {
  return tags(html, "link").flatMap((tag) => {
    const relations = (attribute(tag, "rel") ?? "")
      .toLowerCase()
      .split(/\s+/);
    return relations.includes("canonical") ? [attribute(tag, "href")] : [];
  });
}

function elementText(html, tagName) {
  const escaped = tagName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = html.match(
    new RegExp(`<${escaped}\\b[^>]*>([\\s\\S]*?)</${escaped}>`, "i"),
  );
  return match?.[1]?.replace(/<[^>]+>/g, "").trim() ?? null;
}

function badgeSeoErrors(html) {
  const errors = [];
  const prefix = "badges.html";
  const title = elementText(html, "title");
  const description = metaContent(html, "name", "description");
  const robots = metaContent(html, "name", "robots");
  if (!title) errors.push(`${prefix} is missing a title`);
  if (!description) errors.push(`${prefix} is missing a description`);
  if (!robots || /(?:^|,)\s*noindex\s*(?:,|$)/i.test(robots)) {
    errors.push(`${prefix} must have indexable robots metadata`);
  }

  const canonicals = canonicalHrefs(html);
  if (canonicals.length !== 1 || !canonicals[0]) {
    errors.push(`${prefix} must have exactly one canonical URL`);
  } else {
    try {
      const canonical = new URL(canonicals[0]);
      if (
        canonical.protocol !== "https:" ||
        canonical.pathname !== "/badges" ||
        canonical.search ||
        canonical.hash
      ) {
        errors.push(
          `${prefix} canonical must be an HTTPS /badges URL without query or fragment`,
        );
      }
    } catch {
      errors.push(`${prefix} canonical is not an absolute URL`);
    }
  }

  const requiredMeta = [
    ["property", "og:type", "website"],
    ["property", "og:image:type", "image/png"],
    ["property", "og:image:width", "1200"],
    ["property", "og:image:height", "630"],
    ["name", "twitter:card", "summary_large_image"],
    ["name", "twitter:image:type", "image/png"],
    ["name", "twitter:image:width", "1200"],
    ["name", "twitter:image:height", "630"],
  ];
  for (const [key, name, expected] of requiredMeta) {
    if (metaContent(html, key, name) !== expected) {
      errors.push(`${prefix} requires ${name}=${expected}`);
    }
  }

  const ogImage = metaContent(html, "property", "og:image");
  const twitterImage = metaContent(html, "name", "twitter:image");
  for (const [label, value] of [
    ["Open Graph", ogImage],
    ["Twitter", twitterImage],
  ]) {
    try {
      const image = new URL(value ?? "");
      if (image.protocol !== "https:" || !image.pathname.endsWith(".png")) {
        errors.push(`${prefix} ${label} image must be an absolute HTTPS PNG`);
      }
    } catch {
      errors.push(`${prefix} ${label} image must be an absolute HTTPS PNG`);
    }
  }
  if (ogImage && twitterImage && ogImage !== twitterImage) {
    errors.push(`${prefix} Open Graph and Twitter images must match`);
  }

  const jsonLd = [
    ...html.matchAll(
      /<script\b[^>]*\btype\s*=\s*(?:"application\/ld\+json"|'application\/ld\+json')[^>]*>([\s\S]*?)<\/script>/gi,
    ),
  ];
  if (jsonLd.length === 0) {
    errors.push(`${prefix} is missing JSON-LD`);
  }
  for (const match of jsonLd) {
    try {
      JSON.parse(match[1]);
    } catch {
      errors.push(`${prefix} contains invalid JSON-LD`);
    }
  }

  return errors;
}

function headerRules(contents) {
  const rules = [];
  let current = null;
  for (const rawLine of contents.split(/\r?\n/)) {
    const line = rawLine.replace(/(?:^|\s+)#.*$/, "");
    if (!line.trim()) continue;
    if (!/^\s/.test(line)) {
      current = { source: line.trim(), headers: new Map() };
      rules.push(current);
      continue;
    }
    if (!current) continue;
    const separator = line.indexOf(":");
    if (separator === -1) continue;
    const name = line.slice(0, separator).trim().toLowerCase();
    const value = line.slice(separator + 1).trim();
    current.headers.set(name, value);
  }
  return rules;
}

function sourceMatches(source, pathname) {
  const expression = source
    .split("/")
    .map((segment) => {
      if (segment === "*") return ".*";
      if (segment.startsWith(":")) return "[^/]+";
      return segment
        .replace(/[.+?^${}()|[\]\\]/g, "\\$&")
        .replaceAll("*", ".*");
    })
    .join("/");
  return new RegExp(`^${expression}$`).test(pathname);
}

function exactRedirectChain(rules, source) {
  const exact = new Map(
    rules
      .filter(
        ({ source: from, destination }) =>
          !/[*:]/.test(from) &&
          destination.startsWith("/") &&
          !/[*:]/.test(destination),
      )
      .map(({ source: from, destination }) => [from, destination]),
  );
  const hops = [];
  const seen = new Set();
  let cursor = source;
  while (exact.has(cursor) && !seen.has(cursor)) {
    seen.add(cursor);
    const destination = exact.get(cursor);
    hops.push([cursor, destination]);
    cursor = destination;
  }
  return { hops, destination: cursor };
}

function hasRobotsNoindex(html) {
  return /<meta\b[^>]*\bname=["']robots["'][^>]*\bcontent=["'][^"']*\bnoindex\b/i.test(
    html,
  );
}

function exactRedirectCycles(rules) {
  const exact = new Map(
    rules
      .filter(
        ({ source, destination }) =>
          !/[*:]/.test(source) &&
          destination.startsWith("/") &&
          !/[*:]/.test(destination),
      )
      .map(({ source, destination }) => [source, destination]),
  );
  const cycles = [];

  for (const source of exact.keys()) {
    const seen = new Set();
    let cursor = source;
    while (exact.has(cursor)) {
      if (seen.has(cursor)) {
        cycles.push([...seen, cursor].join(" → "));
        break;
      }
      seen.add(cursor);
      cursor = exact.get(cursor);
    }
  }
  return [...new Set(cycles)];
}

export function auditPagesRouting({ distDir }) {
  const absoluteDist = path.resolve(distDir);
  const errors = [];

  if (!fs.existsSync(absoluteDist)) {
    return [`Build output does not exist: ${absoluteDist}`];
  }

  for (const relative of REQUIRED_FILE_ROUTES) {
    if (!fs.existsSync(path.join(absoluteDist, relative))) {
      errors.push(`Missing file-format route: ${relative}`);
    }
    const directoryRoute = relative.replace(/\.html$/, "/index.html");
    if (fs.existsSync(path.join(absoluteDist, directoryRoute))) {
      errors.push(
        `Directory-format route can trigger Pages slash normalization: ${directoryRoute}`,
      );
    }
  }

  for (const relative of ["404.html", "facebook/react.html"]) {
    if (!fs.existsSync(path.join(absoluteDist, relative))) {
      errors.push(`Missing static-first routing asset: ${relative}`);
    }
  }

  const redirectsPath = path.join(absoluteDist, "_redirects");
  if (!fs.existsSync(redirectsPath)) {
    errors.push("Missing Cloudflare Pages _redirects");
  } else {
    const rules = redirectRules(fs.readFileSync(redirectsPath, "utf8"));
    for (const rule of rules) {
      if (rule.extra.length > 0) {
        errors.push(`Invalid _redirects rule: ${JSON.stringify(rule)}`);
      }
    }

    for (const [source, destination] of CANONICAL_SLASH_ROUTES) {
      if (
        !rules.some(
          (rule) =>
            rule.source === source &&
            rule.destination === destination &&
            rule.status === "301",
        )
      ) {
        errors.push(`Missing canonical redirect: ${source} → ${destination}`);
      }
    }

    const sourceCounts = new Map();
    for (const { source } of rules) {
      sourceCounts.set(source, (sourceCounts.get(source) ?? 0) + 1);
    }
    for (const [source, count] of sourceCounts) {
      if (count > 1) {
        errors.push(`Duplicate redirect source: ${source}`);
      }
    }

    const slashBadgeRules = rules.filter(({ source }) =>
      sourceMatches(source, "/badges/"),
    );
    if (
      slashBadgeRules.length !== 1 ||
      slashBadgeRules[0].source !== "/badges/" ||
      slashBadgeRules[0].destination !== "/badges" ||
      slashBadgeRules[0].status !== "301"
    ) {
      errors.push(
        "/badges/ must match exactly one redirect: /badges/ → /badges 301",
      );
    }
    if (rules.some(({ source }) => sourceMatches(source, "/badges"))) {
      errors.push("/badges must resolve to badges.html without another redirect");
    }
    const badgeChain = exactRedirectChain(rules, "/badges/");
    if (
      badgeChain.hops.length !== 1 ||
      badgeChain.destination !== "/badges"
    ) {
      errors.push("/badges/ must reach /badges in exactly one redirect");
    }

    if (
      rules.some(
        ({ source }) =>
          source === "/*" ||
          (/^\/:[A-Za-z]\w*\/:[A-Za-z]\w*$/.test(source) &&
            !source.endsWith("/")),
      )
    ) {
      errors.push(
        "A catch-all redirect shadows generated repository HTML; fallback must run from 404.html",
      );
    }

    for (const cycle of exactRedirectCycles(rules)) {
      errors.push(`Redirect cycle: ${cycle}`);
    }
  }

  const headersPath = path.join(absoluteDist, "_headers");
  if (!fs.existsSync(headersPath)) {
    errors.push("Missing Cloudflare Pages _headers");
  } else {
    const rules = headerRules(fs.readFileSync(headersPath, "utf8"));
    const sourceCounts = new Map();
    for (const { source } of rules) {
      sourceCounts.set(source, (sourceCounts.get(source) ?? 0) + 1);
    }
    for (const [source, count] of sourceCounts) {
      if (count > 1) errors.push(`Duplicate _headers block: ${source}`);
    }
    const badges = rules.find(({ source }) => source === "/badges");
    if (
      badges?.headers.get("cache-control") !==
      "public, max-age=0, must-revalidate"
    ) {
      errors.push(
        "/badges must use Cache-Control: public, max-age=0, must-revalidate",
      );
    }
  }

  const badgesPath = path.join(absoluteDist, "badges.html");
  if (fs.existsSync(badgesPath)) {
    errors.push(...badgeSeoErrors(fs.readFileSync(badgesPath, "utf8")));
  }

  const notFoundPath = path.join(absoluteDist, "404.html");
  if (fs.existsSync(notFoundPath)) {
    const html = fs.readFileSync(notFoundPath, "utf8");
    if (!hasRobotsNoindex(html)) {
      errors.push("404.html must remain noindex");
    }
    if (!/name="gitdebt-route-fallback"\s+content="missing-repo"/i.test(html)) {
      errors.push("404.html is missing the repository fallback marker");
    }
  }

  for (const relative of ["profile.html", "report.html"]) {
    const filePath = path.join(absoluteDist, relative);
    if (
      fs.existsSync(filePath) &&
      !hasRobotsNoindex(fs.readFileSync(filePath, "utf8"))
    ) {
      errors.push(`${relative} must remain noindex`);
    }
  }

  return errors;
}

function runCli() {
  const distIndex = process.argv.indexOf("--dist");
  const distDir =
    distIndex === -1 ? "dist" : process.argv[distIndex + 1] ?? "dist";
  const errors = auditPagesRouting({ distDir });
  if (errors.length === 0) {
    console.log("Pages routing audit: static-first routes and fallback are valid");
    return;
  }
  for (const error of errors) console.error(`error: ${error}`);
  process.exitCode = 1;
}

const isCli =
  process.argv[1] &&
  import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href;
if (isCli) runCli();
