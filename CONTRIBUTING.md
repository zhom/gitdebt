# Contributing to gitdebt

Contributions are welcome! Please do not create PRs for the sake of being
added to the contributors list. Reviewing PRs takes time, so please create PRs
only if you believe your change will improve gitdebt for yourself and others.
If you are thinking of making a significant change, please get in touch with
the maintainer first by opening an issue.

## Before Starting

- Search existing issues and PRs related to your change
- Confirm no other contributors are working on the same issue
- Check that the change fits the project's goals (see the product boundary in
  [`AGENTS.md`](AGENTS.md))

## License

By contributing, you agree that your contributions are licensed under the
project's [MIT license](LICENSE). You retain all rights to use your
contributions elsewhere.

## Development Setup

Requirements:

- Rust 1.94 (see `rust-toolchain.toml`)
- Node.js 22.13+
- pnpm
- Docker
- git

```bash
git checkout -b feature/my-feature-name

# Database (Postgres + Redis in Docker)
scripts/db.sh up

# Backend API
export DATABASE_URL=postgres://gitdebt:gitdebt@localhost:5432/gitdebt
export GITHUB_TOKEN=ghp_...
cargo run -p backend --bin gitdebt-api

# Worker (second shell, same env) — drains ingestion and analysis queues;
# without it, analyses stay pending forever
cargo run -p backend --bin gitdebt-worker

# Frontend (third shell)
pnpm install --frozen-lockfile
PUBLIC_API_BASE=http://localhost:8787 pnpm --filter gitdebt-frontend dev

# Extension
npm --prefix extension ci
npm --prefix extension run start:firefox   # or start:chrome
```

Remaining configuration is documented in `backend/.env.example`.

## Quality Checks

Run before every commit:

```bash
# Backend
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
GITDEBT_TEST_DATABASE_URL=postgres://gitdebt:gitdebt@localhost:5432/gitdebt \
  cargo test --workspace
cargo build --workspace --all-targets

# Frontend
pnpm --filter gitdebt-frontend test:seo
pnpm --filter gitdebt-frontend build

# Extension
npm --prefix extension ci
npm --prefix extension run package
```

This runs:

- **rustfmt + Clippy**: Rust formatting and linting; warnings are errors
- **cargo test**: unit and Postgres-backed integration tests (needs the local
  database from `scripts/db.sh up`)
- **test:seo**: static-routing, sitemap, and SEO audits
- **Astro build**: warning-free static production build
- **Extension package**: extension tests plus `web-ext lint`, then writes the
  store archives to `extension/dist/`

## Key Rules

- **Product boundary**: star analytics stay factual and repo-focused. Never
  add fake-star detection, account scoring, suspicious-user labels, or
  stargazer profiles
- **Star reads use Postgres**: do not add another GitHub stargazer pagination
  path
- **Completeness transactions**: writers replace entity data and flip the
  `*_complete` flag atomically; readers never see partial data. Changes to
  `backend/src/db.rs` or `backend/src/cache.rs` require an invariant test
- **Deterministic renderers**: identical inputs must produce identical SVG and
  raster bytes, and SVGs must stay correct when rendered as a single frame
- **Warning-free builds**: compiler, Clippy, Astro, and lint warnings fail CI
- **No lock file changes**: don't update `Cargo.lock`, `pnpm-lock.yaml`, or
  `extension/package-lock.json` unless updating dependencies is the purpose of
  the PR

## Pull Request Guidelines

- Reference related issues (`Fixes #123` or `Refs #123`)
- Include screenshots or recordings for UI changes
- Ensure "Allow edits from maintainers" is checked

## Architecture

- **Backend**: Rust API and ingestion workers, `backend/` (`gitdebt-api`
  serves HTTP, `gitdebt-worker` runs ingestion and analysis)
- **Frontend**: Astro 7 static site with React islands, `frontend/`
- **Extension**: browser-native MV3 extension, `extension/`

## Getting Help

- **Issues**: Bug reports and feature requests
- **Discussions**: Questions and general discussion
