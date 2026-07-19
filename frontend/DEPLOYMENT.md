# Frontend deployment

gitdebt is a fully static Astro 7 site deployed to Cloudflare Pages. It uses
no Pages Functions and no Cloudflare Worker.

## Routing contract

Astro builds with `build.format: "file"` and `trailingSlash: "never"`.
Therefore `/about` is backed by `about.html`, and `/owner/repo` is backed by
`owner/repo.html`. This pairing is important on Cloudflare Pages:
directory-style `about/index.html` is normalized to `/about/`, which conflicts
with no-trailing-slash canonical URLs and can loop if `_redirects` sends the
slash form back to `/about`.

Cloudflare applies `_redirects` before it checks for a matching asset. Do not
add a catch-all such as:

```text
/:owner/:repo /report?repo=:owner/:repo 302
```

That rule would intercept generated repository snapshots too. The static-first
fallback instead lives in `404.html`:

1. Pages serves a generated `owner/repo.html` when it exists.
2. A missing path receives the custom, `noindex` 404 page.
3. Its small route helper recognizes a valid, non-reserved two-segment GitHub
   slug and uses `location.replace()` to open
   `/report?repo=owner%2Frepo`.
4. Legal, product, sitemap, comparison, and malformed routes remain genuine
   404s and cannot enter a redirect loop.

The `/report` shell and signed-in `/profile` aggregate shell are also
`noindex`; the next successful refresh promotes a completed repo into its
canonical, indexable static snapshot. The build runs
`scripts/audit-routing.mjs` and fails if directory output returns, a redirect
cycle appears, a catch-all shadows assets, or the 404/live-shell SEO
safeguards are removed.

## How freshness works

The build asks the Rust API for its analyzed-repository catalog, then generates:

- crawlable `/{owner}/{repo}` report pages for the newest tracked repos;
- user/org aggregate pages for the matching owners;
- curated category and head-to-head comparison pages;
- a same-origin sitemap containing only repo pages emitted by that build.

`/report?repo=owner/repo` is the live, client-rendered entry point. It works as
soon as someone searches for a repo, follows a missing snapshot URL, or opens a
repo from the browser extension. After the backend finishes ingesting that
repo, the next static refresh adds its crawlable snapshot.

The production workflow runs after frontend pushes, on manual dispatch, on a
`refresh-static-pages` repository dispatch, and every hour as a safety net.
The backend does not hold a GitHub token and therefore does not emit the
repository dispatch itself. It is an optional hook for an external scheduler;
the checked-in hourly schedule is the default freshness mechanism.

## One-time Cloudflare setup

1. Create a **Direct Upload** Pages project named `gitdebt` with production
   branch `main`.
2. Bind `gitdebt.com` as its custom domain.
3. Create a Cloudflare API token with `Account / Cloudflare Pages / Edit`.
4. Add `CLOUDFLARE_ACCOUNT_ID` and `CLOUDFLARE_API_TOKEN` as GitHub Actions
   secrets.
5. Keep `api.gitdebt.com` pointed at the Dokploy backend.

Direct Upload is intentional: the scheduled workflow can rebuild snapshots
without manufacturing commits.

## Local production build

From the repository root:

```bash
PUBLIC_API_BASE=https://api.gitdebt.com \
PUBLIC_SITE_URL=https://gitdebt.com \
pnpm --filter gitdebt-frontend build
```

Set `STATIC_CATALOG_REQUIRED=1` for a release build. That makes the build fail
instead of publishing an incomplete dynamic catalog when the backend is
unavailable. `STATIC_REPO_LIMIT` defaults to `1000` and is capped at `8000`;
keep the final output below the Pages limit of 20,000 files.

To deploy manually after building:

```bash
cd frontend
CLOUDFLARE_ACCOUNT_ID=... CLOUDFLARE_API_TOKEN=... \
  pnpm exec wrangler pages deploy dist --project-name=gitdebt --branch=main
```

Preview deployments retain canonical URLs pointing at `gitdebt.com`.
Cloudflare Pages automatically adds `X-Robots-Tag: noindex` to preview URLs.
