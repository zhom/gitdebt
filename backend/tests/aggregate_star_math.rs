//! The cross-repo star-history sum, against a real Postgres.
//!
//! The per-day merge used to happen in Rust over one row per repo per day;
//! it now happens in SQL, so the arithmetic that decides what a profile
//! chart shows is no longer covered by a pure unit test. These cases pin
//! it: overlapping days add, days unique to one repo pass through, the
//! output is date-ascending, and history that has not been proved complete
//! is the caller's gate — the loader itself reads raw rows.
//!
//! Gated on `GITDEBT_TEST_DATABASE_URL` (same convention as
//! `login_repos.rs`) so `cargo test` stays green without a database:
//!
//! ```bash
//! scripts/db.sh up
//! GITDEBT_TEST_DATABASE_URL=postgres://gitdebt:gitdebt@localhost:5432/gitdebt \
//!   cargo test --test aggregate_star_math
//! ```

use chrono::{NaiveDate, TimeZone, Utc};
use gitdebt::aggregate::{deltas_to_series, load_merged_day_deltas};
use gitdebt::db::Db;

fn day(value: &str) -> NaiveDate {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").expect("test date parses")
}

async fn test_db() -> Option<Db> {
    let url = std::env::var("GITDEBT_TEST_DATABASE_URL").ok()?;
    gitdebt::test_db::connect(&url).await.ok()
}

async fn cleanup(db: &Db, prefix: &str) {
    for statement in [
        "DELETE FROM repo_stargazers WHERE repo LIKE $1",
        "DELETE FROM repos WHERE repo LIKE $1",
    ] {
        sqlx::query(statement)
            .bind(format!("{prefix}%"))
            .execute(&db.pool)
            .await
            .expect("cleanup");
    }
}

/// Seed one repository with complete `github_api` history and one star per
/// `(day, count)` pair.
async fn seed_repo(db: &Db, repo: &str, stars: &[(&str, i64)]) {
    sqlx::query(
        "INSERT INTO repos \
             (repo, metadata_fetched_at, missing, history_complete, \
              stargazers_complete, history_source) \
         VALUES ($1, NOW(), FALSE, TRUE, TRUE, 'github_api') \
         ON CONFLICT (repo) DO UPDATE SET history_source = 'github_api'",
    )
    .bind(repo)
    .execute(&db.pool)
    .await
    .expect("seed repos row");
    let mut position = 0i64;
    for (date, count) in stars {
        for _ in 0..*count {
            position += 1;
            sqlx::query(
                "INSERT INTO repo_stargazers (repo, position, starred_at) \
                 VALUES ($1, $2, $3)",
            )
            .bind(repo)
            .bind(position)
            // Noon UTC: far enough from either midnight that a session in a
            // shifted time zone would bucket it into a different day if the
            // query stopped normalizing to UTC.
            .bind(Utc.from_utc_datetime(&day(date).and_hms_opt(12, 0, 0).expect("noon is valid")))
            .execute(&db.pool)
            .await
            .expect("seed star row");
        }
    }
}

#[tokio::test]
async fn cross_repo_day_deltas_sum_in_sql() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let prefix = "gitdebt-test-starmath/";
    cleanup(&db, prefix).await;

    let a = format!("{prefix}a");
    let b = format!("{prefix}b");
    // Overlap on day 1; day 3 is unique to A, day 2 unique to B.
    seed_repo(&db, &a, &[("2021-01-01", 3), ("2021-01-03", 1)]).await;
    seed_repo(&db, &b, &[("2021-01-01", 4), ("2021-01-02", 2)]).await;

    let merged = load_merged_day_deltas(&db, &[a.clone(), b.clone()])
        .await
        .expect("merged deltas load");
    assert_eq!(
        merged,
        vec![
            (day("2021-01-01"), 7),
            (day("2021-01-02"), 2),
            (day("2021-01-03"), 1),
        ],
        "overlapping days add, unique days pass through, output is date-ascending"
    );

    let (series, total) = deltas_to_series(&merged);
    assert_eq!(total, 10);
    let plotted: Vec<(String, u32)> = series
        .iter()
        .map(|point| (point.at.date_naive().to_string(), point.stars))
        .collect();
    assert_eq!(
        plotted,
        vec![
            ("2021-01-01".to_string(), 7),
            ("2021-01-02".to_string(), 9),
            ("2021-01-03".to_string(), 10),
        ]
    );

    // A single repository comes back as its own sorted series.
    let solo = load_merged_day_deltas(&db, std::slice::from_ref(&b))
        .await
        .expect("merged deltas load");
    assert_eq!(solo, vec![(day("2021-01-01"), 4), (day("2021-01-02"), 2)]);

    // No repositories means no query and no rows — the aggregate of an
    // account with nothing complete yet is empty, never an error.
    assert!(
        load_merged_day_deltas(&db, &[])
            .await
            .expect("empty set loads")
            .is_empty()
    );

    // The active-history view keys off the selected source, so a repository
    // parked on archive history contributes nothing from the exact table.
    sqlx::query("UPDATE repos SET history_source = 'gh_archive' WHERE repo = $1")
        .bind(&a)
        .execute(&db.pool)
        .await
        .expect("switch source");
    let after = load_merged_day_deltas(&db, &[a, b])
        .await
        .expect("merged deltas load");
    assert_eq!(
        after,
        vec![(day("2021-01-01"), 4), (day("2021-01-02"), 2)],
        "only the selected history source contributes"
    );

    cleanup(&db, prefix).await;
}
