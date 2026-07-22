//! Exercises the daily snapshot transaction against real Postgres.

use gitdebt::{db::Db, leaderboard};
use sqlx::Row;

#[tokio::test]
async fn refresh_materializes_daily_weekly_monthly_and_star_rows() {
    let Ok(url) = std::env::var("GITDEBT_TEST_DATABASE_URL") else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let db = Db::connect(&url).await.expect("connect test db");
    let repo = format!("leaderboard-test-{}/public", std::process::id());

    sqlx::query("DELETE FROM repo_star_arrivals WHERE repo = $1")
        .bind(&repo)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM repos WHERE repo = $1")
        .bind(&repo)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO repos \
         (repo, history_complete, history_source, star_count, metadata_fetched_at, missing) \
         VALUES ($1, TRUE, 'gh_archive', 3, NOW(), FALSE)",
    )
    .bind(&repo)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO repo_star_arrivals (repo, position, source_event_id, starred_at) VALUES \
         ($1, 1, 'snapshot-1', NOW() - INTERVAL '1 hour'), \
         ($1, 2, 'snapshot-2', NOW() - INTERVAL '2 days'), \
         ($1, 3, 'snapshot-3', NOW() - INTERVAL '10 days')",
    )
    .bind(&repo)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM leaderboard_snapshot_state")
        .execute(&db.pool)
        .await
        .unwrap();

    assert!(leaderboard::refresh_if_stale(&db).await.unwrap());
    let rows = sqlx::query(
        "SELECT metric, window_days, velocity FROM leaderboard_snapshots \
         WHERE repo = $1 ORDER BY metric, window_days",
    )
    .bind(&repo)
    .fetch_all(&db.pool)
    .await
    .unwrap();
    let values: Vec<(String, i32, i64)> = rows
        .into_iter()
        .map(|row| {
            (
                row.get("metric"),
                row.get("window_days"),
                row.get("velocity"),
            )
        })
        .collect();
    assert_eq!(
        values,
        vec![
            ("stars".into(), 1, 1),
            ("stars".into(), 7, 2),
            ("stars".into(), 30, 3),
            ("velocity".into(), 1, 1),
            ("velocity".into(), 7, 2),
            ("velocity".into(), 30, 3),
        ]
    );

    sqlx::query("DELETE FROM repo_star_arrivals WHERE repo = $1")
        .bind(&repo)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM repos WHERE repo = $1")
        .bind(&repo)
        .execute(&db.pool)
        .await
        .unwrap();
    db.pool.close().await;
}
