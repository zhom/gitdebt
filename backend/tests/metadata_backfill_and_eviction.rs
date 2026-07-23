//! Integration tests for worker-correctness invariants, against a real
//! Postgres:
//!
//!   1. The metadata backfill sweep: legacy rows with complete history but
//!      no `metadata_fetched_at` stamp (ingested before the public-metadata
//!      read gate existed) are re-enqueued into the durable star-fetch
//!      queue, and a single `put_repo_metadata` write — exactly what the
//!      claim paths perform — opens every reader gate.
//!   2. Replica-safe clone eviction: `evict_to_quota` must neither count
//!      nor "evict" a `repo_history` row whose clone path does not exist on
//!      this replica's filesystem (the bytes live on another replica).
//!   3. The orphaned-clone sweep: bare-clone directories on this replica's
//!      disk that no `repo_history.clone_path` row references (another
//!      replica evicted the shared row) are deleted once older than the
//!      in-flight-clone guard, while referenced or freshly-modified
//!      directories — and anything outside `REPOS_DIR` — survive.
//!
//! Gated on `GITDEBT_TEST_DATABASE_URL` so `cargo test` stays green in
//! environments without a database:
//!
//! ```bash
//! scripts/db.sh up
//! GITDEBT_TEST_DATABASE_URL=postgres://gitdebt:gitdebt@localhost:5432/gitdebt \
//!   cargo test --test metadata_backfill_and_eviction
//! ```
//!
//! Each test namespaces its repos with a unique prefix and cleans up after
//! itself, so the suite is safe to run repeatedly against a shared dev
//! database without colliding.

use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use chrono::{TimeZone, Utc};
use gitdebt::{cache::Cache, db::Db, queue, repo_history::RepoStorage, repo_stats, worker};
use sqlx::postgres::PgPoolOptions;
use tokio::sync::Mutex;

static SCHEMA_READY: OnceLock<()> = OnceLock::new();
static SCHEMA_LOCK: Mutex<()> = Mutex::const_new(());

/// Returns a connected `Db` if a test database is configured, else `None`
/// (the test then no-ops). Keeps the suite green where no DB exists.
async fn test_db() -> Option<Db> {
    let url = std::env::var("GITDEBT_TEST_DATABASE_URL").ok()?;

    // Tokio creates one runtime per #[tokio::test], so a PgPool must not be
    // shared through a static OnceCell: the runtime that owns its maintenance
    // tasks can disappear while sibling tests still use the pool. Initialize
    // the schema exactly once, then give every test a small runtime-local pool.
    let schema_guard = SCHEMA_LOCK.lock().await;
    if SCHEMA_READY.get().is_none() {
        let db = gitdebt::test_db::connect(&url)
            .await
            .expect("connect test db");
        SCHEMA_READY.set(()).expect("schema initialized once");
        return Some(db);
    }
    drop(schema_guard);

    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect test pool");
    Some(Db { pool })
}

async fn cleanup(db: &Db, prefix: &str) {
    let like = format!("{prefix}%");
    let _ = sqlx::query("DELETE FROM star_fetch_queue WHERE repo LIKE $1")
        .bind(&like)
        .execute(&db.pool)
        .await;
    let _ = sqlx::query("DELETE FROM repo_stargazers WHERE repo LIKE $1")
        .bind(&like)
        .execute(&db.pool)
        .await;
    let _ = sqlx::query("DELETE FROM repo_star_arrivals WHERE repo LIKE $1")
        .bind(&like)
        .execute(&db.pool)
        .await;
    let _ = sqlx::query("DELETE FROM repo_history WHERE repo LIKE $1")
        .bind(&like)
        .execute(&db.pool)
        .await;
    let _ = sqlx::query("DELETE FROM repos WHERE repo LIKE $1")
        .bind(&like)
        .execute(&db.pool)
        .await;
}

/// Mirror of the profile-card owned-repos visibility gate
/// (`load_user_card_data` in `api.rs`): a repo only contributes to the user
/// card once it is non-missing AND publicly proven via `metadata_fetched_at`.
async fn user_card_visible_repos(db: &Db, login: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM repos \
         WHERE repos.repo LIKE $1 || '/%' \
           AND NOT repos.missing \
           AND repos.metadata_fetched_at IS NOT NULL",
    )
    .bind(login)
    .fetch_one(&db.pool)
    .await
    .expect("card visibility gate query")
}

#[tokio::test]
async fn metadata_backfill_sweep_heals_legacy_complete_repos() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let owner = "gitdebt-test-meta-sweep";
    let prefix = "gitdebt-test-meta-sweep/";
    cleanup(&db, prefix).await;

    let cache = Cache::new(db.clone());
    let at = Utc.timestamp_opt(1_700_000_000, 0).unwrap();

    // The broken state 14697fd left behind: history fully ingested and
    // complete, but `metadata_fetched_at` never stamped.
    let legacy = format!("{prefix}legacy");
    cache
        .put_repo_stargazers(&legacy, &[(1, at)])
        .await
        .unwrap();
    let state = cache
        .get_archive_backfill_state(&legacy)
        .await
        .unwrap()
        .expect("repos row exists");
    assert!(state.exact_history_complete);
    assert!(
        state.metadata_missing,
        "legacy rows must surface the missing public-metadata stamp \
         so the archive claim path knows to heal them"
    );

    // Controls the sweep must NOT pick up.
    let healed = format!("{prefix}healed");
    cache
        .put_repo_stargazers(&healed, &[(1, at)])
        .await
        .unwrap();
    cache
        .put_repo_metadata(&healed, Some(9002), 1, 0, None)
        .await
        .unwrap();
    let tombstoned = format!("{prefix}tombstoned");
    cache
        .put_repo_stargazers(&tombstoned, &[(1, at)])
        .await
        .unwrap();
    cache.mark_repo_missing(&tombstoned).await.unwrap();
    let cold = format!("{prefix}cold");
    sqlx::query("INSERT INTO repos (repo) VALUES ($1)")
        .bind(&cold)
        .execute(&db.pool)
        .await
        .unwrap();
    let parked = format!("{prefix}parked");
    cache
        .put_repo_stargazers(&parked, &[(1, at)])
        .await
        .unwrap();
    queue::enqueue(&db, &parked, 0).await.unwrap();
    queue::mark_restricted(&db, &parked, "test park")
        .await
        .unwrap();

    // Every reader gate is closed before the heal.
    assert!(cache.get_repo_stargazers(&legacy).await.unwrap().is_none());
    assert!(!cache.repo_stargazers_complete(&legacy).await.unwrap());
    assert_eq!(cache.get_repo_star_count(&legacy).await.unwrap(), None);
    assert_eq!(user_card_visible_repos(&db, owner).await, 1);

    // One sweep pass enqueues exactly the legacy row from this fixture.
    let swept = worker::sweep_missing_metadata(&db).await.unwrap();
    assert!(swept.contains(&legacy), "legacy repo is swept");
    assert!(
        !swept.contains(&healed),
        "stamped repos are not re-enqueued"
    );
    assert!(!swept.contains(&tombstoned), "tombstones stay untouched");
    assert!(!swept.contains(&cold), "cold repos have nothing to surface");
    assert!(
        !swept.contains(&parked),
        "terminal queue parks are not revived by the sweep"
    );
    let (status, priority): (String, i64) =
        sqlx::query_as("SELECT status, priority FROM star_fetch_queue WHERE repo = $1")
            .bind(&legacy)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(status, "pending");
    assert_eq!(priority, 0, "ordinary popularity-snapshot priority");
    let parked_status: String =
        sqlx::query_scalar("SELECT status FROM star_fetch_queue WHERE repo = $1")
            .bind(&parked)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(parked_status, "dead");

    // Simulate the claim path's heal: both the archive coordinator and the
    // fallback worker write metadata via `put_repo_metadata` before touching
    // any history — no stargazer pagination involved.
    cache
        .put_repo_metadata(&legacy, Some(9001), 1, 0, None)
        .await
        .unwrap();
    queue::complete(&db, &legacy).await.unwrap();

    // Every gate opens.
    assert_eq!(
        cache.get_repo_stargazers(&legacy).await.unwrap().unwrap(),
        vec![at]
    );
    assert!(cache.repo_stargazers_complete(&legacy).await.unwrap());
    assert_eq!(cache.get_repo_star_count(&legacy).await.unwrap(), Some(1));
    assert_eq!(user_card_visible_repos(&db, owner).await, 2);
    assert!(
        !cache
            .get_archive_backfill_state(&legacy)
            .await
            .unwrap()
            .unwrap()
            .metadata_missing
    );

    // A second pass no longer selects the healed repo.
    let second = worker::sweep_missing_metadata(&db).await.unwrap();
    assert!(!second.contains(&legacy));

    // The sweep is global by design; restore the shared dev database's queue
    // state for any rows it enqueued outside this test's namespace.
    for repo in swept.into_iter().chain(second) {
        if !repo.starts_with(prefix) {
            queue::complete(&db, &repo).await.unwrap();
        }
    }
    cleanup(&db, prefix).await;
}

#[tokio::test]
async fn eviction_only_touches_clones_present_on_this_replica() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-evict-replica/";
    cleanup(&db, prefix).await;

    const TIB: u64 = 1024 * 1024 * 1024 * 1024;
    let clone_root = tempfile::tempdir().expect("temp clone root");
    let storage = RepoStorage {
        root: clone_root.path().to_path_buf(),
        quota_bytes: 2 * TIB,
        high_watermark_pct: 80, // target = 1.6 TiB
    };

    // A row whose clone lives on ANOTHER replica: the global DB row points
    // at a path that does not exist on this replica's filesystem. Sized far
    // above the quota so the old (global-sum) behavior would have "evicted"
    // it — trivially succeeding on the missing path and then nulling a row
    // that still has real bytes elsewhere.
    let foreign = format!("{prefix}remote");
    let foreign_path = clone_root.path().join("remote.git"); // never created
    repo_stats::record_clone(&db, &foreign, &foreign_path, 5 * TIB)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE repo_history SET last_visited_at = NOW() - INTERVAL '3 days' WHERE repo = $1",
    )
    .bind(&foreign)
    .execute(&db.pool)
    .await
    .unwrap();

    let freed = repo_stats::evict_to_quota(&db, &storage).await.unwrap();
    assert_eq!(
        freed, 0,
        "a clone absent from this replica's disk must not count toward the quota"
    );
    let foreign_row: (Option<String>, Option<i64>) =
        sqlx::query_as("SELECT clone_path, clone_size_bytes FROM repo_history WHERE repo = $1")
            .bind(&foreign)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert!(
        foreign_row.0.is_some() && foreign_row.1.is_some(),
        "the foreign row must not be marked evicted"
    );

    // A genuinely local, stale clone over the target IS evicted — and the
    // foreign row still survives the pass that does real work.
    let local = format!("{prefix}local");
    let local_path = clone_root.path().join("local.git");
    tokio::fs::create_dir_all(&local_path).await.unwrap();
    tokio::fs::write(local_path.join("marker"), b"x")
        .await
        .unwrap();
    repo_stats::record_clone(&db, &local, &local_path, 10 * TIB)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE repo_history SET last_visited_at = NOW() - INTERVAL '3 days' WHERE repo = $1",
    )
    .bind(&local)
    .execute(&db.pool)
    .await
    .unwrap();

    let freed = repo_stats::evict_to_quota(&db, &storage).await.unwrap();
    assert_eq!(freed, 10 * TIB, "only the local clone's bytes are freed");
    assert!(!local_path.exists(), "the local clone directory is removed");
    let local_row: (Option<String>, Option<i64>) =
        sqlx::query_as("SELECT clone_path, clone_size_bytes FROM repo_history WHERE repo = $1")
            .bind(&local)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert!(
        local_row.0.is_none() && local_row.1.is_none(),
        "the local row is marked evicted"
    );
    let foreign_row: Option<String> =
        sqlx::query_scalar("SELECT clone_path FROM repo_history WHERE repo = $1")
            .bind(&foreign)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert!(
        foreign_row.is_some(),
        "the foreign row survives a pass that evicts local clones"
    );

    cleanup(&db, prefix).await;
}

/// Backdate a directory's mtime so the orphan sweep's in-flight-clone guard
/// sees it as old. Setting times through an open handle is portable across
/// the unix targets the worker runs on.
fn set_dir_mtime_hours_ago(path: &std::path::Path, hours: u64) {
    let target = SystemTime::now() - Duration::from_secs(hours * 60 * 60);
    let dir = std::fs::File::open(path).expect("open dir for mtime backdate");
    dir.set_modified(target).expect("backdate dir mtime");
}

/// The cross-replica orphan defect: clone paths derive purely from the slug,
/// so replica B evicting repo X NULLs the single global `repo_history` row
/// while replica A still holds physical bytes at the same path string —
/// invisible to `evict_to_quota` (which only ranks referenced rows) and
/// therefore never freed. The sweep must delete exactly such stale
/// unreferenced directories, and nothing else.
#[tokio::test]
async fn orphan_sweep_removes_only_stale_unreferenced_clone_dirs() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let owner = "gitdebt-test-orphan-sweep";
    let prefix = "gitdebt-test-orphan-sweep/";
    cleanup(&db, prefix).await;

    let clone_root = tempfile::tempdir().expect("temp repos dir");
    let root = clone_root.path();
    let storage = RepoStorage {
        root: root.to_path_buf(),
        quota_bytes: 1024 * 1024 * 1024 * 1024, // irrelevant to the sweep
        high_watermark_pct: 80,
    };

    // Stale orphan: old mtime, real pack bytes, NO repo_history row (the
    // shared row was NULLed by another replica's eviction). MUST be deleted.
    let orphan = root.join(owner).join("orphan.git");
    std::fs::create_dir_all(orphan.join("objects/pack")).expect("orphan tree");
    std::fs::write(orphan.join("objects/pack/pack-fake.pack"), vec![0u8; 2048])
        .expect("orphan pack");
    set_dir_mtime_hours_ago(&orphan, 48);

    // Referenced clone: old mtime but a matching clone_path row — survives
    // regardless of age (the row, not freshness, is what protects it).
    let tracked_repo = format!("{prefix}tracked");
    let tracked = storage.path_for(&tracked_repo);
    std::fs::create_dir_all(tracked.join("objects/pack")).expect("tracked tree");
    repo_stats::record_clone(&db, &tracked_repo, &tracked, 123)
        .await
        .unwrap();
    set_dir_mtime_hours_ago(&tracked, 48);

    // Fresh orphan: no row, but modified within 24h — presumed to be an
    // in-flight clone that has not written its row yet. Survives.
    let recent = root.join(owner).join("recent.git");
    std::fs::create_dir_all(&recent).expect("recent dir");

    // A symlinked ".git" entry pointing OUTSIDE the repos dir: the sweep
    // must neither follow nor delete through it.
    let outside = tempfile::tempdir().expect("outside dir");
    let outside_clone = outside.path().join("victim.git");
    std::fs::create_dir_all(&outside_clone).expect("outside clone");
    set_dir_mtime_hours_ago(&outside_clone, 48);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside_clone, root.join(owner).join("escape.git"))
        .expect("plant symlink");

    let freed = repo_stats::sweep_orphaned_clones(&db, &storage)
        .await
        .unwrap();
    assert_eq!(freed, 2048, "exactly the stale orphan's pack bytes freed");
    assert!(!orphan.exists(), "stale unreferenced orphan is deleted");
    assert!(
        tracked.exists(),
        "referenced dir survives despite old mtime"
    );
    assert!(
        recent.exists(),
        "recent orphan survives the in-flight guard"
    );
    assert!(
        outside_clone.exists(),
        "a symlink inside REPOS_DIR never reaches targets outside it"
    );

    // The sweep touches only the disk: the referenced row keeps its path.
    let tracked_row: Option<String> =
        sqlx::query_scalar("SELECT clone_path FROM repo_history WHERE repo = $1")
            .bind(&tracked_repo)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert!(tracked_row.is_some(), "tracked row keeps its clone_path");

    // Idempotent: a second pass finds nothing more to remove.
    let again = repo_stats::sweep_orphaned_clones(&db, &storage)
        .await
        .unwrap();
    assert_eq!(again, 0, "second pass is a no-op");

    cleanup(&db, prefix).await;
}
