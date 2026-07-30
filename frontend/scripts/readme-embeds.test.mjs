import assert from "node:assert/strict";
import { test } from "node:test";

import {
  assetUrl,
  bestEmbed,
  bestEmbedLanguage,
  markdownEmbed,
  pictureEmbed,
  profileEmbedAssets,
  readmeLink,
  repoEmbedAssets,
} from "../src/lib/readme-embeds.ts";

const API = "https://api.gitdebt.com";
const SITE = "https://gitdebt.com";
const SLUG = "acme/widget";

const assets = repoEmbedAssets(SLUG);
const byId = (id) => assets.find((asset) => asset.id === id);

test("every repository asset targets the repository it was asked for", () => {
  assert.ok(assets.length > 10, "the catalog is not a stub");
  for (const asset of assets) {
    assert.ok(
      asset.path.startsWith(`/api/repos/${SLUG}/`),
      `${asset.id} points somewhere else: ${asset.path}`,
    );
    assert.ok(asset.alt.length > 0, `${asset.id} has no alt text`);
    assert.ok(asset.placement.length > 0, `${asset.id} has no placement`);
    assert.ok(asset.formats.length > 0, `${asset.id} has no formats`);
  }
});

test("asset ids are unique, so a lookup table cannot silently drop one", () => {
  const ids = assets.map((asset) => asset.id);
  assert.equal(new Set(ids).size, ids.length);
});

test("theme is baked only where an asset actually has two variants", () => {
  const chart = byId("chart");
  assert.equal(
    assetUrl(API, chart, { theme: "dark" }),
    `${API}/api/repos/${SLUG}/chart.svg?theme=dark`,
  );
  const og = byId("og");
  assert.equal(og.themed, false);
  assert.equal(
    assetUrl(API, og, { theme: "dark" }),
    `${API}/api/repos/${SLUG}/og.png`,
  );
});

test("a format swap keeps the asset-defining query string", () => {
  assert.equal(
    assetUrl(API, byId("badge-metrics"), { format: "png", theme: "light" }),
    `${API}/api/repos/${SLUG}/badge.png?metrics=stars,forks&theme=light`,
  );
});

test("only routes that rasterize to GIF advertise one", () => {
  assert.ok(byId("chart").formats.includes("gif"));
  assert.ok(byId("heatmap").formats.includes("gif"));
  assert.ok(!byId("badge-metrics").formats.includes("gif"));
  assert.ok(!byId("usage").formats.includes("gif"));
  assert.ok(!byId("og").formats.includes("gif"));
});

test("README attribution rides the link, never the image URL", () => {
  const link = readmeLink(SITE, `/${SLUG}`);
  assert.equal(link, `${SITE}/${SLUG}?ref=readme`);
  for (const asset of assets) {
    const snippet = bestEmbed(API, asset, link);
    for (const url of snippet.matchAll(/(?:src|srcset)="([^"]+)"/g)) {
      assert.ok(
        !url[1].includes("ref="),
        `${asset.id} leaked attribution into an image URL`,
      );
    }
    assert.ok(snippet.includes("ref=readme"), `${asset.id} lost its attribution`);
  }
});

test("a trailing slash on the site origin never doubles up", () => {
  assert.equal(readmeLink(`${SITE}/`, `/${SLUG}`), `${SITE}/${SLUG}?ref=readme`);
  assert.equal(readmeLink(SITE, SLUG), `${SITE}/${SLUG}?ref=readme`);
});

test("published snippets are static: animation stays explicit", () => {
  for (const asset of assets) {
    assert.ok(
      !bestEmbed(API, asset, readmeLink(SITE, `/${SLUG}`)).includes("animate="),
      `${asset.id} published motion nobody asked for`,
    );
  }
});

test("snippets carry no cache-busting revision", () => {
  for (const asset of assets) {
    assert.ok(
      !bestEmbed(API, asset, readmeLink(SITE, `/${SLUG}`)).includes("render="),
      `${asset.id} baked a build revision into a README`,
    );
  }
});

test("a themed asset publishes both variants through <picture>", () => {
  const snippet = pictureEmbed(API, byId("chart"), readmeLink(SITE, `/${SLUG}`));
  assert.match(snippet, /prefers-color-scheme: dark/);
  assert.ok(snippet.includes("theme=dark"));
  assert.ok(snippet.includes("theme=light"));
  assert.match(snippet, /alt="acme\/widget star history"/);
  assert.equal(bestEmbedLanguage(byId("chart")), "html");
});

test("an unthemed asset publishes as plain Markdown", () => {
  const og = byId("og");
  assert.equal(bestEmbedLanguage(og), "markdown");
  assert.equal(
    markdownEmbed(API, og, "LINK"),
    `[![${og.alt}](${API}/api/repos/${SLUG}/og.png)](LINK)`,
  );
});

test("profile assets target the account they were asked for", () => {
  const profile = profileEmbedAssets("octocat");
  assert.ok(profile.length > 0);
  for (const asset of profile) {
    assert.ok(asset.path.startsWith("/api/users/octocat/"));
    assert.ok(asset.alt.includes("octocat"));
  }
});
