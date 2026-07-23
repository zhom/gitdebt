# syntax=docker/dockerfile:1.7

FROM rust:1.94.0-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY assets ./assets
COPY backend ./backend
COPY frontend/src/data/categories.ts ./frontend/src/data/categories.ts

RUN --mount=type=cache,id=gitdebt-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=gitdebt-cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=gitdebt-cargo-target,target=/app/target \
    cargo build --release --locked -p backend --bins \
    && cp /app/target/release/gitdebt-api /tmp/gitdebt-api \
    && cp /app/target/release/gitdebt-worker /tmp/gitdebt-worker

# Background tier: queue pools + GH Archive coordinators + health server.
# Mount a persistent volume at /var/lib/gitdebt/repos (REPOS_DIR).
FROM debian:bookworm-slim AS worker

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl git tar \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
RUN mkdir -p /var/lib/gitdebt/repos

COPY --from=builder /tmp/gitdebt-worker /usr/local/bin/gitdebt-worker

EXPOSE 8788
STOPSIGNAL SIGTERM

CMD ["gitdebt-worker"]

# HTTP tier: stateless, scale horizontally. Optionally mount the worker's
# clone volume read-only for usage-manifest reads (git is required for them).
# Last stage on purpose: a targetless `docker build .` must produce the api.
FROM debian:bookworm-slim AS api

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl git \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /tmp/gitdebt-api /usr/local/bin/gitdebt-api

EXPOSE 8787
STOPSIGNAL SIGTERM

CMD ["gitdebt-api"]
