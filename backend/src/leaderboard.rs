//! Durable daily repository rankings.
//!
//! A refresh scans completed public histories once, then atomically replaces a
//! tiny snapshot used by the public API. It deliberately stores repository
//! aggregates only: no actor or stargazer identity is collected.

use anyhow::{Context, Result};
use sqlx::{Postgres, Transaction};
use std::time::Duration;

use crate::db::Db;

const REFRESH_LOCK_ID: i64 = 0x6769_7464_6c62_6401;
const REFRESH_EVERY_HOURS: i64 = 24;

/// Start the lightweight freshness coordinator. The transaction-level
/// advisory lock makes this safe when more than one API replica is running.
pub fn spawn(db: Db) {
    tokio::spawn(async move {
        loop {
            if let Err(error) = refresh_if_stale(&db).await {
                tracing::warn!(%error, "leaderboard snapshot refresh failed");
            }
            tokio::time::sleep(Duration::from_secs(60 * 60)).await;
        }
    });
}

pub async fn refresh_if_stale(db: &Db) -> Result<bool> {
    let mut tx = db.pool.begin().await.context("begin leaderboard refresh")?;
    let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock($1)")
        .bind(REFRESH_LOCK_ID)
        .fetch_one(&mut *tx)
        .await
        .context("lock leaderboard refresh")?;
    if !acquired {
        return Ok(false);
    }

    let fresh: bool = sqlx::query_scalar(
        "SELECT EXISTS (\
             SELECT 1 FROM leaderboard_snapshot_state \
             WHERE id = TRUE \
               AND computed_at > NOW() - make_interval(hours => $1)\
         )",
    )
    .bind(REFRESH_EVERY_HOURS as i32)
    .fetch_one(&mut *tx)
    .await
    .context("check leaderboard freshness")?;
    if fresh {
        return Ok(false);
    }

    refresh(&mut tx).await?;
    tx.commit().await.context("commit leaderboard refresh")?;
    tracing::info!("leaderboard daily/weekly/monthly snapshot refreshed");
    Ok(true)
}

async fn refresh(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    sqlx::query("DELETE FROM leaderboard_snapshots")
        .execute(&mut **tx)
        .await
        .context("clear previous leaderboard snapshot")?;

    // `activity` is materialized once so the 30-day history range is scanned
    // once even though it feeds three rankings. Eligibility is restricted to
    // complete, successfully fetched public metadata; private/404 tombstones
    // can therefore never enter a snapshot.
    sqlx::query(
        "WITH eligible AS MATERIALIZED (\
             SELECT repo, COALESCE(star_count, 0)::BIGINT AS stars \
             FROM repos \
             WHERE history_complete = TRUE \
               AND missing = FALSE \
               AND metadata_fetched_at IS NOT NULL \
               AND star_count IS NOT NULL\
         ), activity AS MATERIALIZED (\
             SELECT s.repo, \
                    COUNT(*) FILTER (WHERE s.starred_at >= NOW() - INTERVAL '1 day')::BIGINT AS gained_1d, \
                    COUNT(*) FILTER (WHERE s.starred_at >= NOW() - INTERVAL '7 days')::BIGINT AS gained_7d, \
                    COUNT(*)::BIGINT AS gained_30d \
             FROM active_repo_star_history s \
             JOIN eligible e ON e.repo = s.repo \
             WHERE s.starred_at >= NOW() - INTERVAL '30 days' \
             GROUP BY s.repo\
         ), ranked AS MATERIALIZED (\
             SELECT e.repo, e.stars, \
                    COALESCE(a.gained_1d, 0) AS gained_1d, \
                    COALESCE(a.gained_7d, 0) AS gained_7d, \
                    COALESCE(a.gained_30d, 0) AS gained_30d, \
                    ROW_NUMBER() OVER (ORDER BY e.stars DESC, e.repo ASC) AS stars_rank, \
                    ROW_NUMBER() OVER (ORDER BY COALESCE(a.gained_1d, 0) DESC, e.repo ASC) AS rank_1d, \
                    ROW_NUMBER() OVER (ORDER BY COALESCE(a.gained_7d, 0) DESC, e.repo ASC) AS rank_7d, \
                    ROW_NUMBER() OVER (ORDER BY COALESCE(a.gained_30d, 0) DESC, e.repo ASC) AS rank_30d \
             FROM eligible e LEFT JOIN activity a ON a.repo = e.repo\
         ) \
         INSERT INTO leaderboard_snapshots \
             (metric, window_days, rank, repo, stars, velocity, computed_at) \
         SELECT 'stars', 1, stars_rank, repo, stars, gained_1d, NOW() FROM ranked \
         UNION ALL \
         SELECT 'stars', 7, stars_rank, repo, stars, gained_7d, NOW() FROM ranked \
         UNION ALL \
         SELECT 'stars', 30, stars_rank, repo, stars, gained_30d, NOW() FROM ranked \
         UNION ALL \
         SELECT 'velocity', 1, rank_1d, repo, stars, gained_1d, NOW() FROM ranked WHERE gained_1d > 0 \
         UNION ALL \
         SELECT 'velocity', 7, rank_7d, repo, stars, gained_7d, NOW() FROM ranked WHERE gained_7d > 0 \
         UNION ALL \
         SELECT 'velocity', 30, rank_30d, repo, stars, gained_30d, NOW() FROM ranked WHERE gained_30d > 0",
    )
    .execute(&mut **tx)
    .await
    .context("build leaderboard snapshot")?;

    sqlx::query(
        "INSERT INTO leaderboard_snapshot_state (id, computed_at) VALUES (TRUE, NOW()) \
         ON CONFLICT (id) DO UPDATE SET computed_at = EXCLUDED.computed_at",
    )
    .execute(&mut **tx)
    .await
    .context("mark leaderboard snapshot complete")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{REFRESH_EVERY_HOURS, REFRESH_LOCK_ID};

    #[test]
    fn refresh_cadence_and_lock_are_stable() {
        assert_eq!(REFRESH_EVERY_HOURS, 24);
        assert_ne!(REFRESH_LOCK_ID, 0);
    }
}
