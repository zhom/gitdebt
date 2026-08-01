import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, test } from "node:test";
import { auditPagesRouting } from "./audit-routing.mjs";
import {
  baseRepoSegment,
  isReservedFirstSegment,
  liveReportRepo,
  missingProfileReportTarget,
  missingRepoReportTarget,
  profileLogin,
  RESERVED_FIRST_SEGMENTS,
} from "../src/lib/static-routing.mjs";

const temporaryDirectories = [];

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

function fixture() {
  const directory = fs.mkdtempSync(
    path.join(os.tmpdir(), "gitdebt-routing-"),
  );
  temporaryDirectories.push(directory);
  return directory;
}

function write(directory, relative, contents = "") {
  const filePath = path.join(directory, relative);
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, contents);
}

const HEADERS = `/*
  Content-Security-Policy: frame-ancestors 'none'

/badges
  Cache-Control: public, max-age=0, must-revalidate

/report
  X-Robots-Tag: noindex, follow

/profile
  X-Robots-Tag: noindex, follow
`;

function validOutput(directory, redirects = null) {
  for (const route of [
    "about",
    "badges",
    "compare",
    "leaderboard",
    "privacy",
    "profile",
    "terms",
  ]) {
    write(directory, `${route}.html`, "<html></html>");
  }
  write(
    directory,
    "badges.html",
    `<!doctype html><html lang="en"><head>
<title>GitHub README badges · gitdebt</title>
<meta name="description" content="Build accessible GitHub README badges.">
<meta name="robots" content="index,follow">
<link rel="canonical" href="https://gitdebt.com/badges">
<meta property="og:type" content="website">
<meta property="og:image" content="https://api.gitdebt.com/api/og.png">
<meta property="og:image:type" content="image/png">
<meta property="og:image:width" content="1200">
<meta property="og:image:height" content="630">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:image" content="https://api.gitdebt.com/api/og.png">
<meta name="twitter:image:type" content="image/png">
<meta name="twitter:image:width" content="1200">
<meta name="twitter:image:height" content="630">
<script type="application/ld+json">{"@context":"https://schema.org","@type":"CollectionPage"}</script>
</head><body><h1>Badges</h1></body></html>`,
  );
  const noindex =
    '<meta name="robots" content="noindex,follow">';
  write(
    directory,
    "404.html",
    `${noindex}<meta name="gitdebt-route-fallback" content="missing-repo">`,
  );
  write(directory, "profile.html", noindex);
  write(directory, "report.html", noindex);
  write(directory, "facebook/react.html", "<html></html>");
  write(
    directory,
    "_redirects",
    redirects ??
      `/about/ /about 301
/badges/ /badges 301
/compare/ /compare 301
/leaderboard/ /leaderboard 301
/report/ /report 301
/privacy/ /privacy 301
/profile/ /profile 301
/terms/ /terms 301
/index.md https://api.gitdebt.com/api/md/ 302
/u/:login/ /:login 301
/u/:login /:login 301
/:first/:second/ /:first/:second 301
/vs/:owner1/:repo1/:owner2/:repo2/ /vs/:owner1/:repo1/:owner2/:repo2 301
/*.md https://api.gitdebt.com/api/md/:splat 302
`,
  );
  write(directory, "_headers", HEADERS);
}

test("missing two-segment repo paths fall back to the noindex live report", () => {
  assert.equal(
    missingRepoReportTarget("/Some-Owner/Repo.js"),
    "/some-owner/repo.js",
  );
  assert.equal(
    missingRepoReportTarget("/new-owner/new-repo/"),
    "/new-owner/new-repo",
  );
});

test("only the exact `.md` representation suffix is dropped from a segment", () => {
  assert.equal(baseRepoSegment("tauri-wd.md"), "tauri-wd");
  // `.md` is the only representation the site emits, so no other extension is
  // stripped from a repository that legitimately carries one.
  assert.equal(baseRepoSegment("tauri-wd.json"), "tauri-wd.json");
  assert.equal(baseRepoSegment("tool.js"), "tool.js");
  assert.equal(baseRepoSegment("markdown"), "markdown");
  // A repository really named `.md` keeps its name; only a suffix is dropped.
  assert.equal(baseRepoSegment(".md"), ".md");
});

test("the site's own `.md` representation resolves to the base repository", () => {
  assert.equal(missingRepoReportTarget("/zhom/tauri-wd.md"), "/zhom/tauri-wd");
  assert.equal(
    missingRepoReportTarget("/zhom/tauri-wd.json"),
    "/zhom/tauri-wd.json",
  );
  assert.equal(missingRepoReportTarget("/zhom/.md"), "/zhom/.md");
});

test("the live report reads the same slug the 404 fallback resolves", () => {
  assert.deepEqual(liveReportRepo("/zhom/tauri-wd.md", ""), {
    owner: "zhom",
    repo: "tauri-wd",
  });
  assert.deepEqual(liveReportRepo("/Some-Owner/Repo.js", ""), {
    owner: "some-owner",
    repo: "repo.js",
  });
  assert.deepEqual(liveReportRepo("/new-owner/new-repo/", ""), {
    owner: "new-owner",
    repo: "new-repo",
  });
  // The homepage lookup hands the slug over as a query parameter, which wins.
  assert.deepEqual(liveReportRepo("/report", "?repo=Facebook/React.md"), {
    owner: "facebook",
    repo: "react",
  });
});

test("the live report refuses anything that is not a repository slug", () => {
  for (const [pathname, search] of [
    ["/report", ""],
    ["/only-one-segment", ""],
    ["/one/two/three", ""],
    ["/vs/a", ""],
    ["/owner/..md", ""],
    ["/owner/", ""],
    ["/report", "?repo=vs/anything"],
    ["/report", "?repo=owner/repo/extra"],
  ]) {
    assert.equal(
      liveReportRepo(pathname, search),
      null,
      `${pathname}${search} must not enqueue an analysis`,
    );
  }
  assert.equal(liveReportRepo(undefined, undefined), null);
});

test("application, legal, malformed, and non-repo routes stay on the 404", () => {
  for (const pathname of [
    "/about",
    "/privacy/policy",
    "/profile/zhom",
    "/terms/archive",
    "/compare/frontend-frameworks",
    "/u/zhom",
    "/vs/a/b/c/d",
    "/only-one-segment",
    "/one/two/three",
    "/bad%2Frepo/name",
    "/owner/%zz",
  ]) {
    assert.equal(
      missingRepoReportTarget(pathname),
      null,
      `${pathname} must not enter the repo fallback`,
    );
  }
});

test("single missing segments resolve to a root profile report", () => {
  assert.equal(missingProfileReportTarget("/Zhom"), "/zhom");
  assert.equal(missingProfileReportTarget("/some-user/"), "/some-user");
  assert.equal(missingProfileReportTarget("/%7Ahom"), "/zhom");
  // An agent following the site's documented `.md` convention one segment up.
  assert.equal(missingProfileReportTarget("/zhom.md"), "/zhom");
});

test("reserved first segments are never published or resolved as profiles", () => {
  for (const segment of [
    "about",
    "badges",
    "compare",
    "leaderboard",
    "privacy",
    "profile",
    "report",
    "terms",
    "vs",
    "api",
    "sitemaps",
    "404",
    "u",
    "_astro",
  ]) {
    assert.ok(isReservedFirstSegment(segment), `${segment} must be reserved`);
    assert.equal(profileLogin(segment), null, `${segment} must not be a login`);
    assert.equal(
      missingProfileReportTarget(`/${segment}`),
      null,
      `/${segment} must not open a profile`,
    );
  }
  for (const asset of ["robots.txt", "favicon.ico", "sitemap-index.xml"]) {
    assert.ok(RESERVED_FIRST_SEGMENTS.has(asset));
  }
});

test("malformed, multi-segment, and over-long logins stay on the 404", () => {
  for (const pathname of [
    "/",
    "/owner/repo",
    "/one/two/three",
    "/dot.login",
    "/-leading",
    "/trailing-",
    `/${"a".repeat(40)}`,
    "/%zz",
  ]) {
    assert.equal(
      missingProfileReportTarget(pathname),
      null,
      `${pathname} must not enter the profile fallback`,
    );
  }
});

test("accepts file-format static output with an asset-safe fallback", () => {
  const directory = fixture();
  validOutput(directory);
  assert.deepEqual(auditPagesRouting({ distDir: directory }), []);
});

test("root profiles coexist with repository pages but reserved names do not", () => {
  const directory = fixture();
  validOutput(directory);
  write(directory, "facebook.html", "<html></html>");
  assert.deepEqual(auditPagesRouting({ distDir: directory }), []);

  write(directory, "sitemaps.html", "<html></html>");
  const errors = auditPagesRouting({ distDir: directory });
  assert.ok(
    errors.some((error) => error.includes("sitemaps.html")),
    errors.join("\n"),
  );
});

test("requires the legacy /u profile prefix to reach the root in one hop", () => {
  const directory = fixture();
  validOutput(
    directory,
    `/about/ /about 301
/badges/ /badges 301
/compare/ /compare 301
/leaderboard/ /leaderboard 301
/report/ /report 301
/privacy/ /privacy 301
/profile/ /profile 301
/terms/ /terms 301
/:first/:second/ /:first/:second 301
`,
  );
  const errors = auditPagesRouting({ distDir: directory });
  assert.ok(
    errors.some((error) =>
      error.startsWith("Missing legacy profile redirect: /u/:login"),
    ),
    errors.join("\n"),
  );
});

test("rejects a generic two-segment slash rule declared before /u/:login/", () => {
  const directory = fixture();
  validOutput(
    directory,
    `/about/ /about 301
/badges/ /badges 301
/compare/ /compare 301
/leaderboard/ /leaderboard 301
/report/ /report 301
/privacy/ /privacy 301
/profile/ /profile 301
/terms/ /terms 301
/:first/:second/ /:first/:second 301
/u/:login/ /:login 301
/u/:login /:login 301
`,
  );
  const errors = auditPagesRouting({ distDir: directory });
  assert.ok(
    errors.some((error) =>
      error.includes("must be declared before /:first/:second/"),
    ),
    errors.join("\n"),
  );
});

test("rejects redirect loops, directory output, and asset-shadowing fallbacks", () => {
  const directory = fixture();
  validOutput(
    directory,
    `/about/ /about 301
/about /about/ 301
/:owner/:repo /report?repo=:owner/:repo 302
`,
  );
  write(directory, "about/index.html", "<html></html>");

  const errors = auditPagesRouting({ distDir: directory });
  assert.ok(
    errors.some((error) => error.includes("Directory-format route")),
    errors.join("\n"),
  );
  assert.ok(
    errors.some((error) => error.includes("shadows generated repository HTML")),
    errors.join("\n"),
  );
  assert.ok(
    errors.some((error) => error.startsWith("Redirect cycle:")),
    errors.join("\n"),
  );
});

test("proves badges slash canonicalization is one-way and exactly one hop", () => {
  const directory = fixture();
  validOutput(directory);
  assert.deepEqual(auditPagesRouting({ distDir: directory }), []);

  validOutput(
    directory,
    `/about/ /about 301
/badges/ /badges 301
/badges /badges/ 301
/compare/ /compare 301
/leaderboard/ /leaderboard 301
/report/ /report 301
/privacy/ /privacy 301
/profile/ /profile 301
/terms/ /terms 301
`,
  );
  const errors = auditPagesRouting({ distDir: directory });
  assert.ok(
    errors.some((error) =>
      error.includes("/badges must resolve to badges.html"),
    ),
    errors.join("\n"),
  );
  assert.ok(
    errors.some((error) => error.includes("exactly one redirect")),
    errors.join("\n"),
  );
});

test("rejects a Markdown header block the site no longer has anything to type", () => {
  const directory = fixture();
  validOutput(directory);
  assert.deepEqual(auditPagesRouting({ distDir: directory }), []);

  // Nothing under `/*.md` is a Pages asset any more, so the only response the
  // block can reach is the HTML 404 fallback, relabelled as Markdown.
  write(
    directory,
    "_headers",
    `${HEADERS}\n/*.md\n  Content-Type: text/markdown; charset=utf-8\n`,
  );
  assert.ok(
    auditPagesRouting({ distDir: directory }).some((error) =>
      error.includes("/*.md headers mislabel the HTML 404 fallback"),
    ),
  );
});

test("rejects incomplete badges social metadata and duplicate header blocks", () => {
  const directory = fixture();
  validOutput(directory);
  write(
    directory,
    "badges.html",
    `<html><head>
<title>Badges</title>
<meta name="description" content="Badge builder">
<meta name="robots" content="index,follow">
<link rel="canonical" href="https://gitdebt.com/badges">
<meta property="og:type" content="website">
<meta property="og:image" content="https://api.gitdebt.com/api/og.svg">
<meta name="twitter:card" content="summary">
<meta name="twitter:image" content="https://api.gitdebt.com/api/og.svg">
<script type="application/ld+json">{"@context":"https://schema.org"}</script>
</head></html>`,
  );
  write(
    directory,
    "_headers",
    `/badges
  Cache-Control: public, max-age=0, must-revalidate
/badges
  Cache-Control: public, max-age=0, must-revalidate
`,
  );
  const errors = auditPagesRouting({ distDir: directory });
  assert.ok(
    errors.some((error) => error.includes("og:image:type=image/png")),
    errors.join("\n"),
  );
  assert.ok(
    errors.some((error) => error.includes("Open Graph image")),
    errors.join("\n"),
  );
  assert.ok(
    errors.includes("Duplicate _headers block: /badges"),
    errors.join("\n"),
  );
});
