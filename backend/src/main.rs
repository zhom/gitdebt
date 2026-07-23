//! `gitdebt-api`: the HTTP tier. Router + ApiState + TCP serve only — every
//! background pool (star-fetch, repo analysis, GH Archive, leaderboard)
//! lives in the `gitdebt-worker` binary. Any number of api replicas can run
//! against the same Postgres + Redis.

use std::{net::SocketAddr, sync::Arc};

use anyhow::{Context, Result};
use gitdebt::{
    analyzer::AnalyzerCtx,
    api::{ApiState, router, spawn_invalidation_listener},
    bootstrap,
};

#[tokio::main]
async fn main() -> Result<()> {
    bootstrap::init_process();
    let services = bootstrap::connect_services().await?;

    // Redis backs the cross-replica HTTP admission limiter and the cache
    // invalidation bus. Release deployments require it; debug builds fall
    // back to per-process limiting so local dev needs no Redis.
    let redis_url = std::env::var("REDIS_URL")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let redis = match redis_url {
        Some(url) => Some(gitdebt::redis::RedisHandle::connect(&url)?),
        None if cfg!(debug_assertions) => {
            tracing::warn!(
                "REDIS_URL not set; rate limits and cache invalidation are per-process only"
            );
            None
        }
        None => anyhow::bail!("REDIS_URL must be set in release deployments"),
    };

    // Read-only view over the worker's clone volume: the usage endpoint
    // reads package manifests out of existing bare clones. A missing or
    // empty mount degrades gracefully to "no manifest-backed package data"
    // — it never clones and never errors.
    let storage = Arc::new(gitdebt::repo_history::RepoStorage::from_env());

    let analyzer = AnalyzerCtx {
        github: services.github,
        cache: services.cache,
    };
    let api_state = ApiState::new(analyzer, services.gh_app, storage, redis)?;
    spawn_invalidation_listener(&api_state);

    let addr = bootstrap::bind_addr(8787);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    tracing::info!(%addr, "gitdebt-api listening");
    // Graceful shutdown: SIGTERM (container stop / redeploy) lets in-flight
    // requests finish instead of dropping connections.
    // `into_make_service_with_connect_info::<SocketAddr>` registers the
    // peer socket as `ConnectInfo` on each request; the admission limiter's
    // key extraction requires it to gate forwarding headers on the trusted
    // proxy set — without this wiring, dev mode (no proxy in front) would
    // key every request on "unknown".
    axum::serve(
        listener,
        router(api_state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(bootstrap::shutdown_signal())
    .await?;
    tracing::info!("server shut down cleanly");
    Ok(())
}
