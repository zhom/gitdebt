import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const contentSrc = readFileSync(join(root, "content.js"), "utf8");
const popupSrc = readFileSync(join(root, "popup.js"), "utf8");
const manifest = JSON.parse(readFileSync(join(root, "manifest.json"), "utf8"));
const packageJson = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));

function extractFn(src, name) {
  const start = src.indexOf("function " + name + "(");
  assert.notEqual(start, -1, `could not find function ${name}`);
  const open = src.indexOf("{", start);
  let depth = 0;
  for (let i = open; i < src.length; i++) {
    if (src[i] === "{") depth++;
    if (src[i] !== "}") continue;
    depth--;
    if (depth === 0) {
      const body = src.slice(start, i + 1);
      return new Function(`${body}\nreturn ${name};`)();
    }
  }
  throw new Error(`unbalanced braces extracting ${name}`);
}

const classifyAnalyze = extractFn(contentSrc, "classifyAnalyze");
const pingPayload = extractFn(contentSrc, "pingPayload");
const parseCount = extractFn(contentSrc, "parseCount");

test("analysis responses have safe terminal and pending states", () => {
  assert.equal(classifyAnalyze({ not_found: true }), "not_found");
  assert.equal(classifyAnalyze({ history_unavailable: true }), "retrying");
  assert.equal(classifyAnalyze({ history_status: "retrying" }), "retrying");
  assert.equal(classifyAnalyze({ history_status: "not_public" }), "not_found");
  assert.equal(classifyAnalyze({ backfilling: true }), "backfilling");
  assert.equal(
    classifyAnalyze({ history_complete: true, pending: false, queued: 42 }),
    "ready"
  );
  for (const value of [
    null,
    undefined,
    {},
    { pending: true },
    { history_complete: false }
  ]) {
    assert.equal(classifyAnalyze(value), "pending");
  }
});

test("freshness pings omit unreadable star counts", () => {
  for (const stars of [null, undefined, NaN, "12"]) {
    assert.deepEqual(pingPayload("o", "r", stars), { owner: "o", repo: "r" });
  }
  assert.deepEqual(
    pingPayload("o", "r", 0),
    { owner: "o", repo: "r", stars: 0 }
  );
});

test("GitHub star counts parse without treating labels as numbers", () => {
  assert.equal(parseCount("12,345"), 12345);
  assert.equal(parseCount("12.3k"), 12300);
  assert.equal(parseCount("1.2M"), 1200000);
  assert.equal(parseCount("Star"), null);
  assert.equal(parseCount(""), null);
});

test("the store manifest keeps permissions and data declarations narrow", () => {
  assert.equal(manifest.manifest_version, 3);
  assert.equal(manifest.incognito, "not_allowed");
  assert.deepEqual(manifest.permissions, ["storage", "activeTab"]);
  assert.equal("host_permissions" in manifest, false);
  assert.deepEqual(manifest.content_scripts[0].matches, ["https://github.com/*/*"]);
  assert.deepEqual(manifest.web_accessible_resources, [
    {
      resources: ["icons/icon-32.png"],
      matches: ["https://github.com/*/*"]
    }
  ]);
  assert.deepEqual(
    manifest.browser_specific_settings.gecko.data_collection_permissions.required,
    ["browsingActivity", "websiteContent"]
  );
  assert.ok(
    Number.parseFloat(
      manifest.browser_specific_settings.gecko.strict_min_version
    ) >= 140
  );
});

test("package metadata and production settings stay in sync", () => {
  assert.equal(packageJson.version, manifest.version);
  assert.equal(popupSrc.includes("gd-api-base"), false);
  assert.equal(contentSrc.includes("http://localhost"), false);
  assert.ok(contentSrc.includes("isPublicRepoPage()"));
  assert.ok(
    contentSrc.includes('octolytics-dimension-repository_public')
  );
  assert.ok(contentSrc.includes("gitdebt:get-repo-context"));
  assert.ok(popupSrc.includes("gitdebt:get-repo-context"));
  for (const stat of ["bus-factor", "commit-trend"]) {
    assert.ok(contentSrc.includes(`name: "${stat}"`));
  }
});
