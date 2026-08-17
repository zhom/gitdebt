/**
 * The JavaScript half of the cross-language goldens.
 *
 * Two documents are checked in under `backend/tests/fixtures/`:
 *
 * - `embed-parity.md` — every embeddable asset, its catalog metadata, and the
 *   snippet it produces.
 * - `prompt-parity.md` — the full "Ask an agent" prompt in every state that
 *   changes it.
 *
 * `backend/tests/parity.rs` renders both from `agent_embeds.rs` /
 * `agent_prompt.rs` and asserts byte equality; this file asserts the same of
 * `src/lib/readme-embeds.ts` and `src/lib/agent-prompt.ts`. The API now serves
 * the Markdown an agent reads while these modules still feed the /badges page
 * and the "Ask an agent" button, so without a shared golden the two could
 * quietly hand out different snippets — and a differently worded prompt — for
 * the same repository.
 *
 * The headers are reproduced literally rather than sliced off the fixtures: a
 * comparison against bytes taken from the file under test would pass no matter
 * what either implementation did to it.
 */

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";

import {
  PLACEHOLDER_SLUG,
  profileAgentPrompt,
  repoAgentPrompt,
  starFacts,
} from "../src/lib/agent-prompt.ts";
import {
  EMBED_RULES,
  assetUrl,
  bestEmbed,
  bestEmbedLanguage,
  profileEmbedAssets,
  readmeLink,
  repoEmbedAssets,
} from "../src/lib/readme-embeds.ts";

const SITE = "https://gitdebt.com";
const API = "https://api.gitdebt.com";
const SLUG = "OWNER/REPO";
const LOGIN = "LOGIN";
/**
 * `OWNER/REPO` is the placeholder slug, so it always selects the "resolve the
 * repository" opening. Reaching the other branch needs a slug that is not the
 * placeholder; everything else about the two renders is identical.
 */
const RESOLVED_SLUG = "owner/repo";

const FIXTURES = path.resolve(
  import.meta.dirname,
  "../../backend/tests/fixtures",
);

const EMBED_FIXTURE = path.join(FIXTURES, "embed-parity.md");
const PROMPT_FIXTURE = path.join(FIXTURES, "prompt-parity.md");

const EMBED_HEADER = `<!--
embed-parity.md — the cross-language golden for gitdebt's README embed catalog.

What this is: every asset gitdebt can put in somebody else's README — for a
repository and for a profile — with the catalog metadata the /badges page and
the API's Markdown both read off it, and the exact snippet each one produces.

Two implementations render it: backend/src/agent_embeds.rs, asserted by
backend/tests/parity.rs, and frontend/src/lib/readme-embeds.ts, asserted by
frontend/scripts/embed-parity.test.mjs. Both compare byte for byte.

A diff here means the two implementations have drifted. Fix the drift: work out
which side is wrong and change that code. Do not regenerate this file to make a
test pass — that only lets the API and the /badges page disagree quietly.

Fixed inputs: slug OWNER/REPO, login LOGIN, site https://gitdebt.com,
api https://api.gitdebt.com.

Format, deliberately trivial to reproduce in any language:
  "# Repository assets — OWNER/REPO", then one section per asset in catalog
  order, then "# Profile assets — LOGIN" and its sections, then "# Rules" with
  EMBED_RULES as "- " bullets, one line each. An asset section is "## " + the
  asset id, a blank line, one "- key: value" line per catalog field, one
  "- url(FORMAT): " line per advertised format, a blank line, then the
  published snippet fenced in its own language. Sections are separated by one
  blank line and the file ends with a single newline.
-->`;

const PROMPT_HEADER = `<!--
prompt-parity.md — the cross-language golden for the "Ask an agent" prompt.

What this is: the complete prompt gitdebt hands a coding agent, rendered in
every state that changes it — a repository with nothing measured, one with a
complete star history, one whose curve is historical star activity (with and
without a resolved total), and a profile with and without measured totals.

Two implementations render it: backend/src/agent_prompt.rs, asserted by
backend/tests/parity.rs, and frontend/src/lib/agent-prompt.ts, asserted by
frontend/scripts/embed-parity.test.mjs. Both compare byte for byte. The
frontend copy still backs the clipboard button while the Rust copy is what the
API serves, so a sentence reworded on one side and not the other would ship two
different prompts for the same repository.

A diff here means the two implementations have drifted. Fix the drift: work out
which side is wrong and change that code. Do not regenerate this file to make a
test pass.

Fixed inputs, no wall clock anywhere: site https://gitdebt.com,
api https://api.gitdebt.com, login LOGIN, and a synthetic star history of 600
daily points of +3 stars from 2013-03-09 followed by 90 daily points of +30.
OWNER/REPO is the placeholder slug, which is what selects the "resolve the
repository" opening; owner/repo is a resolved slug, the only way to reach the
other branch.

Format: one "===== BEGIN <label> =====" / "===== END <label> =====" pair per
rendered prompt with the prompt verbatim in between, blocks separated by one
blank line, single trailing newline.
-->`;

const DAY_MS = 86_400_000;

/**
 * A star series with a lifetime pace and a much faster trailing quarter, so the
 * derived summary exercises both windows, the "accelerating" verdict, and the
 * first-star month label rather than leaving them null.
 */
function fixedHistory() {
  const start = Date.UTC(2013, 2, 9);
  const points = [];
  for (let index = 0; index < 600; index += 1) {
    points.push({
      date: new Date(start + index * DAY_MS).toISOString().slice(0, 10),
      stars: (index + 1) * 3,
    });
  }
  for (let index = 1; index <= 90; index += 1) {
    points.push({
      date: new Date(start + (599 + index) * DAY_MS).toISOString().slice(0, 10),
      stars: 1_800 + index * 30,
    });
  }
  return points;
}

/** One asset: every catalog field, every advertised URL, and its snippet. */
function assetSection(api, asset, link) {
  const lines = [
    `## ${asset.id}`,
    "",
    `- name: ${asset.name}`,
    `- purpose: ${asset.purpose}`,
    `- placement: ${asset.placement}`,
    `- group: ${asset.group}`,
    `- themed: ${asset.themed}`,
    `- formats: ${asset.formats.join(", ")}`,
  ];
  for (const format of asset.formats) {
    lines.push(`- url(${format}): ${assetUrl(api, asset, { format })}`);
  }
  lines.push(
    "",
    `\`\`\`${bestEmbedLanguage(asset)}`,
    bestEmbed(api, asset, link),
    "```",
  );
  return lines.join("\n");
}

/** The mirror of `parity::embed_parity_document`. */
export function embedParityDocument(slug, login, site, api) {
  const sections = [EMBED_HEADER, `# Repository assets — ${slug}`];
  const repoLink = readmeLink(site, `/${slug}`);
  for (const asset of repoEmbedAssets(slug)) {
    sections.push(assetSection(api, asset, repoLink));
  }

  sections.push(`# Profile assets — ${login}`);
  const profileLink = readmeLink(site, `/${login}`);
  for (const asset of profileEmbedAssets(login)) {
    sections.push(assetSection(api, asset, profileLink));
  }

  sections.push(`# Rules\n\n${EMBED_RULES.map((rule) => `- ${rule}`).join("\n")}`);
  return `${sections.join("\n\n")}\n`;
}

/** One rendered prompt, delimited so the prompt's own headings stay readable. */
function promptSection(label, body) {
  return `===== BEGIN ${label} =====\n${body}===== END ${label} =====`;
}

/** The mirror of `parity::prompt_parity_document`. */
export function promptParityDocument(site, api) {
  const history = fixedHistory();
  const complete = starFacts(history, 4_500, false);
  const approximate = starFacts(history, 4_500, true);
  const approximateWithoutTotal = starFacts(history, null, true);

  return `${[
    PROMPT_HEADER,
    promptSection(
      `repo ${PLACEHOLDER_SLUG} — nothing measured`,
      repoAgentPrompt({
        slug: PLACEHOLDER_SLUG,
        siteOrigin: site,
        apiBase: api,
      }),
    ),
    promptSection(
      `repo ${RESOLVED_SLUG} — complete star history`,
      repoAgentPrompt({
        slug: RESOLVED_SLUG,
        siteOrigin: site,
        apiBase: api,
        stars: complete,
      }),
    ),
    promptSection(
      `repo ${RESOLVED_SLUG} — approximate star history`,
      repoAgentPrompt({
        slug: RESOLVED_SLUG,
        siteOrigin: site,
        apiBase: api,
        stars: approximate,
      }),
    ),
    promptSection(
      `repo ${RESOLVED_SLUG} — approximate star history, total not resolved`,
      repoAgentPrompt({
        slug: RESOLVED_SLUG,
        siteOrigin: site,
        apiBase: api,
        stars: approximateWithoutTotal,
      }),
    ),
    promptSection(
      `profile ${LOGIN} — measured`,
      profileAgentPrompt({
        login: LOGIN,
        siteOrigin: site,
        apiBase: api,
        totalStars: 90_120,
        reposIncluded: 42,
      }),
    ),
    promptSection(
      `profile ${LOGIN} — nothing measured`,
      profileAgentPrompt({ login: LOGIN, siteOrigin: site, apiBase: api }),
    ),
  ].join("\n\n")}\n`;
}

test("readme-embeds.ts reproduces the embed golden byte for byte", () => {
  assert.equal(
    embedParityDocument(SLUG, LOGIN, SITE, API),
    fs.readFileSync(EMBED_FIXTURE, "utf8"),
  );
});

test("agent-prompt.ts reproduces the prompt golden byte for byte", () => {
  assert.equal(
    promptParityDocument(SITE, API),
    fs.readFileSync(PROMPT_FIXTURE, "utf8"),
  );
});

test("the goldens are rendered, not memoized: the same inputs render twice alike", () => {
  assert.equal(
    embedParityDocument(SLUG, LOGIN, SITE, API),
    embedParityDocument(SLUG, LOGIN, SITE, API),
  );
  assert.equal(promptParityDocument(SITE, API), promptParityDocument(SITE, API));
});

/**
 * The goldens are only worth their bytes if they cover the whole catalog. A
 * new asset that nobody added a section for has to fail here rather than sit
 * unguarded.
 */
test("the embed golden covers every asset in the catalog", () => {
  const golden = fs.readFileSync(EMBED_FIXTURE, "utf8");
  const assets = [
    ...repoEmbedAssets(SLUG),
    ...profileEmbedAssets(LOGIN),
  ];
  assert.equal(golden.split("\n## ").length - 1, assets.length);
  for (const asset of assets) {
    assert.ok(
      golden.includes(`\n- purpose: ${asset.purpose}\n`),
      `${asset.id} is missing from the golden`,
    );
    for (const format of asset.formats) {
      assert.ok(
        golden.includes(`- url(${format}): ${assetUrl(API, asset, { format })}\n`),
        `${asset.id} does not publish its ${format} URL in the golden`,
      );
    }
  }
});
