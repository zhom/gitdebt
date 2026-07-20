use std::{net::SocketAddr, sync::Arc};

use anyhow::{Context, Result};
use gitdebt::{
    analyzer::AnalyzerCtx,
    api::{ApiState, router},
    auth::GithubAppConfig,
    cache::Cache,
    db::Db,
    github::GithubClient,
    rate_limit::RateLimitTracker,
};
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let token = std::env::var("GITHUB_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty());
    if token.is_none() {
        if cfg!(debug_assertions) {
            tracing::warn!("GITHUB_TOKEN not set; unauthenticated requests are limited to 60/hour");
        } else {
            anyhow::bail!("GITHUB_TOKEN must be set in release deployments");
        }
    }

    let database_url = std::env::var("DATABASE_URL").context(
        "DATABASE_URL must be set (postgres://user:pass@host:port/db). \
                  Run `scripts/db.sh up` to start a local Postgres in Docker.",
    )?;
    let db = Db::connect(&database_url).await?;
    tracing::info!("postgres connected; schema applied");
    let cache = Cache::new(db.clone());

    let rate = Arc::new(RateLimitTracker::load(db).await?);
    let github = Arc::new(GithubClient::new(token.as_deref(), rate.clone())?);

    // Repo-history analysis pool. Separate queue, separate workload
    // shape: clones are disk-heavy + CPU-heavy, not GitHub-API-bound.
    // One worker is the right default — parallel disk thrashes the cache
    // and git CLI subprocesses already use multiple cores internally.
    let storage = std::sync::Arc::new(gitdebt::repo_history::RepoStorage::from_env());
    let requested_analysis_workers: usize = std::env::var("REPO_ANALYSIS_WORKERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let analysis_workers = requested_analysis_workers.clamp(1, 2);
    if requested_analysis_workers != analysis_workers {
        tracing::warn!(
            requested = requested_analysis_workers,
            effective = analysis_workers,
            "REPO_ANALYSIS_WORKERS capped to protect git subprocess and thread capacity"
        );
    }
    let reset = gitdebt::repo_analysis::reset_inflight_on_startup(cache.db()).await?;
    if reset > 0 {
        tracing::info!(
            reset_count = reset,
            "repo-analysis: reset expired in_progress leases"
        );
    }
    let analysis_revived = gitdebt::repo_analysis::revive_retryable_on_startup(cache.db()).await?;
    if analysis_revived > 0 {
        tracing::info!(
            revived = analysis_revived,
            "repo-analysis: revived jobs parked by older releases"
        );
    }
    gitdebt::repo_analysis::spawn_pool(
        gitdebt::repo_analysis::AnalysisCtx {
            db: cache.db().clone(),
            storage: storage.clone(),
            github: github.clone(),
        },
        analysis_workers,
    );
    tracing::info!(analysis_workers, "repo-analysis worker pool started");

    // Star-history acquisition. With GH Archive enabled there is exactly one
    // BigQuery coordinator: it batches repos into shared corpus scans, while
    // WORKER_COUNT only controls the inexpensive GitHub metadata lookups
    // needed to resolve stable numeric repo IDs. The legacy GitHub
    // stargazer-list pool remains an explicit fallback for local/dev installs.
    let worker_count: usize = std::env::var("WORKER_COUNT")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|n| *n >= 1)
        .unwrap_or(1);
    let star_reset = gitdebt::queue::reset_inflight_on_startup(cache.db()).await?;
    if star_reset > 0 {
        tracing::info!(
            reset_count = star_reset,
            "star-fetch: reset expired in_progress leases"
        );
    }
    let archive_client = gitdebt::gh_archive::GhArchiveBigQueryClient::from_env()
        .await
        .context("GH Archive BigQuery configuration/authentication failed")?;
    if let Some(archive_client) = archive_client {
        let revived = gitdebt::queue::revive_retryable_for_archive(cache.db()).await?;
        if revived > 0 {
            tracing::info!(revived, "gh-archive: revived retryable history jobs");
        }
        gitdebt::archive_worker::spawn(gitdebt::archive_worker::ArchiveWorkerCtx::from_env(
            Arc::new(archive_client),
            github.clone(),
            cache.clone(),
            worker_count,
        ));
        gitdebt::archive_hourly_db::spawn(cache.db().clone())
            .context("GH Archive hourly follower configuration failed")?;
        tracing::info!(
            metadata_concurrency = worker_count,
            "GH Archive historical coordinator and hourly follower started"
        );
    } else if cfg!(debug_assertions) {
        gitdebt::worker::spawn_pool(
            gitdebt::worker::WorkerCtx::new(github.clone(), cache.clone()),
            worker_count,
        );
        tracing::warn!(
            worker_count,
            "GH Archive disabled; using the restricted GitHub stargazer-list fallback"
        );
    } else {
        anyhow::bail!(
            "GH_ARCHIVE_ENABLED=1 and valid BigQuery credentials are required in release deployments"
        );
    }

    let analyzer = AnalyzerCtx { github, cache };
    let gh_app =
        GithubAppConfig::from_env().context("GitHub App config invalid; refusing to start")?;
    if gh_app.is_some() {
        tracing::info!("GitHub App OAuth configured (tokens encrypted at rest)");
    } else {
        tracing::warn!(
            "GitHub App not configured (set GITHUB_APP_CLIENT_ID, GITHUB_APP_CLIENT_SECRET, \
             GITHUB_WEBHOOK_SECRET, SESSION_SECRET, TOKEN_ENCRYPTION_KEY); \
             /auth/* and /webhooks/github will 503"
        );
    }
    let api_state = ApiState::new(analyzer, gh_app, storage.clone())?;

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8787);
    // 0.0.0.0 in container deployments (Dokploy fronts traffic);
    // localhost-only when BIND_LOCAL=1 keeps dev safe.
    let host: [u8; 4] = if std::env::var("BIND_LOCAL").ok().as_deref() == Some("1") {
        [127, 0, 0, 1]
    } else {
        [0, 0, 0, 0]
    };
    let addr = SocketAddr::from((host, port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    tracing::info!(%addr, "gitdebt-api listening");
    // Graceful shutdown: SIGTERM (Docker stop, Dokploy redeploy) lets
    // in-flight requests finish instead of dropping connections. Workers
    // exit when the runtime stops. Their state is durable in Postgres; repo
    // analysis leases heartbeat every 30 seconds and stale claims recover
    // after two minutes, while archive claims recover after 15 minutes.
    // `into_make_service_with_connect_info::<SocketAddr>` registers the
    // peer socket as `ConnectInfo` on each request. tower_governor's
    // SmartIpKeyExtractor falls back to that when no `X-Forwarded-For`
    // is present — without this wiring, dev mode (no proxy in front)
    // would fail key extraction and reject every request.
    axum::serve(
        listener,
        router(api_state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    tracing::info!("server shut down cleanly");
    Ok(())
}

async fn shutdown_signal() {
    use tokio::signal;
    let ctrl_c = async {
        signal::ctrl_c().await.expect("install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => tracing::info!("ctrl+c received; shutting down"),
        _ = terminate => tracing::info!("SIGTERM received; shutting down"),
    }
}
