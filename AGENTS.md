# gitdebt agent guide

gitdebt is an open-source GitHub star-history and repository-health analytics
tool. The backend is Rust/Axum/Postgres built as two binaries — `gitdebt-api`
serves HTTP and `gitdebt-worker` runs ingestion and analysis; any replica
count of either is safe. The frontend is static Astro/React, and `extension/`
is a zero-build MV3 browser extension. The visual system is a light-first
dimensioned technical drawing; light is the default theme for the site and for
server-rendered assets.

## Design system

The product measures a repository, so every surface is drawn as a technical
drawing of one. `DESIGN.md` is the full record; these are the rules that a
change can silently break.

- **Every line terminates on something real.** A dimension line spans two
  measured points and carries a value; a leader points at a datum; a frame
  encloses a sheet. A rule that measures nothing, separates nothing and
  encloses nothing does not belong on the sheet. There is no page grid, no
  graph paper, no texture, no gradient, and no glow anywhere in the product.
- **One saturated colour.** Drafting red (`#cc291f` light, `#f0674e` dark) is
  spent only on a measured value and its terminators, on the primary data
  trace, and on at most one primary action per surface. It is never a tag, a
  status dot, or a category colour. Everything else is graphite in three steps.
- **Three type voices.** Format 1452 (self-hosted, OFL) letters the drawing —
  headings, field labels, measured values. `system-ui` carries prose. Geist
  Mono carries data. A caption is prose set small, not a field label; keeping
  those distinct is what stops one tracked-caps costume landing on every short
  string.
- **Square, with one cut.** Everything is square except a 10px chamfer on the
  bottom-right of a panel and of the primary action. Depth comes from the
  paper/table step and from line weight, not from shadow.
- **Content is never gated on an animation.** Nothing starts at `opacity: 0`
  waiting for JavaScript, a timeline, or an observer. Charts render as
  server-side SVG plus a screen-reader table, so the data is in the markup
  before any script runs. Motion is the pen travelling over a finished drawing,
  and every animation resolves instantly under `prefers-reduced-motion`.
- The site's `globals.css` holds the palette in oklch and `backend/src/theme.rs`
  holds the same colours as hex. They must not drift: if they do, a chart stops
  belonging to the page that embeds it.

## Product boundary

- Star history is the core product; repo-health charts are the differentiator.
- Never add fake-star detection, suspicious-account labels, per-stargazer
  scoring, or name-and-shame features.
- Store star timestamps plus the opaque GH Archive event ID needed for
  idempotency. Do not collect actors, stargazer profiles, or event payloads.
- Do not add another code path that paginates GitHub's stargazers endpoint.
  Star-series read surfaces must use Postgres.
- Provenance copy states SOURCE, COVERAGE DATE, and STATE — never a count,
  percentage, completeness score, or any figure implying how many stars are
  missing. An archive series counts re-stars and can exceed the repository's own
  star total, so a gap number is confidently wrong exactly where it is most
  eye-catching. User-visible copy names the source as "historical data" and
  never names GH Archive; the module and env-var names below still do, and that
  asymmetry is deliberate. `history-freshness.ts` owns every one of these strings;
  `SeriesProvenance.tsx` and `provenance-embed.ts` render them and add none of
  their own. Enforced by `history-freshness.test.mjs` and
  `provenance-embed.test.mjs`.
- There is no repository connection flow. `repo_star_grants` exists in `db.rs`
  with no reader, no writer, and no route, and its schema comment reads as
  though the feature shipped. Until it does, no copy may offer connecting a
  repository as a remedy for the July 2026 stargazer restriction.

## Repository map

```text
backend/   Rust API + worker binaries, Postgres cache, charts, rasterization
frontend/  Astro 7 static site, React islands, Tailwind v4
extension/ Browser-native MV3 extension
scripts/   Local database helpers
```

`gitdebt-api` needs Postgres (`DATABASE_URL`) and Redis (`REDIS_URL`);
`gitdebt-worker` needs Postgres. `scripts/db.sh up` starts both stores
locally.

Read the relevant module before changing it. Important backend modules:

- `db.rs`, `cache.rs`: schema and completeness contracts
- `queue.rs`, `worker.rs`, `analyzer.rs`: non-blocking ingestion
- `gh_archive.rs`, `archive_worker.rs`, `gh_archive_hourly.rs`: BigQuery
  history and raw-hour forward ingestion
- `chart.rs`, `repo_charts.rs`, `cards.rs`, `badge.rs`, `og.rs`: rendering
- `export.rs`, `aggregate.rs`, `usage.rs`: Postgres-backed data surfaces
- `repo_endpoints.rs`, `api.rs`: routing and response policy

## Commands

```bash
# Backend
scripts/db.sh up
GITDEBT_TEST_DATABASE_URL=postgres://gitdebt:gitdebt@localhost:5432/gitdebt \
  cargo test --workspace
cargo build --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check

# Frontend
pnpm install --frozen-lockfile
pnpm --filter gitdebt-frontend test          # every scripts/*.test.mjs
PUBLIC_API_BASE=https://api.gitdebt.com \
  pnpm --filter gitdebt-frontend build       # check + build + 3 audit scripts

# Extension
npm --prefix extension ci
npm --prefix extension run package
```

## Required invariants

- Builds and tests must complete without compiler, Astro, or lint warnings.
- Preserve user changes in a dirty worktree; do not rewrite unrelated files.
- Readers must never serve partial cache data. A `*_complete` flag gates reads.
- Writers replace entity data and flip completeness atomically in one
  transaction. Errors must leave data incomplete.
- Exact GitHub-API snapshots are not re-fetched after completion. Approximate
  GH Archive activity may refresh from later day/hour partitions.
- GitHub 404/private/deleted entities are tombstoned.
- Cold analysis requests enqueue durable work and return promptly; never
  paginate GitHub synchronously on a request path.
- Queue jobs, GitHub budgets, and worker state remain durable in Postgres.
- Validate every repo slug and external registry override. Never echo internal
  errors in 5xx responses.
- Forwarded client-IP headers are trusted only from configured proxies.
- OAuth tokens stay encrypted at rest; session and webhook checks remain
  constant-time.

If `db.rs` or `cache.rs` changes, add a test that exercises the affected cache
or startup invariant.

## Rendering and API contracts

- Renderers are deterministic: identical inputs must produce identical bytes.
- SVGs bake concrete theme colors. README assets default to static output;
  animation is explicit.
- Raster and SVG cache keys must include all data-affecting options.
- Date ranges use inclusive `from`/`to` plus optional `rebase=1`; invalid ranges
  return 400.
- Exports, aggregates, cards, leaderboards, and sitemap feeds read Postgres and
  do not call GitHub on their request paths.
- OG images are 1200×630 PNGs.
- Keep README/extension attribution values (`ref=readme`, `ref=extension`)
  stable and out of image URLs.

When adding a repo-health chart: implement a pure renderer with math tests,
wire it through the shared SVG/PNG/WebP dispatcher, then surface it in the
frontend grid.

## Frontend contract

- Astro output stays fully static; do not add Pages Functions, a server adapter,
  Worker middleware, or `prerender = false`.
- `build-catalog.ts` is the single build-time catalog source.
- Production catalog failures must fail the build instead of publishing an
  accidentally empty catalog.
- Sitemap URLs must exactly match emitted, indexable pages.
- Never run two frontend builds against one `dist`. `astro build` empties
  `outDir` at startup only, so an overlapping build keeps writing pages from its
  own older catalog window into the same tree. The survivors are pages the
  second build's window no longer contains, and `audit-seo` then fails with
  `Indexable pages outside the sitemap` naming repositories that look like a
  sitemap bug and are not. Confirm no build is running, and check the reported
  page count against the HTML file count before believing such a failure.
- `/report` remains the live client-side discovery route.

## Finish checklist

Run the smallest relevant tests while iterating, then the full affected
backend/frontend/extension checks before handoff. Report any test not run and
any remaining security-audit warning.
