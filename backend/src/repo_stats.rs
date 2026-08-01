//! Persists `repo_history::CommitInfo` aggregates into Postgres. Updates
//! per-file, per-author, per-day rows; eviction-aware.
//!
//! Idempotency: each commit is identified by SHA via `last_analyzed_sha`
//! on `repo_history`. The walker only yields commits past that SHA, so
//! re-running this aggregator over the same range double-counts —
//! responsibility for "don't replay history" sits in `repo_history`.
//!
//! Memory: nothing here is proportional to the number of commits. Callers
//! walking a full history fold each batch into a [`CommitAggregator`] and
//! drop it; what survives is keyed by authors, files, days and file pairs,
//! all of which describe the repository's shape rather than its age.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
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
    lines_added: i64,
    lines_deleted: i64,
    binary_changes: i64,
    last_modified_at: DateTime<Utc>,
}

/// Per-day code movement collected from Git's `--numstat` output.
#[derive(Clone, Debug, Default, PartialEq)]
struct ChangeDayAgg {
    lines_added: i64,
    lines_deleted: i64,
    files_changed: i64,
    binary_files: i64,
    large_changes: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct CouplingAgg {
    cochanges: i64,
    fix_commits: i64,
}

/// One borrowed co-change row on its way to the chunked upsert.
type CouplingRow<'a> = (&'a (Arc<str>, Arc<str>), &'a CouplingAgg);

/// Per-day TODO delta for one batch.
#[derive(Clone, Debug, Default, PartialEq)]
struct TodoAgg {
    added: i64,
    removed: i64,
}

/// Fully-reduced aggregates for a run of commits. Computed in Rust so the
/// DB sees a handful of chunked multi-row upserts instead of ~O(commits ×
/// paths) single-row statements. The reduction reproduces the OLD per-row
/// upsert semantics exactly (see field docs on [`AuthorAgg`]); the SQL
/// `ON CONFLICT` clauses then fold these values into any pre-existing DB
/// rows with the same COALESCE / LEAST / GREATEST rules.
///
/// Every map here is keyed by something the repository *has* — an author, a
/// file, a day, a file pair — never by a commit. That is the whole point of
/// the type: its size tracks the repository's SHAPE, not the length of its
/// history, so a caller can stream a million commits through
/// [`CommitAggregator`] and hold only this.
pub struct Aggregates {
    authors: HashMap<String, AuthorAgg>,
    author_commit_days: HashMap<(String, NaiveDate), i64>,
    files: HashMap<Arc<str>, FileAgg>,
    commit_days: HashMap<NaiveDate, i64>,
    change_days: HashMap<NaiveDate, ChangeDayAgg>,
    couplings: HashMap<(Arc<str>, Arc<str>), CouplingAgg>,
    todo_days: HashMap<NaiveDate, TodoAgg>,
    commits_seen: usize,
}

impl Aggregates {
    /// How many commits were folded in. This is the value the non-exact
    /// `total_commits` upsert increments by, so it must survive the commits
    /// themselves being dropped.
    pub fn commits_seen(&self) -> usize {
        self.commits_seen
    }

    pub fn is_empty(&self) -> bool {
        self.commits_seen == 0
    }
}

const LARGE_CHANGE_LINES: u64 = 1_000;
const MAX_COUPLING_FILES_PER_COMMIT: usize = 12;
const MAX_STORED_COUPLINGS: i64 = 2_000;

/// In-memory ceiling on distinct co-change pairs held while walking.
///
/// The per-commit `MAX_COUPLING_FILES_PER_COMMIT` guard bounds how many pairs
/// ONE commit contributes (at most 66), but nothing bounded the accumulated
/// set: a repository with a million commits can reach tens of millions of
/// distinct pairs, and this is the only aggregate that grows combinatorially
/// rather than with the repository's file/author/day counts. Since the write
/// keeps just `MAX_STORED_COUPLINGS` rows, two orders of magnitude of
/// headroom over that is ample to decide the winners correctly while capping
/// this map in the low tens of MB.
const MAX_TRACKED_COUPLINGS: usize = 250_000;

/// Streaming reduction of commits into per-author / per-file / per-day
/// aggregates.
///
/// Feed commits in oldest-first order with [`push`](Self::push) or
/// [`extend`](Self::extend) and drop each batch as soon as it is folded in;
/// only [`Aggregates`] survives. Order-sensitive exactly where the OLD
/// row-by-row code was: author name takes the last (newest) commit's value,
/// while avatar/login keep the first non-null — matching the old
/// `EXCLUDED.author_name` / `COALESCE(existing, EXCLUDED)` upsert. Batch
/// boundaries are NOT observable: every decision below is made per commit,
/// so the same commits in the same order produce identical aggregates
/// whatever chunking the caller uses.
pub struct CommitAggregator {
    out: Aggregates,
    /// One `Arc<str>` per distinct path, shared by the `files` keys and by
    /// both halves of every coupling key. A path in a 12-file commit would
    /// otherwise be copied into up to 11 separate pair keys.
    paths: HashSet<Arc<str>>,
    /// Cochange count below which pairs are dropped by [`prune_couplings`].
    /// Rises monotonically so that pruning cannot oscillate.
    coupling_floor: i64,
    pruned_couplings: u64,
    wide_commits: u64,
    wide_commit_logged: bool,
}

impl Default for CommitAggregator {
    fn default() -> Self {
        Self::new()
    }
}

impl CommitAggregator {
    pub fn new() -> Self {
        Self {
            out: Aggregates {
                authors: HashMap::new(),
                author_commit_days: HashMap::new(),
                files: HashMap::new(),
                commit_days: HashMap::new(),
                change_days: HashMap::new(),
                couplings: HashMap::new(),
                todo_days: HashMap::new(),
                commits_seen: 0,
            },
            paths: HashSet::new(),
            coupling_floor: 0,
            pruned_couplings: 0,
            wide_commits: 0,
            wide_commit_logged: false,
        }
    }

    pub fn extend<'a, I>(&mut self, commits: I)
    where
        I: IntoIterator<Item = &'a CommitInfo>,
    {
        for commit in commits {
            self.push(commit);
        }
    }

    pub fn commits_seen(&self) -> usize {
        self.out.commits_seen
    }

    pub fn push(&mut self, commit: &CommitInfo) {
        self.out.commits_seen += 1;
        let committed_at = commit.committed_at;
        let day = commit.committed_day;

        self.push_author(commit, committed_at);

        *self
            .out
            .author_commit_days
            .entry((commit.author_email.clone(), day))
            .or_insert(0) += 1;
        *self.out.commit_days.entry(day).or_insert(0) += 1;

        let change_day = self.out.change_days.entry(day).or_default();
        change_day.lines_added = change_day
            .lines_added
            .saturating_add(i64::try_from(commit.lines_added).unwrap_or(i64::MAX));
        change_day.lines_deleted = change_day
            .lines_deleted
            .saturating_add(i64::try_from(commit.lines_deleted).unwrap_or(i64::MAX));
        change_day.files_changed = change_day
            .files_changed
            .saturating_add(i64::try_from(commit.paths_changed.len()).unwrap_or(i64::MAX));
        change_day.binary_files = change_day
            .binary_files
            .saturating_add(i64::from(commit.binary_files));
        if commit.lines_added.saturating_add(commit.lines_deleted) >= LARGE_CHANGE_LINES
            || commit.paths_changed.len() >= 50
        {
            change_day.large_changes += 1;
        }

        if commit.todo_added > 0 || commit.todo_removed > 0 {
            let t = self.out.todo_days.entry(day).or_default();
            t.added += commit.todo_added as i64;
            t.removed += commit.todo_removed as i64;
        }

        let fix_inc: i64 = if commit.is_fix { 1 } else { 0 };
        // Tests and older callers may provide only the compatibility path
        // list. Real analysis uses file_changes populated by --numstat.
        if commit.file_changes.is_empty() {
            for path in &commit.paths_changed {
                self.push_file(path, 0, 0, false, committed_at, fix_inc);
            }
        } else {
            for change in &commit.file_changes {
                self.push_file(
                    &change.path,
                    change.lines_added,
                    change.lines_deleted,
                    change.binary,
                    committed_at,
                    fix_inc,
                );
            }
        }

        self.push_couplings(commit, fix_inc);
    }

    /// Consume the accumulator. Takes `self` because the aggregates are the
    /// only thing worth keeping and the caller should not be able to keep
    /// pushing into a set it has already written.
    pub fn finish(self) -> Aggregates {
        if self.wide_commits > 0 || self.pruned_couplings > 0 {
            tracing::info!(
                commits = self.out.commits_seen,
                files = self.out.files.len(),
                authors = self.out.authors.len(),
                couplings = self.out.couplings.len(),
                wide_commits = self.wide_commits,
                pruned_couplings = self.pruned_couplings,
                coupling_floor = self.coupling_floor,
                "coupling evidence was bounded during aggregation"
            );
        }
        self.out
    }

    fn intern(&mut self, path: &str) -> Arc<str> {
        if let Some(existing) = self.paths.get(path) {
            return existing.clone();
        }
        let interned: Arc<str> = Arc::from(path);
        self.paths.insert(interned.clone());
        interned
    }

    fn push_author(&mut self, commit: &CommitInfo, committed_at: DateTime<Utc>) {
        // Derived lazily inside the closures: for a repository with a handful
        // of authors and a million commits these two helpers would otherwise
        // run an md5 and two allocations per commit for a value that is
        // discarded on every hit after the first.
        self.out
            .authors
            .entry(commit.author_email.clone())
            .and_modify(|a| {
                // name: last-wins (oldest-first ⇒ newest commit's name).
                a.name.clear();
                a.name.push_str(&commit.author_name);
                // avatar/login: first non-null sticks.
                if a.avatar.is_none() {
                    a.avatar = avatar_for_email(&commit.author_email);
                }
                if a.login.is_none() {
                    a.login = github_login_for_email(&commit.author_email);
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
                avatar: avatar_for_email(&commit.author_email),
                login: github_login_for_email(&commit.author_email),
                commits: 1,
                first_commit_at: committed_at,
                last_commit_at: committed_at,
            });
    }

    fn push_file(
        &mut self,
        path: &str,
        lines_added: u64,
        lines_deleted: u64,
        binary: bool,
        committed_at: DateTime<Utc>,
        fix_inc: i64,
    ) {
        if let Some(f) = self.out.files.get_mut(path) {
            f.commits += 1;
            f.fix_commits += fix_inc;
            f.lines_added = f
                .lines_added
                .saturating_add(i64::try_from(lines_added).unwrap_or(i64::MAX));
            f.lines_deleted = f
                .lines_deleted
                .saturating_add(i64::try_from(lines_deleted).unwrap_or(i64::MAX));
            f.binary_changes += i64::from(binary);
            if committed_at > f.last_modified_at {
                f.last_modified_at = committed_at;
            }
            return;
        }
        let key = self.intern(path);
        self.out.files.insert(
            key,
            FileAgg {
                commits: 1,
                fix_commits: fix_inc,
                lines_added: i64::try_from(lines_added).unwrap_or(i64::MAX),
                lines_deleted: i64::try_from(lines_deleted).unwrap_or(i64::MAX),
                binary_changes: i64::from(binary),
                last_modified_at: committed_at,
            },
        );
    }

    fn push_couplings(&mut self, commit: &CommitInfo, fix_inc: i64) {
        // Borrowed, sorted and deduped before anything is interned: a
        // tree-wide commit must cost one pointer vector, not N string
        // allocations, on its way to being skipped.
        let mut coupled: Vec<&str> = commit
            .paths_changed
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        coupled.sort_unstable();
        coupled.dedup();
        if coupled.len() < 2 {
            return;
        }
        if coupled.len() > MAX_COUPLING_FILES_PER_COMMIT {
            // A commit touching this many files is a rename sweep, a
            // reindent or a vendored-dependency bump. Every pair it would
            // contribute is an artifact of the mechanical edit rather than
            // evidence that the files are related, and there are O(n²) of
            // them, so the widest commits are also the ones that would
            // dominate the map.
            self.wide_commits += 1;
            if !self.wide_commit_logged {
                self.wide_commit_logged = true;
                tracing::info!(
                    sha = %commit.sha,
                    files = coupled.len(),
                    limit = MAX_COUPLING_FILES_PER_COMMIT,
                    "skipping file-coupling evidence from a tree-wide commit \
                     (logged once per analysis run)"
                );
            }
            return;
        }

        // Interned first so the O(n²) inner loop only ever clones `Arc`
        // handles: a hit on an existing pair costs a refcount bump instead of
        // two heap allocations, which at up to 66 pairs per commit over a
        // million commits is the difference that matters.
        let interned: Vec<Arc<str>> = coupled.iter().map(|path| self.intern(path)).collect();
        for left in 0..interned.len() - 1 {
            for right in left + 1..interned.len() {
                let coupling = self
                    .out
                    .couplings
                    .entry((interned[left].clone(), interned[right].clone()))
                    .or_default();
                coupling.cochanges += 1;
                coupling.fix_commits += fix_inc;
            }
        }

        // Checked per commit rather than per batch so the result cannot
        // depend on how the caller chunks its walk.
        if self.out.couplings.len() > MAX_TRACKED_COUPLINGS {
            self.prune_couplings();
        }
    }

    /// Drop the weakest co-change evidence until the map is back under half
    /// its ceiling. Weakest-first is the defensible order: only the top
    /// [`MAX_STORED_COUPLINGS`] pairs are ever written, and a pair seen once
    /// or twice in a repository that has already produced a quarter of a
    /// million distinct pairs cannot reach that set.
    fn prune_couplings(&mut self) {
        let before = self.out.couplings.len();
        while self.out.couplings.len() > MAX_TRACKED_COUPLINGS / 2 {
            // Terminates unconditionally: the floor rises every iteration and
            // every entry's cochange count is finite.
            self.coupling_floor += 1;
            let floor = self.coupling_floor;
            self.out.couplings.retain(|_, agg| agg.cochanges >= floor);
        }
        self.pruned_couplings += (before - self.out.couplings.len()) as u64;
    }
}

/// Batch form of [`CommitAggregator`], kept so existing callers and tests
/// read unchanged. Streaming the same commits through the accumulator
/// produces an identical [`Aggregates`].
fn aggregate_commits(commits: &[CommitInfo]) -> Aggregates {
    let mut aggregator = CommitAggregator::new();
    aggregator.extend(commits);
    aggregator.finish()
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
    write_aggregates_at_head(
        db,
        repo,
        &aggregate_commits(commits),
        analyzed_head_sha,
        false,
        None,
    )
    .await
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
    apply_aggregates_at_head_with_total(
        db,
        repo,
        &aggregate_commits(commits),
        analyzed_head_sha,
        reachable_commits,
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
    replace_aggregates_at_head(
        db,
        repo,
        &aggregate_commits(commits),
        analyzed_head_sha,
        reachable_commits,
    )
    .await
}

/// [`apply_commits_at_head_with_total`] for a caller that folded the walk
/// through a [`CommitAggregator`] instead of materializing it.
///
/// This is the shape a full-history walk should use: the commits themselves
/// are dropped batch by batch, and only the aggregates — bounded by the
/// repository's file/author/day counts rather than by its age — reach here.
/// The atomicity contract is unchanged and is the reason this still takes the
/// WHOLE run's aggregates in one call: see [`write_aggregates_in_tx`].
pub async fn apply_aggregates_at_head_with_total(
    db: &Db,
    repo: &str,
    aggregates: &Aggregates,
    analyzed_head_sha: &str,
    reachable_commits: usize,
) -> Result<()> {
    write_aggregates_at_head(
        db,
        repo,
        aggregates,
        analyzed_head_sha,
        false,
        Some(i64::try_from(reachable_commits).unwrap_or(i64::MAX)),
    )
    .await
}

/// [`replace_commits_at_head`] for a streamed walk.
pub async fn replace_aggregates_at_head(
    db: &Db,
    repo: &str,
    aggregates: &Aggregates,
    analyzed_head_sha: &str,
    reachable_commits: usize,
) -> Result<()> {
    write_aggregates_at_head(
        db,
        repo,
        aggregates,
        analyzed_head_sha,
        true,
        Some(i64::try_from(reachable_commits).unwrap_or(i64::MAX)),
    )
    .await
}

async fn write_aggregates_at_head(
    db: &Db,
    repo: &str,
    aggregates: &Aggregates,
    analyzed_head_sha: &str,
    replace: bool,
    exact_total: Option<i64>,
) -> Result<()> {
    let mut tx = db.pool.begin().await.context("begin tx")?;
    write_aggregates_in_tx(
        &mut tx,
        repo,
        aggregates,
        analyzed_head_sha,
        replace,
        exact_total,
    )
    .await?;
    tx.commit().await.context("commit tx")?;
    Ok(())
}

/// Every aggregate table AND the `last_analyzed_sha` cursor, written inside
/// ONE caller-supplied transaction.
///
/// The upserts below accumulate (`existing + EXCLUDED`, `commits + EXCLUDED.commits`),
/// so they are *not* idempotent on their own: replaying a range adds it a
/// second time and no constraint notices. What makes an incremental run safe
/// is that the cursor advances in the same transaction as the delta it
/// summarizes. A crash or error anywhere in here rolls back both, so the
/// retry re-derives the identical commit range from the identical cursor and
/// applies it exactly once. Two transactions would make a crash between them
/// either double-count the range forever or skip it forever, with nothing
/// able to tell afterwards which happened.
///
/// Taking the transaction as a parameter (rather than opening one here) is
/// also what lets `rollback_leaves_aggregates_and_cursor_untouched` assert
/// that property instead of assuming it.
///
/// This is also why a streaming caller must still write ONE `Aggregates` per
/// run rather than flushing each walked batch: a per-batch write would have
/// to advance the cursor per batch to stay crash-safe, and a cursor that
/// advances mid-walk publishes a repository whose aggregates cover only part
/// of the range a reader is being told is analyzed.
async fn write_aggregates_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    repo: &str,
    agg: &Aggregates,
    analyzed_head_sha: &str,
    replace: bool,
    exact_total: Option<i64>,
) -> Result<()> {
    let now = Utc::now();

    if replace {
        for table in [
            "repo_author_commit_days",
            "repo_commit_days",
            "repo_todo_deltas",
            "repo_file_stats",
            "repo_file_couplings",
        ] {
            let sql = format!("DELETE FROM {table} WHERE repo = $1");
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(repo)
                .execute(&mut **tx)
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
        .execute(&mut **tx)
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
        .execute(&mut **tx)
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
        .execute(&mut **tx)
        .await
        .context("upsert author commit days")?;
    }

    // Per-day commit buckets
    let day_rows: Vec<(&NaiveDate, &i64)> = agg.commit_days.iter().collect();
    for chunk in day_rows.chunks(UPSERT_CHUNK) {
        let mut days: Vec<NaiveDate> = Vec::with_capacity(chunk.len());
        let mut counts: Vec<i64> = Vec::with_capacity(chunk.len());
        let mut lines_added: Vec<i64> = Vec::with_capacity(chunk.len());
        let mut lines_deleted: Vec<i64> = Vec::with_capacity(chunk.len());
        let mut files_changed: Vec<i64> = Vec::with_capacity(chunk.len());
        let mut binary_files: Vec<i64> = Vec::with_capacity(chunk.len());
        let mut large_changes: Vec<i64> = Vec::with_capacity(chunk.len());
        for (day, c) in chunk {
            days.push(**day);
            counts.push(**c);
            let changes = agg.change_days.get(day).cloned().unwrap_or_default();
            lines_added.push(changes.lines_added);
            lines_deleted.push(changes.lines_deleted);
            files_changed.push(changes.files_changed);
            binary_files.push(changes.binary_files);
            large_changes.push(changes.large_changes);
        }
        sqlx::query(
            "INSERT INTO repo_commit_days \
                (repo, day, commits, lines_added, lines_deleted, files_changed, binary_files, large_changes) \
             SELECT $1, d, c, a, x, f, b, l \
             FROM UNNEST($2::date[], $3::bigint[], $4::bigint[], $5::bigint[], \
                         $6::bigint[], $7::bigint[], $8::bigint[]) AS t(d, c, a, x, f, b, l) \
             ON CONFLICT (repo, day) DO UPDATE SET \
                commits = repo_commit_days.commits + EXCLUDED.commits, \
                lines_added = repo_commit_days.lines_added + EXCLUDED.lines_added, \
                lines_deleted = repo_commit_days.lines_deleted + EXCLUDED.lines_deleted, \
                files_changed = repo_commit_days.files_changed + EXCLUDED.files_changed, \
                binary_files = repo_commit_days.binary_files + EXCLUDED.binary_files, \
                large_changes = repo_commit_days.large_changes + EXCLUDED.large_changes",
        )
        .bind(repo)
        .bind(&days)
        .bind(&counts)
        .bind(&lines_added)
        .bind(&lines_deleted)
        .bind(&files_changed)
        .bind(&binary_files)
        .bind(&large_changes)
        .execute(&mut **tx)
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
        .execute(&mut **tx)
        .await
        .context("upsert todo deltas")?;
    }

    // Per-file aggregates
    let file_rows: Vec<(&Arc<str>, &FileAgg)> = agg.files.iter().collect();
    for chunk in file_rows.chunks(UPSERT_CHUNK) {
        let mut paths: Vec<String> = Vec::with_capacity(chunk.len());
        let mut counts: Vec<i64> = Vec::with_capacity(chunk.len());
        let mut fixes: Vec<i64> = Vec::with_capacity(chunk.len());
        let mut lines_added: Vec<i64> = Vec::with_capacity(chunk.len());
        let mut lines_deleted: Vec<i64> = Vec::with_capacity(chunk.len());
        let mut binary_changes: Vec<i64> = Vec::with_capacity(chunk.len());
        let mut modified: Vec<DateTime<Utc>> = Vec::with_capacity(chunk.len());
        for (path, f) in chunk {
            paths.push(path.to_string());
            counts.push(f.commits);
            fixes.push(f.fix_commits);
            lines_added.push(f.lines_added);
            lines_deleted.push(f.lines_deleted);
            binary_changes.push(f.binary_changes);
            modified.push(f.last_modified_at);
        }
        sqlx::query(
            "INSERT INTO repo_file_stats \
                (repo, path, commits, fix_commits, lines_added, lines_deleted, binary_changes, last_modified_at) \
             SELECT $1, p, c, fx, a, d, b, m \
             FROM UNNEST($2::text[], $3::bigint[], $4::bigint[], $5::bigint[], \
                         $6::bigint[], $7::bigint[], $8::timestamptz[]) \
                  AS t(p, c, fx, a, d, b, m) \
             ON CONFLICT (repo, path) DO UPDATE SET \
                commits = repo_file_stats.commits + EXCLUDED.commits, \
                fix_commits = repo_file_stats.fix_commits + EXCLUDED.fix_commits, \
                lines_added = repo_file_stats.lines_added + EXCLUDED.lines_added, \
                lines_deleted = repo_file_stats.lines_deleted + EXCLUDED.lines_deleted, \
                binary_changes = repo_file_stats.binary_changes + EXCLUDED.binary_changes, \
                last_modified_at = GREATEST(repo_file_stats.last_modified_at, EXCLUDED.last_modified_at)",
        )
        .bind(repo)
        .bind(&paths)
        .bind(&counts)
        .bind(&fixes)
        .bind(&lines_added)
        .bind(&lines_deleted)
        .bind(&binary_changes)
        .bind(&modified)
        .execute(&mut **tx)
        .await
        .context("upsert file stats")?;
    }

    // Strongest file-coupling relationships: each count is the number of
    // bounded commits in which the two files changed together.
    let coupling_rows: Vec<CouplingRow<'_>> = agg.couplings.iter().collect();
    for chunk in coupling_rows.chunks(UPSERT_CHUNK) {
        let mut paths_a = Vec::with_capacity(chunk.len());
        let mut paths_b = Vec::with_capacity(chunk.len());
        let mut cochanges = Vec::with_capacity(chunk.len());
        let mut fix_commits = Vec::with_capacity(chunk.len());
        for ((path_a, path_b), coupling) in chunk {
            paths_a.push(path_a.to_string());
            paths_b.push(path_b.to_string());
            cochanges.push(coupling.cochanges);
            fix_commits.push(coupling.fix_commits);
        }
        sqlx::query(
            "INSERT INTO repo_file_couplings (repo, path_a, path_b, cochanges, fix_commits) \
             SELECT $1, a, b, c, f \
             FROM UNNEST($2::text[], $3::text[], $4::bigint[], $5::bigint[]) AS t(a, b, c, f) \
             ON CONFLICT (repo, path_a, path_b) DO UPDATE SET \
                cochanges = repo_file_couplings.cochanges + EXCLUDED.cochanges, \
                fix_commits = repo_file_couplings.fix_commits + EXCLUDED.fix_commits",
        )
        .bind(repo)
        .bind(&paths_a)
        .bind(&paths_b)
        .bind(&cochanges)
        .bind(&fix_commits)
        .execute(&mut **tx)
        .await
        .context("upsert file couplings")?;
    }
    sqlx::query(
        "DELETE FROM repo_file_couplings \
         WHERE repo = $1 AND (path_a, path_b) IN ( \
             SELECT path_a, path_b FROM repo_file_couplings \
             WHERE repo = $1 \
             ORDER BY cochanges DESC, fix_commits DESC, path_a, path_b \
             OFFSET $2 \
         )",
    )
    .bind(repo)
    .bind(MAX_STORED_COUPLINGS)
    .execute(&mut **tx)
    .await
    .context("bound stored file couplings")?;

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
        .execute(&mut **tx)
        .await
        .context("replace repo_history")?;
    } else {
        let added = i64::try_from(agg.commits_seen()).unwrap_or(i64::MAX);
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
        .execute(&mut **tx)
        .await
        .context("update repo_history")?;
    }

    if replace {
        // Authors the rebuilt window no longer contains were zeroed above and
        // never re-inserted; drop them so the contributor surfaces do not show
        // people with no commits in the analyzed window.
        sqlx::query("DELETE FROM repo_author_stats WHERE repo = $1 AND commits = 0")
            .bind(repo)
            .execute(&mut **tx)
            .await
            .context("drop authors outside the rebuilt window")?;
    }

    Ok(())
}

/// Record that an analysis run confirmed the stored aggregates are already at
/// the repository's current head. The cursor, the head, and every aggregate
/// are unchanged, and the row must already exist (the run opened a clone for
/// it). Without this stamp a repository whose HEAD never moves stays
/// permanently outside the freshness window and is re-queued and re-fetched
/// by every view.
///
/// `analysis_duration_ms` is cleared rather than restamped. It means "how long
/// the last run that actually walked commits took", and it feeds both the
/// per-repo progress ETA and the fleet sample that `last_analyzed_at DESC`
/// orders. Keeping the previous full run's number would re-promote a stale
/// twenty-minute measurement to the front of that sample on every no-op, while
/// writing this run's own wall time would be worse still: a head-confirmation
/// is by construction the shortest possible run and the newest row, so on a
/// warm corpus it would drag the fleet median down to a couple of seconds and
/// promise every queued repository a wait it cannot meet. NULL drops the row
/// out of the sample (and out of `idx_repo_history_duration_recent`) and makes
/// this repository's own estimate honestly unknown until it is walked again.
pub async fn touch_analyzed_at(db: &Db, repo: &str) -> Result<()> {
    sqlx::query(
        "UPDATE repo_history SET last_analyzed_at = NOW(), analysis_duration_ms = NULL \
         WHERE repo = $1",
    )
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

/// One local bare clone an eviction pass may consider.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CloneEntry {
    repo: String,
    path: PathBuf,
    bytes: u64,
    last_visited: Option<DateTime<Utc>>,
}

/// An ordered eviction plan: work `scored` first, then `protected` as a last
/// resort. Clones an analysis is currently walking appear in NEITHER list at
/// any watermark, so executing a whole plan can only ever fail to free enough
/// space — it can never delete a tree out from under a running walk.
#[derive(Debug, Default, PartialEq, Eq)]
struct EvictionPlan {
    /// Idle clones, biggest × stalest first.
    scored: Vec<CloneEntry>,
    /// Recently-visited clones, least-recently-visited first.
    protected: Vec<CloneEntry>,
    /// Bytes held by in-flight clones. Counted against the quota (they do
    /// occupy the disk) but never offered as victims.
    in_flight_bytes: u64,
    in_flight_clones: usize,
}

/// How stale an `in_progress` lease may be before its clone stops counting as
/// in flight.
///
/// A live run heartbeats its lease every 30s and the claim path steals a job
/// whose lease is over two minutes old, so any healthy analysis sits far
/// inside this window. The grace is deliberately much wider than the steal
/// window because the two errors are not symmetric: protecting a clone whose
/// worker actually died costs one delayed eviction, while deleting a clone
/// whose worker is merely starved of database round-trips costs a full
/// re-clone — of a repository that is, by construction, one this replica is
/// spending hours on.
const IN_FLIGHT_LEASE_GRACE: chrono::TimeDelta = chrono::TimeDelta::minutes(15);

/// Repositories whose analysis is in flight anywhere in the fleet.
///
/// The durable `repo_analysis_queue` rows are the source of truth here, not a
/// process-local registry of this replica's own runs. Two reasons the local
/// view is insufficient:
///
///   * Worker replicas can share one repos volume (`RepoStorage::path_for`
///     derives the path from the slug alone, so every replica addresses repo X
///     at the same path). On such a deployment a neighbour's in-flight clone is
///     an ordinary local directory that this pass would happily delete, and no
///     amount of local bookkeeping can see the neighbour's claim.
///   * `mark_evicted` NULLs a globally shared `repo_history` row, so even on
///     separate volumes a local-only decision desynchronizes another replica's
///     accounting for a repository it is actively working on.
///
/// The queue rows also already cover this replica's own runs: `claim_one`
/// writes `in_progress` before the clone is opened and `complete`/`fail`
/// clears it only after the last aggregate transaction has committed, so the
/// durable set is a superset of the local one for the whole window that
/// matters.
async fn in_flight_repos(db: &Db) -> Result<HashSet<String>> {
    let repos: Vec<String> = sqlx::query_scalar(
        "SELECT repo FROM repo_analysis_queue \
         WHERE status = 'in_progress' \
           AND (claimed_at IS NULL OR claimed_at >= $1)",
    )
    .bind(Utc::now() - IN_FLIGHT_LEASE_GRACE)
    .fetch_all(&db.pool)
    .await
    .context("load in-flight analysis claims")?;
    Ok(repos.into_iter().collect())
}

/// Anti-thrash window: a clone visited this recently is only evicted as a last
/// resort. Without the guard a popular hot repo gets evicted, re-cloned by the
/// next request, evicts another similar-sized repo, and the disk thrashes in a
/// loop ("thundering herd of clones"). 24h matches the "weekly cron plus
/// occasional ad-hoc analysis" cadence expected of the analysis worker.
const MIN_AGE_HOURS: i64 = 24;

/// Pure ordering half of [`evict_to_quota`]: partition local clones into the
/// in-flight set (never evicted), the scored pass, and the last-resort pass.
/// Kept pure so the "an analysis in flight is never a victim" invariant is
/// unit-testable without a database or a filesystem.
fn plan_evictions(
    local: Vec<CloneEntry>,
    in_flight: &HashSet<String>,
    now: DateTime<Utc>,
) -> EvictionPlan {
    let mut plan = EvictionPlan::default();
    let mut scored: Vec<(f64, CloneEntry)> = Vec::new();
    for entry in local {
        // Checked before anything else, and before the watermark is even
        // consulted: `record_clone` stamps `last_visited_at` at the START of a
        // run, so a clone being walked right now is otherwise the freshest —
        // and, for the repositories that make quota pressure happen at all,
        // the biggest — thing on the disk. Deleting it makes the run fail,
        // retry, re-clone the same gigabytes and evict the next victim, which
        // is a livelock rather than a cache policy.
        if in_flight.contains(&entry.repo) {
            plan.in_flight_bytes = plan.in_flight_bytes.saturating_add(entry.bytes);
            plan.in_flight_clones += 1;
            continue;
        }
        // Recently visited clones go to the back of the queue rather than out
        // of it. Excluding them outright made the quota unenforceable exactly
        // under load: a busy pool touches every clone it holds within the
        // guard window, so the candidate set emptied and the pass freed
        // nothing while the disk kept growing.
        if let Some(visited) = entry.last_visited
            && (now - visited).num_hours() < MIN_AGE_HOURS
        {
            plan.protected.push(entry);
            continue;
        }
        let idle_days = entry
            .last_visited
            .map(|dt| (now - dt).num_days().max(1) as f64)
            .unwrap_or(1.0);
        scored.push((entry.bytes as f64 * idle_days, entry));
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    plan.scored = scored.into_iter().map(|(_score, entry)| entry).collect();
    plan.protected
        .sort_by_key(|entry| (entry.last_visited, entry.repo.clone()));
    plan
}

/// Delete one clone and clear its row, returning the bytes actually freed.
async fn evict_entry(db: &Db, entry: &CloneEntry, reason: &'static str) -> u64 {
    if let Err(error) = evict_clone(&entry.path).await {
        tracing::warn!(repo = %entry.repo, %error, "evict failed; skipping");
        return 0;
    }
    mark_evicted(db, &entry.repo).await.ok();
    tracing::info!(repo = %entry.repo, bytes = entry.bytes, reason, "evicted bare clone");
    entry.bytes
}

/// Eviction pass. Sorts repos by `score = bytes × days_idle` and removes the
/// biggest+stalest clones until we're under the high-watermark. Run after
/// every analysis (cheap when nothing's near full).
///
/// In-flight safety: clones claimed by a live analysis anywhere in the fleet
/// are excluded from both passes (see [`in_flight_repos`]). They still count
/// toward `used`, so the pass keeps trying to free space elsewhere; it simply
/// cannot pick them. If that leaves the volume over its watermark the pass
/// says so at ERROR rather than returning quietly — a full disk that silently
/// stops all analysis is worse than a slow one, and the operator response
/// (more disk, a bigger quota, or less analysis concurrency) needs a signal.
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
    let local: Vec<CloneEntry> = rows
        .into_iter()
        .filter_map(|(repo, path, bytes, last_visited)| {
            let path = PathBuf::from(path?);
            let bytes = bytes? as u64;
            if !path.exists() {
                return None;
            }
            Some(CloneEntry {
                repo,
                path,
                bytes,
                last_visited,
            })
        })
        .collect();
    let mut used: u64 = local.iter().map(|entry| entry.bytes).sum();
    if used <= target {
        return Ok(0);
    }

    // Read BEFORE the first delete, and propagated rather than defaulted to
    // "nothing is running": a pass that cannot tell which clones are in use
    // must free nothing at all. Skipping this sweep costs disk headroom until
    // the next completed job calls it again; guessing costs a live analysis.
    let in_flight = in_flight_repos(db).await?;
    let plan = plan_evictions(local, &in_flight, Utc::now());

    let mut freed = 0u64;
    for entry in &plan.scored {
        if used <= target {
            break;
        }
        let bytes = evict_entry(db, entry, "idle").await;
        used = used.saturating_sub(bytes);
        freed = freed.saturating_add(bytes);
    }

    if used > target && !plan.protected.is_empty() {
        // The working set alone exceeds the quota. Fall back to plain LRU
        // over the protected clones: re-cloning one of them later is strictly
        // better than letting the volume fill.
        tracing::warn!(
            used,
            target,
            protected = plan.protected.len(),
            "clone quota exceeded by the active working set; evicting least-recently-visited clones"
        );
        for entry in &plan.protected {
            if used <= target {
                break;
            }
            let bytes = evict_entry(db, entry, "recently-visited-under-pressure").await;
            used = used.saturating_sub(bytes);
            freed = freed.saturating_add(bytes);
        }
    }

    if used > target {
        // Actionable on purpose: the numbers below say whether the volume is
        // held by running analyses (raise the quota or lower analysis
        // concurrency) or by clones whose deletion failed (a disk or
        // permissions fault). Silence here means analysis stops fleet-wide the
        // moment the volume actually fills, with no prior warning.
        tracing::error!(
            used,
            target,
            quota_bytes = storage.quota_bytes,
            freed,
            in_flight_clones = plan.in_flight_clones,
            in_flight_bytes = plan.in_flight_bytes,
            "clone quota still exceeded after a full eviction pass; the remaining clones are \
             being analyzed right now or could not be deleted"
        );
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
            file_changes: paths
                .iter()
                .map(|path| crate::repo_history::FileChange {
                    path: (*path).to_string(),
                    lines_added: 3,
                    lines_deleted: 1,
                    binary: false,
                })
                .collect(),
            lines_added: 3 * paths.len() as u64,
            lines_deleted: paths.len() as u64,
            binary_files: 0,
            todo_added,
            todo_removed,
        }
    }

    /// Reference reduction that mirrors the OLD row-by-row upsert exactly,
    /// in plain Rust (no DB). Applying each commit in order with the same
    /// COALESCE/LEAST/GREATEST/last-wins rules the SQL used. The batched
    /// `aggregate_commits` must produce identical maps.
    fn oracle(commits: &[CommitInfo]) -> Aggregates {
        let mut authors: HashMap<String, AuthorAgg> = HashMap::new();
        let mut author_commit_days: HashMap<(String, NaiveDate), i64> = HashMap::new();
        let mut files: HashMap<Arc<str>, FileAgg> = HashMap::new();
        let mut commit_days: HashMap<NaiveDate, i64> = HashMap::new();
        let mut change_days: HashMap<NaiveDate, ChangeDayAgg> = HashMap::new();
        let mut couplings: HashMap<(Arc<str>, Arc<str>), CouplingAgg> = HashMap::new();
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
            let change_day = change_days.entry(c.committed_day).or_default();
            change_day.lines_added += i64::try_from(c.lines_added).unwrap_or(i64::MAX);
            change_day.lines_deleted += i64::try_from(c.lines_deleted).unwrap_or(i64::MAX);
            change_day.files_changed += i64::try_from(c.paths_changed.len()).unwrap_or(i64::MAX);
            change_day.binary_files += i64::from(c.binary_files);
            if c.lines_added.saturating_add(c.lines_deleted) >= LARGE_CHANGE_LINES
                || c.paths_changed.len() >= 50
            {
                change_day.large_changes += 1;
            }
            if c.todo_added > 0 || c.todo_removed > 0 {
                let t = todo_days.entry(c.committed_day).or_default();
                t.added += c.todo_added as i64;
                t.removed += c.todo_removed as i64;
            }
            for change in &c.file_changes {
                let p: Arc<str> = Arc::from(change.path.as_str());
                let fix_inc = if c.is_fix { 1 } else { 0 };
                match files.get_mut(&p) {
                    None => {
                        files.insert(
                            p.clone(),
                            FileAgg {
                                commits: 1,
                                fix_commits: fix_inc,
                                lines_added: change.lines_added as i64,
                                lines_deleted: change.lines_deleted as i64,
                                binary_changes: i64::from(change.binary),
                                last_modified_at: c.committed_at,
                            },
                        );
                    }
                    Some(f) => {
                        f.commits += 1;
                        f.fix_commits += fix_inc;
                        f.lines_added += change.lines_added as i64;
                        f.lines_deleted += change.lines_deleted as i64;
                        f.binary_changes += i64::from(change.binary);
                        f.last_modified_at = f.last_modified_at.max(c.committed_at);
                    }
                }
            }
            let mut paths: Vec<Arc<str>> = c
                .paths_changed
                .iter()
                .map(|path| Arc::from(path.as_str()))
                .collect();
            paths.sort_unstable();
            paths.dedup();
            if (2..=MAX_COUPLING_FILES_PER_COMMIT).contains(&paths.len()) {
                for left in 0..paths.len() - 1 {
                    for right in left + 1..paths.len() {
                        let coupling = couplings
                            .entry((paths[left].clone(), paths[right].clone()))
                            .or_default();
                        coupling.cochanges += 1;
                        coupling.fix_commits += i64::from(c.is_fix);
                    }
                }
            }
        }
        Aggregates {
            authors,
            author_commit_days,
            files,
            commit_days,
            change_days,
            couplings,
            todo_days,
            commits_seen: commits.len(),
        }
    }

    fn pair(a: &str, b: &str) -> (Arc<str>, Arc<str>) {
        (Arc::from(a), Arc::from(b))
    }

    fn assert_same_aggregates(a: &Aggregates, b: &Aggregates, what: &str) {
        assert_eq!(a.authors, b.authors, "{what}: authors map");
        assert_eq!(
            a.author_commit_days, b.author_commit_days,
            "{what}: author_commit_days map"
        );
        assert_eq!(a.files, b.files, "{what}: files map");
        assert_eq!(a.commit_days, b.commit_days, "{what}: commit_days map");
        assert_eq!(a.change_days, b.change_days, "{what}: change_days map");
        assert_eq!(a.couplings, b.couplings, "{what}: couplings map");
        assert_eq!(a.todo_days, b.todo_days, "{what}: todo_days map");
        assert_eq!(a.commits_seen, b.commits_seen, "{what}: commits_seen");
    }

    fn assert_equiv(commits: &[CommitInfo]) {
        assert_same_aggregates(&aggregate_commits(commits), &oracle(commits), "batch");
        // Every batching of the same commits must land on the same
        // aggregates, because the streaming caller chooses the chunk size.
        for chunk in [1usize, 2, 3, 7, 1024] {
            let mut streamed = CommitAggregator::new();
            for batch in commits.chunks(chunk) {
                streamed.extend(batch);
            }
            assert_same_aggregates(
                &streamed.finish(),
                &oracle(commits),
                &format!("streamed in chunks of {chunk}"),
            );
        }
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
        assert_eq!(agg.files["x.rs"].lines_added, 6);
        assert_eq!(agg.files["x.rs"].lines_deleted, 2);
        assert_eq!(agg.files["y.rs"].commits, 1);
        assert_eq!(
            agg.couplings[&pair("x.rs", "y.rs")],
            CouplingAgg {
                cochanges: 1,
                fix_commits: 0,
            }
        );
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
        assert!(agg.change_days.is_empty());
        assert!(agg.couplings.is_empty());
        assert!(agg.todo_days.is_empty());
        assert!(agg.is_empty());
        assert_eq!(agg.commits_seen(), 0);
    }

    /// The property the whole streaming restructure rests on: a caller that
    /// folds commits in batch by batch and drops each batch must land on
    /// exactly the aggregates the all-at-once function produced, for every
    /// batch size — otherwise "hold the aggregates, not the history" would be
    /// a behaviour change rather than a memory fix.
    #[test]
    fn streaming_matches_the_batch_function_over_a_long_history() {
        let names = ["Alice", "Bob", "Carol"];
        let emails = [
            "a@b.c",
            "7+bob@users.noreply.github.com",
            "carol@example.org",
        ];
        let paths = ["src/a.rs", "src/b.rs", "src/c.rs", "docs/d.md", "README"];
        let commits: Vec<CommitInfo> = (0..500)
            .map(|i| {
                let touched: Vec<&str> = (0..=(i % 4))
                    .map(|offset| paths[(i + offset) as usize % paths.len()])
                    .collect();
                commit(
                    emails[i as usize % emails.len()],
                    names[i as usize % names.len()],
                    i * 3_600,
                    i % 5 == 0,
                    &touched,
                    (i % 3) as u32,
                    (i % 7) as u32,
                )
            })
            .collect();

        assert_equiv(&commits);
        let batched = aggregate_commits(&commits);
        assert_eq!(batched.commits_seen(), 500);
        assert!(!batched.couplings.is_empty(), "fixture exercises couplings");
        assert!(
            batched.authors.len() == 3,
            "fixture exercises author merges"
        );
    }

    /// A tree-wide commit contributes O(files²) pairs that are artifacts of a
    /// mechanical edit. It must contribute none of them, and the aggregator
    /// must say so.
    #[test]
    fn wide_commits_contribute_no_coupling_evidence() {
        let wide: Vec<String> = (0..MAX_COUPLING_FILES_PER_COMMIT + 1)
            .map(|i| format!("f{i}"))
            .collect();
        let wide_refs: Vec<&str> = wide.iter().map(String::as_str).collect();
        let commits = vec![
            commit("a@b.c", "A", 0, false, &wide_refs, 0, 0),
            commit("a@b.c", "A", 10, false, &["f0", "f1"], 0, 0),
        ];

        let mut aggregator = CommitAggregator::new();
        aggregator.extend(&commits);
        assert_eq!(aggregator.wide_commits, 1);
        let agg = aggregator.finish();

        // Only the narrow commit's single pair survives; per-file stats still
        // count every path the wide commit touched.
        assert_eq!(agg.couplings.len(), 1);
        assert_eq!(agg.couplings[&pair("f0", "f1")].cochanges, 1);
        assert_eq!(agg.files.len(), MAX_COUPLING_FILES_PER_COMMIT + 1);
        assert_eq!(agg.files["f0"].commits, 2);
    }

    /// The one aggregate that grows combinatorially rather than with the
    /// repository's shape has to stay bounded no matter how long the history
    /// is, and it has to drop the weakest evidence rather than the newest.
    #[test]
    fn coupling_pruning_keeps_the_strongest_pairs_and_bounds_the_map() {
        let mut aggregator = CommitAggregator::new();
        // Each cold commit touches the widest still-eligible file set, all
        // names unique, so it contributes 66 pairs that are never seen again
        // — the long tail that makes this map grow without bound. One hot
        // pair recurs alongside them.
        let pairs_per_commit =
            MAX_COUPLING_FILES_PER_COMMIT * (MAX_COUPLING_FILES_PER_COMMIT - 1) / 2;
        let cold_commits = MAX_TRACKED_COUPLINGS / pairs_per_commit + 2;
        for i in 0..cold_commits {
            aggregator.push(&commit(
                "a@b.c",
                "A",
                i as i64,
                false,
                &["hot/left", "hot/right"],
                0,
                0,
            ));
            let cold: Vec<String> = (0..MAX_COUPLING_FILES_PER_COMMIT)
                .map(|f| format!("cold/{i}/{f}"))
                .collect();
            let cold_refs: Vec<&str> = cold.iter().map(String::as_str).collect();
            aggregator.push(&commit("a@b.c", "A", i as i64, false, &cold_refs, 0, 0));
        }
        assert!(aggregator.pruned_couplings > 0, "the bound actually fired");
        let agg = aggregator.finish();

        assert!(
            agg.couplings.len() <= MAX_TRACKED_COUPLINGS,
            "the coupling map stays bounded across an unbounded history"
        );
        let hot = agg
            .couplings
            .get(&pair("hot/left", "hot/right"))
            .expect("the strongest pair survives pruning");
        assert_eq!(
            hot.cochanges as usize, cold_commits,
            "pruning drops the weakest evidence, never the strongest"
        );
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

    fn clone_entry(repo: &str, gib: u64, visited_hours_ago: Option<i64>) -> CloneEntry {
        CloneEntry {
            repo: repo.to_string(),
            path: PathBuf::from(format!("/repos/{repo}.git")),
            bytes: gib * 1024 * 1024 * 1024,
            last_visited: visited_hours_ago.map(|hours| at(0) - chrono::Duration::hours(hours)),
        }
    }

    fn in_flight(repos: &[&str]) -> HashSet<String> {
        repos.iter().map(|r| (*r).to_string()).collect()
    }

    fn repos_of(entries: &[CloneEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.repo.as_str()).collect()
    }

    /// The regression this exists for: an in-flight clone is exactly the
    /// profile the scored pass likes best (huge) and the fallback reaches
    /// first, so it must be off the table before either pass sees it.
    #[test]
    fn plan_never_offers_an_in_flight_clone_as_a_victim() {
        let now = at(0);
        let local = vec![
            // Being walked right now: biggest on disk, and `record_clone`
            // stamped last_visited at the start of the run.
            clone_entry("torvalds/linux", 6, Some(0)),
            // Idle for a week: the clone the pass is supposed to take.
            clone_entry("hexojs/hexo", 1, Some(24 * 7)),
            // Visited an hour ago by a finished run: last-resort material.
            clone_entry("django/django", 2, Some(1)),
        ];

        let plan = plan_evictions(local, &in_flight(&["torvalds/linux"]), now);

        assert_eq!(repos_of(&plan.scored), vec!["hexojs/hexo"]);
        assert_eq!(repos_of(&plan.protected), vec!["django/django"]);
        assert_eq!(plan.in_flight_clones, 1);
        assert_eq!(plan.in_flight_bytes, 6 * 1024 * 1024 * 1024);
    }

    /// The failure the operator described: the working set alone is over
    /// quota, so the fallback runs — and must still find nothing to delete
    /// when every remaining clone is being analyzed.
    #[test]
    fn plan_is_empty_when_every_clone_is_in_flight() {
        let now = at(0);
        let local = vec![
            clone_entry("torvalds/linux", 6, Some(0)),
            clone_entry("vercel/next.js", 2, Some(24 * 30)),
        ];

        let plan = plan_evictions(
            local,
            &in_flight(&["torvalds/linux", "vercel/next.js"]),
            now,
        );

        assert!(
            plan.scored.is_empty() && plan.protected.is_empty(),
            "a sweep may fail to free space, but never at the cost of a running walk"
        );
        assert_eq!(plan.in_flight_clones, 2);
        assert_eq!(plan.in_flight_bytes, 8 * 1024 * 1024 * 1024);
    }

    /// Staleness beats size in the scored pass, and the fallback is plain LRU.
    #[test]
    fn plan_orders_scored_by_size_times_idle_days_and_fallback_by_lru() {
        let now = at(0);
        let local = vec![
            clone_entry("small/ancient", 1, Some(24 * 100)), // 100 GiB-days
            clone_entry("big/recent", 4, Some(24 * 2)),      // 8 GiB-days
            clone_entry("never/visited", 2, None),           // 2 GiB-days
            clone_entry("hot/newest", 1, Some(1)),
            clone_entry("hot/older", 1, Some(20)),
        ];

        let plan = plan_evictions(local, &HashSet::new(), now);

        assert_eq!(
            repos_of(&plan.scored),
            vec!["small/ancient", "big/recent", "never/visited"]
        );
        assert_eq!(repos_of(&plan.protected), vec!["hot/older", "hot/newest"]);
    }

    async fn purge_stats(db: &Db, repo: &str) {
        for table in [
            "repo_author_commit_days",
            "repo_commit_days",
            "repo_todo_deltas",
            "repo_file_stats",
            "repo_file_couplings",
            "repo_author_stats",
            "repo_history",
        ] {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "DELETE FROM {table} WHERE repo = $1"
            )))
            .bind(repo)
            .execute(&db.pool)
            .await
            .expect("purge stats fixture");
        }
    }

    /// Everything the incremental write touches, in one comparable value:
    /// the cursor plus every aggregate row.
    type StatsSnapshot = (
        Vec<(Option<String>, Option<String>, i64)>,
        Vec<(String, i64, i64, i64, i64)>,
        Vec<(String, i64)>,
        Vec<(NaiveDate, i64, i64)>,
        Vec<(NaiveDate, i64, i64)>,
        Vec<(String, String, i64)>,
    );

    async fn snapshot_stats(db: &Db, repo: &str) -> StatsSnapshot {
        let history = sqlx::query_as(
            "SELECT last_analyzed_sha, head_sha, total_commits FROM repo_history WHERE repo = $1",
        )
        .bind(repo)
        .fetch_all(&db.pool)
        .await
        .expect("read repo_history");
        let files = sqlx::query_as(
            "SELECT path, commits, fix_commits, lines_added, lines_deleted \
             FROM repo_file_stats WHERE repo = $1 ORDER BY path",
        )
        .bind(repo)
        .fetch_all(&db.pool)
        .await
        .expect("read repo_file_stats");
        let authors = sqlx::query_as(
            "SELECT author_email, commits FROM repo_author_stats WHERE repo = $1 \
             ORDER BY author_email",
        )
        .bind(repo)
        .fetch_all(&db.pool)
        .await
        .expect("read repo_author_stats");
        let days = sqlx::query_as(
            "SELECT day, commits, lines_added FROM repo_commit_days WHERE repo = $1 ORDER BY day",
        )
        .bind(repo)
        .fetch_all(&db.pool)
        .await
        .expect("read repo_commit_days");
        let todos = sqlx::query_as(
            "SELECT day, todo_added, todo_removed FROM repo_todo_deltas WHERE repo = $1 \
             ORDER BY day",
        )
        .bind(repo)
        .fetch_all(&db.pool)
        .await
        .expect("read repo_todo_deltas");
        let couplings = sqlx::query_as(
            "SELECT path_a, path_b, cochanges FROM repo_file_couplings WHERE repo = $1 \
             ORDER BY path_a, path_b",
        )
        .bind(repo)
        .fetch_all(&db.pool)
        .await
        .expect("read repo_file_couplings");
        (history, files, authors, days, todos, couplings)
    }

    /// The correctness core of incremental analysis: the aggregate delta and
    /// the cursor advance are one transaction, so a failed apply leaves the
    /// repository exactly where the previous successful run left it — and the
    /// retry therefore re-derives the identical range from the identical
    /// cursor. If the cursor moved on its own connection, the rollback below
    /// would leave `last_analyzed_sha` at "head-2" with none of head-2's
    /// commits counted, and that range would be lost permanently.
    #[tokio::test]
    async fn rollback_leaves_aggregates_and_cursor_untouched() {
        let Some(db) = crate::test_db::shared().await else {
            eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
            return;
        };
        let repo = "gitdebt-test-stats/rollback";
        purge_stats(&db, repo).await;

        let base = vec![commit("a@b.c", "A", 0, false, &["x.rs"], 1, 0)];
        apply_commits_at_head(&db, repo, &base, "head-1")
            .await
            .expect("seed committed baseline");
        let before = snapshot_stats(&db, repo).await;
        assert_eq!(
            before.0,
            vec![(Some("head-1".into()), Some("head-1".into()), 1)]
        );

        let more = vec![
            commit("a@b.c", "A", 100, true, &["x.rs", "y.rs"], 2, 1),
            commit("d@e.f", "D", 200, false, &["y.rs"], 0, 0),
        ];
        let mut tx = db.pool.begin().await.expect("begin");
        write_aggregates_in_tx(
            &mut tx,
            repo,
            &aggregate_commits(&more),
            "head-2",
            false,
            None,
        )
        .await
        .expect("apply inside the caller's transaction");
        // Read through the same transaction first: without this the test would
        // also pass if the write silently did nothing at all.
        let (staged_sha, staged_total): (Option<String>, i64) = sqlx::query_as(
            "SELECT last_analyzed_sha, total_commits FROM repo_history WHERE repo = $1",
        )
        .bind(repo)
        .fetch_one(&mut *tx)
        .await
        .expect("read staged cursor");
        assert_eq!(staged_sha.as_deref(), Some("head-2"));
        assert_eq!(staged_total, 3, "the delta is staged inside the tx");
        tx.rollback().await.expect("rollback");

        let after = snapshot_stats(&db, repo).await;
        assert_eq!(
            before, after,
            "a rolled-back apply must move neither the aggregates nor the cursor"
        );

        purge_stats(&db, repo).await;
    }
}
