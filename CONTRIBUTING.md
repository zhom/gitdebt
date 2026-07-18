# Contributing to gitdebt

Issues and focused pull requests are welcome. Before starting a large change,
open an issue so the direction can be agreed without wasting work.

## Ground rules

- Keep star analytics factual and repo-focused. Do not add fake-star
  detection, account scoring, suspicious-user labels, or stargazer profiles.
- Star-series readers use Postgres. Do not add another GitHub stargazer
  pagination path.
- Preserve complete-cache transactions and add an invariant test for changes
  in `cache.rs` or `db.rs`.
- Keep generated SVGs deterministic and correct when GitHub strips SMIL.
- `cargo build` and the frontend production build must be warning-free.

## Local checks

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace

pnpm install --frozen-lockfile
PUBLIC_API_BASE=https://api.gitdebt.com \
PUBLIC_SITE_URL=https://gitdebt.com \
pnpm --filter gitdebt-frontend build

npm --prefix extension ci
npm --prefix extension run package
```

The Postgres-backed integration tests use
`GITDEBT_TEST_DATABASE_URL`; the local default is documented in `AGENTS.md`.

By contributing, you agree that your contribution is licensed under MIT.
