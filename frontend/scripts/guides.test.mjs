import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";

/**
 * The house rules for `src/content/guides/*.md`.
 *
 * The content schema checks types; these check what a schema cannot: that the
 * served `<title>` and description fit a search result, that the steps are
 * numbered in order, that every related link resolves, and that each guide is
 * listed in `llms.txt` for agents.
 */

const ROOT = new URL("../", import.meta.url).pathname;
const GUIDES = path.join(ROOT, "src/content/guides");
const TITLE_SUFFIX = " · gitdebt";
const SALES_WORDS = [
  "powerful",
  "seamless",
  "seamlessly",
  "effortless",
  "effortlessly",
  "robust",
  "unlock",
  "supercharge",
  "game-changer",
  "leverage",
  "streamline",
  "cutting-edge",
  "best-in-class",
];

/** The front matter subset the guides use: scalars and one list of strings. */
function parseGuide(file) {
  const source = fs.readFileSync(path.join(GUIDES, file), "utf8");
  const match = /^---\n([\s\S]*?)\n---\n([\s\S]*)$/.exec(source);
  assert.ok(match, `${file}: missing front matter`);
  const data = {};
  let listKey = null;
  for (const line of match[1].split("\n")) {
    const item = /^ {2}- (.+)$/.exec(line);
    if (item && listKey) {
      data[listKey].push(item[1].trim());
      continue;
    }
    const pair = /^([A-Za-z]+):\s*(.*)$/.exec(line);
    assert.ok(pair, `${file}: unreadable front matter line ${JSON.stringify(line)}`);
    const [, key, raw] = pair;
    if (raw === "") {
      listKey = key;
      data[key] = [];
    } else {
      listKey = null;
      data[key] = raw.replace(/^"(.*)"$/, "$1");
    }
  }
  return { file, slug: file.replace(/\.md$/, ""), data, body: match[2] };
}

function prose(body) {
  return body.replace(/```[\s\S]*?```/g, " ");
}

function wordCount(body) {
  return prose(body)
    .replace(/[|#>*`_\-[\]()]/g, " ")
    .split(/\s+/)
    .filter((word) => /[A-Za-z0-9]/.test(word)).length;
}

const guides = fs
  .readdirSync(GUIDES)
  .filter((file) => file.endsWith(".md"))
  .sort()
  .map(parseGuide);

test("there are 8 to 15 guides with unique order and slugs", () => {
  assert.ok(guides.length >= 8 && guides.length <= 15, `${guides.length} guides`);
  const orders = guides.map((guide) => Number(guide.data.order));
  assert.equal(new Set(orders).size, orders.length, "order values must be unique");
  for (const guide of guides) {
    assert.match(guide.slug, /^[a-z0-9]+(?:-[a-z0-9]+)*$/, `${guide.file}: slug`);
  }
});

test("titles and descriptions fit a search result", () => {
  const titles = new Set();
  const descriptions = new Set();
  for (const { file, data } of guides) {
    assert.match(data.title, /^How to /, `${file}: the H1 states the task`);
    const served = `${data.metaTitle}${TITLE_SUFFIX}`;
    assert.ok(served.length < 60, `${file}: <title> is ${served.length} characters`);
    assert.ok(
      data.description.length < 155,
      `${file}: description is ${data.description.length} characters`,
    );
    assert.ok(!titles.has(data.metaTitle), `${file}: duplicate metaTitle`);
    assert.ok(!descriptions.has(data.description), `${file}: duplicate description`);
    titles.add(data.metaTitle);
    descriptions.add(data.description);
    assert.match(data.updatedDate, /^\d{4}-\d{2}-\d{2}$/, `${file}: updatedDate`);
  }
});

test("related guides resolve, and never to the guide itself", () => {
  const slugs = new Set(guides.map((guide) => guide.slug));
  for (const { file, slug, data } of guides) {
    assert.ok(data.related.length >= 2 && data.related.length <= 4, `${file}: 2 to 4 related`);
    for (const related of data.related) {
      assert.ok(slugs.has(related), `${file}: unknown related guide ${related}`);
      assert.notEqual(related, slug, `${file}: names itself as related`);
    }
  }
});

test("links between guides point at guides that exist", () => {
  const slugs = new Set(guides.map((guide) => guide.slug));
  for (const { file, body } of guides) {
    for (const [, target] of body.matchAll(/\]\(\/guides\/([^)#?]+)\)/g)) {
      assert.ok(slugs.has(target), `${file}: link to missing guide ${target}`);
    }
  }
});

test("steps are numbered H2s in order", () => {
  for (const { file, body } of guides) {
    const numbered = [...prose(body).matchAll(/^## (\d+)\. /gm)].map((m) => Number(m[1]));
    assert.ok(numbered.length >= 3, `${file}: at least three numbered steps`);
    numbered.forEach((value, index) => {
      assert.equal(value, index + 1, `${file}: step ${index + 1} is numbered ${value}`);
    });
    assert.doesNotMatch(prose(body), /^# /m, `${file}: the page letters the H1`);
  }
});

test("each guide runs 700 to 1800 words of prose", () => {
  for (const { file, body } of guides) {
    const words = wordCount(body);
    assert.ok(words >= 700 && words <= 1800, `${file}: ${words} words`);
  }
});

test("the copy keeps the house style", () => {
  for (const { file, data, body } of guides) {
    const text = `${data.title}\n${data.metaTitle}\n${data.description}\n${body}`;
    assert.doesNotMatch(text, /[–—]/, `${file}: no en or em dashes`);
    const words = prose(text).toLowerCase();
    for (const word of SALES_WORDS) {
      assert.doesNotMatch(
        words,
        new RegExp(`(?<![\\w-])${word}(?![\\w-])`),
        `${file}: drop "${word}"`,
      );
    }
  }
});

test("llms.txt lists the index and every guide", () => {
  const llms = fs.readFileSync(path.join(ROOT, "public/llms.txt"), "utf8");
  assert.ok(llms.includes("https://gitdebt.com/guides\n"), "llms.txt names /guides");
  for (const { slug } of guides) {
    assert.ok(
      llms.includes(`https://gitdebt.com/guides/${slug}`),
      `llms.txt is missing /guides/${slug}`,
    );
  }
});
