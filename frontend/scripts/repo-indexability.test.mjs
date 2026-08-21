import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, test } from "node:test";

import {
  BUILD_DEFECT_META as AUDIT_MARKER,
  auditSeo,
} from "./audit-seo.mjs";
import {
  BUILD_DEFECT_META,
  repoIndexability,
} from "../src/lib/repo-indexability.ts";

/** The boolean `[owner]/[repo].astro` emitted before the reasons were split. */
const legacyNoindex = ({ hasSnapshot, fetchError, hasHistory }) =>
  !hasSnapshot || fetchError !== null || !hasHistory;

const outcome = (overrides) => ({
  hasSnapshot: true,
  fetchError: null,
  notFound: false,
  hasHistory: true,
  ...overrides,
});

const CASES = [
  {
    name: "a repository with a payload and a series is indexable",
    input: outcome({}),
    reason: null,
    buildDefect: false,
  },
  {
    name: "a cold repository is de-indexed for having no history, not for failing",
    // The backend answers 200 with `history: []` for a repository it has not
    // read yet. This is the case that must never fail a build.
    input: outcome({ hasHistory: false }),
    reason: "no-history",
    buildDefect: false,
  },
  {
    name: "a tombstoned repository is a definitive answer, not an unreachable one",
    input: outcome({ hasSnapshot: false, notFound: true, hasHistory: false }),
    reason: "not-found",
    buildDefect: false,
  },
  {
    name: "a network failure is a build defect",
    input: outcome({
      hasSnapshot: false,
      fetchError: "fetch failed",
      hasHistory: false,
    }),
    reason: "unreachable",
    buildDefect: true,
  },
  {
    name: "an error status is a build defect",
    input: outcome({
      hasSnapshot: false,
      fetchError: "backend returned 503",
      hasHistory: false,
    }),
    reason: "unreachable",
    buildDefect: true,
  },
  {
    name: "no payload, no error and no tombstone is still a build defect",
    input: outcome({ hasSnapshot: false, hasHistory: false }),
    reason: "unreachable",
    buildDefect: true,
  },
  {
    name: "an error outranks a payload that arrived beside it",
    input: outcome({ fetchError: "backend returned 502" }),
    reason: "unreachable",
    buildDefect: true,
  },
];

for (const { name, input, reason, buildDefect } of CASES) {
  test(`reason: ${name}`, () => {
    const result = repoIndexability(input);
    assert.equal(result.reason, reason);
    assert.equal(result.buildDefect, buildDefect);
    assert.equal(result.noindex, reason !== null);
  });
}

test("splitting the reason changed no page's robots meta", () => {
  // The whole point: search engines see exactly what they saw before. Only the
  // build can now tell the two situations apart.
  for (const { name, input } of CASES) {
    assert.equal(
      repoIndexability(input).noindex,
      legacyNoindex(input),
      `robots parity broke for: ${name}`,
    );
  }
});

test("only an unreadable snapshot is ever a build defect", () => {
  // Guards the regression that would hurt most: a production build failing
  // because a repository is merely cold.
  for (const { input } of CASES) {
    if (repoIndexability(input).buildDefect) {
      assert.ok(
        input.fetchError !== null || !input.hasSnapshot,
        "a repository the backend answered for was called a defect",
      );
      assert.equal(input.notFound, false);
    }
  }
});

test("the page's marker and the audit's matcher are the same two strings", () => {
  // audit-seo.mjs runs without type stripping and cannot import the module the
  // pages use, so the copies are kept honest here instead.
  assert.deepEqual({ ...AUDIT_MARKER }, { ...BUILD_DEFECT_META });
});

const temporaryDirectories = [];
const SITE = "https://example.com";

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

function fixture() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "gitdebt-defect-"));
  temporaryDirectories.push(directory);
  return directory;
}

function write(directory, relative, contents) {
  const filePath = path.join(directory, relative);
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, contents);
}

function page({ route, title, description, robots = "index,follow", marker }) {
  const canonical = `${SITE}${route === "/" ? "/" : route}`;
  return `<!doctype html>
<html lang="en">
  <head>
    <title>${title}</title>
    <meta name="description" content="${description}">
    <meta name="robots" content="${robots}">
    ${marker ? `<meta name="${BUILD_DEFECT_META.name}" content="${marker}">` : ""}
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
  <body><h1>${title}</h1></body>
</html>`;
}

/** Home page, one cold repository, and `broken` repositories that never loaded. */
function build({ broken = [] } = {}) {
  const directory = fixture();
  write(
    directory,
    "index.html",
    page({ route: "/", title: "Home", description: "Star history analytics" }),
  );
  write(
    directory,
    "acme/cold.html",
    page({
      route: "/acme/cold",
      title: "acme/cold",
      description: "A repository with no star history yet",
      robots: "noindex,follow",
    }),
  );
  for (const slug of broken) {
    write(
      directory,
      `${slug}.html`,
      page({
        route: `/${slug}`,
        title: slug,
        description: `A repository whose snapshot never loaded: ${slug}`,
        robots: "noindex,follow",
        marker: BUILD_DEFECT_META.unreachable,
      }),
    );
  }
  write(
    directory,
    "robots.txt",
    `User-agent: *\nAllow: /\nSitemap: ${SITE}/sitemap.xml\n`,
  );
  write(
    directory,
    "sitemap.xml",
    `<?xml version="1.0"?><urlset><url><loc>${SITE}/</loc></url></urlset>`,
  );
  return directory;
}

test("a clean build counts no unreachable snapshots", () => {
  const result = auditSeo({
    distDir: build(),
    site: SITE,
    productionBuild: false,
  });
  assert.deepEqual(result.errors, []);
  assert.equal(result.unreachableSnapshots, 0);
  assert.deepEqual(result.unreachableRoutes, []);
});

test("marked pages are counted apart from the pages that are merely cold", () => {
  // Both pages carry `noindex,follow`; only one of them is a defect. Folding
  // them together is what let four builds of identical code publish 439, 514,
  // 380 and 154 indexable pages and call every one of them a pass.
  const result = auditSeo({
    distDir: build({ broken: ["acme/broken", "acme/alsobroken"] }),
    site: SITE,
    productionBuild: false,
  });
  assert.equal(result.pages, 4);
  assert.equal(result.indexablePages, 1);
  assert.equal(result.unreachableSnapshots, 2);
  assert.deepEqual(result.unreachableRoutes, [
    "/acme/alsobroken",
    "/acme/broken",
  ]);
  // A local build without network access still exits 0.
  assert.deepEqual(result.errors, []);
});

test("the same output fails when the build declares itself production", () => {
  const result = auditSeo({
    distDir: build({ broken: ["acme/broken"] }),
    site: SITE,
    productionBuild: true,
  });
  assert.equal(result.unreachableSnapshots, 1);
  const failure = result.errors.find((error) =>
    error.startsWith("Static snapshot refresh failed for"),
  );
  assert.ok(failure, result.errors.join("\n"));
  // Names the endpoint and the reason, like the catalog failure it mirrors.
  assert.match(failure, /\/api\/repos\/\{owner\}\/\{repo\}\/analyze/);
  assert.match(failure, /STATIC_CATALOG_REQUIRED=1/);
  assert.match(failure, /\/acme\/broken/);
});

test("a production build with no marker is not accused of anything", () => {
  const result = auditSeo({
    distDir: build(),
    site: SITE,
    productionBuild: true,
  });
  assert.deepEqual(result.errors, []);
});

test("a stray marker on an indexable page is still counted", () => {
  // Defensive: the marker is the signal, not the robots value it travels with.
  const directory = build();
  write(
    directory,
    "acme/odd.html",
    page({
      route: "/acme/odd",
      title: "acme/odd",
      description: "Marked yet indexable, which should never happen",
      marker: BUILD_DEFECT_META.unreachable,
    }),
  );
  const result = auditSeo({
    distDir: directory,
    site: SITE,
    productionBuild: false,
  });
  assert.deepEqual(result.unreachableRoutes, ["/acme/odd"]);
});
