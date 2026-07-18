//! Integration tests for the `login → public repos` mapping cache backing
//! the org/user aggregate charts, against a real Postgres.
//!
//! Gated on `GITDEBT_TEST_DATABASE_URL` (same convention as
//! `queue_and_analyze.rs`) so `cargo test` stays green in environments
//! without a database. To run:
//!
//! ```bash
//! scripts/db.sh up
//! GITDEBT_TEST_DATABASE_URL=postgres://gitdebt:gitdebt@localhost:5432/gitdebt \
//!   cargo test --test login_repos
//! ```
//!
//! What these lock down is the cache invariant from CLAUDE.md applied to
//! the new tables: readers never trust partial data (`get_login_repos`
//! returns `None` unless `complete = TRUE`), and writers commit atomically
//! (`put_login_repos` replaces the whole set + flips the flag in one
//! transaction; `mark_login_missing` tombstones + clears rows in one
//! transaction).

use gitdebt::cache::Cache;
use gitdebt::db::Db;

async fn test_db() -> Option<Db> {
    let url = std::env::var("GITDEBT_TEST_DATABASE_URL").ok()?;
    Some(Db::connect(&url).await.expect("connect test db"))
}

async fn cleanup(db: &Db, login: &str) {
    let _ = sqlx::query("DELETE FROM login_repos WHERE login = $1")
        .bind(login)
        .execute(&db.pool)
        .await;
    let _ = sqlx::query("DELETE FROM login_repo_lists WHERE login = $1")
        .bind(login)
        .execute(&db.pool)
        .await;
}

#[tokio::test]
async fn unknown_login_reads_as_none() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let cache = Cache::new(db.clone());
    let login = "gitdebt-test-login-unknown";
    cleanup(&db, login).await;

    assert!(cache.get_login_repos_meta(login).await.unwrap().is_none());
    assert!(cache.get_login_repos(login).await.unwrap().is_none());
}

#[tokio::test]
async fn put_then_get_roundtrip_in_rank_order() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let cache = Cache::new(db.clone());
    let login = "gitdebt-test-login-roundtrip";
    cleanup(&db, login).await;

    // Input order (stars-desc, per the aggregate module) is the rank.
    let repos = vec![
        (format!("{login}/big"), 500i64),
        (format!("{login}/mid"), 50i64),
        (format!("{login}/small"), 5i64),
    ];
    cache.put_login_repos(login, &repos).await.unwrap();

    let meta = cache.get_login_repos_meta(login).await.unwrap().unwrap();
    assert!(meta.complete, "put marks the list complete");
    assert!(!meta.missing);
    assert!(meta.fetched_at.is_some(), "put stamps fetched_at");
    assert!(
        meta.fresh_within(chrono::Duration::hours(1)),
        "a just-written list is fresh"
    );

    let got = cache.get_login_repos(login).await.unwrap().unwrap();
    assert_eq!(got, repos, "read returns the exact set in rank order");

    cleanup(&db, login).await;
}

#[tokio::test]
async fn put_replaces_wholesale_not_merges() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let cache = Cache::new(db.clone());
    let login = "gitdebt-test-login-replace";
    cleanup(&db, login).await;

    let first = vec![
        (format!("{login}/gone"), 100i64),
        (format!("{login}/kept"), 10i64),
    ];
    cache.put_login_repos(login, &first).await.unwrap();

    // Second fetch: `gone` was deleted upstream, `new` appeared, `kept`'s
    // count changed. The visible set must be exactly the new one.
    let second = vec![
        (format!("{login}/new"), 900i64),
        (format!("{login}/kept"), 12i64),
    ];
    cache.put_login_repos(login, &second).await.unwrap();

    let got = cache.get_login_repos(login).await.unwrap().unwrap();
    assert_eq!(
        got, second,
        "old rows are gone, counts updated, rank re-derived"
    );

    cleanup(&db, login).await;
}

#[tokio::test]
async fn incomplete_list_is_never_served() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let cache = Cache::new(db.clone());
    let login = "gitdebt-test-login-partial";
    cleanup(&db, login).await;

    // Simulate a writer that died mid-replace: meta row exists with
    // complete = FALSE and some rows present (what a crashed transaction
    // could NOT actually leave behind — but what a bug in a future writer
    // might). The reader must refuse to serve it.
    sqlx::query(
        "INSERT INTO login_repo_lists (login, fetched_at, complete, missing) \
         VALUES ($1, NOW(), FALSE, FALSE)",
    )
    .bind(login)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO login_repos (login, repo, stars, rank) VALUES ($1, $2, 1, 0)")
        .bind(login)
        .bind(format!("{login}/half-written"))
        .execute(&db.pool)
        .await
        .unwrap();

    assert!(
        cache.get_login_repos(login).await.unwrap().is_none(),
        "readers never trust partial data: complete = FALSE ⇒ None"
    );
    // The meta read still works so callers can decide to re-fetch.
    let meta = cache.get_login_repos_meta(login).await.unwrap().unwrap();
    assert!(!meta.complete);

    cleanup(&db, login).await;
}

#[tokio::test]
async fn missing_tombstone_clears_rows_and_put_clears_tombstone() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let cache = Cache::new(db.clone());
    let login = "gitdebt-test-login-missing";
    cleanup(&db, login).await;

    // Cached list exists, then GitHub starts 404ing the login.
    let repos = vec![(format!("{login}/r"), 3i64)];
    cache.put_login_repos(login, &repos).await.unwrap();
    cache.mark_login_missing(login).await.unwrap();

    let meta = cache.get_login_repos_meta(login).await.unwrap().unwrap();
    assert!(meta.missing, "tombstone recorded");
    assert!(!meta.complete, "tombstone clears completeness");
    assert!(meta.fetched_at.is_some(), "tombstone is TTL-stamped");
    assert!(
        cache.get_login_repos(login).await.unwrap().is_none(),
        "no rows served for a tombstoned login"
    );
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM login_repos WHERE login = $1")
        .bind(login)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(n, 0, "tombstone deletes the cached rows");

    // A later successful fetch (account recreated / renamed back) clears
    // the tombstone atomically.
    cache.put_login_repos(login, &repos).await.unwrap();
    let meta = cache.get_login_repos_meta(login).await.unwrap().unwrap();
    assert!(!meta.missing);
    assert!(meta.complete);
    assert_eq!(cache.get_login_repos(login).await.unwrap().unwrap(), repos);

    cleanup(&db, login).await;
}

#[tokio::test]
async fn empty_repo_list_is_a_valid_complete_list() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let cache = Cache::new(db.clone());
    let login = "gitdebt-test-login-empty";
    cleanup(&db, login).await;

    // A login with zero public repos caches an empty-but-complete list, so
    // the aggregate doesn't re-hit GitHub on every request.
    cache.put_login_repos(login, &[]).await.unwrap();
    let meta = cache.get_login_repos_meta(login).await.unwrap().unwrap();
    assert!(meta.complete);
    let got = cache.get_login_repos(login).await.unwrap().unwrap();
    assert!(got.is_empty());

    cleanup(&db, login).await;
}
