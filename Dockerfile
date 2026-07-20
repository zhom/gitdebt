# syntax=docker/dockerfile:1.7

FROM rust:1.94.0-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY assets ./assets
COPY backend ./backend

RUN --mount=type=cache,id=gitdebt-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=gitdebt-cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=gitdebt-cargo-target,target=/app/target \
    cargo build --release --locked -p backend --bin gitdebt-api \
    && cp /app/target/release/gitdebt-api /tmp/gitdebt-api

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl git tar \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
RUN mkdir -p /var/lib/gitdebt/repos

COPY --from=builder /tmp/gitdebt-api /usr/local/bin/gitdebt-api

EXPOSE 8787
STOPSIGNAL SIGTERM

CMD ["gitdebt-api"]
