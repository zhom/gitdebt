//! `gitdebt-worker`: every background pool — repo-history analysis,
//! star-history acquisition (GH Archive coordinator + hourly follower, or
//! the debug stargazer-list fallback), and the leaderboard refresher — plus
//! a minimal health server. Any number of replicas is safe: queues claim
//! with `FOR UPDATE SKIP LOCKED`, and the two GH Archive singletons elect a
//! leader through session-level advisory locks.

use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use gitdebt::{bootstrap, db::Db};

#[tokio::main]
async fn main() -> Result<()> {
    bootstrap::init_process();
    let services = bootstrap::connect_services().await?;
    let cache = services.cache.clone();
    let db = services.db.clone();
    let database_url = services.database_url.clone();
    bootstrap::spawn_service_heartbeat(db.clone(), "worker");

    // Daily leaderboard snapshots. Already replica-safe: the refresh takes a
    // transaction-scoped advisory lock and non-winners return immediately.
    gitdebt::leaderboard::spawn(db.clone());

    // Repo-history analysis pool. Separate queue, separate workload
    // shape: clones are disk-heavy + CPU-heavy, not GitHub-API-bound.
    // Interactive profile/report requests enqueue durable priority work.
    // Default to half the visible CPU quota on a production-sized host while
    // retaining headroom for Postgres, HTTP, and raster work. The explicit
    // ceiling prevents a bad env value from launching an unbounded number of
    // git subprocesses.
    let storage = Arc::new(gitdebt::repo_history::RepoStorage::from_env());
    let default_analysis_workers = std::thread::available_parallelism()
        .map(|cpus| cpus.get().div_ceil(2).clamp(1, 8))
        .unwrap_or(2);
    let requested_analysis_workers: usize = std::env::var("REPO_ANALYSIS_WORKERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default_analysis_workers);
    let analysis_workers = requested_analysis_workers.clamp(1, 8);
    if requested_analysis_workers != analysis_workers {
        tracing::warn!(
            requested = requested_analysis_workers,
            effective = analysis_workers,
            "REPO_ANALYSIS_WORKERS capped to protect git subprocess and thread capacity"
        );
    }
    let reset = gitdebt::repo_analysis::reset_inflight_on_startup(&db).await?;
    if reset > 0 {
        tracing::info!(
            reset_count = reset,
            "repo-analysis: reset expired in_progress leases"
        );
    }
    let analysis_revived = gitdebt::repo_analysis::revive_retryable_on_startup(&db).await?;
    if analysis_revived > 0 {
        tracing::info!(
            revived = analysis_revived,
            "repo-analysis: revived jobs parked by older releases"
        );
    }
    // Star-history acquisition. With GH Archive enabled the leader-elected
    // BigQuery coordinator batches repos into shared corpus scans, while
    // WORKER_COUNT only controls the inexpensive GitHub metadata lookups
    // needed to resolve stable numeric repo IDs. The legacy GitHub
    // stargazer-list pool remains an explicit fallback for local/dev installs.
    let requested_worker_count: usize = std::env::var("WORKER_COUNT")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|n| *n >= 1)
        .unwrap_or(8);
    let worker_count = requested_worker_count.clamp(1, 16);
    if requested_worker_count != worker_count {
        tracing::warn!(
            requested = requested_worker_count,
            effective = worker_count,
            "WORKER_COUNT capped to protect GitHub and socket capacity"
        );
    }
    let star_reset = gitdebt::queue::reset_inflight_on_startup(&db).await?;
    if star_reset > 0 {
        tracing::info!(
            reset_count = star_reset,
            "star-fetch: reset expired in_progress leases"
        );
    }

    // Deployment bootstrap: the comparison catalog is embedded from the
    // frontend's single source of truth. Offer every curated repo to both
    // durable queues before starting the pools, so a fresh deployment begins
    // useful work immediately instead of waiting for a visitor to discover
    // each comparison page. Fresh jobs and active jobs are deduplicated.
    let catalog_size = gitdebt::catalog::curated_repos().len();
    let (catalog_star_jobs, catalog_analysis_jobs) = gitdebt::catalog::enqueue_curated(&db).await?;
    tracing::info!(
        catalog_size,
        catalog_star_jobs,
        catalog_analysis_jobs,
        "curated comparison catalog offered to durable queues"
    );

    gitdebt::repo_analysis::spawn_pool(
        gitdebt::repo_analysis::AnalysisCtx {
            db: db.clone(),
            storage: storage.clone(),
            github: services.github.clone(),
            gh_app: services.gh_app.as_ref().cloned().map(Arc::new),
        },
        analysis_workers,
    );
    tracing::info!(analysis_workers, "repo-analysis worker pool started");
    // Heal legacy rows with complete history but no public-metadata stamp
    // (invisible to every reader). Startup + hourly, bounded, idempotent;
    // the star-fetch claim path writes the metadata. Runs in archive and
    // fallback modes alike.
    gitdebt::worker::spawn_metadata_backfill(db.clone());
    // Orphaned-clone sweep. Clone paths derive purely from the slug, so N
    // replicas store repo X at the same path string while sharing ONE
    // repo_history row; when another replica evicts X it NULLs that row and
    // this replica's physical copy becomes invisible to the quota
    // accountant — never counted, never evictable. Startup + periodic
    // passes delete local bare-clone dirs no clone_path row references
    // (a 24h mtime guard protects in-flight clones).
    gitdebt::repo_stats::spawn_orphan_clone_sweep(db.clone(), storage.clone());
    let archive_client = gitdebt::gh_archive::GhArchiveBigQueryClient::from_env()
        .await
        .context("GH Archive BigQuery configuration/authentication failed")?;
    if let Some(archive_client) = archive_client {
        let revived = gitdebt::queue::revive_retryable_for_archive(&db).await?;
        if revived > 0 {
            tracing::info!(revived, "gh-archive: revived retryable history jobs");
        }
        gitdebt::archive_worker::spawn(
            gitdebt::archive_worker::ArchiveWorkerCtx::from_env(
                Arc::new(archive_client),
                services.github.clone(),
                cache.clone(),
                worker_count,
            ),
            database_url.clone(),
        );
        gitdebt::archive_hourly_db::spawn(db.clone(), database_url.clone())
            .context("GH Archive hourly follower configuration failed")?;
        tracing::info!(
            metadata_concurrency = worker_count,
            "GH Archive historical coordinator and hourly follower contending for leadership"
        );
    } else if cfg!(debug_assertions) {
        gitdebt::worker::spawn_pool(
            gitdebt::worker::WorkerCtx::new(services.github.clone(), cache.clone()),
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

    // Minimal health server for orchestrator probes. Serving it with
    // graceful shutdown also gives the process its SIGTERM handling: when
    // the server stops, main returns, the runtime drops, and every worker
    // task ends. Their state is durable in Postgres; repo-analysis leases
    // heartbeat every 30 seconds and recover after two minutes, archive
    // batch leases heartbeat and recover after 15 minutes, and advisory-lock
    // leadership releases with the closed connections.
    let addr = bootstrap::bind_addr(8788);
    let app = Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .with_state(db);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    tracing::info!(%addr, "gitdebt-worker health server listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(bootstrap::shutdown_signal())
        .await?;
    tracing::info!("worker shut down cleanly");
    Ok(())
}

async fn db_ok(db: &Db) -> bool {
    match sqlx::query("SELECT 1").execute(&db.pool).await {
        Ok(_) => true,
        Err(error) => {
            tracing::error!(%error, "worker health check: database unavailable");
            false
        }
    }
}

/// Liveness: the process is up and can reach Postgres (cheap `SELECT 1`).
async fn health(State(db): State<Db>) -> impl IntoResponse {
    if db_ok(&db).await {
        (StatusCode::OK, "ok")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "database unavailable")
    }
}

/// Readiness: same database ping, JSON envelope for orchestrators.
async fn ready(State(db): State<Db>) -> impl IntoResponse {
    if db_ok(&db).await {
        (StatusCode::OK, Json(serde_json::json!({ "ready": true })))
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "ready": false, "error": "database unavailable" })),
        )
    }
}
