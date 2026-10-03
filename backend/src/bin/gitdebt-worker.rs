//! `gitdebt-worker`: every background pool — repo-history analysis,
//! star-history acquisition (GitHub's star-history endpoint, plus the GH
//! Archive hourly follower for series not yet moved onto it), and the
//! leaderboard refresher — plus a minimal health server. Any number of
//! replicas is safe: queues claim with `FOR UPDATE SKIP LOCKED`, and the
//! hourly follower elects a leader through a session-level advisory lock.

use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use gitdebt::{bootstrap, db::Db};

/// Star-history workers. The work is GitHub-rate-limit bound, not CPU bound:
/// extra tasks buy nothing but hidden round-trip latency, while each one holds
/// a Postgres connection to write with. Four keeps that pressure off a
/// database sharing 12 vCPU with the analysis pool and this host's other
/// tenants.
const STAR_WORKERS: usize = 4;

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

    // Repo-history analysis pool. Separate queue, separate workload shape:
    // clones are disk-heavy + CPU-heavy, not GitHub-API-bound. Interactive
    // profile/report requests enqueue durable priority work. How many run at
    // once, and why that number for this host, lives with the pool itself
    // (`repo_analysis::ANALYSIS_WORKERS`) because `repo_history` divides the
    // host's cores by it for `pack.threads` and for the walk fan-out.
    let storage = Arc::new(gitdebt::repo_history::RepoStorage::from_env());
    let analysis_workers = gitdebt::repo_analysis::configured_analysis_workers();
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
    let star_reset = gitdebt::queue::reset_inflight_on_startup(&db).await?;
    if star_reset > 0 {
        tracing::info!(
            reset_count = star_reset,
            "star-fetch: reset expired in_progress leases"
        );
    }

    // Jobs parked by older releases — `restricted` when GitHub closed the
    // stargazer list, or whole batches after shared BigQuery failures — can
    // all succeed against GitHub's star history. Revived before the pool
    // starts so the first claims already see them.
    let revived = gitdebt::queue::revive_parked_on_startup(&db).await?;
    if revived > 0 {
        tracing::info!(revived, "star-history: revived parked jobs");
    }

    // Deployment bootstrap: the comparison catalog is embedded from the
    // frontend's single source of truth. An empty parse means the embedded
    // source moved or was renamed, which must fail the deployment rather than
    // silently publish comparison pages with no data behind them.
    let catalog = gitdebt::catalog::curated_repos();
    if catalog.is_empty() {
        anyhow::bail!("curated comparison catalog unexpectedly contains no repositories");
    }
    // Star history: one set-based statement that only touches cold or stale
    // rows. Safe to offer in full, once, at startup: a read is a few requests
    // per repository, paced by the worker against the GitHub budget.
    let catalog_star_jobs = gitdebt::queue::enqueue_cold_or_stale_many(&db, &catalog, 0)
        .await
        .context("enqueue curated star histories")?;

    tracing::info!(
        catalog_size = catalog.len(),
        catalog_star_jobs,
        "curated comparison catalog offered to the star-history queue"
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
    // Repo health: a bounded drip *after* the pool exists, never a boot-time
    // flood. Offering all ~117 curated repositories at once filled the queue's
    // global capacity with priority-0 rows before a single worker was running,
    // so a visitor's own enqueue was rejected as `AtCapacity` and every
    // profile fan-out queued behind the whole backfill.
    gitdebt::repo_analysis::spawn_catalog_backfill(db.clone(), catalog);
    // Heal legacy rows with complete history but no public-metadata stamp
    // (invisible to every reader). Startup + hourly, bounded, idempotent;
    // the star-history claim path writes the metadata.
    gitdebt::worker::spawn_metadata_backfill(db.clone());
    // Keep the numbers on README-embedded badges, cards, and OG images
    // current for repositories that are only ever seen through an embed and
    // therefore never hit the site's own refresh path.
    gitdebt::worker::spawn_metadata_refresh(cache.clone(), services.github.clone());
    // Orphaned-clone sweep. Clone paths derive purely from the slug, so N
    // replicas store repo X at the same path string while sharing ONE
    // repo_history row; when another replica evicts X it NULLs that row and
    // this replica's physical copy becomes invisible to the quota
    // accountant — never counted, never evictable. Startup + periodic
    // passes delete local bare-clone dirs no clone_path row references
    // (a 24h mtime guard protects in-flight clones).
    gitdebt::repo_stats::spawn_orphan_clone_sweep(db.clone(), storage.clone());
    // Star history comes from GitHub's own star-history endpoint: exact, net
    // of unstars, complete back to each repository's creation, and public.
    // It is the only writer of new series, so the GH Archive backfill
    // coordinator is no longer started: two writers draining one queue would
    // race to publish different measurements of the same repository.
    gitdebt::worker::spawn_pool(
        gitdebt::worker::WorkerCtx::new(services.github.clone(), cache.clone()),
        STAR_WORKERS,
    );
    // Moves every remaining archive, spliced and frozen series onto GitHub's
    // star history, most-viewed first, then keeps the ones nobody is looking
    // at from going stale.
    gitdebt::worker::spawn_star_history_sweep(db.clone(), services.github.clone());
    tracing::info!(star_workers = STAR_WORKERS, "star-history pool started");
    // Archive-backed series keep advancing hour by hour until the sweep
    // reaches them. Once none remain the follower tracks nothing and stops
    // downloading (see `gh_archive_hourly`).
    if gitdebt::gh_archive::is_configured() {
        gitdebt::archive_hourly_db::spawn(db.clone(), database_url.clone())
            .context("GH Archive hourly follower configuration failed")?;
        tracing::info!("GH Archive hourly follower contending for leadership");
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
        .with_state(db.clone());
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    tracing::info!(%addr, "gitdebt-worker health server listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(bootstrap::shutdown_signal())
        .await?;
    // The analysis pool is about to stop existing, and only this process knows
    // that. Waiting for the two-minute lease to expire instead handed the
    // incoming deployment a queue whose in-progress rows described workers
    // nobody was running — they kept spending catalog-concurrency and
    // queue-capacity budget until some worker happened to steal them minutes
    // later. Runs abandoned here are durable and idempotent: nothing was
    // committed unless the whole aggregate transaction was.
    match gitdebt::repo_analysis::release_pool_claims(&db).await {
        Ok(0) => {}
        Ok(released) => tracing::info!(released, "repo-analysis: handed back in-flight jobs"),
        Err(error) => tracing::warn!(%error, "repo-analysis: releasing in-flight jobs failed"),
    }
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
