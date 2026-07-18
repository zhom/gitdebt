# gitdebt — agent guide

An open-source **GitHub star-history + repo-"debt" analytics** tool — a faster, more insightful alternative to star-history.com. Backend in Rust + Postgres, frontend in Astro. Charts a repo's cumulative star history (single repo and multi-repo overlay, embeddable in READMEs) and surfaces code-health/debt signals: bug-magnet files, churn, contributors, language lines, commit heatmap, TODO/FIXME trend, bus factor, commit trend. Around the charts: raw star exports (CSV/JSON), README stat cards (user + repo), org/user aggregate star history, and a celebratory repo leaderboard.

> **Product constraint (2026-05):** gitdebt does **NOT** do fake-star detection and contains **no** code that marks users/accounts as suspicious. That direction was removed deliberately — it can't be done correctly without privileged access to GitHub's full dataset. Do **not** reintroduce "fake / real / organic / suspicious" star framing, per-stargazer scoring, a detector pipeline, or a name-and-shame leaderboard. For star history we only need stargazer *timestamps*, not profiles/events.

## Layout

```
gitdebt/
├── Cargo.toml          # Rust workspace
├── docker-compose.yml  # local Postgres
├── scripts/db.sh       # postgres up/down/psql/logs
├── backend/            # Rust (axum) — gitdebt-api on :8787
│   └── src/
│       ├── main.rs
│       ├── lib.rs
│       ├── api.rs            # axum routes
│       ├── analyzer.rs       # pipeline: cache check → enqueue fetch → assemble payload
│       ├── cache.rs          # Postgres-backed cache repository
│       ├── db.rs             # sqlx PgPool + schema
│       ├── queue.rs          # persistent fetch queue (FOR UPDATE SKIP LOCKED)
│       ├── worker.rs         # background fetch workers
│       ├── rate_limit.rs     # GitHub rate-limit tracker (persistent)
│       ├── github.rs         # GitHub client + Link-header pagination
│       ├── chart.rs          # star-history SVG renderer (single + multi-repo overlay, embeddable)
│       ├── export.rs         # full-granularity star export (stars.csv/json) + from/to range math
│       ├── aggregate.rs      # org/user aggregate star history (sums a login's repos from Postgres)
│       ├── cards.rs          # README stat cards (user profile + repo) — github-readme-stats-style
│       ├── repo_charts.rs    # repo-debt stat SVGs (bug-magnets, churn, contributors, lines, heatmap, TODO, bus-factor, commit-trend)
│       ├── repo_endpoints.rs # routes for the repo-debt stat charts + raster (svg/png/webp)
│       └── raster.rs         # SVG → PNG/WebP rasterizer (resvg/usvg + encoders)
├── frontend/           # Astro 7 + React island + Tailwind v4 + shadcn/ui
└── extension/          # MV3 browser extension (injects gitdebt panel on github.com)
```

## Database

Postgres 16 in Docker, named volume `gitdebt_postgres-data` for persistence. Schema is applied idempotently by `db.rs::Db::connect` on every server startup, so there are no migration files to manage by hand.

```bash
scripts/db.sh up      # start postgres, wait until healthy
scripts/db.sh psql    # open a psql shell
scripts/db.sh down    # stop, keep the volume
scripts/db.sh logs    # tail postgres logs
```

`DATABASE_URL=postgres://gitdebt:gitdebt@localhost:5432/gitdebt` (matches the docker-compose defaults). Set in `.env` at repo root or in the shell.

**Why Postgres, not Turso/libsql?** The cache writes are append-heavy (one row per stargazer per repo — a repo's stargazer *timestamps* — easily millions of writes per popular repo at scale). Turso's free-tier write quota was burned through in a single migration; their paid plan is fine but local Postgres is free and faster for development. The `Db` struct is a thin sqlx PgPool wrapper, so swapping back to a hosted Postgres later (Neon, Supabase, RDS) is a `DATABASE_URL` change.

## Commands

Backend:
```bash
cargo build              # debug; must compile clean (see "no warnings" below)
cargo test               # all chart/export/aggregate math is unit-tested
cargo run -p backend     # starts gitdebt-api on :8787
```

Frontend:
```bash
cd frontend
npm install
npm run dev              # http://localhost:14321
npm run build            # production bundle; must complete without errors
```

Required env (in `.env` at repo root or process env):

```
GITHUB_TOKEN=ghp_...                                           # required for sane rate limits
DATABASE_URL=postgres://gitdebt:gitdebt@localhost:5432/gitdebt # matches scripts/db.sh defaults
PORT=8787                                                      # optional
RUST_LOG=info,gitdebt=debug                                    # optional
WORKER_COUNT=1                                                 # optional — star-fetch workers (default 1, avoids burstiness)
REPO_ANALYSIS_WORKERS=1                                        # optional — git-clone analysis workers
MAX_STARGAZER_PAGES=400                                        # optional — per-job cap (~40k stars) before partial+requeue
MAX_PENDING_FETCHES=5000                                       # optional — global enqueue ceiling (anti-abuse)
TRUSTED_PROXIES=...                                            # optional — CIDRs whose cf-connecting-ip/XFF we trust (default loopback+RFC1918+Cloudflare)
METRICS_TOKEN=...                                              # optional — bearer token gating /metrics
```

### Operability & hardening (added in the 2026-06 audit pass)

- **Health:** `GET /health` is liveness (static); `GET /ready` runs `SELECT 1` + reports queue depth (use it for the load-balancer); `GET /metrics` (JSON, optionally `METRICS_TOKEN`-gated) exposes per-source GitHub budget remaining + queue depths — **the key signal is budget exhaustion**.
- **Reliability:** 404/private/deleted repos are **tombstoned** (`repos.missing`) and queue jobs go terminal (`dead`) after `MAX_ATTEMPTS` or an immediate NotFound, so the extension can't re-enqueue them forever. `/analyze` carries `pending`, `backfilling` (repo over the page cap, still backfilling), and `not_found`.
- **Security invariants:** every repo handler validates `is_valid_slug`; the registry package overrides (`?npm/crate/pypi/docker`) are validated (no path traversal/SSRF); 5xx responses never echo internal error text; request bodies are size-capped; the per-IP rate-limit key only trusts forwarding headers from `TRUSTED_PROXIES` (so the origin must not be directly reachable). Token-at-rest is AES-GCM; sessions/webhooks use constant-time HMAC.
- **Git efficiency:** clones are **blobless + single-branch** (`--filter=blob:none --single-branch`); history is walked in **one streaming `git log` pass** (not per-commit subprocesses); commit aggregates are batched (UNNEST upserts); author enrichment is negative-cached. Refresh uses an explicit refspec so HEAD actually advances on a bare clone.

## Standards

### No warnings

Both `cargo build` and `npm run build` must succeed without warnings. New code must not regress this. If a warning is unavoidable (e.g. third-party macro), suppress it narrowly with `#[allow(...)]` and a comment explaining why — don't blanket-allow.

### Caching invariants

The cache schema in `db.rs` has `*_complete` flags on `users` and `repos`. The contract is:

1. **Readers never trust partial data.** `cache.get_*` returns `None` unless the `*_complete` flag is `1`.
2. **Writers commit atomically.** A `put_*` call replaces all rows for the entity *and* flips the flag inside a single transaction. A failure mid-pagination must leave `*_complete = 0` so the next request re-fetches.
3. **Stargazer timestamps fetched once are never re-fetched.** A repo's stargazer arrival timestamps are cached forever for v0 (we store only timestamps — no per-stargazer profiles or events). (Adding a TTL is a separate, deliberate design decision — propose it, don't slip it in.)
4. **404s are tombstoned.** `mark_user_missing` writes a profile row with `complete=1` so we don't re-poll deleted accounts.

If you change anything in `cache.rs` or `db.rs`, add a unit test that demonstrates the invariant still holds — corrupted reports are the most expensive bug class in this codebase.

### Charts: star history + repo-debt

- **Star history is the core.** `chart.rs::render_svg` plots a cumulative star-count line. The multi-repo overlay endpoint (`/api/chart.svg?repos=a/b,c/d`) plots N repos on shared axes — this is the table-stakes star-history.com feature and the README-embed distribution loop. SVG motion is explicit (`animate=1`) and on-site only; copied README SVGs use `animate=0`, with static attributes equal to the finished frame because GitHub sanitizes SMIL. Actual README motion is the opt-in, play-once `/api/repos/:o/:r/chart.gif?motion=draw` route. It reads complete Postgres history only, stays under the GIF size/frame caps in `animated_gif.rs`, and supports the same theme, range, axis, and log options as the single-repo SVG. Support `type=date|timeline` (timeline = aligned to days-since-first-star).
- **Date-range windowing** (`export.rs::RangeSpec`): every star-series surface takes `from`/`to` (inclusive `YYYY-MM-DD`) plus `rebase=1` (rebase cumulative totals to the window start) — the single/multi/user chart endpoints, the usage overlay, and the exports. Invalid dates or `from > to` → 400. The parse/filter math is pure and unit-tested; the range is part of every render cache key.
- **Raw star export** (`export.rs`): `GET /api/repos/:o/:r/stars.csv` (header `date,total,delta`, one row per day, full granularity — no downsampling) and `stars.json` (`{repo,total_stars,complete,series:[{date,total,delta}]}`). Per-day rows are aggregated **in SQL** (never one row per stargazer in memory), read Postgres only — never GitHub — and honor the cache invariants: empty series until `stargazers_complete`, plain 404 for tombstoned repos.
- **Org/user aggregate** (`aggregate.rs`): `GET /api/users/:login/analyze` (`{login,repos_included,repos_pending,total_stars,history:[{date,stars}]}`) + `/api/users/:login/chart.svg[.png|.webp]` sum the cumulative series across a login's top public repos (top 50 by stars). The `login → repos` mapping is cached in Postgres (`login_repo_lists`/`login_repos`: 12h TTL, atomic replace, 404 tombstone, same complete-flag invariant); the one synchronous GitHub call (the repos list) is budget-probed *and* throttled by a process-wide fixed window. Star data comes exclusively from Postgres; cold repos ride the existing star-fetch queue (enqueues capped per build) and are reported in `repos_pending` — the request never blocks on stars.
- **Repo-debt stats are the differentiator** vs star-history.com: bug-magnet files, churn, contributors, language lines, commit heatmap, TODO/FIXME trend, bus factor (top-author commit shares), commit trend (monthly commit volume) — see `repo_charts.rs` / `repo_endpoints.rs` (`bus-factor.{svg,png,webp}`, `commit-trend.{svg,png,webp}`, …). Each is a pure (data → SVG) function with unit-tested math. Their shared SVG dispatcher honors `animate=0|1`, defaults to static, and keeps raster cache keys independent of presentation-only motion.
- **README stat cards** (`cards.rs`): `/api/users/:login/card.svg` (stars/commits/contribs/repos-tracked + rank ring) and `/api/repos/:o/:r/card.svg` (stars/forks/star spark/language bar), plus `.png`/`.webp`. github-readme-stats-compatible URL muscle memory (`hide=`, `show=`, `card_width=`, `hide_rank=`, `rank_icon=`, `custom_title=`, `show_icons=`, `number_format=`; GRS color/locale params are accepted-and-ignored — see the `cards.rs` module docs before "fixing" that). Rendered entirely from our Postgres — zero GitHub calls on the request path, and the user card reads only commit-authorship aggregates, never stargazer profiles. Stats are **lower bounds over tracked repos**; the mandatory "N repos tracked" footer is the honesty framing. Pending/empty cards render short-TTL and are never inserted into the 24h caches.
- **Leaderboard data** (`GET /api/leaderboard.json?metric=stars|velocity&per=50&page=0` → `{metric,page,per_page,window_days,repos:[{rank,repo,stars,velocity}]}`): ranked repos straight from `repos`/`repo_stargazers` (velocity = stars added in the trailing 7 days). Complete-history repos only, tombstones excluded, no GitHub calls; memoized 5 min in its own moka cache. Celebratory popularity/growth rankings **of repos** — never anything about accounts or star provenance.
- **Embed-loop conventions:** every copyable embed snippet (`EmbedSnippet.tsx` charts, `StatCard.tsx` stat charts, `BadgeStudio.tsx` badges/cards) wraps the image in a link back to the matching gitdebt page with `?ref=readme`; extension-injected links carry `?ref=extension`. Auto-theme snippets use GitHub's `<picture>` pattern with separate baked `theme=dark` and `theme=light` assets for both SVG and GIF. Cards additionally bake a "via gitdebt" `<a>` linkback into the SVG itself. Keep the `ref` values stable — they're the attribution channel for the distribution loop, and they must stay out of the image URLs themselves (only the linkback carries them, so the CDN cache never fragments).
- **Stars vs. real usage** (`usage.rs`, `/api/repos/:o/:r/usage[.svg|.png|.webp]`): resolves a repo's published package (npm / crates.io / PyPI / Docker — from clone manifests, then repo-name heuristic, then `?npm=/crate=/pypi=/docker=` overrides) and overlays cumulative downloads on a secondary axis against stars. Registry responses cached in `usage_cache` (≈18h TTL); best-effort — a source that 404s/errors is omitted, never fatal. Go has no public download metrics.
- **Badges** (`badge.rs`, `/api/repos/:o/:r/badge.svg`): compact, configurable (`metrics=stars,forks,downloads`, `style=flat|modern|glass|terminal`, `animate=0|1`, `source=`, `theme=`). Animations use `<animate fill="freeze">` and static attrs equal the end state, so GitHub's SMIL-sanitized README render shows the correct frozen frame. `forks_count` lives on repo metadata.
- **OG social cards** (`og.rs`): **PNG** at exactly 1200×630 (social platforms reject SVG og:images) — `/api/repos/:o/:r/og.png` (repo), `/api/og.png?repos=a/b,c/d` (compare), `/api/og.png` (default site). Branded dark Signal card; reuses the same font-family strings the charts use so text rasterizes. Frontend points `og:image`/`twitter:image` here.
- **Sitemap data** (`/api/sitemap/repos?page=N&per=20000` → `{total,page,per_page,repos:[{slug,updated_at}]}`): lists analyzed repos. The frontend wraps these into XML on its own origin (`/sitemap-index.xml` → `/sitemaps/repos-N.xml` + `/sitemaps/pages.xml`) so all sitemap URLs are same-origin for Search Console.

### Browser extension + passive ingestion

`extension/` is a zero-build MV3 extension that injects a gitdebt panel (star history + repo-debt charts) into `github.com/{owner}/{repo}` pages. It fires on every repo a user opens, so the ingestion path is built for volume:

- **`analyze` never blocks.** Cold/stale repos enqueue a fetch and return `pending: true` with whatever's cached (empty history when cold) — it must never synchronously paginate stargazers. The `star_fetch_queue` (`queue.rs`) + worker (`worker.rs`) drain under `RateLimitTracker`, so the queue physically can't exceed the GitHub budget; jobs are prioritized by `repos.view_count`. Big repos are page-capped (`MAX_STARGAZER_PAGES`) and re-queued partial — `// TODO: GH Archive backfill` is the real fix at scale. Refreshes fetch only the new tail pages (incremental).
- **`POST /api/ext/ping {owner,repo,stars}`** is the freshness + popularity signal: the client-observed star count is an *untrusted hint*, **never persisted** — it's compared to the cached total and enqueues a refresh past a threshold/TTL, and records a view. Keep the count out of cacheable GET URLs (it would fragment the CDN cache). The extension is anonymous; fetches debit the shared PAT budget (upgrade path: PAT pool / GH Archive).
- All chart SVGs bake concrete per-theme hex colors (no CSS vars) so they render in any embedding context; for theme-aware embeds point a `<picture>` at the `light`/`dark` URLs. Series colors come from a shared categorical palette (brand lime first).

When adding a new stat chart:
1. New pure `render_*` in `repo_charts.rs` with unit-tested math.
2. Wire a `StatKind` + route in `repo_endpoints.rs` (svg/png/webp dispatch is shared).
3. Surface it on the frontend repo page grid.

### GitHub API discipline

- **NOTE — stargazers endpoint restriction (GitHub changelog 2026-06-30):** `/repos/{owner}/{repo}/stargazers` is being restricted. Do **not** add new code paths that paginate it; the existing queue/worker path is the only consumer and is on borrowed time. Every star-series surface added since (exports, org/user aggregates, leaderboard, cards) reads exclusively from Postgres (`repo_stargazers`, `repos`, `repo_*` tables). Star *acquisition* must move to GH Archive — that roadmap item is promoted from non-goal to the planned primary source (see the roadmap section).
- Use `parse_next_link` (Link-header pagination), never page-counter loops. Per-job we cap at `MAX_STARGAZER_PAGES` and re-queue partial rather than looping unbounded.
- We fetch only a repo's stargazer *timestamps* and its metadata — no per-user profiles, starred lists, or events. Full historical backfill at scale is a GH Archive (BigQuery) job, not more REST pagination. Don't try to fake it.
- Never mark a fetch complete on `RateLimited` or any error path.

### Chart determinism

`chart::render_svg` is a pure function. Same input → same bytes. This matters because the SVG endpoint is cacheable upstream and bytes-equal across runs makes ETag short-circuiting work.

## GitHub API rate-limit protections

We comply with [GitHub's official guidance](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api) and the [dev.to checklist](https://dev.to/mehmetakar/api-rate-limit-exceeded-github-how-to-fix-4h6n). Status of each:

| Protection | Where |
|---|---|
| Authenticated requests (Bearer token) | `github.rs::GithubClient::new` |
| Persistent rate-limit tracker reading `x-ratelimit-*` | `rate_limit.rs` (`api_quota` table — survives restarts) |
| Block on exhaustion until `x-ratelimit-reset` | `RateLimitTracker::acquire` |
| Honor `Retry-After` for secondary/abuse rate limits | `RateLimitTracker::mark_exhausted` |
| Differentiate primary (rate=0) vs secondary (Retry-After) vs plain 403 | `github.rs::send` |
| Exponential backoff on transient errors | `worker.rs` (1, 2, 4, ... 32s capped) |
| Single worker by default (avoid burstiness) | `main.rs` |
| Cache forever after first fetch | `cache.rs` (`*_complete` flag invariant) |
| Bytes-deterministic SVG + 24h `Cache-Control` | `chart.rs`, `api.rs` |

**Gaps deliberately not closed in v0:**

- **Conditional requests with `If-None-Match` (ETag)**. Becomes valuable once we add a refresh TTL — until then we never re-fetch a complete user. When adding TTL, also add an `etag` column to each cache table and send `If-None-Match`. 304 responses don't count against the rate budget.
- **GraphQL**. The REST endpoints we use are individually cheap; a GraphQL query that fetched a repo's stargazer timestamps in larger pages (100 → 100s per call, via `stargazers { edges { starredAt } }`) could cut round trips, but requires a parallel client implementation. Worth it once the worker is the bottleneck.
- **Search API rate limits** (30/min) — we don't use search, so no protection needed.

## GitHub App: OAuth + webhook wiring

The backend implements the GitHub App's user-authorization flow + webhook receiver. Routes:

- `GET /auth/github/start` — sets a CSRF cookie, 302s to GitHub's authorize URL
- `GET /auth/github/callback` — verifies CSRF, exchanges code for user access token, fetches `/user`, upserts `app_users`, sets a HMAC-signed session cookie
- `POST /auth/logout` — clears session cookie
- `GET /api/me` — returns the logged-in user (or 401)
- `POST /webhooks/github` — verifies `X-Hub-Signature-256` (HMAC-SHA256), handles `installation` events to keep the `installations` table fresh

**Env required for OAuth:** `GITHUB_APP_CLIENT_ID`, `GITHUB_APP_CLIENT_SECRET`, `GITHUB_WEBHOOK_SECRET`, `SESSION_SECRET`, `OAUTH_REDIRECT_URI`. If they're missing the auth/webhook routes return 503; rest of the server runs fine. For local dev, set `OAUTH_REDIRECT_URI=http://localhost:8787/auth/github/callback`.

**Session cookies are HMAC-SHA256 signed**, format `<user_id>.<expiry_unix>.<sig_hex>`. 30-day TTL. `HttpOnly`, `SameSite=Lax`. Set `COOKIE_SECURE=1` in production.

**Token storage:** `app_users.access_token` / `refresh_token` are **encrypted at rest with AES-GCM** (`crypto.rs`), keyed by `TOKEN_ENCRYPTION_KEY` (the server refuses to start if the GitHub App is configured but the key is unset). Rotating the key requires re-encrypting every stored token — see "Token rotation" under Deployment.

**Per-token rate limits:** `RateLimitTracker` is keyed by source string (`github:default:<hash>`, `github:user:<hash>`, etc) — each token gets its own 5k/hr bucket persisted to `api_quota`. `GithubClient::for_user_token` constructs an ad-hoc client tied to a user's OAuth token, so when a logged-in user makes a request that hits GitHub, calls debit *their* bucket (multiplying your aggregate ceiling per signed-in user). Background workers still use the env PAT's bucket.

## GitHub App: which permissions to request

For gitdebt's actual workload — reading public repo metadata and a repo's public stargazer timestamps — the App needs **almost nothing**. The endpoints we hit (`/repos/:o/:r` metadata and the repo's stargazer *timestamps*) are public and accessible to any authenticated request. We do **not** fetch per-stargazer profiles, starred lists, or events.

**Recommended permissions when registering the App:**

- **Repository permissions**
  - `Metadata: Read-only` ← *required, this is the mandatory minimum for any GitHub App*
  - Everything else: **No access**

- **Account / User permissions**
  - All: **No access**

- **Subscribe to events:** none (skip webhooks for v0; users are more comfortable installing apps that don't ask for event subscriptions)

That's it. Asking for less = higher install conversion. Anything we'd add (Contents, Issues, Pull requests, etc.) is a yellow flag in the install dialog and isn't needed.

**Auth flow:** server-to-server (no user OAuth). When a user installs the App on their account/org, GitHub gives us an installation ID; we exchange the App's JWT for an installation access token and use that for API calls. The installation's rate budget covers calls made with that token.

**Rate-limit math for an App:**
- Base: 5,000/hr per installation (server-to-server)
- +50/hr per repository in the installation (cap +12,500)
- +50/hr per user in the org (cap +12,500)
- Hard cap: 12,500/hr per installation on standard accounts; 15,000/hr on Enterprise Cloud

The 1M/hr ceiling only emerges in aggregate: 100 installations × 12,500 = 1.25M/hr aggregate. Each installation's calls are isolated to that bucket — you can't pool them. Strategy is "many small installs" not "one fat install."

For requests coming from anonymous gitdebt.com visitors (no auth), use a fallback "default" installation on a dedicated org for baseline budget.

## Frontend architecture (Astro 7 + static Cloudflare Pages)

The frontend is **`output: "static"` with no adapter**, deployed to Cloudflare
Pages. There are no Pages Functions and no Cloudflare Worker invocations.

| Route | Mode | Freshness |
|---|---|---|
| `/` and marketing pages | static | rebuilt on frontend changes |
| `/report?repo=o/r` | static shell + React client | live API data immediately |
| `/[owner]/[repo]` | generated static snapshot | refreshed by scheduled/event-driven builds |
| `/leaderboard` | generated static snapshot | refreshed by builds |
| `/compare/[category]` | generated static snapshot | refreshed by builds |
| `/u/[login]` | generated static snapshot | refreshed by builds |
| `/sitemap-index.xml` | generated static XML | exactly matches emitted tracked-repo pages |

`src/lib/build-catalog.ts` queries `/api/sitemap/repos` at build time. It
combines tracked repos with the deliberately curated category members, then
generates repo, owner, category, and curated head-to-head paths through
`getStaticPaths`. The catalog is capped by `STATIC_REPO_LIMIT` (default 1000,
hard cap 8000) so the build stays below Cloudflare Pages' 20,000-file free-plan
limit. Production sets `STATIC_CATALOG_REQUIRED=1`; a backend outage must fail a
deployment rather than publish an accidentally empty catalog.

`/report` is the discovery path used by the homepage and extension. It parses
the repo query on the client, polls `/analyze`, and renders the live charts.
Once ingestion completes, the next build promotes that repo into a crawlable
static `/{owner}/{repo}` page with title, description, canonical URL, PNG OG
card, JSON-LD, and unique repo-derived copy. Cold/no-history snapshots are
`noindex`.

The refresh workflow (`.github/workflows/deploy-pages.yml`) runs on frontend
pushes, manual dispatch, `repository_dispatch` type `refresh-static-pages`, and
every three hours as a safety net. A backend completion hook can send the
repository dispatch for faster promotion without manufacturing commits.

Per-page details:

- **Repo snapshots** contain crawlable star totals, trend copy, JSON-LD
  `SoftwareSourceCode`, and an OG PNG. The `<RepoHero>` island polls for fresher
  numbers after hydration.
- **Leaderboard** remains celebratory and repo-focused: fastest-growing
  (trailing 7 days) plus most-starred across the tracked dataset.
- **Category compare pages** remain a small, hand-written set from
  `src/data/categories.ts`; never template or auto-generate their intros.
- **User/org pages** aggregate the built catalog's owners and remain unlisted
  from the sitemap until there is a deliberate login listing contract.
- **Sitemaps** use the same in-memory catalog as `getStaticPaths`, so Search
  Console never receives a repo URL that was omitted from the deployment.

Build-time configuration uses `import.meta.env` through
`src/lib/static-api-base.ts`. Do not reintroduce `cloudflare:workers`,
`@astrojs/cloudflare`, Worker cache middleware, or `prerender = false` routes.

## Deployment

Backend → Dokploy (VPS, Nixpacks builder). Frontend → Cloudflare Pages
(static assets only).

### Backend (Dokploy)

- `nixpacks.toml` at repo root drives the build. Installs `git`, `pkg-config`, `openssl`, `gnutar` (the repo-history feature shells to git CLI; tokei materializes HEAD via `git archive | tar`).
- Build: `cargo build --release --bin gitdebt-api`. Run: `./target/release/gitdebt-api`.
- Listen address: defaults to `0.0.0.0:$PORT` so Dokploy's reverse proxy can reach it. Set `BIND_LOCAL=1` to scope to localhost (dev only).
- **Required env in production:**
  - `DATABASE_URL` — connection string for managed Postgres (Neon / Supabase / RDS / Dokploy's own Postgres service).
  - `GITHUB_TOKEN` — fallback PAT for anonymous-visitor traffic.
  - `SESSION_SECRET` — `openssl rand -hex 64`.
  - `TOKEN_ENCRYPTION_KEY` — `openssl rand -base64 32`. **Refuses to start if the GitHub App is configured but this isn't set.**
  - `GITHUB_APP_CLIENT_ID`, `GITHUB_APP_CLIENT_SECRET`, `GITHUB_WEBHOOK_SECRET`.
  - `OAUTH_REDIRECT_URI` — must exactly match the App's settings page (`https://api.gitdebt.com/auth/github/callback` or wherever the backend lives).
  - `PUBLIC_FRONTEND_ORIGIN` — exact origin of the frontend (`https://gitdebt.com`). Used for the credentialed CORS allow-list on `/api/me` + `/auth/*`.
  - `COOKIE_SECURE=1` — sets `Secure` on session cookies.
  - `REPOS_DIR=/var/lib/gitdebt/repos` — point at a mounted volume sized for your quota (≥100 GB). Persists clones across redeploys.
- Bring your own Postgres. The `docker-compose.yml` in this repo exists for local dev only; in Dokploy provision a Postgres service separately.
- Graceful shutdown: SIGTERM (Dokploy redeploy) drains in-flight requests via `axum::serve.with_graceful_shutdown`. Worker state is durable in Postgres, so a hard kill is also safe (queue rows persist; on restart `reset_inflight_on_startup` resets stuck claims).

### Frontend (Cloudflare Pages)

- Create a Direct Upload Pages project named `gitdebt`; do not enable Pages
  Functions.
- GitHub Actions builds `frontend/dist` with `PUBLIC_API_BASE`,
  `PUBLIC_SITE_URL`, `STATIC_CATALOG_REQUIRED=1`, and `STATIC_REPO_LIMIT`, then
  runs `wrangler pages deploy`.
- Required GitHub secrets: `CLOUDFLARE_ACCOUNT_ID` and a
  `CLOUDFLARE_API_TOKEN` with Account / Cloudflare Pages / Edit.
- Bind `gitdebt.com` to the Pages project and `api.gitdebt.com` to Dokploy.
- See `frontend/DEPLOYMENT.md` for the refresh and manual-deploy contract.

### Token rotation

- `SESSION_SECRET`: rotate with care. Old session cookies are invalidated → users log out. Plan a maintenance window or stagger rolling restarts.
- `TOKEN_ENCRYPTION_KEY`: rotation requires re-encrypting every `app_users.access_token` row with the new key. Don't rotate without writing that migration first — rotating without re-encryption locks every existing user out of their stored OAuth token (they have to re-auth).

### Backups

Daily `pg_dump` to off-host storage. Skeleton:

```bash
docker exec gitdebt-postgres pg_dump -U gitdebt -d gitdebt --format=custom \
  | aws s3 cp - s3://gitdebt-backups/$(date -u +%F).pgdump
```

Drop into cron / Dokploy scheduled job.

## Architecture roadmap

- **GH Archive backfill — PROMOTED (2026-06), no longer a non-goal.** With `/repos/{o}/{r}/stargazers` being restricted (GitHub changelog 2026-06-30), WatchEvent history from GH Archive (BigQuery, parquet over the last decade) is the planned primary star-acquisition source — it moves ingestion off GitHub's REST API entirely. Every read surface already consumes stars from Postgres only, so the swap is confined to the worker/ingestion side.
- **GraphQL client** — fewer round trips per user, separate point-budget. Worth it when scaling past one installation's quota.

(Two former roadmap items shipped in 2026-06 in deliberately celebratory, repo-focused form — `/leaderboard` and `/u/{login}` — with none of the account-judgment framing they were originally scoped around. Don't re-scope them toward accounts; see the product constraint at the top.)
