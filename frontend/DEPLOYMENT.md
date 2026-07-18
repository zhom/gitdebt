# Frontend deployment

gitdebt is a fully static Astro 7 site deployed to Cloudflare Pages. It uses
no Pages Functions and no Cloudflare Worker.

## How freshness works

The build asks the Rust API for its analyzed-repository catalog, then generates:

- crawlable `/{owner}/{repo}` report pages for the newest tracked repos;
- user/org aggregate pages for the matching owners;
- curated category and head-to-head comparison pages;
- a same-origin sitemap containing only repo pages emitted by that build.

`/report?repo=owner/repo` is the live, client-rendered entry point. It works as
soon as someone searches for a repo or opens it from the browser extension.
After the backend finishes ingesting that repo, the next static refresh adds
its crawlable `/{owner}/{repo}` snapshot.

The production workflow runs after frontend pushes, on manual dispatch, on a
`refresh-static-pages` repository dispatch, and every three hours as a safety
net. Three hours leaves comfortable room under Cloudflare Pages' 500 monthly
build allowance; the event-driven trigger can refresh sooner when ingestion
finishes.

## One-time Cloudflare setup

1. Create a **Direct Upload** Pages project named `gitdebt` with production
   branch `main`.
2. Bind `gitdebt.com` as its custom domain.
3. Create a Cloudflare API token with `Account / Cloudflare Pages / Edit`.
4. Add `CLOUDFLARE_ACCOUNT_ID` and `CLOUDFLARE_API_TOKEN` as GitHub Actions
   secrets.
5. Keep `api.gitdebt.com` pointed at the Dokploy backend.

Direct Upload is intentional: Git integration cannot initiate a build on a
timer without a new commit, while this workflow can rebuild snapshots without
manufacturing commits.

## Local production build

From the repository root:

```bash
PUBLIC_API_BASE=https://api.gitdebt.com \
PUBLIC_SITE_URL=https://gitdebt.com \
pnpm --filter gitdebt-frontend build
```

Set `STATIC_CATALOG_REQUIRED=1` for a release build. That makes the build fail
instead of silently publishing a site with an empty dynamic catalog when the
backend is unavailable. `STATIC_REPO_LIMIT` defaults to `1000` and is capped at
`8000`; keep the final output below the Pages free-plan limit of 20,000 files.

To deploy manually after building:

```bash
cd frontend
CLOUDFLARE_ACCOUNT_ID=... CLOUDFLARE_API_TOKEN=... \
  pnpm exec wrangler pages deploy dist --project-name=gitdebt --branch=main
```

Preview deployments should retain canonical URLs pointing at `gitdebt.com`.
If preview indexing becomes noisy, add an `X-Robots-Tag: noindex` response
rule for `*.pages.dev` in Cloudflare; a static `_headers` file cannot vary by
hostname.
