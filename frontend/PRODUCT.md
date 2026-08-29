# Product

<!-- impeccable:product-schema 1 -->

> Written unattended on 2026-08-29 from the repository, `CLAUDE.md`, `AGENTS.md`,
> and the user's scheduled brief. The interview round did not run: the user
> scheduled this session to start at 04:48 and instructed it to begin. Facts
> marked **[inferred]** were not confirmed by the user and are open to
> correction.

## Platform

web

## Users

Two audiences, both developers, both arriving from GitHub.

- **The evaluator.** A developer deciding whether to adopt or keep a dependency.
  They have many tabs open and one question: is this project actually alive, or
  is it a popular corpse? They arrive by searching the repository name, or from
  a link someone pasted in a review. They want an answer in seconds and they
  leave. **[inferred from the product surface and the `/report` discovery route]**
- **The maintainer.** An open-source author who wants a star-history image in
  their README. They arrive to get a snippet, paste it, and go. They come back
  when the chart looks wrong or when they want a different one.
  **[inferred; supported by `/badges`, `ref=readme` attribution, and the embed library]**

Neither audience is logged in for the primary job. GitHub OAuth exists and
gates a "your live repositories" list, but every read surface works signed out.

## Product Purpose

gitdebt answers two questions about any public GitHub repository, and renders
both as images you can paste anywhere:

1. **When did attention arrive?** The full star history, drawn from historical
   public event data plus forward ingestion, held in Postgres.
2. **What does the commit history say about upkeep?** Four readings —
   maintenance cadence, ownership concentration, repair load, and debt markers —
   derived from commits, not from stars.

Success is a developer getting a truthful answer fast, and a maintainer getting
a chart that keeps working in a README for years.

## Positioning

Star history alone is a commodity; several tools draw that curve. gitdebt's
mechanism a neighbour could not truthfully copy is the pairing: **the star curve
and the repair-load reading on the same repository, on one page, from two
independent sources.** Stars are what the crowd did. The commit history is what
the maintainers did. gitdebt is the only surface that puts the popularity claim
next to the upkeep evidence.

The second differentiator is durability: every chart is a deterministic,
cacheable image URL that renders without JavaScript and survives in a README.

## Operating Context

- The product is read far more often as an **embedded image in someone else's
  README** than as a visited page. The rendered asset is a primary surface, not
  an export.
- Discovery is overwhelmingly organic search on `owner/repo`. Static pages are
  prerendered for a catalog of repositories; every other repository is resolved
  live in the browser.
- Machine readers matter. The site ships Markdown and prompt surfaces for coding
  agents (`/api/md/{owner}/{repo}`, the `agent_*` backend modules), and an
  agent that lands on a missing page must still be able to reach real data.
- A browser extension surfaces the same charts on github.com itself.
- Assets are consumed on GitHub, where **light and dark both occur**, so every
  rendered asset ships both and the snippet switches between them.

## Capabilities and Constraints

Confirmed from `CLAUDE.md` / `AGENTS.md`; these are binding.

- **Two binaries.** `gitdebt-api` (HTTP, Postgres + Redis) and `gitdebt-worker`
  (ingestion, Postgres). Any replica count of either is safe.
- **Static frontend, always.** Astro output stays fully static. No server
  adapter, no Pages Functions, no middleware, no `prerender = false`.
- **Deterministic renderers.** Identical inputs must produce identical bytes.
  SVGs bake concrete theme colors. README assets are static by default;
  animation is explicit. OG images are 1200x630 PNGs.
- **Never paginate stargazers.** Star-series reads come from Postgres. No second
  code path against GitHub's stargazers endpoint.
- **Completeness gates reads.** A `*_complete` flag; writers flip it atomically;
  errors leave data incomplete.
- **Cold requests enqueue.** No synchronous GitHub pagination on a request path.

### Product boundaries that are not negotiable

- **No fake-star detection**, suspicious-account labels, per-stargazer scoring,
  or name-and-shame features. Ever.
- **No stargazer profiling.** Star timestamps plus an opaque event ID for
  idempotency; no actors, no profiles, no payloads.
- **Provenance copy states SOURCE, COVERAGE DATE, and STATE only.** Never a
  count, a percentage, a completeness score, or any figure implying how many
  stars are missing — an archive series counts re-stars and can exceed the
  repository's own total, so a gap number is confidently wrong. User-visible
  copy says "historical data" and never names the archive. `history-freshness.ts`
  owns every one of these strings.
- **There is no repository connection flow.** No copy may offer connecting a
  repository as a remedy for the July 2026 stargazer restriction.

### Terminology

"Star history", "repository health", "historical data" (never the archive's
name), "report" (the per-repository page), "embed" / "snippet" (the README
asset).

## Brand Commitments

- Name is lowercase **gitdebt**. Author entity: Internet Technology Services LLC.
- MIT licensed, open source, free. `github.com/zhom/gitdebt`.
- **Binding visual constraint given by the user in this brief:** the theme is
  **light**, at Vercel's temperature and restraint. The previous dark-first
  dither system is retired and every dither artifact is removed from the
  project. Everything is animated, with unique experiences. Minimal, but it must
  pop.
- The site must be animated, SEO-optimised, sharable, and agent-friendly.

## Evidence on Hand

Real, and usable in the design:

- Live repository data for every analyzed repository, from the product's own API.
- A real leaderboard of star velocity across 1/7/30-day windows.
- A real count of analyzed repositories (`/api/sitemap/repos`).
- The product's own star count on its own nav.
- Curated category sets in `frontend/src/data/categories.ts`.
- The full embed library in `lib/readme-embeds.ts`, byte-parity-locked to the
  Rust renderers, so any count derived from it cannot drift from what ships.

Absences that must not be fabricated: **no customers, no testimonials, no
pricing, no benchmarks, no logo wall, no adoption claims.** The product has no
named users to show. Any figure on any surface must come from the API or from
the embed library.

## Product Principles

1. **The answer, then the evidence.** A visitor should learn whether the
   repository is alive before they learn how gitdebt knows.
2. **Two sources, never blended.** Stars and commits are independent readings.
   The design must keep them legible as separate claims.
3. **The image is the product.** Anything that cannot survive as a static,
   deterministic, JavaScript-free image in someone else's README is a secondary
   surface.
4. **Say what is known; never imply what is not.** Provenance is stated, never
   scored. Missing data is named, never quantified.
5. **Machines are users too.** Every page a person can read, an agent can read.

## Accessibility & Inclusion

- Content is visible without JavaScript; islands enhance, they never gate.
- All motion respects `prefers-reduced-motion`; the site also honours
  `prefers-reduced-transparency` and `prefers-contrast: more` today, and must
  continue to.
- Charts must not carry meaning in hue alone.
