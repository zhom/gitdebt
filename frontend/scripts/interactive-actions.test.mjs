import assert from "node:assert/strict";
import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

/**
 * One rule, protected from two sides.
 *
 * `ui/button.tsx` owns the four action treatments and `ButtonLink.tsx` is the
 * only thing allowed to put one of them on navigation. A page that letters the
 * treatment itself — by calling `buttonVariants()` in its frontmatter, or by
 * copying the variant's class string into a `class=` attribute — forks the
 * action system into a second copy that no longer moves when the first one
 * does. That is how a "primary" ends up meaning two different things on two
 * pages.
 *
 * This test used to guard the same rule for a reason that no longer exists: a
 * textured canvas behind every action, which had to hydrate before the control
 * finished painting. That canvas is gone; the single-source rule is not, so the
 * assertions now name the real one: pages compose actions out of `ButtonLink`,
 * and `ButtonLink` stays an anchor.
 */

const PAGES = fileURLToPath(new URL("../src/pages/", import.meta.url));
const BUTTON = new URL("../src/components/ui/button.tsx", import.meta.url);
const BUTTON_LINK = new URL("../src/components/ButtonLink.tsx", import.meta.url);

async function astroPages(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) files.push(...(await astroPages(path)));
    else if (entry.name.endsWith(".astro")) files.push(path);
  }
  return files;
}

const relative = (path) => path.slice(PAGES.length);

test("Astro pages never call buttonVariants directly", async () => {
  const offenders = [];
  for (const path of await astroPages(PAGES)) {
    const source = await readFile(path, "utf8");
    if (source.includes("buttonVariants(")) offenders.push(relative(path));
  }
  assert.deepEqual(
    offenders,
    [],
    "a button-shaped link must be a <ButtonLink>, so the action treatment has " +
      "exactly one source and navigation stays an anchor",
  );
});

test("Astro pages do not hand-letter a variant's class string", async () => {
  // Read the treatments out of the component rather than restating them, so a
  // renamed utility cannot quietly retire the check.
  const button = await readFile(BUTTON, "utf8");
  const signatures = [
    // `primary`: drafting-red fill on the drawing's lettering.
    "bg-signal text-signal-ink",
    // `quiet`: the drawn edge that takes paper under the pointer.
    "border border-rule-strong bg-transparent text-ink",
    // `danger`: red ink on paper, never a red fill.
    "border border-signal/40 bg-transparent text-signal",
  ];
  for (const signature of signatures) {
    assert.ok(
      button.includes(signature),
      `ui/button.tsx no longer contains "${signature}" — update this test's ` +
        "signatures to the variant strings it actually ships",
    );
  }

  const offenders = [];
  for (const path of await astroPages(PAGES)) {
    const source = await readFile(path, "utf8");
    for (const signature of signatures) {
      if (source.includes(signature)) {
        offenders.push(`${relative(path)} → ${signature}`);
      }
    }
  }
  assert.deepEqual(
    offenders,
    [],
    "a page copied a button variant's classes instead of using ButtonLink; " +
      "the treatment then drifts from ui/button.tsx without anything failing",
  );
});

test("ButtonLink renders an anchor", async () => {
  const source = await readFile(BUTTON_LINK, "utf8");

  assert.match(
    source,
    /<a\b/,
    "ButtonLink must render an <a>: it is navigation wearing the action " +
      "treatment, not a button that navigates",
  );
  assert.doesNotMatch(
    source,
    /<button\b/,
    "ButtonLink must not render a <button>; that is what ui/button.tsx is for",
  );
  assert.match(
    source,
    /React\.forwardRef<HTMLAnchorElement/,
    "ButtonLink's ref must be typed to the element it actually renders",
  );
  assert.match(
    source,
    /href\?|AnchorHTMLAttributes<HTMLAnchorElement>/,
    "ButtonLink must accept the anchor's own attributes, href included",
  );
});

test("ButtonLink takes its treatment from ui/button.tsx", async () => {
  const source = await readFile(BUTTON_LINK, "utf8");
  assert.match(
    source,
    /import \{ buttonVariants \} from "@\/components\/ui\/button"/,
    "ButtonLink is the bridge between navigation and the action system; it " +
      "must read the treatment rather than restate it",
  );
  assert.match(
    source,
    /className=\{buttonVariants\(/,
    "ButtonLink must apply buttonVariants() to the anchor it renders",
  );
});

/**
 * The control itself never moves. The leader arrow does, and that is the one
 * authored gesture an action gets — so the patterns below are anchored to
 * exclude `group-hover:`, which is how the mark inside the anchor is driven.
 * Matching `hover:-translate-y` loosely would forbid the intended gesture and
 * the unintended lift with the same rule.
 */
const NO_MOVEMENT = [
  [/(?<!group-)hover:-?translate-/, "the control itself translating on hover"],
  [/(?<!group-)hover:scale-/, "the control itself scaling on hover"],
  [/active:scale-/, "a press-scale"],
  [/hover:underline/, "a hover underline"],
  [/hover:shadow/, "a hover shadow"],
];

test("no action treatment lifts, scales, or grows an underline on hover", async () => {
  for (const url of [BUTTON, BUTTON_LINK]) {
    const file = url.pathname.split("/").pop();
    const source = await readFile(url, "utf8");
    for (const [pattern, description] of NO_MOVEMENT) {
      assert.doesNotMatch(
        source,
        pattern,
        `${file} has ${description}: an action changes state by changing ink ` +
          "and ground, never by moving",
      );
    }
  }
});

test("the leader arrow is the one thing in an action that moves", async () => {
  const source = await readFile(BUTTON_LINK, "utf8");
  assert.match(
    source,
    /group-hover:-translate-y-px group-hover:translate-x-px/,
    "the leader travels up and to the right, along the axis it points; if it " +
      "stops travelling the action has no authored gesture left",
  );
  assert.match(
    source,
    /motion-reduce:transition-none/,
    "the leader's travel must stand down for prefers-reduced-motion",
  );
});
