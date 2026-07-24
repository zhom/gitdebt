//! Persists `repo_history::CommitInfo` aggregates into Postgres. Updates
//! per-file, per-author, per-day rows; eviction-aware.
//!
//! Idempotency: each commit is identified by SHA via `last_analyzed_sha`
//! on `repo_history`. The walker only yields commits past that SHA, so
//! re-running this aggregator over the same range double-counts —
//! responsibility for "don't replay history" sits in `repo_history`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use md5::{Digest, Md5};

use crate::db::Db;
use crate::repo_history::{CommitInfo, RepoStorage, clone_size_bytes, evict_clone};

/// Avatar URL heuristic from a git author email.
/// 1. GitHub noreply emails (`<id+username>@users.noreply.github.com`)
///    map directly to the user's avatar via the numeric id.
/// 2. Anything else falls back to gravatar (md5 of the lowercased email).
fn avatar_for_email(email: &str) -> Option<String> {
    let trimmed = email.trim().to_lowercase();
    if let Some(local) = trimmed.strip_suffix("@users.noreply.github.com") {
        // Format: "<id>+<login>" or "<login>" (older accounts).
        if let Some((id_str, _login)) = local.split_once('+')
            && id_str.parse::<u64>().is_ok()
        {
            return Some(format!(
                "https://avatars.githubusercontent.com/u/{id_str}?s=80&v=4"
            ));
        }
    }
    // Gravatar fallback. d=identicon makes empty profiles render a stable shape.
    let mut hasher = Md5::new();
    hasher.update(trimmed.as_bytes());
    let hex = hex::encode(hasher.finalize());
    Some(format!(
        "https://www.gravatar.com/avatar/{hex}?d=identicon&s=80"
    ))
}

fn github_login_for_email(email: &str) -> Option<String> {
    let trimmed = email.trim().to_lowercase();
    let local = trimmed.strip_suffix("@users.noreply.github.com")?;
    if let Some((_id, login)) = local.split_once('+')
        && !login.is_empty()
    {
        return Some(login.to_string());
    }
    if !local.is_empty() {
        return Some(local.to_string());
    }
    None
}

/// Pre-reduced per-author aggregate for one `apply_commits` batch.
#[derive(Clone, Debug, PartialEq)]
struct AuthorAgg {
    /// Most-recent commit's name in oldest-first iteration order. The old
    /// per-row upsert set `author_name = EXCLUDED.author_name` on every
    /// commit, so the LAST commit processed wins; commits arrive oldest-
    /// first, so this is the newest commit's name.
    name: String,
    /// First non-null avatar in oldest-first order (old upsert used
    /// `COALESCE(existing, EXCLUDED)`, so the earliest non-null sticks).
    avatar: Option<String>,
    /// First non-null login in oldest-first order (same COALESCE rule).
    login: Option<String>,
    commits: i64,
    first_commit_at: DateTime<Utc>,
    last_commit_at: DateTime<Utc>,
}

/// Per-file aggregate for one batch.
#[derive(Clone, Debug, PartialEq)]
struct FileAgg {
    commits: i64,
    fix_commits: i64,
    last_modified_at: DateTime<Utc>,
}

/// Per-day TODO delta for one batch.
#[derive(Clone, Debug, Default, PartialEq)]
struct TodoAgg {
    added: i64,
    removed: i64,
}

/// Fully-reduced aggregates for a batch of commits. Computed in Rust so the
/// DB sees a handful of chunked multi-row upserts instead of ~O(commits ×
/// paths) single-row statements. The reduction reproduces the OLD per-row
/// upsert semantics exactly (see field docs on [`AuthorAgg`]); the SQL
/// `ON CONFLICT` clauses then fold these per-batch values into any
/// pre-existing DB rows with the same COALESCE / LEAST / GREATEST rules.
struct BatchAggregates {
    authors: HashMap<String, AuthorAgg>,
    author_commit_days: HashMap<(String, NaiveDate), i64>,
    files: HashMap<String, FileAgg>,
    commit_days: HashMap<NaiveDate, i64>,
    todo_days: HashMap<NaiveDate, TodoAgg>,
}

/// Pure reduction of a commit batch into per-author / per-file / per-day
/// aggregates. Order-sensitive only where the OLD code was: author name
/// takes the last (newest) commit's value, while avatar/login keep the
/// first non-null — matching the old `EXCLUDED.author_name` /
/// `COALESCE(existing, EXCLUDED)` upsert. Kept pure + unit-tested to lock
/// equivalence with the previous row-by-row path.
fn aggregate_commits(commits: &[CommitInfo]) -> BatchAggregates {
    let mut authors: HashMap<String, AuthorAgg> = HashMap::new();
    let mut author_commit_days: HashMap<(String, NaiveDate), i64> = HashMap::new();
    let mut files: HashMap<String, FileAgg> = HashMap::new();
    let mut commit_days: HashMap<NaiveDate, i64> = HashMap::new();
    let mut todo_days: HashMap<NaiveDate, TodoAgg> = HashMap::new();

    for commit in commits {
        let committed_at = commit.committed_at;
        let day = commit.committed_day;
        let avatar = avatar_for_email(&commit.author_email);
        let login = github_login_for_email(&commit.author_email);

        authors
            .entry(commit.author_email.clone())
            .and_modify(|a| {
                // name: last-wins (oldest-first ⇒ newest commit's name).
                a.name = commit.author_name.clone();
                // avatar/login: first non-null sticks.
                if a.avatar.is_none() {
                    a.avatar = avatar.clone();
                }
                if a.login.is_none() {
                    a.login = login.clone();
                }
                a.commits += 1;
                if committed_at < a.first_commit_at {
                    a.first_commit_at = committed_at;
                }
                if committed_at > a.last_commit_at {
                    a.last_commit_at = committed_at;
                }
            })
            .or_insert_with(|| AuthorAgg {
                name: commit.author_name.clone(),
                avatar: avatar.clone(),
                login: login.clone(),
                commits: 1,
                first_commit_at: committed_at,
                last_commit_at: committed_at,
            });

        *author_commit_days
            .entry((commit.author_email.clone(), day))
            .or_insert(0) += 1;
        *commit_days.entry(day).or_insert(0) += 1;

        if commit.todo_added > 0 || commit.todo_removed > 0 {
            let t = todo_days.entry(day).or_default();
            t.added += commit.todo_added as i64;
            t.removed += commit.todo_removed as i64;
        }

        let fix_inc: i64 = if commit.is_fix { 1 } else { 0 };
        for path in &commit.paths_changed {
            files
                .entry(path.clone())
                .and_modify(|f| {
                    f.commits += 1;
                    f.fix_commits += fix_inc;
                    if committed_at > f.last_modified_at {
                        f.last_modified_at = committed_at;
                    }
                })
                .or_insert_with(|| FileAgg {
                    commits: 1,
                    fix_commits: fix_inc,
                    last_modified_at: committed_at,
                });
        }
    }

    BatchAggregates {
        authors,
        author_commit_days,
        files,
        commit_days,
        todo_days,
    }
}

/// Rows-per-chunk for the multi-row UNNEST upserts. A few thousand keeps
/// each statement's parameter count well under Postgres's 65535-bind limit
/// (the widest table here binds 7 columns ⇒ ~14k binds at 2000 rows) while
/// still collapsing a 100k-commit repo's writes into a handful of
/// round-trips instead of hundreds of thousands.
const UPSERT_CHUNK: usize = 2000;

/// Apply a batch of new commits to all relevant aggregate tables in one
/// transaction. Caller passes only commits not yet seen (per the
/// last_analyzed_sha convention) — this function does NOT deduplicate.
///
/// Writes are aggregated in Rust first ([`aggregate_commits`]) and emitted
/// as chunked multi-row `INSERT … SELECT … FROM UNNEST(…) ON CONFLICT …`
/// upserts. This produces identical final values to the previous
/// row-by-row loop but with far fewer statements / shorter-held row locks.
pub async fn apply_commits(db: &Db, repo: &str, commits: &[CommitInfo]) -> Result<()> {
    if commits.is_empty() {
        return Ok(());
    }
    let head_sha = commits
        .last()
        .map(|commit| commit.sha.as_str())
        .unwrap_or_default();
    apply_commits_at_head(db, repo, commits, head_sha).await
}

/// Apply a complete analyzed range and atomically advance its cursor to the
/// actual repository HEAD. `HEAD` can be a merge commit omitted by the
/// `--no-merges` fact walk; storing the real head prevents the next
/// incremental run from replaying commits on merged branches. An empty fact
/// range still advances the cursor for merge-only changes.
pub async fn apply_commits_at_head(
    db: &Db,
    repo: &str,
    commits: &[CommitInfo],
    analyzed_head_sha: &str,
) -> Result<()> {
    write_commits_at_head(db, repo, commits, analyzed_head_sha, false, None).await
}

/// Increment commit-derived aggregates while pinning `total_commits` to the
/// exact reachable graph count (including merges).
pub async fn apply_commits_at_head_with_total(
    db: &Db,
    repo: &str,
    commits: &[CommitInfo],
    analyzed_head_sha: &str,
    reachable_commits: usize,
) -> Result<()> {
    write_commits_at_head(
        db,
        repo,
        commits,
        analyzed_head_sha,
        false,
        Some(i64::try_from(reachable_commits).unwrap_or(i64::MAX)),
    )
    .await
}

/// Atomically replace all commit-derived aggregates for `repo` with a fresh
/// bounded analysis window while storing the exact reachable commit count.
/// This repairs earlier truncated cursors without exposing an empty or partial
/// set to readers between DELETE and INSERT.
pub async fn replace_commits_at_head(
    db: &Db,
    repo: &str,
    commits: &[CommitInfo],
    analyzed_head_sha: &str,
    reachable_commits: usize,
) -> Result<()> {
    write_commits_at_head(
        db,
        repo,
        commits,
        analyzed_head_sha,
        true,
        Some(i64::try_from(reachable_commits).unwrap_or(i64::MAX)),
    )
    .await
}

async fn write_commits_at_head(
    db: &Db,
    repo: &str,
    commits: &[CommitInfo],
    analyzed_head_sha: &str,
    replace: bool,
    exact_total: Option<i64>,
) -> Result<()> {
    let agg = aggregate_commits(commits);

    let mut tx = db.pool.begin().await.context("begin tx")?;
    let now = Utc::now();

    if replace {
        for table in [
            "repo_author_commit_days",
            "repo_commit_days",
            "repo_todo_deltas",
            "repo_file_stats",
        ] {
            let sql = format!("DELETE FROM {table} WHERE repo = $1");
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(repo)
                .execute(&mut *tx)
                .await
                .with_context(|| format!("replace {table}"))?;
        }
        // `repo_author_stats` is rebuilt in place rather than deleted: it also
        // carries the GitHub identity enrichment (login, avatar, and the
        // negative-cache stamp that keeps unresolvable authors from being
        // re-queried). Deleting the rows discarded all of it and made the next
        // enrichment sweep re-resolve every author of the repository from
        // scratch. Zeroing the commit-derived columns lets the upsert below
        // rebuild them exactly as a fresh insert would.
        sqlx::query(
            "UPDATE repo_author_stats              SET commits = 0, first_commit_at = NULL, last_commit_at = NULL              WHERE repo = $1",
        )
        .bind(repo)
        .execute(&mut *tx)
        .await
        .context("reset author commit aggregates")?;
    }

    // Authors
    let author_rows: Vec<(&String, &AuthorAgg)> = agg.authors.iter().collect();
    for chunk in author_rows.chunks(UPSERT_CHUNK) {
        let mut emails: Vec<String> = Vec::with_capacity(chunk.len());
        let mut names: Vec<String> = Vec::with_capacity(chunk.len());
        let mut avatars: Vec<Option<String>> = Vec::with_capacity(chunk.len());
        let mut logins: Vec<Option<String>> = Vec::with_capacity(chunk.len());
        let mut counts: Vec<i64> = Vec::with_capacity(chunk.len());
        let mut firsts: Vec<DateTime<Utc>> = Vec::with_capacity(chunk.len());
        let mut lasts: Vec<DateTime<Utc>> = Vec::with_capacity(chunk.len());
        for (email, a) in chunk {
            emails.push((*email).clone());
            names.push(a.name.clone());
            avatars.push(a.avatar.clone());
            logins.push(a.login.clone());
            counts.push(a.commits);
            firsts.push(a.first_commit_at);
            lasts.push(a.last_commit_at);
        }
        sqlx::query(
            "INSERT INTO repo_author_stats (repo, author_email, author_name, avatar_url, \
                                            github_login, commits, first_commit_at, last_commit_at) \
             SELECT $1, e, n, av, lg, c, f, l \
             FROM UNNEST($2::text[], $3::text[], $4::text[], $5::text[], \
                         $6::bigint[], $7::timestamptz[], $8::timestamptz[]) \
                  AS t(e, n, av, lg, c, f, l) \
             ON CONFLICT (repo, author_email) DO UPDATE SET \
                author_name = EXCLUDED.author_name, \
                avatar_url = COALESCE(repo_author_stats.avatar_url, EXCLUDED.avatar_url), \
                github_login = COALESCE(repo_author_stats.github_login, EXCLUDED.github_login), \
                commits = repo_author_stats.commits + EXCLUDED.commits, \
                first_commit_at = LEAST(repo_author_stats.first_commit_at, EXCLUDED.first_commit_at), \
                last_commit_at = GREATEST(repo_author_stats.last_commit_at, EXCLUDED.last_commit_at)",
        )
        .bind(repo)
        .bind(&emails)
        .bind(&names)
        .bind(&avatars)
        .bind(&logins)
        .bind(&counts)
        .bind(&firsts)
        .bind(&lasts)
        .execute(&mut *tx)
        .await
        .context("upsert authors")?;
    }

    // Per-author/day buckets back truthful user streaks. Store the same stable
    // author key as repo_author_stats; GitHub-login enrichment can then join
    // the two without rewriting historical day rows.
    let author_day_rows: Vec<(&(String, NaiveDate), &i64)> =
        agg.author_commit_days.iter().collect();
    for chunk in author_day_rows.chunks(UPSERT_CHUNK) {
        let mut emails: Vec<String> = Vec::with_capacity(chunk.len());
        let mut days: Vec<NaiveDate> = Vec::with_capacity(chunk.len());
        let mut counts: Vec<i64> = Vec::with_capacity(chunk.len());
        for ((email, day), count) in chunk {
            emails.push(email.clone());
            days.push(*day);
            counts.push(**count);
        }
        sqlx::query(
            "INSERT INTO repo_author_commit_days (repo, author_email, day, commits) \
             SELECT $1, e, d, c \
             FROM UNNEST($2::text[], $3::date[], $4::bigint[]) AS t(e, d, c) \
             ON CONFLICT (repo, author_email, day) DO UPDATE SET \
                commits = repo_author_commit_days.commits + EXCLUDED.commits",
        )
        .bind(repo)
        .bind(&emails)
        .bind(&days)
        .bind(&counts)
        .execute(&mut *tx)
        .await
        .context("upsert author commit days")?;
    }

    // Per-day commit buckets
    let day_rows: Vec<(&NaiveDate, &i64)> = agg.commit_days.iter().collect();
    for chunk in day_rows.chunks(UPSERT_CHUNK) {
        let mut days: Vec<NaiveDate> = Vec::with_capacity(chunk.len());
        let mut counts: Vec<i64> = Vec::with_capacity(chunk.len());
        for (day, c) in chunk {
            days.push(**day);
            counts.push(**c);
        }
        sqlx::query(
            "INSERT INTO repo_commit_days (repo, day, commits) \
             SELECT $1, d, c FROM UNNEST($2::date[], $3::bigint[]) AS t(d, c) \
             ON CONFLICT (repo, day) DO UPDATE SET \
                commits = repo_commit_days.commits + EXCLUDED.commits",
        )
        .bind(repo)
        .bind(&days)
        .bind(&counts)
        .execute(&mut *tx)
        .await
        .context("upsert commit days")?;
    }

    // Per-day TODO deltas
    let todo_rows: Vec<(&NaiveDate, &TodoAgg)> = agg.todo_days.iter().collect();
    for chunk in todo_rows.chunks(UPSERT_CHUNK) {
        let mut days: Vec<NaiveDate> = Vec::with_capacity(chunk.len());
        let mut added: Vec<i64> = Vec::with_capacity(chunk.len());
        let mut removed: Vec<i64> = Vec::with_capacity(chunk.len());
        for (day, t) in chunk {
            days.push(**day);
            added.push(t.added);
            removed.push(t.removed);
        }
        sqlx::query(
            "INSERT INTO repo_todo_deltas (repo, day, todo_added, todo_removed) \
             SELECT $1, d, a, r FROM UNNEST($2::date[], $3::bigint[], $4::bigint[]) AS t(d, a, r) \
             ON CONFLICT (repo, day) DO UPDATE SET \
                todo_added = repo_todo_deltas.todo_added + EXCLUDED.todo_added, \
                todo_removed = repo_todo_deltas.todo_removed + EXCLUDED.todo_removed",
        )
        .bind(repo)
        .bind(&days)
        .bind(&added)
        .bind(&removed)
        .execute(&mut *tx)
        .await
        .context("upsert todo deltas")?;
    }

    // Per-file aggregates
    let file_rows: Vec<(&String, &FileAgg)> = agg.files.iter().collect();
    for chunk in file_rows.chunks(UPSERT_CHUNK) {
        let mut paths: Vec<String> = Vec::with_capacity(chunk.len());
        let mut counts: Vec<i64> = Vec::with_capacity(chunk.len());
        let mut fixes: Vec<i64> = Vec::with_capacity(chunk.len());
        let mut modified: Vec<DateTime<Utc>> = Vec::with_capacity(chunk.len());
        for (path, f) in chunk {
            paths.push((*path).clone());
            counts.push(f.commits);
            fixes.push(f.fix_commits);
            modified.push(f.last_modified_at);
        }
        sqlx::query(
            "INSERT INTO repo_file_stats (repo, path, commits, fix_commits, last_modified_at) \
             SELECT $1, p, c, fx, m \
             FROM UNNEST($2::text[], $3::bigint[], $4::bigint[], $5::timestamptz[]) \
                  AS t(p, c, fx, m) \
             ON CONFLICT (repo, path) DO UPDATE SET \
                commits = repo_file_stats.commits + EXCLUDED.commits, \
                fix_commits = repo_file_stats.fix_commits + EXCLUDED.fix_commits, \
                last_modified_at = GREATEST(repo_file_stats.last_modified_at, EXCLUDED.last_modified_at)",
        )
        .bind(repo)
        .bind(&paths)
        .bind(&counts)
        .bind(&fixes)
        .bind(&modified)
        .execute(&mut *tx)
        .await
        .context("upsert file stats")?;
    }

    // Bump cumulative commit count + last_analyzed metadata on repo_history.
    if let Some(total) = exact_total {
        sqlx::query(
            "INSERT INTO repo_history (repo, last_analyzed_sha, last_analyzed_at, head_sha, total_commits) \
             VALUES ($1, $2, $3, $2, $4) \
             ON CONFLICT (repo) DO UPDATE SET \
                last_analyzed_sha = EXCLUDED.last_analyzed_sha, \
                last_analyzed_at = EXCLUDED.last_analyzed_at, \
                head_sha = EXCLUDED.head_sha, \
                total_commits = EXCLUDED.total_commits",
        )
        .bind(repo)
        .bind(analyzed_head_sha)
        .bind(now)
        .bind(total.max(0))
        .execute(&mut *tx)
        .await
        .context("replace repo_history")?;
    } else {
        let added = commits.len() as i64;
        sqlx::query(
            "INSERT INTO repo_history (repo, last_analyzed_sha, last_analyzed_at, head_sha, total_commits) \
             VALUES ($1, $2, $3, $2, $4) \
             ON CONFLICT (repo) DO UPDATE SET \
                last_analyzed_sha = EXCLUDED.last_analyzed_sha, \
                last_analyzed_at = EXCLUDED.last_analyzed_at, \
                head_sha = EXCLUDED.head_sha, \
                total_commits = repo_history.total_commits + $4",
        )
        .bind(repo)
        .bind(analyzed_head_sha)
        .bind(now)
        .bind(added)
        .execute(&mut *tx)
        .await
        .context("update repo_history")?;
    }

    if replace {
        // Authors the rebuilt window no longer contains were zeroed above and
        // never re-inserted; drop them so the contributor surfaces do not show
        // people with no commits in the analyzed window.
        sqlx::query("DELETE FROM repo_author_stats WHERE repo = $1 AND commits = 0")
            .bind(repo)
            .execute(&mut *tx)
            .await
            .context("drop authors outside the rebuilt window")?;
    }

    tx.commit().await.context("commit tx")?;
    Ok(())
}

/// Record that an analysis run confirmed the stored aggregates are already at
/// the repository's current head. Only `last_analyzed_at` moves: the cursor,
/// the head, and every aggregate are unchanged, and the row must already
/// exist (the run opened a clone for it). Without this stamp a repository
/// whose HEAD never moves stays permanently outside the freshness window and
/// is re-queued and re-fetched by every view.
pub async fn touch_analyzed_at(db: &Db, repo: &str) -> Result<()> {
    sqlx::query("UPDATE repo_history SET last_analyzed_at = NOW() WHERE repo = $1")
        .bind(repo)
        .execute(&db.pool)
        .await?;
    Ok(())
}

/// Mark a repo's clone evicted. Aggregates remain intact; only the
/// clone_path / clone_size_bytes fields are cleared.
pub async fn mark_evicted(db: &Db, repo: &str) -> Result<()> {
    sqlx::query(
        "UPDATE repo_history SET clone_path = NULL, clone_size_bytes = NULL WHERE repo = $1",
    )
    .bind(repo)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// Update clone_path + clone_size_bytes after a successful clone/fetch.
pub async fn record_clone(db: &Db, repo: &str, path: &std::path::Path, size: u64) -> Result<()> {
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO repo_history (repo, clone_path, clone_size_bytes, last_visited_at) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (repo) DO UPDATE SET \
            clone_path = EXCLUDED.clone_path, \
            clone_size_bytes = EXCLUDED.clone_size_bytes, \
            last_visited_at = EXCLUDED.last_visited_at",
    )
    .bind(repo)
    .bind(path.to_string_lossy().as_ref())
    .bind(size as i64)
    .bind(now)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// Persist the user-visible scope and wall time of the last successful run.
/// The aggregate/head transaction remains the correctness boundary; this is
/// operational metadata used for honest progress estimates and coverage copy.
pub async fn record_analysis_details(
    db: &Db,
    repo: &str,
    duration_ms: i64,
    scope_commits: usize,
    truncated: bool,
) -> Result<()> {
    sqlx::query(
        "UPDATE repo_history SET analysis_duration_ms = $1, analysis_scope_commits = $2, \
         analysis_truncated = $3, analysis_revision = $4 WHERE repo = $5",
    )
    .bind(duration_ms.max(0))
    .bind(i64::try_from(scope_commits).unwrap_or(i64::MAX))
    .bind(truncated)
    .bind(crate::repo_analysis::CURRENT_ANALYSIS_REVISION)
    .bind(repo)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// Eviction pass. Sorts repos by `score = bytes / max(1, days_idle)` and
/// removes biggest+stalest clones until we're under the high-watermark.
/// Run after every analysis (cheap when nothing's near full).
///
/// Replica safety: `repo_history` rows are global but clones live on ONE
/// replica's local volume, so both the quota accounting and the candidate
/// set consider only clones whose path exists on THIS replica's disk.
/// "Evicting" a path that is absent locally would trivially succeed and
/// then null out a row whose bytes still occupy another replica's volume,
/// orphaning them; and counting foreign rows against the local quota would
/// effectively divide the quota by the replica count.
pub async fn evict_to_quota(db: &Db, storage: &RepoStorage) -> Result<u64> {
    let target = storage.quota_bytes * (storage.high_watermark_pct as u64) / 100;
    // Cheap pre-check: the pass runs on the worker's critical path every N
    // completed jobs, and the common case is "well under quota". One aggregate
    // answers that without shipping a row per clone to the process or stat()ing
    // each of them.
    let recorded: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(clone_size_bytes), 0)::BIGINT FROM repo_history          WHERE clone_path IS NOT NULL AND clone_size_bytes IS NOT NULL",
    )
    .fetch_one(&db.pool)
    .await?;
    if (recorded.max(0) as u64) <= target {
        return Ok(0);
    }
    let rows = sqlx::query_as::<
        _,
        (
            String,
            Option<String>,
            Option<i64>,
            Option<chrono::DateTime<chrono::Utc>>,
        ),
    >(
        "SELECT repo, clone_path, clone_size_bytes, last_visited_at \
         FROM repo_history \
         WHERE clone_path IS NOT NULL AND clone_size_bytes IS NOT NULL \
         ORDER BY clone_size_bytes DESC NULLS LAST",
    )
    .fetch_all(&db.pool)
    .await?;

    // Only clones present on this replica's filesystem participate.
    let local: Vec<(String, std::path::PathBuf, u64, Option<DateTime<Utc>>)> = rows
        .into_iter()
        .filter_map(|(repo, path, bytes, last_visited)| {
            let path = path.map(std::path::PathBuf::from)?;
            let bytes = bytes? as u64;
            if !path.exists() {
                return None;
            }
            Some((repo, path, bytes, last_visited))
        })
        .collect();
    let mut used: u64 = local.iter().map(|(_, _, bytes, _)| *bytes).sum();
    if used <= target {
        return Ok(0);
    }

    // Score each candidate. Repos visited within MIN_AGE_HOURS are
    // protected — without that guard, a popular hot repo gets evicted,
    // re-cloned by the next request, evicts another similar-sized repo,
    // and we thrash the disk in a loop ("thundering herd of clones").
    // 24h matches the "weekly cron + occasional ad-hoc analysis" cadence
    // we expect for the analysis worker.
    const MIN_AGE_HOURS: i64 = 24;
    let now = Utc::now();
    let mut protected: Vec<(String, std::path::PathBuf, u64, Option<DateTime<Utc>>)> = Vec::new();
    let mut scored: Vec<(f64, String, std::path::PathBuf, u64)> = local
        .into_iter()
        .filter_map(|(repo, path, bytes, last_visited)| {
            // Recently visited clones go to the back of the queue rather than
            // out of it. Excluding them outright made the quota
            // unenforceable exactly under load: a busy pool touches every
            // clone it holds within the guard window, so the candidate set
            // emptied and the pass freed nothing while the disk kept growing.
            if let Some(visited) = last_visited
                && (now - visited).num_hours() < MIN_AGE_HOURS
            {
                protected.push((repo, path, bytes, Some(visited)));
                return None;
            }
            let idle_days = last_visited
                .map(|dt| (now - dt).num_days().max(1) as f64)
                .unwrap_or(1.0);
            let score = bytes as f64 * idle_days;
            Some((score, repo, path, bytes))
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut freed = 0u64;
    for (_score, repo, path, bytes) in scored {
        if used <= target {
            break;
        }
        if let Err(e) = evict_clone(&path).await {
            tracing::warn!(repo, error = %e, "evict failed; skipping");
            continue;
        }
        mark_evicted(db, &repo).await.ok();
        used = used.saturating_sub(bytes);
        freed = freed.saturating_add(bytes);
        tracing::info!(repo, bytes, "evicted bare clone");
    }

    if used > target && !protected.is_empty() {
        // The working set alone exceeds the quota. Fall back to plain LRU
        // over the protected clones: re-cloning one of them later is strictly
        // better than letting the volume fill.
        tracing::warn!(
            used,
            target,
            protected = protected.len(),
            "clone quota exceeded by the active working set; evicting least-recently-visited clones"
        );
        protected.sort_by_key(|(_, _, _, last_visited)| *last_visited);
        for (repo, path, bytes, _) in protected {
            if used <= target {
                break;
            }
            if let Err(e) = evict_clone(&path).await {
                tracing::warn!(repo, error = %e, "evict failed; skipping");
                continue;
            }
            mark_evicted(db, &repo).await.ok();
            used = used.saturating_sub(bytes);
            freed = freed.saturating_add(bytes);
            tracing::info!(
                repo,
                bytes,
                "evicted recently-visited bare clone under quota pressure"
            );
        }
    }
    Ok(freed)
}

/// Minimum time since a candidate directory's last modification before the
/// orphan sweep may delete it. `git clone` creates the target directory at
/// start but `record_clone` writes the referencing row only after the fetch
/// completes, so a fresh unreferenced directory is most likely an in-flight
/// clone rather than garbage.
const ORPHAN_MIN_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// Cadence of the background orphan sweep after the startup pass. Orphans
/// accumulate at cross-replica-eviction speed (slow), so hours are plenty.
const ORPHAN_SWEEP_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// Collect candidate bare-clone directories under `root`, matching the
/// storage layout `<root>/<owner>/<repo>.git` (plus `<root>/<x>.git` for
/// defense in depth). Symlinks are never followed: a link planted inside
/// the repos dir must not let the sweep reach — or count — anything else.
fn collect_bare_clone_dirs(root: &Path) -> Vec<PathBuf> {
    fn scan(dir: &Path, out: &mut Vec<PathBuf>, recurse: bool) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            // `DirEntry::file_type` does NOT follow symlinks, so a
            // symlinked "clone" is skipped entirely rather than resolved.
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let path = entry.path();
            let is_bare = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".git"));
            if is_bare {
                out.push(path);
            } else if recurse {
                // An owner directory: bare clones sit one level below.
                scan(&path, out, false);
            }
        }
    }
    let mut out = Vec::new();
    scan(root, &mut out, true);
    out
}

/// Orphaned-clone sweep: delete bare-clone directories on THIS replica's
/// disk that no `repo_history.clone_path` row references.
///
/// Why orphans exist at all: `RepoStorage::path_for` derives the clone path
/// purely from the slug, so with N worker replicas on separate volumes every
/// replica stores repo X at the SAME path string while `repo_history` holds
/// a single global row. When replica B evicts X it deletes its local copy
/// and NULLs that shared row — replica A's physical copy of X is then
/// referenced by nothing: [`evict_to_quota`] neither counts it toward the
/// quota nor ranks it for eviction, so A's disk fills unboundedly. This
/// sweep is the disk-driven complement to that row-driven eviction.
///
/// Safety rails:
///   * only `*.git` directories at the storage-layout depths are candidates,
///     and symlinks are never followed or deleted;
///   * every candidate is canonicalized and prefix-checked against the
///     canonicalized root, so the sweep cannot touch paths outside
///     `REPOS_DIR`;
///   * directories modified within [`ORPHAN_MIN_AGE`] are skipped — an
///     in-flight clone has a fresh directory but no DB row yet.
///
/// Returns the total bytes freed (per the [`clone_size_bytes`] estimate).
pub async fn sweep_orphaned_clones(db: &Db, storage: &RepoStorage) -> Result<u64> {
    let root = match tokio::fs::canonicalize(&storage.root).await {
        Ok(root) => root,
        // No repos dir yet (fresh replica) ⇒ nothing to sweep.
        Err(_) => return Ok(0),
    };

    let referenced_paths: Vec<String> =
        sqlx::query_scalar("SELECT clone_path FROM repo_history WHERE clone_path IS NOT NULL")
            .fetch_all(&db.pool)
            .await
            .context("load referenced clone paths")?;
    let mut referenced: HashSet<PathBuf> = HashSet::new();
    for raw in referenced_paths {
        let path = PathBuf::from(raw);
        // Keep the canonical form when the path resolves locally (mounted
        // volumes and tempdirs often reach one directory through symlinked
        // prefixes) and the raw form always, so rows written under either
        // spelling of REPOS_DIR still count as references.
        if let Ok(canonical) = tokio::fs::canonicalize(&path).await {
            referenced.insert(canonical);
        }
        referenced.insert(path);
    }

    let now = SystemTime::now();
    let mut freed = 0u64;
    for candidate in collect_bare_clone_dirs(&root) {
        // Canonicalize + prefix-check: the sweep must never delete a path
        // that resolves outside REPOS_DIR, whatever the tree looks like.
        let Ok(canonical) = tokio::fs::canonicalize(&candidate).await else {
            continue; // vanished mid-sweep
        };
        if canonical == root || !canonical.starts_with(&root) {
            tracing::warn!(
                path = %candidate.display(),
                "orphan sweep: candidate resolves outside REPOS_DIR; skipping"
            );
            continue;
        }
        if referenced.contains(&canonical) || referenced.contains(&candidate) {
            continue;
        }
        // In-flight-clone guard: a clone directory appears on disk before
        // its repo_history row does. Anything modified within the window
        // is presumed in progress and left for a later pass. A missing or
        // unreadable mtime counts as fresh (never delete on uncertainty).
        let Ok(meta) = tokio::fs::metadata(&canonical).await else {
            continue;
        };
        let age = meta
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .unwrap_or(Duration::ZERO);
        if age < ORPHAN_MIN_AGE {
            continue;
        }
        let bytes = clone_size_bytes(&canonical);
        if let Err(error) = evict_clone(&canonical).await {
            tracing::warn!(
                path = %canonical.display(),
                %error,
                "orphan sweep: delete failed; skipping"
            );
            continue;
        }
        freed = freed.saturating_add(bytes);
        tracing::info!(
            path = %canonical.display(),
            bytes,
            "orphan sweep: removed unreferenced bare clone"
        );
    }
    Ok(freed)
}

/// Spawn the recurring orphan sweep: one pass at startup, then every
/// [`ORPHAN_SWEEP_INTERVAL`]. Failures log and retry on the next tick; the
/// task holds no state beyond its handles, so any replica count is safe —
/// each replica only ever inspects and deletes its own local disk.
pub fn spawn_orphan_clone_sweep(db: Db, storage: std::sync::Arc<RepoStorage>) {
    tokio::spawn(async move {
        loop {
            match sweep_orphaned_clones(&db, &storage).await {
                Ok(0) => {}
                Ok(freed) => {
                    tracing::info!(freed_bytes = freed, "orphan clone sweep freed disk");
                }
                Err(error) => tracing::warn!(%error, "orphan clone sweep failed"),
            }
            tokio::time::sleep(ORPHAN_SWEEP_INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_600_000_000 + secs, 0).unwrap()
    }

    fn commit(
        email: &str,
        name: &str,
        secs: i64,
        is_fix: bool,
        paths: &[&str],
        todo_added: u32,
        todo_removed: u32,
    ) -> CommitInfo {
        let committed_at = at(secs);
        CommitInfo {
            sha: format!("sha{secs}"),
            author_email: email.to_string(),
            author_name: name.to_string(),
            committed_at,
            committed_day: committed_at.date_naive(),
            message_first_line: if is_fix {
                "fix: x".into()
            } else {
                "feat: x".into()
            },
            is_fix,
            paths_changed: paths.iter().map(|s| s.to_string()).collect(),
            todo_added,
            todo_removed,
        }
    }

    /// Reference reduction that mirrors the OLD row-by-row upsert exactly,
    /// in plain Rust (no DB). Applying each commit in order with the same
    /// COALESCE/LEAST/GREATEST/last-wins rules the SQL used. The batched
    /// `aggregate_commits` must produce identical maps.
    fn oracle(commits: &[CommitInfo]) -> BatchAggregates {
        let mut authors: HashMap<String, AuthorAgg> = HashMap::new();
        let mut author_commit_days: HashMap<(String, NaiveDate), i64> = HashMap::new();
        let mut files: HashMap<String, FileAgg> = HashMap::new();
        let mut commit_days: HashMap<NaiveDate, i64> = HashMap::new();
        let mut todo_days: HashMap<NaiveDate, TodoAgg> = HashMap::new();
        for c in commits {
            let avatar = avatar_for_email(&c.author_email);
            let login = github_login_for_email(&c.author_email);
            // Author: replicate INSERT ... ON CONFLICT row-by-row.
            match authors.get_mut(&c.author_email) {
                None => {
                    authors.insert(
                        c.author_email.clone(),
                        AuthorAgg {
                            name: c.author_name.clone(),
                            avatar: avatar.clone(),
                            login: login.clone(),
                            commits: 1,
                            first_commit_at: c.committed_at,
                            last_commit_at: c.committed_at,
                        },
                    );
                }
                Some(a) => {
                    a.name = c.author_name.clone(); // EXCLUDED.author_name
                    a.avatar = a.avatar.clone().or(avatar.clone()); // COALESCE(existing, EXCLUDED)
                    a.login = a.login.clone().or(login.clone());
                    a.commits += 1;
                    a.first_commit_at = a.first_commit_at.min(c.committed_at);
                    a.last_commit_at = a.last_commit_at.max(c.committed_at);
                }
            }
            *author_commit_days
                .entry((c.author_email.clone(), c.committed_day))
                .or_insert(0) += 1;
            *commit_days.entry(c.committed_day).or_insert(0) += 1;
            if c.todo_added > 0 || c.todo_removed > 0 {
                let t = todo_days.entry(c.committed_day).or_default();
                t.added += c.todo_added as i64;
                t.removed += c.todo_removed as i64;
            }
            for p in &c.paths_changed {
                let fix_inc = if c.is_fix { 1 } else { 0 };
                match files.get_mut(p) {
                    None => {
                        files.insert(
                            p.clone(),
                            FileAgg {
                                commits: 1,
                                fix_commits: fix_inc,
                                last_modified_at: c.committed_at,
                            },
                        );
                    }
                    Some(f) => {
                        f.commits += 1;
                        f.fix_commits += fix_inc;
                        f.last_modified_at = f.last_modified_at.max(c.committed_at);
                    }
                }
            }
        }
        BatchAggregates {
            authors,
            author_commit_days,
            files,
            commit_days,
            todo_days,
        }
    }

    fn assert_equiv(commits: &[CommitInfo]) {
        let a = aggregate_commits(commits);
        let b = oracle(commits);
        assert_eq!(a.authors, b.authors, "authors map");
        assert_eq!(
            a.author_commit_days, b.author_commit_days,
            "author_commit_days map"
        );
        assert_eq!(a.files, b.files, "files map");
        assert_eq!(a.commit_days, b.commit_days, "commit_days map");
        assert_eq!(a.todo_days, b.todo_days, "todo_days map");
    }

    #[test]
    fn aggregate_matches_oracle_basic() {
        // Same author across two days, two files, one fix commit.
        let commits = vec![
            commit("a@b.c", "Alice", 0, false, &["x.rs", "y.rs"], 1, 0),
            commit("a@b.c", "Alice Renamed", 90_000, true, &["x.rs"], 0, 2),
        ];
        assert_equiv(&commits);
        let agg = aggregate_commits(&commits);
        // x.rs: 2 commits, 1 fix; y.rs: 1 commit, 0 fix.
        assert_eq!(agg.files["x.rs"].commits, 2);
        assert_eq!(agg.files["x.rs"].fix_commits, 1);
        assert_eq!(agg.files["y.rs"].commits, 1);
        // Author name takes the LAST (newest) commit's value.
        assert_eq!(agg.authors["a@b.c"].name, "Alice Renamed");
        assert_eq!(agg.authors["a@b.c"].commits, 2);
        // last_modified on x.rs is the newer timestamp.
        assert_eq!(agg.files["x.rs"].last_modified_at, at(90_000));
    }

    #[test]
    fn aggregate_avatar_login_first_non_null_sticks() {
        // A plain email (gravatar avatar, no login) then a noreply email is
        // a DIFFERENT author key, so test first-non-null on a single author
        // by ordering: noreply first (gives login+avatar), plain-name
        // second can't change the key. Instead, exercise the COALESCE rule
        // directly: same email, but the helper derives the same avatar/login
        // each time, so first-non-null == that value. We assert it equals
        // the oracle which encodes the rule.
        let commits = vec![
            commit(
                "123+octo@users.noreply.github.com",
                "Octo",
                0,
                false,
                &["a"],
                0,
                0,
            ),
            commit(
                "123+octo@users.noreply.github.com",
                "Octo2",
                10,
                false,
                &["a"],
                0,
                0,
            ),
        ];
        assert_equiv(&commits);
        let agg = aggregate_commits(&commits);
        let a = &agg.authors["123+octo@users.noreply.github.com"];
        assert_eq!(a.login.as_deref(), Some("octo"));
        assert!(
            a.avatar
                .as_deref()
                .unwrap()
                .contains("avatars.githubusercontent.com")
        );
        assert_eq!(a.name, "Octo2", "name is last-wins");
    }

    #[test]
    fn aggregate_multi_author_and_todo_days() {
        let commits = vec![
            commit("a@b.c", "A", 0, false, &["f1"], 3, 1),
            commit("d@e.f", "D", 100, true, &["f1", "f2"], 0, 0),
            commit("a@b.c", "A", 86_500, false, &[], 2, 0), // next day, no paths
        ];
        assert_equiv(&commits);
        let agg = aggregate_commits(&commits);
        assert_eq!(agg.authors.len(), 2);
        assert_eq!(agg.authors["a@b.c"].commits, 2);
        assert_eq!(agg.authors["d@e.f"].commits, 1);
        let day0 = at(0).date_naive();
        let day1 = at(86_500).date_naive();
        assert_eq!(agg.author_commit_days[&("a@b.c".to_string(), day0)], 1);
        assert_eq!(agg.author_commit_days[&("a@b.c".to_string(), day1)], 1);
        // f1 touched by 2 commits (1 of them a fix).
        assert_eq!(agg.files["f1"].commits, 2);
        assert_eq!(agg.files["f1"].fix_commits, 1);
        // commit_days: day0 has 2 commits, day1 has 1.
        assert_eq!(agg.commit_days[&day0], 2);
        assert_eq!(agg.commit_days[&day1], 1);
        // todo_days: day0 sums added=3, removed=1; day1 added=2.
        assert_eq!(
            agg.todo_days[&day0],
            TodoAgg {
                added: 3,
                removed: 1
            }
        );
        assert_eq!(
            agg.todo_days[&day1],
            TodoAgg {
                added: 2,
                removed: 0
            }
        );
    }

    #[test]
    fn aggregate_skips_zero_todo_days() {
        // A commit with no TODO churn must NOT create a todo_days entry
        // (matches the old `if todo_added>0 || todo_removed>0` guard).
        let commits = vec![commit("a@b.c", "A", 0, false, &["f"], 0, 0)];
        let agg = aggregate_commits(&commits);
        assert!(agg.todo_days.is_empty());
        assert_eq!(agg.commit_days.len(), 1);
    }

    #[test]
    fn aggregate_empty_is_empty() {
        let agg = aggregate_commits(&[]);
        assert!(agg.authors.is_empty());
        assert!(agg.author_commit_days.is_empty());
        assert!(agg.files.is_empty());
        assert!(agg.commit_days.is_empty());
        assert!(agg.todo_days.is_empty());
    }

    #[test]
    fn collect_bare_clone_dirs_matches_layout_and_skips_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // Layout depth 2: <root>/<owner>/<repo>.git — a candidate.
        std::fs::create_dir_all(root.join("owner/repo.git")).unwrap();
        // Defense-in-depth depth 1: <root>/<x>.git — also a candidate.
        std::fs::create_dir_all(root.join("stray.git")).unwrap();
        // Non-.git dirs and plain files are never candidates.
        std::fs::create_dir_all(root.join("owner/not-a-clone")).unwrap();
        std::fs::write(root.join("owner/file.git"), b"not a dir").unwrap();
        // Anything below depth 2 is out of layout and ignored.
        std::fs::create_dir_all(root.join("owner/not-a-clone/deep.git")).unwrap();
        // A symlinked ".git" dir must be skipped, not followed.
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(outside.path().join("victim.git")).unwrap();
            std::os::unix::fs::symlink(
                outside.path().join("victim.git"),
                root.join("owner/escape.git"),
            )
            .unwrap();
        }

        let mut found = collect_bare_clone_dirs(root);
        found.sort();
        assert_eq!(
            found,
            vec![root.join("owner/repo.git"), root.join("stray.git")],
            "exactly the layout-shaped real directories are candidates"
        );
    }
}
