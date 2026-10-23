import assert from "node:assert/strict";
import { test } from "node:test";

import { parseRepoSlug } from "../src/lib/repo-slug.ts";

test("a slug is itself", () => {
  assert.equal(parseRepoSlug("vercel/next.js"), "vercel/next.js");
  assert.equal(parseRepoSlug("  facebook/react  "), "facebook/react");
});

test("every way of pasting a repository names the same slug", () => {
  for (const pasted of [
    "https://github.com/vercel/next.js",
    "http://www.github.com/vercel/next.js/",
    "github.com/vercel/next.js",
    "https://github.com/vercel/next.js/tree/canary/packages",
    "https://github.com/vercel/next.js?tab=readme-ov-file#readme",
    "https://github.com/vercel/next.js.git",
    "git@github.com:vercel/next.js.git",
  ]) {
    assert.equal(parseRepoSlug(pasted), "vercel/next.js", pasted);
  }
});

test("a value that names no repository is refused", () => {
  for (const value of ["", "react", "not a repo!!", "https://gitlab.com/a/b", "github.com/onlyowner"]) {
    assert.equal(parseRepoSlug(value), null, value);
  }
});
