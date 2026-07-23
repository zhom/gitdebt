import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, test } from "node:test";
import { auditAgentSurfaces } from "./audit-agent-surfaces.mjs";

const temporaryDirectories = [];

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

function fixture() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "gitdebt-agents-"));
  temporaryDirectories.push(directory);
  fs.writeFileSync(directory + "/llms.txt", "# gitdebt");
  fs.writeFileSync(directory + "/llms-full.txt", "# gitdebt agent reference");
  fs.writeFileSync(
    directory + "/robots.txt",
    ["GPTBot", "ClaudeBot", "PerplexityBot", "Google-Extended"]
      .map((bot) => `User-agent: ${bot}\nAllow: /`)
      .join("\n\n"),
  );
  return directory;
}

test("accepts HTML pages with exact generated Markdown counterparts", () => {
  const directory = fixture();
  fs.mkdirSync(path.join(directory, "owner", "repo"), { recursive: true });
  fs.writeFileSync(
    path.join(directory, "owner", "repo", "index.html"),
    '<link rel="alternate" type="text/markdown" href="https://gitdebt.com/owner/repo.md">',
  );
  fs.mkdirSync(path.join(directory, "owner"), { recursive: true });
  fs.writeFileSync(path.join(directory, "owner", "repo.md"), "# owner/repo");

  assert.deepEqual(auditAgentSurfaces({ distDir: directory }).errors, []);
});

test("rejects a page whose advertised Markdown file is absent", () => {
  const directory = fixture();
  fs.writeFileSync(
    path.join(directory, "index.html"),
    '<link rel="alternate" type="text/markdown" href="https://gitdebt.com/index.md">',
  );

  const result = auditAgentSurfaces({ distDir: directory });
  assert.ok(result.errors.includes("/: missing generated /index.md"));
});

test("page sitemap and emitted comparison pages share one path source", () => {
  const root = path.resolve(import.meta.dirname, "..");
  const sitemap = fs.readFileSync(path.join(root, "src/pages/sitemaps/pages.xml.ts"), "utf8");
  const comparison = fs.readFileSync(path.join(root, "src/pages/vs/[owner1]/[repo1]/[owner2]/[repo2].astro"), "utf8");
  assert.match(sitemap, /staticComparisonPaths\(\)/);
  assert.match(comparison, /staticComparisonPaths\(\)/);
});

test("profiles live at the root and share one login source", () => {
  const root = path.resolve(import.meta.dirname, "..");
  const read = (relative) => fs.readFileSync(path.join(root, relative), "utf8");

  assert.ok(
    !fs.existsSync(path.join(root, "src/pages/u")),
    "the legacy /u profile route must be gone",
  );
  assert.match(read("src/pages/[login].astro"), /staticLoginPaths\(\)/);
  assert.match(read("src/pages/sitemaps/pages.xml.ts"), /staticLogins\(\)/);
  assert.match(read("src/pages/[...path].md.ts"), /staticLogins\(\)/);

  for (const relative of [
    "src/pages/[login].astro",
    "src/pages/sitemaps/pages.xml.ts",
    "src/pages/[...path].md.ts",
    "src/lib/agent-markdown.ts",
    "src/components/ProfileCardPreview.tsx",
    "src/pages/[owner]/[repo].astro",
  ]) {
    assert.doesNotMatch(
      read(relative),
      /\/u\/\$\{/,
      `${relative} still links the legacy /u profile prefix`,
    );
  }
});

test("the profile drops the monthly commit-volume surface", () => {
  const root = path.resolve(import.meta.dirname, "..");
  const profile = fs.readFileSync(
    path.join(root, "src/components/LiveUserProfile.tsx"),
    "utf8",
  );
  assert.doesNotMatch(profile, /name: "commit-trend"/);
  assert.match(profile, /name: "languages"/);
});
