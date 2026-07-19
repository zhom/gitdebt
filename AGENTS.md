# gitdebt agent guide

gitdebt is an open-source GitHub star-history and repository-health analytics
tool. The backend is Rust/Axum/Postgres, the frontend is static Astro/React,
and `extension/` is a zero-build MV3 browser extension.

## Product boundary

- Star history is the core product; repo-health charts are the differentiator.
- Never add fake-star detection, suspicious-account labels, per-stargazer
  scoring, or name-and-shame features.
- Store stargazer timestamps only. Do not collect stargazer profiles or events.
- Do not add another code path that paginates GitHub's stargazers endpoint.
  Star-series read surfaces must use Postgres.

## Repository map

```text
backend/   Rust API, workers, Postgres cache, charts and rasterization
frontend/  Astro 7 static site, React islands, Tailwind v4
extension/ Browser-native MV3 extension
scripts/   Local database helpers
```

Read the relevant module before changing it. Important backend modules:

- `db.rs`, `cache.rs`: schema and completeness contracts
- `queue.rs`, `worker.rs`, `analyzer.rs`: non-blocking ingestion
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
pnpm --filter gitdebt-frontend test:seo
pnpm --filter gitdebt-frontend build

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
- Cached stargazer timestamps are not re-fetched after completion.
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
- `/report` remains the live client-side discovery route.

## Finish checklist

Run the smallest relevant tests while iterating, then the full affected
backend/frontend/extension checks before handoff. Report any test not run and
any remaining security-audit warning.
