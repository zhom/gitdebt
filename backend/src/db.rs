use anyhow::{Context, Result};
use sqlx::Connection;
use sqlx::PgConnection;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

/// Postgres connections a single gitdebt process may hold.
///
/// Both binaries build their pool from this one number, so its real cost is
/// `2 × POOL_MAX` plus the worker's two out-of-pool leader sessions (the GH
/// Archive coordinator and the hourly follower each pin an advisory lock to a
/// dedicated connection): 16 + 16 + 2 = 34 of the 60 the shared server allows.
///
/// 16, down from 24, because Postgres runs on the same 12-vCPU / 32 GB host as
/// the api, the worker's git clones, and unrelated co-tenant services — gitdebt
/// does not own the box. Production held 28 backends open while the analysis
/// pool and a GH Archive backfill wrote at the same time, logging a statement
/// over the 1 s slow threshold every second or two: past that point more
/// concurrent writers bought contention, not throughput. Postgres cannot
/// usefully run more active queries than the host has cores, so a ceiling near
/// the core count converts oversubscription into a bounded
/// [`POOL_ACQUIRE_TIMEOUT`] wait on our side instead of an unbounded one inside
/// the server, where it would also slow every co-tenant's queries.
///
/// 16 covers both binaries' real demand. The worker's simultaneous claimants
/// are the repo-analysis pool (deliberately few, fat workers), the
/// star/metadata pool, the two archive singletons, and a handful of periodic
/// sweeps — under a dozen in the worst case. The api is bounded by
/// `api::MAX_INFLIGHT_REQUESTS`, but only its Postgres-backed handlers hold a
/// slot, and 16 statements in flight at single-digit-millisecond latency is an
/// order of magnitude more read throughput than this host is asked for.
const POOL_MAX: u32 = 16;
const POOL_ACQUIRE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const POOL_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5 * 60);
const POOL_MAX_LIFETIME: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// Server-side ceiling on any single statement issued through a pool.
///
/// This is the bound that actually ends a runaway query, and nothing else in
/// the process can do it: dropping a sqlx future sends no `CancelRequest`, so
/// an abandoned statement keeps its backend, its locks and its core until
/// Postgres itself stops it. [`POOL_MAX_LIFETIME`] is not a substitute — sqlx
/// evaluates it on the return-to-pool path, which a connection whose statement
/// never finishes never reaches.
///
/// Ten minutes is above every legitimate statement here (the slowest measured
/// analysis write is seconds; the widest profile aggregate is milliseconds)
/// and far below the hours a wedged one will otherwise run.
///
/// Relying on a `postgresql.conf` GUC for this was what left production
/// exposed: the server value is only armed at statement start, so a reload
/// cannot bound statements already running, and a database rebuilt or restored
/// without that line silently loses the protection. Setting it at connect time
/// makes it a property of the application, not of one server's configuration.
const STATEMENT_TIMEOUT: &str = "600s";

/// Companion bound: a transaction that stops making progress while holding
/// locks is as damaging as a slow statement, and `statement_timeout` alone
/// does not cover the gaps between statements.
const IDLE_IN_TRANSACTION_TIMEOUT: &str = "300s";

/// Connection options carrying the statement bounds and an identity.
///
/// `application_name` is diagnostic, and it is load-bearing during an incident:
/// without it, attributing a backend in `pg_stat_activity` to a binary — or to
/// a *container generation* — means correlating `client_addr` against live
/// container IPs, which stops working the moment those IPs are recycled.
/// Stamping the process makes an orphan self-identifying, so reaping one is a
/// targeted statement rather than archaeology.
fn connect_options(database_url: &str, service: &str) -> Result<PgConnectOptions> {
    Ok(database_url
        .parse::<PgConnectOptions>()
        .context("parse DATABASE_URL")?
        .options([
            ("statement_timeout", STATEMENT_TIMEOUT),
            (
                "idle_in_transaction_session_timeout",
                IDLE_IN_TRANSACTION_TIMEOUT,
            ),
        ])
        .application_name(&format!("gitdebt-{service}:{}", std::process::id())))
}

fn pool_options(max_connections: u32) -> PgPoolOptions {
    PgPoolOptions::new()
        .max_connections(max_connections.max(1))
        .acquire_timeout(POOL_ACQUIRE_TIMEOUT)
        .idle_timeout(Some(POOL_IDLE_TIMEOUT))
        .max_lifetime(Some(POOL_MAX_LIFETIME))
}

/// Serializes the idempotent startup schema across processes and test tasks.
///
/// PostgreSQL's `CREATE TABLE IF NOT EXISTS` is not safe when two sessions
/// create the same table at exactly the same time: both can pass the existence
/// check, then one loses while inserting the table's implicit row type into
/// `pg_type`. A session advisory lock prevents that catalog race and stays held
/// through the out-of-transaction concurrent-index maintenance that follows.
/// Contenders poll `pg_try_advisory_lock` instead of blocking inside Postgres:
/// a blocked advisory-lock transaction can itself be waited on by
/// `CREATE INDEX CONCURRENTLY`, creating a deadlock cycle.
const SCHEMA_MIGRATION_LOCK_ID: i64 = 0x6769_7464_6562_7401;

/// Bump whenever [`SCHEMA`] gains a required table, column, constraint, or
/// non-concurrent index. Once this revision is recorded, later process starts
/// can avoid taking DDL locks against live worker transactions.
const CURRENT_SCHEMA_VERSION: i32 = 4;

/// Attempts at the schema transaction before startup fails. The statements
/// are idempotent; retrying absorbs a `lock_timeout` abort caused by a
/// long-running transaction that happens to be open during a deploy.
const SCHEMA_MIGRATION_ATTEMPTS: u32 = 4;

/// Schema applied on every startup. Idempotent — uses IF NOT EXISTS.
///
/// Completeness invariant: a `repos` row's selected history is only readable
/// when `history_complete = true`. `stargazers_complete` remains the stricter
/// legacy flag for an exact GitHub-API membership snapshot; GH Archive
/// WatchEvents are stored and labeled separately because they do not include
/// unstars. Partial writes MUST leave both relevant flags false.
///
/// `api_quota` persists each GitHub token's current rate-limit budget so
/// restarts do not rediscover exhaustion through failed requests.
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS repo_stargazers (
    repo        TEXT NOT NULL,
    position    BIGINT NOT NULL,
    starred_at  TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (repo, position)
);

-- GH Archive WatchEvents are public star-addition actions, not the current
-- stargazer membership returned by GitHub's legacy endpoint: unstars are not
-- emitted and coverage begins on 2011-02-12. Keep them physically separate so
-- source semantics cannot be confused. `active_repo_star_history` exposes the
-- selected source to read surfaces without duplicating source-branching SQL.
CREATE TABLE IF NOT EXISTS repo_star_arrivals (
    repo            TEXT NOT NULL,
    position        BIGINT NOT NULL,
    source_event_id TEXT,
    starred_at      TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (repo, position)
);
ALTER TABLE repo_star_arrivals ADD COLUMN IF NOT EXISTS source_event_id TEXT;
-- Migrate older installations away from persisted stargazer identities.
-- Pagination position is non-identifying and keeps chunk retries idempotent
-- when multiple stars share an identical timestamp.
ALTER TABLE repo_stargazers ADD COLUMN IF NOT EXISTS position BIGINT;
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'repo_stargazers' AND column_name = 'login'
    ) THEN
        EXECUTE 'WITH ranked AS (
            SELECT ctid, ROW_NUMBER() OVER (
                PARTITION BY repo ORDER BY starred_at, login
            ) AS n FROM repo_stargazers
        )
        UPDATE repo_stargazers AS stars
        SET position = ranked.n
        FROM ranked WHERE stars.ctid = ranked.ctid';
        ALTER TABLE repo_stargazers DROP CONSTRAINT IF EXISTS repo_stargazers_pkey;
        ALTER TABLE repo_stargazers DROP COLUMN login;
        ALTER TABLE repo_stargazers ALTER COLUMN position SET NOT NULL;
        ALTER TABLE repo_stargazers ADD PRIMARY KEY (repo, position);
    END IF;
END $$;
-- NOTE: the secondary indexes on this table (the global (starred_at, repo)
-- velocity index and the per-repo (repo, starred_at)
-- window index) are intentionally NOT created here. `repo_stargazers` is
-- the largest, most write-heavy table in the schema; a plain CREATE INDEX
-- inside this one-shot schema transaction takes a lock that blocks writes
-- for the whole build and stalls startup before /health can bind. They are
-- created with CREATE INDEX CONCURRENTLY in a separate post-connect step —
-- see `CONCURRENT_INDEXES` / `Db::ensure_concurrent_indexes`.

CREATE TABLE IF NOT EXISTS repos (
    repo                  TEXT PRIMARY KEY NOT NULL,
    github_id             BIGINT,
    stargazers_fetched_at TIMESTAMPTZ,
    stargazers_complete   BOOLEAN NOT NULL DEFAULT FALSE,
    history_complete      BOOLEAN NOT NULL DEFAULT FALSE,
    star_count            BIGINT,
    history_source        TEXT,
    history_observed_count BIGINT,
    history_coverage_start TIMESTAMPTZ,
    history_coverage_end   TIMESTAMPTZ,
    archive_complete      BOOLEAN NOT NULL DEFAULT FALSE,
    archive_fetched_at    TIMESTAMPTZ,
    archive_truncated_before BOOLEAN NOT NULL DEFAULT FALSE,
    archive_cursor        DATE,
    forks_count           BIGINT,
    created_at            TIMESTAMPTZ,
    archived              BOOLEAN NOT NULL DEFAULT FALSE,
    pushed_at             TIMESTAMPTZ,
    updated_at            TIMESTAMPTZ,
    default_branch        TEXT,
    license_spdx          TEXT,
    topics                TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    has_issues            BOOLEAN NOT NULL DEFAULT FALSE,
    has_discussions       BOOLEAN NOT NULL DEFAULT FALSE,
    has_pages             BOOLEAN NOT NULL DEFAULT FALSE,
    is_template           BOOLEAN NOT NULL DEFAULT FALSE,
    subscribers_count     BIGINT NOT NULL DEFAULT 0,
    -- GitHub includes open pull requests in open_issues_count.
    open_issues_count     BIGINT NOT NULL DEFAULT 0,
    metadata_fetched_at   TIMESTAMPTZ,
    -- Popularity signal driven by the browser-extension `/api/ext/ping`
    -- endpoint (and any analyze hit). view_count is a cheap monotonic
    -- counter; last_viewed_at is the most-recent view. Both drive the
    -- star-fetch queue priority (hot repos jump the line) and the future
    -- "hot repos" surface. Written best-effort, off the request latency
    -- path, so a write failure never breaks a lookup.
    view_count            BIGINT NOT NULL DEFAULT 0,
    last_viewed_at        TIMESTAMPTZ,
    -- `missing` tombstones a repo GitHub reports as 404 (private, deleted,
    -- or a typo). The star-fetch worker treats NotFound as terminal: it
    -- sets this flag and does NOT requeue, and the analyze / ext-ping
    -- enqueue paths short-circuit on it so a 404 repo can't be re-enqueued
    -- on every page view (budget drain). Cleared implicitly only if a
    -- future successful metadata/stargazer fetch overwrites it.
    missing               BOOLEAN NOT NULL DEFAULT FALSE
);
ALTER TABLE repos ADD COLUMN IF NOT EXISTS github_id              BIGINT;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS history_complete       BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS forks_count         BIGINT;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS created_at          TIMESTAMPTZ;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS archived            BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS pushed_at           TIMESTAMPTZ;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS updated_at          TIMESTAMPTZ;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS default_branch      TEXT;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS license_spdx        TEXT;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS topics              TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[];
ALTER TABLE repos ADD COLUMN IF NOT EXISTS has_issues          BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS has_discussions     BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS has_pages           BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS is_template         BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS subscribers_count   BIGINT NOT NULL DEFAULT 0;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS open_issues_count   BIGINT NOT NULL DEFAULT 0;
-- Repair partially-deployed versions of these columns. `ADD COLUMN IF NOT
-- EXISTS` does not add a default or NOT NULL constraint when the name already
-- exists, and a NULL would make the typed cache summary unreadable. The
-- catalog guard makes the table rewrite/lock a one-time repair.
DO $$
DECLARE
    repairs TEXT[][] := ARRAY[
        ['archived', 'FALSE'],
        ['topics', 'ARRAY[]::TEXT[]'],
        ['has_issues', 'FALSE'],
        ['has_discussions', 'FALSE'],
        ['has_pages', 'FALSE'],
        ['is_template', 'FALSE'],
        ['subscribers_count', '0'],
        ['open_issues_count', '0']
    ];
    repair TEXT[];
BEGIN
    FOREACH repair SLICE 1 IN ARRAY repairs
    LOOP
        IF EXISTS (
            SELECT 1 FROM information_schema.columns
            WHERE table_name = 'repos' AND column_name = repair[1]
              AND (is_nullable = 'YES' OR column_default IS NULL)
        ) THEN
            EXECUTE format(
                'UPDATE repos SET %I = %s WHERE %I IS NULL',
                repair[1], repair[2], repair[1]
            );
            EXECUTE format(
                'ALTER TABLE repos ALTER COLUMN %I SET DEFAULT %s',
                repair[1], repair[2]
            );
            EXECUTE format(
                'ALTER TABLE repos ALTER COLUMN %I SET NOT NULL',
                repair[1]
            );
        END IF;
    END LOOP;
END $$;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS metadata_fetched_at TIMESTAMPTZ;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS view_count          BIGINT NOT NULL DEFAULT 0;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS last_viewed_at      TIMESTAMPTZ;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS missing             BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS history_source         TEXT;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS history_observed_count BIGINT;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS history_coverage_start TIMESTAMPTZ;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS history_coverage_end   TIMESTAMPTZ;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS archive_complete       BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS archive_fetched_at     TIMESTAMPTZ;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS archive_truncated_before BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE repos ADD COLUMN IF NOT EXISTS archive_cursor          DATE;
-- Existing installations used stargazers_complete as the only source flag.
-- Preserve those exact histories as the active source during the migration.
UPDATE repos
SET history_complete = TRUE,
    history_source = COALESCE(history_source, 'github_api'),
    history_observed_count = COALESCE(history_observed_count, star_count)
WHERE stargazers_complete = TRUE AND history_complete = FALSE;
CREATE INDEX IF NOT EXISTS idx_repos_github_id ON repos (github_id)
    WHERE github_id IS NOT NULL;
-- Owner-prefix lookups for the profile cards (`WHERE repo LIKE $1 || '/%'`
-- in api.rs::load_user_card_data). Under a non-C collation the plain PK
-- btree cannot serve a LIKE prefix, so every card render was a sequential
-- scan; text_pattern_ops restores the range-scan plan regardless of locale.
CREATE INDEX IF NOT EXISTS idx_repos_repo_prefix ON repos (repo text_pattern_ops);
-- Most-starred leaderboard ordering (api.rs::load_leaderboard_rows,
-- metric=stars). Partial over exactly the rows that ranking selects, with
-- the query's ORDER BY baked in, so the top-N page is an ordered index
-- scan + LIMIT instead of sorting every tracked repo per cache miss.
-- Landing-page activity pulse: `WHERE last_viewed_at IS NOT NULL AND NOT
-- missing AND metadata_fetched_at IS NOT NULL ORDER BY last_viewed_at DESC`.
CREATE INDEX IF NOT EXISTS idx_repos_last_viewed
    ON repos (last_viewed_at DESC, repo ASC)
    WHERE last_viewed_at IS NOT NULL AND NOT missing AND metadata_fetched_at IS NOT NULL;
-- Public-metadata refresh sweep: popularity-first over stale rows.
CREATE INDEX IF NOT EXISTS idx_repos_metadata_staleness
    ON repos (view_count DESC, metadata_fetched_at)
    WHERE missing = FALSE AND metadata_fetched_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_repos_history_star_count
    ON repos (star_count DESC, repo ASC)
    WHERE history_complete AND NOT missing AND star_count IS NOT NULL;

-- Daily, Postgres-owned leaderboard materialization. Public request paths read
-- these small rows instead of grouping the multi-million-row star tables.
-- Refreshes replace all rows in one transaction, so readers see either the
-- previous complete snapshot or the next complete snapshot, never a partial
-- ranking.
CREATE TABLE IF NOT EXISTS leaderboard_snapshots (
    metric       TEXT NOT NULL,
    window_days  INTEGER NOT NULL,
    rank         BIGINT NOT NULL,
    repo         TEXT NOT NULL,
    stars        BIGINT NOT NULL,
    velocity     BIGINT NOT NULL,
    computed_at  TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (metric, window_days, repo),
    UNIQUE (metric, window_days, rank)
);
CREATE TABLE IF NOT EXISTS leaderboard_snapshot_state (
    id           BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    computed_at  TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_leaderboard_snapshot_page
    ON leaderboard_snapshots (metric, window_days, rank);

CREATE OR REPLACE VIEW active_repo_star_history AS
    SELECT stars.repo, stars.position, stars.starred_at
    FROM repo_stargazers AS stars
    JOIN repos ON repos.repo = stars.repo
    WHERE repos.history_source = 'github_api'
    UNION ALL
    SELECT arrivals.repo, arrivals.position, arrivals.starred_at
    FROM repo_star_arrivals AS arrivals
    JOIN repos ON repos.repo = arrivals.repo
    WHERE repos.history_source = 'gh_archive';

-- Star-history fetch queue. Keyed by repo slug (owner/repo, lowercased).
-- A repo is enqueued on a cold/stale/unknown lookup and drained by the
-- background star-fetch worker(s) in worker.rs. Dedup: an already
-- pending/in_progress row is never re-enqueued (the worker is the single
-- writer of the stargazer cache, so two jobs for the same repo would
-- race on the same rows). Priority is popularity-first (view_count DESC)
-- then enqueue order (FIFO) so hot repos drain first under a tight
-- GitHub budget. `partial` marks a job that hit the per-attempt page cap
-- (`worker::MAX_STARGAZER_PAGES`) and was re-enqueued to continue later — the
-- stargazer cache stays `*_complete = FALSE` until a job finishes the
-- whole list.
CREATE TABLE IF NOT EXISTS star_fetch_queue (
    repo         TEXT PRIMARY KEY NOT NULL,
    status       TEXT NOT NULL DEFAULT 'pending',
    attempts     BIGINT NOT NULL DEFAULT 0,
    partial      BOOLEAN NOT NULL DEFAULT FALSE,
    next_page    BIGINT NOT NULL DEFAULT 1,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    priority     BIGINT NOT NULL DEFAULT 0,
    last_error   TEXT,
    enqueued_at  TIMESTAMPTZ NOT NULL,
    claimed_at   TIMESTAMPTZ,
    worker_id    TEXT
);
ALTER TABLE star_fetch_queue ADD COLUMN IF NOT EXISTS next_page BIGINT NOT NULL DEFAULT 1;
ALTER TABLE star_fetch_queue ADD COLUMN IF NOT EXISTS next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
CREATE INDEX IF NOT EXISTS idx_star_fetch_queue_status
    ON star_fetch_queue(status, priority DESC, enqueued_at);
CREATE INDEX IF NOT EXISTS idx_star_fetch_queue_available
    ON star_fetch_queue(status, next_attempt_at, priority DESC, enqueued_at);

CREATE TABLE IF NOT EXISTS api_quota (
    source       TEXT PRIMARY KEY NOT NULL,
    remaining    BIGINT NOT NULL,
    limit_total  BIGINT NOT NULL,
    reset_at     BIGINT NOT NULL,
    updated_at   TIMESTAMPTZ NOT NULL
);

-- Cross-process deployment visibility. The API and worker are separate
-- binaries by design; without a durable heartbeat an API-only deployment can
-- look healthy while every cold comparison remains queued forever.
CREATE TABLE IF NOT EXISTS service_heartbeats (
    instance_id TEXT PRIMARY KEY NOT NULL,
    service     TEXT NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    seen_at    TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_service_heartbeats_service_seen
    ON service_heartbeats(service, seen_at DESC);

-- Durable checkpoint for the raw hourly GH Archive follower. An hour and all
-- of its matching tracked-repo events commit in one transaction, so replay
-- after a crash is idempotent without persisting actors or event payloads.
CREATE TABLE IF NOT EXISTS gh_archive_hours (
    archive_hour TIMESTAMPTZ PRIMARY KEY,
    status       TEXT NOT NULL,
    attempts     BIGINT NOT NULL DEFAULT 0,
    event_count  BIGINT NOT NULL DEFAULT 0,
    processed_at TIMESTAMPTZ,
    last_error   TEXT
);

-- Logged-in gitdebt.com users. PK is GitHub's user id (stable across
-- login renames). Token columns contain versioned AES-GCM ciphertext;
-- plaintext tokens never reach Postgres.
CREATE TABLE IF NOT EXISTS app_users (
    id                          BIGINT PRIMARY KEY NOT NULL,
    login                       TEXT NOT NULL UNIQUE,
    name                        TEXT,
    avatar_url                  TEXT,
    email                       TEXT,
    access_token                TEXT NOT NULL,
    refresh_token               TEXT,
    token_expires_at            TIMESTAMPTZ,
    refresh_token_expires_at    TIMESTAMPTZ,
    created_at                  TIMESTAMPTZ NOT NULL,
    updated_at                  TIMESTAMPTZ NOT NULL
);

-- Org/account installations of the gitdebt GitHub App. Populated by the
-- webhook receiver when installations are created/deleted. Used later to
-- mint installation tokens for server-to-server calls (own rate budget).
CREATE TABLE IF NOT EXISTS installations (
    id              BIGINT PRIMARY KEY NOT NULL,
    account_login   TEXT NOT NULL,
    account_id      BIGINT,
    account_type    TEXT,
    repository_selection TEXT,
    suspended       BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_installations_account ON installations(account_login);

-- ===========================================================================
-- Repo history analysis (gitoxide-based; commit-graph stats per repo).
-- ===========================================================================

-- Per-repo analysis state. last_analyzed_sha drives incremental walks;
-- clone_path is NULL when the bare clone has been evicted. All
-- aggregates in the per-X tables persist across eviction.
CREATE TABLE IF NOT EXISTS repo_history (
    repo                 TEXT PRIMARY KEY NOT NULL,
    last_analyzed_sha    TEXT,
    last_analyzed_at     TIMESTAMPTZ,
    head_sha             TEXT,
    clone_path           TEXT,
    clone_size_bytes     BIGINT,
    last_visited_at      TIMESTAMPTZ,
    total_commits        BIGINT NOT NULL DEFAULT 0,
    analysis_duration_ms BIGINT,
    analysis_scope_commits BIGINT,
    analysis_truncated   BOOLEAN NOT NULL DEFAULT FALSE,
    analysis_revision    INTEGER NOT NULL DEFAULT 0
);
ALTER TABLE repo_history ADD COLUMN IF NOT EXISTS analysis_duration_ms BIGINT;
ALTER TABLE repo_history ADD COLUMN IF NOT EXISTS analysis_scope_commits BIGINT;
ALTER TABLE repo_history ADD COLUMN IF NOT EXISTS analysis_truncated BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE repo_history ADD COLUMN IF NOT EXISTS analysis_revision INTEGER NOT NULL DEFAULT 0;
-- Fleet-wide "how long does an analysis take" sample (progress ETAs). The
-- partial predicate matches the query exactly, so the LIMIT 20 is an ordered
-- index scan instead of a scan-and-sort of every analyzed repository.
CREATE INDEX IF NOT EXISTS idx_repo_history_duration_recent
    ON repo_history (last_analyzed_at DESC NULLS LAST)
    WHERE analysis_duration_ms IS NOT NULL;

-- Repository setup/readiness facts derived from a completed clone analysis.
-- Writers replace the row for an observed head SHA; readers can therefore
-- reject a stale readiness snapshot when repo_history advances.
CREATE TABLE IF NOT EXISTS repo_readiness (
    repo                TEXT PRIMARY KEY NOT NULL,
    head_sha            TEXT NOT NULL,
    readme              BOOLEAN NOT NULL DEFAULT FALSE,
    security            BOOLEAN NOT NULL DEFAULT FALSE,
    cla                 BOOLEAN NOT NULL DEFAULT FALSE,
    code_of_conduct     BOOLEAN NOT NULL DEFAULT FALSE,
    contributing        BOOLEAN NOT NULL DEFAULT FALSE,
    license             BOOLEAN NOT NULL DEFAULT FALSE,
    codeowners          BOOLEAN NOT NULL DEFAULT FALSE,
    changelog           BOOLEAN NOT NULL DEFAULT FALSE,
    issue_templates     BOOLEAN NOT NULL DEFAULT FALSE,
    pr_template         BOOLEAN NOT NULL DEFAULT FALSE,
    ci                  BOOLEAN NOT NULL DEFAULT FALSE,
    tests               BOOLEAN NOT NULL DEFAULT FALSE,
    dependency_updates  BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Per-file aggregates. fix_commits = commit count where the message
-- matches /\b(fix|bug|hotfix|patch)\b/i; commits = total commits touching
-- that path. Stays bounded by unique paths (chromium ~700k).
CREATE TABLE IF NOT EXISTS repo_file_stats (
    repo               TEXT NOT NULL,
    path               TEXT NOT NULL,
    commits            BIGINT NOT NULL DEFAULT 0,
    fix_commits        BIGINT NOT NULL DEFAULT 0,
    lines_added        BIGINT NOT NULL DEFAULT 0,
    lines_deleted      BIGINT NOT NULL DEFAULT 0,
    binary_changes     BIGINT NOT NULL DEFAULT 0,
    last_modified_at   TIMESTAMPTZ,
    PRIMARY KEY (repo, path)
);
ALTER TABLE repo_file_stats ADD COLUMN IF NOT EXISTS lines_added BIGINT NOT NULL DEFAULT 0;
ALTER TABLE repo_file_stats ADD COLUMN IF NOT EXISTS lines_deleted BIGINT NOT NULL DEFAULT 0;
ALTER TABLE repo_file_stats ADD COLUMN IF NOT EXISTS binary_changes BIGINT NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_repo_file_fix ON repo_file_stats(repo, fix_commits DESC);
CREATE INDEX IF NOT EXISTS idx_repo_file_recent ON repo_file_stats(repo, last_modified_at DESC);

-- Pairs of files changed by the same commit. Paths are stored in canonical
-- lexical order by the analysis writer so one logical pair has one key.
CREATE TABLE IF NOT EXISTS repo_file_couplings (
    repo       TEXT NOT NULL,
    path_a     TEXT NOT NULL,
    path_b     TEXT NOT NULL,
    cochanges  BIGINT NOT NULL DEFAULT 0,
    fix_commits BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (repo, path_a, path_b)
);
ALTER TABLE repo_file_couplings
    ADD COLUMN IF NOT EXISTS fix_commits BIGINT NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_repo_file_couplings_repo_cochanges
    ON repo_file_couplings(repo, cochanges DESC);

-- Per-author aggregates for the contributors chart.
--
-- `enrich_attempted_at` is a negative-cache timestamp: the GitHub
-- email→login enrichment is attempted at most once per TTL per author,
-- stamped on EVERY attempt (success or miss). Without it, authors whose
-- email never resolves to a GitHub login (gravatar-fallback rows) were
-- re-queried against the GitHub API on every single analysis run, burning
-- the shared PAT budget forever. The enrichment query skips rows stamped
-- within the TTL.
CREATE TABLE IF NOT EXISTS repo_author_stats (
    repo               TEXT NOT NULL,
    author_email       TEXT NOT NULL,
    author_name        TEXT,
    avatar_url         TEXT,
    github_login       TEXT,
    commits            BIGINT NOT NULL DEFAULT 0,
    first_commit_at    TIMESTAMPTZ,
    last_commit_at     TIMESTAMPTZ,
    enrich_attempted_at TIMESTAMPTZ,
    PRIMARY KEY (repo, author_email)
);
CREATE INDEX IF NOT EXISTS idx_repo_author_commits ON repo_author_stats(repo, commits DESC);
ALTER TABLE repo_author_stats ADD COLUMN IF NOT EXISTS enrich_attempted_at TIMESTAMPTZ;
-- Case-folded login lookup for the profile-card aggregation
-- (`WHERE LOWER(github_login) = $1` across ALL repos — see the user card
-- in api.rs/cards.rs). Partial: rows without a login can never match, so
-- indexing them is dead weight. Without this, that query is a full scan
-- of a many-million-row table.
CREATE INDEX IF NOT EXISTS idx_repo_author_login
    ON repo_author_stats (LOWER(github_login)) WHERE github_login IS NOT NULL;

-- Per-author/day commit counts. This is the minimum extra grain required for
-- truthful profile streaks: repository-wide daily totals cannot prove that a
-- particular person committed on that day. The author email already exists in
-- repo_author_stats; this table stores no additional identity or payload.
CREATE TABLE IF NOT EXISTS repo_author_commit_days (
    repo          TEXT NOT NULL,
    author_email  TEXT NOT NULL,
    day           DATE NOT NULL,
    commits       BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (repo, author_email, day)
);

-- Per-day commit count for the heatmap.
CREATE TABLE IF NOT EXISTS repo_commit_days (
    repo          TEXT NOT NULL,
    day           DATE NOT NULL,
    commits       BIGINT NOT NULL DEFAULT 0,
    lines_added   BIGINT NOT NULL DEFAULT 0,
    lines_deleted BIGINT NOT NULL DEFAULT 0,
    files_changed BIGINT NOT NULL DEFAULT 0,
    binary_files  BIGINT NOT NULL DEFAULT 0,
    large_changes BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (repo, day)
);
ALTER TABLE repo_commit_days ADD COLUMN IF NOT EXISTS lines_added BIGINT NOT NULL DEFAULT 0;
ALTER TABLE repo_commit_days ADD COLUMN IF NOT EXISTS lines_deleted BIGINT NOT NULL DEFAULT 0;
ALTER TABLE repo_commit_days ADD COLUMN IF NOT EXISTS files_changed BIGINT NOT NULL DEFAULT 0;
ALTER TABLE repo_commit_days ADD COLUMN IF NOT EXISTS binary_files BIGINT NOT NULL DEFAULT 0;
ALTER TABLE repo_commit_days ADD COLUMN IF NOT EXISTS large_changes BIGINT NOT NULL DEFAULT 0;
-- `idx_repo_commit_days_year` used to be created here as
-- `ON repo_commit_days(repo, day)` — column-for-column the PRIMARY KEY above.
-- The planner can never prefer a non-unique duplicate of the PK, so it served
-- no read while being maintained on every row of every analysis pass, doubling
-- this table's index write volume and its dead-entry production. It is dropped
-- in `CONCURRENT_INDEX_DROPS` and replaced by the covering index below.

-- TODO/FIXME deltas per day. Cumulative sum from first day → cur day
-- gives the running count at any point in time.
CREATE TABLE IF NOT EXISTS repo_todo_deltas (
    repo           TEXT NOT NULL,
    day            DATE NOT NULL,
    todo_added     BIGINT NOT NULL DEFAULT 0,
    todo_removed   BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (repo, day)
);

-- Repo analysis queue (separate from the user-fetch queue — the workload
-- profiles are different: clones are disk-heavy, walks are CPU-heavy,
-- but neither is GitHub-API rate-limited).
CREATE TABLE IF NOT EXISTS repo_analysis_queue (
    repo          TEXT PRIMARY KEY NOT NULL,
    status        TEXT NOT NULL DEFAULT 'pending',
    phase         TEXT NOT NULL DEFAULT 'queued',
    priority      BIGINT NOT NULL DEFAULT 0,
    requested_by_user_id BIGINT REFERENCES app_users(id) ON DELETE SET NULL,
    enqueued_at   TIMESTAMPTZ NOT NULL,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    claimed_at    TIMESTAMPTZ,
    started_at    TIMESTAMPTZ,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    total_units   BIGINT,
    completed_units BIGINT NOT NULL DEFAULT 0,
    last_error    TEXT,
    attempts      INT NOT NULL DEFAULT 0,
    worker_id     TEXT
);
ALTER TABLE repo_analysis_queue ADD COLUMN IF NOT EXISTS next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
ALTER TABLE repo_analysis_queue ADD COLUMN IF NOT EXISTS phase TEXT NOT NULL DEFAULT 'queued';
ALTER TABLE repo_analysis_queue ADD COLUMN IF NOT EXISTS priority BIGINT NOT NULL DEFAULT 0;
ALTER TABLE repo_analysis_queue ADD COLUMN IF NOT EXISTS requested_by_user_id BIGINT REFERENCES app_users(id) ON DELETE SET NULL;
ALTER TABLE repo_analysis_queue ADD COLUMN IF NOT EXISTS started_at TIMESTAMPTZ;
ALTER TABLE repo_analysis_queue ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
ALTER TABLE repo_analysis_queue ADD COLUMN IF NOT EXISTS total_units BIGINT;
ALTER TABLE repo_analysis_queue ADD COLUMN IF NOT EXISTS completed_units BIGINT NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_repo_queue_status ON repo_analysis_queue(status, enqueued_at);
CREATE INDEX IF NOT EXISTS idx_repo_queue_available
    ON repo_analysis_queue(status, next_attempt_at, enqueued_at);
CREATE INDEX IF NOT EXISTS idx_repo_queue_priority_available
    ON repo_analysis_queue(status, next_attempt_at, priority DESC, enqueued_at);
-- Both queue tables take many UPDATEs per job (claim, lease heartbeat every
-- 30s, phase/progress writes, completion). They are small and hot, and share
-- autovacuum workers with the multi-million-row star tables whose vacuums run
-- for minutes — so they need their own aggressive schedule, plus free space
-- per page to keep those updates HOT (no index maintenance per version).
-- Applied once: `ALTER TABLE ... SET` takes a table lock even when the value
-- is unchanged, and this runs on every process start.
DO $$
DECLARE
    queue_table TEXT;
BEGIN
    FOREACH queue_table IN ARRAY ARRAY['repo_analysis_queue', 'star_fetch_queue']
    LOOP
        IF NOT EXISTS (
            SELECT 1 FROM pg_class
            WHERE relname = queue_table
              AND reloptions::TEXT LIKE '%autovacuum_vacuum_scale_factor=0.02%'
        ) THEN
            EXECUTE format(
                'ALTER TABLE %I SET (autovacuum_vacuum_scale_factor = 0.02, '
                'autovacuum_vacuum_threshold = 50, '
                'autovacuum_analyze_scale_factor = 0.05, fillfactor = 70)',
                queue_table
            );
        END IF;
    END LOOP;
END$$;

-- Tokei lines-of-code aggregates per repo. One row per language.
-- Replaced wholesale on each analysis run (truncate-then-insert in one
-- transaction) so the row set always reflects HEAD exactly — no stale
-- "deleted language" rows lying around.
CREATE TABLE IF NOT EXISTS repo_lines (
    repo            TEXT NOT NULL,
    language        TEXT NOT NULL,
    files           BIGINT NOT NULL DEFAULT 0,
    lines_code      BIGINT NOT NULL DEFAULT 0,
    lines_blank     BIGINT NOT NULL DEFAULT 0,
    lines_comment   BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (repo, language)
);
-- `lines_exact = FALSE` marks a file census: `files` is meaningful, the line
-- columns are zero because the count was not run, not because the repository
-- has no code. Readers that cannot tell the two apart print a confident
-- "0 lines of code" for every large or asset-heavy repository.
ALTER TABLE repo_lines ADD COLUMN IF NOT EXISTS lines_exact BOOLEAN NOT NULL DEFAULT TRUE;
CREATE INDEX IF NOT EXISTS idx_repo_lines_code ON repo_lines(repo, lines_code DESC);
-- Profile language totals stopped scanning this table by owner prefix: the
-- profile resolves a bounded owned-repo set from `repos` first and reads
-- `repo_lines` by slug, which the primary key already serves. The old
-- text_pattern_ops index is now write cost with no reader.
DROP INDEX IF EXISTS idx_repo_lines_repo_prefix;

-- ===========================================================================
-- External package-download usage cache (npm / crates.io / PyPI / Docker Hub).
-- ===========================================================================
--
-- One row per (source, package). `body` is the already-normalized JSON the
-- usage endpoint serves (a `DownloadStats` blob), NOT the raw registry
-- response — we parse + downsample on fetch so the read path is a single
-- decode. `fetched_at` drives the ~12-24h TTL: downloads update at most
-- daily, so re-fetching more often just burns registry rate budget. A miss
-- (or stale row) triggers a best-effort refresh; a refresh failure leaves
-- the stale row in place so we degrade to last-known data rather than null.
CREATE TABLE IF NOT EXISTS usage_cache (
    source       TEXT NOT NULL,
    package      TEXT NOT NULL,
    body         TEXT NOT NULL,
    fetched_at   TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (source, package)
);

-- ===========================================================================
-- Org/user aggregate star charts: cached `login → public repos` mapping.
-- ===========================================================================
--
-- `login_repo_lists` is the per-login meta row: fetch timestamp (drives the
-- read-side TTL), completeness flag, and a 404 tombstone. `login_repos`
-- holds the per-repo rows (slug, star count at fetch time, rank). The
-- writer (`cache.rs::put_login_repos`) replaces the whole set and flips
-- `complete` inside ONE transaction; readers return nothing unless
-- `complete` is TRUE — the same invariant the stargazer cache honors.
-- Unlike the forever-tombstone on `repos.missing`, a missing login is
-- re-checked once the TTL lapses (accounts get renamed/recreated).
--
-- `account_type` is GitHub's own `User`/`Organization` discriminator for the
-- login, captured at fetch time. A login can be either, the two kinds are
-- listed through different endpoints with different visibility filters, and
-- guessing produces silently wrong or empty organization profiles — so the
-- kind is resolved and stored, never inferred. `public_repos` is the
-- account's authoritative public-repository count, which lets a capped list
-- state its coverage instead of presenting a truncated set as the whole
-- account. `list_truncated` records that a deeper walk could still improve
-- the list, so the background completion knows when to stop retrying.
CREATE TABLE IF NOT EXISTS login_repo_lists (
    login          TEXT PRIMARY KEY NOT NULL,
    fetched_at     TIMESTAMPTZ,
    complete       BOOLEAN NOT NULL DEFAULT FALSE,
    missing        BOOLEAN NOT NULL DEFAULT FALSE,
    account_type   TEXT,
    public_repos   BIGINT,
    list_truncated BOOLEAN NOT NULL DEFAULT FALSE
);
ALTER TABLE login_repo_lists ADD COLUMN IF NOT EXISTS account_type   TEXT;
ALTER TABLE login_repo_lists ADD COLUMN IF NOT EXISTS public_repos   BIGINT;
ALTER TABLE login_repo_lists ADD COLUMN IF NOT EXISTS list_truncated BOOLEAN NOT NULL DEFAULT FALSE;
CREATE TABLE IF NOT EXISTS login_repos (
    login   TEXT NOT NULL,
    repo    TEXT NOT NULL,
    stars   BIGINT NOT NULL DEFAULT 0,
    rank    BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (login, repo)
);
CREATE INDEX IF NOT EXISTS idx_login_repos_rank ON login_repos(login, rank);

-- Schema version. Single row, bumped after each one-time repair.
-- v1: initial schema. v2: timestamp columns moved from TEXT to TIMESTAMPTZ.
-- v3: clear false repo tombstones created when restricted stargazer 404s
--     were incorrectly treated as repository-metadata 404s.
CREATE TABLE IF NOT EXISTS schema_version (
    id           INTEGER PRIMARY KEY,
    version      INTEGER NOT NULL,
    applied_at   TIMESTAMPTZ NOT NULL
);
INSERT INTO schema_version (id, version, applied_at)
VALUES (1, 1, NOW())
ON CONFLICT (id) DO NOTHING;

-- ===========================================================================
-- Migration v1 → v2: TEXT timestamps → TIMESTAMPTZ.
--
-- TEXT timestamps cost ~3× the storage of TIMESTAMPTZ and force every
-- read site to parse RFC3339 by hand, which compounds on tables with
-- tens of millions of rows. Each ALTER below is wrapped in a guard
-- that checks `information_schema.columns` so re-running on an already-
-- migrated database is a no-op. Idempotent; safe to apply on every
-- startup.
-- ===========================================================================
DO $$
DECLARE
    -- (table, column) pairs for every TEXT timestamp in the schema.
    -- Rows that the DOMAIN-side cast can't parse (none expected for our
    -- own data) would error here — the whole migration aborts and the
    -- prior state is preserved by Postgres tx semantics.
    pairs TEXT[][] := ARRAY[
        ['repo_stargazers','starred_at'],
        ['repos','stargazers_fetched_at'],
        ['repos','metadata_fetched_at'],
        ['repos','created_at'],
        ['repos','pushed_at'],
        ['repos','updated_at'],
        ['api_quota','updated_at'],
        ['star_fetch_queue','enqueued_at'],
        ['star_fetch_queue','claimed_at'],
        ['repos','last_viewed_at'],
        ['app_users','token_expires_at'],
        ['app_users','refresh_token_expires_at'],
        ['app_users','created_at'],
        ['app_users','updated_at'],
        ['installations','created_at'],
        ['installations','updated_at'],
        ['repo_history','last_analyzed_at'],
        ['repo_history','last_visited_at'],
        ['repo_readiness','updated_at'],
        ['repo_file_stats','last_modified_at'],
        ['repo_author_stats','first_commit_at'],
        ['repo_author_stats','last_commit_at'],
        ['repo_analysis_queue','enqueued_at'],
        ['repo_analysis_queue','claimed_at']
    ];
    p TEXT[];
BEGIN
    FOREACH p SLICE 1 IN ARRAY pairs
    LOOP
        IF EXISTS (
            SELECT 1 FROM information_schema.columns
            WHERE table_name = p[1] AND column_name = p[2]
              AND data_type = 'text'
        ) THEN
            EXECUTE format(
                'ALTER TABLE %I ALTER COLUMN %I TYPE TIMESTAMPTZ USING %I::TIMESTAMPTZ',
                p[1], p[2], p[2]
            );
        END IF;
    END LOOP;
END$$;

-- ===========================================================================
-- Migration v2 → v3: repair ambiguous stargazer-endpoint 404 tombstones.
--
-- Older workers classified a 404 from `/stargazers` as proof that the repo
-- itself was missing. GitHub now restricts that endpoint independently, so
-- valid public repos were tombstoned and their jobs parked dead. Clear those
-- flags once and requeue only jobs carrying that old NotFound error. The new
-- worker verifies `/repos/{owner}/{repo}`: genuinely missing repos are
-- tombstoned again, while existing repos are parked as history-restricted.
-- ===========================================================================
DO $$
BEGIN
    IF (SELECT version FROM schema_version WHERE id = 1) < 3 THEN
        UPDATE repos SET missing = FALSE WHERE missing = TRUE;
        UPDATE star_fetch_queue
        SET status = 'pending',
            attempts = 0,
            partial = FALSE,
            next_page = 1,
            last_error = NULL,
            claimed_at = NULL,
            worker_id = NULL
        WHERE status = 'dead'
          AND last_error LIKE 'repo not found:%';
        UPDATE schema_version
        SET version = 3, applied_at = NOW()
        WHERE id = 1;
    END IF;
END$$;
UPDATE schema_version
SET version = 4, applied_at = NOW()
WHERE id = 1 AND version < 4;
"#;

/// Large-table indexes built with `CREATE INDEX CONCURRENTLY` *after* the
/// schema transaction, keyed by index name (for the invalid-index cleanup).
///
/// Why not inline in `SCHEMA`: `repo_stargazers` is the largest write-heavy
/// table; a plain `CREATE INDEX` holds a lock that blocks writes for the
/// whole build and stalls startup before `/health` binds. `CONCURRENTLY`
/// builds without blocking writes — but it cannot run inside a transaction
/// block or a multi-statement batch, so each is a single autocommit
/// statement run via the simple-query protocol (`raw_sql`), and the whole
/// step is best-effort (log + continue) so a transient failure never blocks
/// the server from serving reads.
///
/// The three shapes:
///   * `login` — the `WHERE login = $1` reverse lookup.
///   * `(starred_at, repo)` — the GLOBAL velocity GROUP BY
///     (`WHERE starred_at >= … GROUP BY repo`, leaderboard velocity metric):
///     leading `starred_at` makes the trailing window a range scan, trailing
///     `repo` lets the aggregate finish index-only. Kept — a `(repo, …)`
///     index cannot serve this cross-repo range.
///   * `(repo, starred_at)` — the PER-REPO predicates (leaderboard stars
///     LATERAL `WHERE s.repo = r.repo AND s.starred_at >= …`, the export /
///     card 30-day windows, per-repo series): leading `repo` narrows to the
///     repo, trailing `starred_at` makes the window a range scan. The
///     `(repo, position)` PK can't serve a `starred_at` range within a repo.
const CONCURRENT_INDEXES: &[(&str, &str)] = &[
    (
        "idx_repo_stargazers_starred_at",
        "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_repo_stargazers_starred_at \
         ON repo_stargazers(starred_at, repo)",
    ),
    (
        "idx_repo_stargazers_repo_starred_at",
        "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_repo_stargazers_repo_starred_at \
         ON repo_stargazers(repo, starred_at)",
    ),
    (
        "idx_repo_star_arrivals_starred_at",
        "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_repo_star_arrivals_starred_at \
         ON repo_star_arrivals(starred_at, repo)",
    ),
    (
        "idx_repo_star_arrivals_repo_starred_at",
        "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_repo_star_arrivals_repo_starred_at \
         ON repo_star_arrivals(repo, starred_at)",
    ),
    (
        // The programmatic sitemap orders by a computed "most recently
        // refreshed" timestamp, which no plain column index can serve. This
        // expression index does, and it must stay character-identical to
        // `cache::SITEMAP_UPDATED_AT_SQL`. Built concurrently because `repos`
        // takes a write on every extension ping.
        "idx_repos_sitemap",
        "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_repos_sitemap \
         ON repos ((GREATEST(archive_fetched_at, stargazers_fetched_at, metadata_fetched_at)) DESC, \
                   repo ASC) \
         WHERE history_complete AND NOT missing AND metadata_fetched_at IS NOT NULL",
    ),
    (
        // "Files carrying the churn" reads `WHERE repo = $1 ORDER BY commits
        // DESC`; without it the report path sorts every path row of the
        // repository on every cache miss. Built concurrently for the same
        // reason as the star indexes: this table holds one row per unique path
        // per repository, the analysis workers write to it continuously, and a
        // plain build would hold a lock against them for its whole duration.
        "idx_repo_file_commits",
        "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_repo_file_commits \
         ON repo_file_stats(repo, commits DESC, path)",
    ),
    (
        "idx_repo_star_arrivals_source_event",
        "CREATE UNIQUE INDEX CONCURRENTLY IF NOT EXISTS idx_repo_star_arrivals_source_event \
         ON repo_star_arrivals(repo, source_event_id) WHERE source_event_id IS NOT NULL",
    ),
    (
        // The profile heatmap and trend read
        // `WHERE repo = ANY($1) AND day BETWEEN $2 AND $3` and then SUM the
        // `commits` column. `PRIMARY KEY (repo, day)` narrows the rows, but
        // `commits` lives only in the heap, so every matching entry costs a
        // heap fetch. INCLUDE carries the summed column in the leaf, making the
        // aggregate an index-only scan wherever the visibility map is current.
        "idx_repo_commit_days_covering",
        "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_repo_commit_days_covering \
         ON repo_commit_days(repo, day) INCLUDE (commits)",
    ),
];

/// Indexes that must not exist any more.
///
/// Dropped concurrently, for the same reason the list above builds
/// concurrently: a plain `DROP INDEX` takes an ACCESS EXCLUSIVE lock on the
/// table and would block the analysis writers for its duration during startup.
///
/// A drop here is permanent. Removing a name from this list does not recreate
/// the index — re-add it to [`CONCURRENT_INDEXES`] if it is ever wanted back.
const CONCURRENT_INDEX_DROPS: &[&str] = &[
    // Column-for-column identical to `repo_commit_days`'s PRIMARY KEY
    // `(repo, day)`. Never chosen by the planner, maintained on every write.
    "idx_repo_commit_days_year",
];

/// The startup schema, exposed so tests in other modules can assert that the
/// index a query depends on is actually created.
#[cfg(test)]
pub(crate) fn schema_sql() -> &'static str {
    SCHEMA
}

/// The `CREATE INDEX` statement for a concurrently-built index, so a query's
/// module can assert that the index serving it still matches.
#[cfg(test)]
pub(crate) fn concurrent_index_sql(name: &str) -> Option<&'static str> {
    CONCURRENT_INDEXES
        .iter()
        .find(|(index, _)| *index == name)
        .map(|(_, sql)| *sql)
}

/// Thin wrapper around sqlx's `PgPool`. Cheap to clone (PgPool is internally
/// `Arc<...>`), so workers and request handlers each hold their own copy.
#[derive(Clone)]
pub struct Db {
    pub pool: PgPool,
}

impl Db {
    /// Connect to the Postgres instance pointed at by `database_url` and
    /// apply the schema.
    pub async fn connect(database_url: &str) -> Result<Self> {
        Self::connect_with_pool_size(database_url, POOL_MAX).await
    }

    /// Open a pool against an already-migrated database, applying no schema.
    ///
    /// Test binaries run many `#[tokio::test]` cases in parallel, each on its
    /// own runtime, so each needs its own pool (sqlx ties pool background
    /// tasks to the creating runtime). Re-running the idempotent schema DDL
    /// per test deadlocks against the queries of tests already in flight, so
    /// only the first test applies the schema and the rest connect with this.
    pub async fn connect_pool_only(database_url: &str, max_connections: u32) -> Result<Self> {
        let pool = pool_options(max_connections)
            .connect_with(connect_options(database_url, "pool")?)
            .await
            .context("connect postgres")?;
        Ok(Self { pool })
    }

    /// `connect` with an explicit pool ceiling.
    pub async fn connect_with_pool_size(database_url: &str, max_connections: u32) -> Result<Self> {
        let pool = pool_options(max_connections)
            .connect_with(connect_options(database_url, "app")?)
            .await
            .context("connect postgres")?;
        let me = Self { pool };
        if me.schema_is_current().await? {
            me.spawn_index_maintenance(database_url.to_string());
            return Ok(me);
        }
        // Keep the session lock on a dedicated connection. Dropping this
        // connection (including cancellation during startup) closes the
        // session and releases the lock instead of returning a still-locked
        // session to the application pool.
        let mut connection = PgConnection::connect(database_url)
            .await
            .context("connect schema session")?;
        loop {
            let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
                .bind(SCHEMA_MIGRATION_LOCK_ID)
                .fetch_one(&mut connection)
                .await
                .context("try schema migration lock")?;
            if acquired {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        // A lock timeout is a transient condition, not a broken schema.
        // Recheck after taking the lock: another process may have completed
        // the migration while this process was waiting.
        let mut migration_result = if me.schema_is_current().await? {
            Ok(())
        } else {
            me.migrate(&mut connection).await
        };
        for attempt in 1..SCHEMA_MIGRATION_ATTEMPTS {
            if migration_result.is_ok() {
                break;
            }
            tracing::warn!(
                attempt,
                error = %migration_result.as_ref().err().map(|e| e.to_string()).unwrap_or_default(),
                "schema migration attempt failed; retrying"
            );
            tokio::time::sleep(std::time::Duration::from_secs(2 * attempt as u64)).await;
            migration_result = me.migrate(&mut connection).await;
        }
        let unlock_result = sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(SCHEMA_MIGRATION_LOCK_ID)
            .execute(&mut connection)
            .await
            .context("unlock schema migration");
        let close_result = connection.close().await.context("close schema session");
        migration_result?;
        unlock_result?;
        close_result?;
        me.spawn_index_maintenance(database_url.to_string());
        Ok(me)
    }

    async fn schema_is_current(&self) -> Result<bool> {
        let version_table_exists: bool =
            sqlx::query_scalar("SELECT to_regclass('public.schema_version') IS NOT NULL")
                .fetch_one(&self.pool)
                .await
                .context("check schema version table")?;
        if !version_table_exists {
            return Ok(false);
        }
        let version: Option<i32> =
            sqlx::query_scalar("SELECT version FROM public.schema_version WHERE id = 1")
                .fetch_optional(&self.pool)
                .await
                .context("read schema version")?;
        Ok(version.is_some_and(|version| version >= CURRENT_SCHEMA_VERSION))
    }

    /// Build the large-table indexes in the background.
    ///
    /// A first-time (or post-invalid-rebuild) `CREATE INDEX CONCURRENTLY` on
    /// the star tables takes minutes, and it holds the schema lock while every
    /// other replica polls for it. Doing that before binding a listener meant
    /// health probes got a refused connection rather than a response, so an
    /// orchestrator killed the replicas that were waiting on the build. Index
    /// absence is a performance condition, not a correctness one, so the
    /// process serves traffic while this proceeds.
    fn spawn_index_maintenance(&self, database_url: String) {
        let db = self.clone();
        tokio::spawn(async move {
            let mut connection = match PgConnection::connect(&database_url).await {
                Ok(connection) => connection,
                Err(error) => {
                    tracing::warn!(%error, "index maintenance: connect failed");
                    return;
                }
            };
            loop {
                match sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_lock($1)")
                    .bind(SCHEMA_MIGRATION_LOCK_ID)
                    .fetch_one(&mut connection)
                    .await
                {
                    Ok(true) => break,
                    Ok(false) => tokio::time::sleep(std::time::Duration::from_secs(5)).await,
                    Err(error) => {
                        tracing::warn!(%error, "index maintenance: lock attempt failed");
                        return;
                    }
                }
            }
            db.ensure_concurrent_indexes(&mut connection).await;
            let _ = sqlx::query("SELECT pg_advisory_unlock($1)")
                .bind(SCHEMA_MIGRATION_LOCK_ID)
                .execute(&mut connection)
                .await;
            let _ = connection.close().await;
        });
    }

    async fn migrate(&self, connection: &mut PgConnection) -> Result<()> {
        let mut transaction = connection
            .begin()
            .await
            .context("begin schema transaction")?;
        // The schema takes ACCESS EXCLUSIVE on tables every request path
        // reads. Postgres queues lock waiters ahead of new readers, so a DDL
        // statement blocked behind one long-running transaction stalls every
        // reader behind it too. Failing fast turns that into a retried boot
        // instead of a site-wide stall, and every statement here is
        // idempotent, so a retry costs nothing.
        sqlx::raw_sql("SET LOCAL lock_timeout = '3s'")
            .execute(&mut *transaction)
            .await
            .context("set schema lock timeout")?;
        sqlx::raw_sql(SCHEMA)
            .execute(&mut *transaction)
            .await
            .context("apply schema")?;
        transaction
            .commit()
            .await
            .context("commit schema transaction")?;
        Ok(())
    }

    /// Build the large-table indexes with `CREATE INDEX CONCURRENTLY`,
    /// outside the schema transaction. Best-effort and idempotent:
    ///   * `IF NOT EXISTS` skips an already-built index.
    ///   * A leftover `INVALID` index from a previously-interrupted
    ///     CONCURRENTLY build is dropped first (otherwise `IF NOT EXISTS`
    ///     would keep the unusable one forever).
    ///   * Any error is logged and skipped — never fatal.
    ///
    /// Each statement runs on its own via the simple-query protocol
    /// (`raw_sql`): `CREATE INDEX CONCURRENTLY` cannot run inside a
    /// transaction block or a multi-statement batch.
    async fn ensure_concurrent_indexes(&self, connection: &mut PgConnection) {
        // Drops run first: a superseded index competes for buffer cache and is
        // maintained on every write until it is gone, so there is nothing to
        // gain by keeping it alive while its replacement builds.
        for name in CONCURRENT_INDEX_DROPS {
            let drop_stmt = format!("DROP INDEX CONCURRENTLY IF EXISTS {name}");
            if let Err(e) = sqlx::raw_sql(sqlx::AssertSqlSafe(drop_stmt))
                .execute(&mut *connection)
                .await
            {
                tracing::warn!(index = %name, error = %e, "drop superseded index (non-fatal)");
            }
        }
        for (name, create_stmt) in CONCURRENT_INDEXES {
            tracing::info!(index = %name, "ensuring database index");
            // Drop a leftover invalid index so IF NOT EXISTS can rebuild it.
            let drop_invalid = format!(
                "DO $$ BEGIN \
                   IF EXISTS ( \
                     SELECT 1 FROM pg_class c \
                     JOIN pg_index i ON i.indexrelid = c.oid \
                     WHERE c.relname = '{name}' AND NOT i.indisvalid \
                   ) THEN EXECUTE 'DROP INDEX IF EXISTS {name}'; \
                   END IF; \
                 END $$;"
            );
            if let Err(e) = sqlx::raw_sql(sqlx::AssertSqlSafe(drop_invalid))
                .execute(&mut *connection)
                .await
            {
                tracing::warn!(index = %name, error = %e, "drop invalid index (non-fatal)");
            }
            if let Err(e) = sqlx::raw_sql(*create_stmt).execute(&mut *connection).await {
                tracing::warn!(index = %name, error = %e, "create concurrent index (non-fatal)");
            }
        }
        tracing::info!("database index maintenance complete");
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CONCURRENT_INDEX_DROPS, CONCURRENT_INDEXES, CURRENT_SCHEMA_VERSION, POOL_MAX, SCHEMA,
        SCHEMA_MIGRATION_LOCK_ID,
    };

    /// Both binaries size their pool from the same constant, so the ceiling is
    /// a fleet-wide budget and not a per-process one — raising it by one adds
    /// two backends. This pins the arithmetic in [`POOL_MAX`]'s docs, because
    /// the Postgres it spends is shared with co-tenant services that have no
    /// way to defend themselves against gitdebt taking the last slot.
    #[test]
    fn pool_ceiling_budgets_the_whole_fleet_not_one_process() {
        /// gitdebt-api and gitdebt-worker.
        const PROCESSES: u32 = 2;
        /// GH Archive coordinator + hourly follower: advisory-lock sessions
        /// held on dedicated connections outside the pool.
        const LEADER_SESSIONS: u32 = 2;
        /// `max_connections` on the shared host's Postgres.
        const SERVER_LIMIT: u32 = 60;
        /// Left for the other services on the box plus an operator's psql.
        const CO_TENANT_RESERVE: u32 = 20;

        let fleet = POOL_MAX * PROCESSES + LEADER_SESSIONS;
        assert!(
            fleet + CO_TENANT_RESERVE <= SERVER_LIMIT,
            "{fleet} connections leaves under {CO_TENANT_RESERVE} for co-tenants"
        );
        // A rolling deploy runs an old and a new process side by side while
        // the old one drains, which must not exhaust the server.
        assert!(
            fleet + POOL_MAX <= SERVER_LIMIT,
            "{fleet} connections leaves no room for an overlapping deploy"
        );
    }

    /// Indexes added for queries that are executed on a fixed cadence rather
    /// than by a person: the progress poll's fleet-wide duration sample, the
    /// landing-page activity pulse, the public-metadata refresh sweep, and the
    /// report's churn ranking. Each one replaces a scan-and-sort of a table
    /// that grows with every repository the product has ever seen, so their
    /// absence is not a slow page — it is a database that saturates on
    /// background traffic alone.
    #[test]
    fn schema_indexes_the_recurring_background_queries() {
        for index in [
            "idx_repo_history_duration_recent",
            "idx_repos_last_viewed",
            "idx_repos_metadata_staleness",
        ] {
            assert!(SCHEMA.contains(index), "missing index {index}");
        }
        // `repo_file_stats` is written continuously by the analysis pool and
        // holds a row per unique path per repository, so its index is built
        // out of the schema transaction like the star tables' are.
        assert!(!SCHEMA.contains("idx_repo_file_commits"));
        assert!(
            CONCURRENT_INDEXES
                .iter()
                .any(|(name, _)| *name == "idx_repo_file_commits")
        );
    }

    /// A file census stores file counts with zero lines. Readers must be able
    /// to tell that apart from a repository that genuinely has no code, or
    /// every large or asset-heavy repository renders "0 lines of code".
    #[test]
    fn schema_records_whether_line_counts_are_exact() {
        assert!(
            SCHEMA.contains("ADD COLUMN IF NOT EXISTS lines_exact BOOLEAN NOT NULL DEFAULT TRUE")
        );
    }

    /// The schema runs on every process start against a live database and
    /// takes ACCESS EXCLUSIVE on tables every request path reads, so it is
    /// bounded by a `lock_timeout` — and a bounded statement must be retried,
    /// or an ordinary long-running transaction during a deploy turns into a
    /// failed boot.
    #[test]
    fn blocked_schema_statements_are_retried_rather_than_fatal() {
        const { assert!(super::SCHEMA_MIGRATION_ATTEMPTS > 1) };
    }

    #[test]
    fn schema_records_the_revision_that_skips_redundant_ddl() {
        assert!(SCHEMA.contains(&format!(
            "SET version = {CURRENT_SCHEMA_VERSION}, applied_at = NOW()"
        )));
    }

    /// The queue tables take many row versions per job. Their storage
    /// parameters must be applied conditionally: `ALTER TABLE ... SET` locks
    /// the table even when nothing changes, and this runs on every start.
    #[test]
    fn queue_autovacuum_settings_are_applied_only_when_absent() {
        assert!(SCHEMA.contains("autovacuum_vacuum_scale_factor = 0.02"));
        assert!(
            SCHEMA.contains("IF NOT EXISTS (\n            SELECT 1 FROM pg_class"),
            "the storage-parameter change must be guarded by a catalog check"
        );
    }

    /// The profile-card login index must stay in the idempotent schema
    /// (no migration files in this repo) and must stay partial — a full
    /// index over NULL logins would be dead weight on every analysis
    /// upsert. Purely additive: the caching invariants live in the
    /// completeness flags, which the second assertion pins in place.
    #[test]
    fn schema_keeps_author_login_index_and_completeness_flags() {
        assert!(SCHEMA.contains("idx_repo_author_login"));
        assert!(
            SCHEMA.contains(
                "ON repo_author_stats (LOWER(github_login)) WHERE github_login IS NOT NULL"
            )
        );
        assert!(SCHEMA.contains("stargazers_complete   BOOLEAN NOT NULL DEFAULT FALSE"));
        assert!(SCHEMA.contains("analysis_revision    INTEGER NOT NULL DEFAULT 0"));
        assert!(
            SCHEMA.contains("IF NOT EXISTS"),
            "schema must stay idempotent"
        );
    }

    #[test]
    fn schema_keeps_idempotent_author_day_storage_for_profile_streaks() {
        assert!(SCHEMA.contains("CREATE TABLE IF NOT EXISTS repo_author_commit_days"));
        assert!(
            SCHEMA.contains("PRIMARY KEY (repo, author_email, day)"),
            "author-day lookups need an indexed, idempotent key"
        );
    }

    #[test]
    fn schema_persists_the_public_repository_metadata_snapshot() {
        for column in [
            "archived",
            "pushed_at",
            "updated_at",
            "default_branch",
            "license_spdx",
            "topics",
            "has_issues",
            "has_discussions",
            "has_pages",
            "is_template",
            "subscribers_count",
            "open_issues_count",
        ] {
            assert!(
                SCHEMA.contains(&format!("ADD COLUMN IF NOT EXISTS {column}")),
                "metadata column {column} needs an idempotent startup migration"
            );
        }
        assert!(SCHEMA.contains("topics              TEXT[] NOT NULL"));
        assert!(SCHEMA.contains("GitHub includes open pull requests in open_issues_count"));
        assert!(SCHEMA.contains("AND (is_nullable = 'YES' OR column_default IS NULL)"));
    }

    #[test]
    fn schema_keeps_analysis_churn_couplings_and_readiness_idempotent() {
        for column in ["lines_added", "lines_deleted", "binary_changes"] {
            assert!(
                SCHEMA.contains(&format!(
                    "ALTER TABLE repo_file_stats ADD COLUMN IF NOT EXISTS {column}"
                )),
                "repo_file_stats.{column} needs an idempotent migration"
            );
        }
        for column in [
            "lines_added",
            "lines_deleted",
            "files_changed",
            "binary_files",
            "large_changes",
        ] {
            assert!(
                SCHEMA.contains(&format!(
                    "ALTER TABLE repo_commit_days ADD COLUMN IF NOT EXISTS {column}"
                )),
                "repo_commit_days.{column} needs an idempotent migration"
            );
        }
        assert!(SCHEMA.contains("CREATE TABLE IF NOT EXISTS repo_file_couplings"));
        assert!(SCHEMA.contains("PRIMARY KEY (repo, path_a, path_b)"));
        assert!(SCHEMA.contains("ADD COLUMN IF NOT EXISTS fix_commits BIGINT NOT NULL DEFAULT 0"));
        assert!(SCHEMA.contains("idx_repo_file_couplings_repo_cochanges"));

        assert!(SCHEMA.contains("CREATE TABLE IF NOT EXISTS repo_readiness"));
        assert!(SCHEMA.contains("repo                TEXT PRIMARY KEY NOT NULL"));
        assert!(SCHEMA.contains("head_sha            TEXT NOT NULL"));
        for definition in [
            "readme              BOOLEAN NOT NULL DEFAULT FALSE",
            "security            BOOLEAN NOT NULL DEFAULT FALSE",
            "cla                 BOOLEAN NOT NULL DEFAULT FALSE",
            "code_of_conduct     BOOLEAN NOT NULL DEFAULT FALSE",
            "contributing        BOOLEAN NOT NULL DEFAULT FALSE",
            "license             BOOLEAN NOT NULL DEFAULT FALSE",
            "codeowners          BOOLEAN NOT NULL DEFAULT FALSE",
            "changelog           BOOLEAN NOT NULL DEFAULT FALSE",
            "issue_templates     BOOLEAN NOT NULL DEFAULT FALSE",
            "pr_template         BOOLEAN NOT NULL DEFAULT FALSE",
            "ci                  BOOLEAN NOT NULL DEFAULT FALSE",
            "tests               BOOLEAN NOT NULL DEFAULT FALSE",
            "dependency_updates  BOOLEAN NOT NULL DEFAULT FALSE",
        ] {
            assert!(
                SCHEMA.contains(definition),
                "repo_readiness definition `{definition}` must stay in the startup schema"
            );
        }
        assert!(SCHEMA.contains("updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()"));
    }

    #[test]
    fn schema_migration_uses_a_stable_advisory_lock_key() {
        assert_ne!(
            SCHEMA_MIGRATION_LOCK_ID, 0,
            "the startup schema lock must use a dedicated non-zero key"
        );
    }

    #[test]
    fn schema_tracks_cross_process_worker_liveness() {
        assert!(SCHEMA.contains("CREATE TABLE IF NOT EXISTS service_heartbeats"));
        assert!(SCHEMA.contains("idx_service_heartbeats_service_seen"));
    }

    #[test]
    fn schema_repairs_ambiguous_stargazer_404_tombstones_once() {
        assert!(SCHEMA.contains("IF (SELECT version FROM schema_version WHERE id = 1) < 3"));
        assert!(SCHEMA.contains("UPDATE repos SET missing = FALSE WHERE missing = TRUE"));
        assert!(SCHEMA.contains("last_error LIKE 'repo not found:%'"));
        assert!(SCHEMA.contains("SET version = 3"));
    }

    #[test]
    fn schema_excludes_removed_account_analytics() {
        assert!(!SCHEMA.contains("CREATE TABLE IF NOT EXISTS users"));
        assert!(!SCHEMA.contains("CREATE TABLE IF NOT EXISTS user_starred_repos"));
        assert!(!SCHEMA.contains("CREATE TABLE IF NOT EXISTS user_events"));
        assert!(!SCHEMA.contains("CREATE TABLE IF NOT EXISTS fetch_queue"));
        assert!(SCHEMA.contains("subscribers_count"));
        assert!(SCHEMA.contains("CREATE TABLE IF NOT EXISTS repo_stargazers"));
        assert!(SCHEMA.contains("position    BIGINT NOT NULL"));
        assert!(!SCHEMA.contains("login       TEXT NOT NULL"));
        assert!(SCHEMA.contains("ALTER TABLE repo_stargazers DROP COLUMN login"));
        assert!(SCHEMA.contains("stargazers_complete   BOOLEAN NOT NULL DEFAULT FALSE"));
        assert!(SCHEMA.contains("next_page    BIGINT NOT NULL DEFAULT 1"));
        assert!(SCHEMA.contains("next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW()"));
        assert!(SCHEMA.contains("github_id             BIGINT"));
        assert!(SCHEMA.contains("history_source        TEXT"));
        assert!(SCHEMA.contains("history_observed_count BIGINT"));
        assert!(SCHEMA.contains("history_coverage_start TIMESTAMPTZ"));
        assert!(SCHEMA.contains("history_coverage_end   TIMESTAMPTZ"));
        assert!(SCHEMA.contains("idx_repos_github_id"));
        assert!(
            SCHEMA.contains(
                "ALTER TABLE star_fetch_queue ADD COLUMN IF NOT EXISTS next_page BIGINT NOT NULL DEFAULT 1"
            ),
            "existing installations need the resumable cursor added idempotently"
        );
        assert!(
            SCHEMA.contains(
                "ALTER TABLE star_fetch_queue ADD COLUMN IF NOT EXISTS next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW()"
            ),
            "star retries need a durable availability timestamp"
        );
        assert!(
            SCHEMA.contains(
                "ALTER TABLE repo_analysis_queue ADD COLUMN IF NOT EXISTS next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW()"
            ),
            "analysis retries need a durable availability timestamp"
        );
    }

    /// The public leaderboard + profile queries must never regress to
    /// sequential scans of the append-heavy tables. The owner-prefix LIKE
    /// lookup that resolves a login's repositories needs a
    /// `text_pattern_ops` index — a plain PK btree can't serve a LIKE
    /// prefix under a non-C collation — and it is the ONE prefix scan a
    /// profile is allowed: every per-repo table is then read by slug.
    #[test]
    fn schema_keeps_leaderboard_and_card_indexes() {
        assert!(SCHEMA.contains("idx_repos_repo_prefix"));
        assert!(SCHEMA.contains("ON repos (repo text_pattern_ops)"));
        assert!(SCHEMA.contains("idx_repos_history_star_count"));
        assert!(
            SCHEMA.contains("DROP INDEX IF EXISTS idx_repo_lines_repo_prefix"),
            "no reader scans repo_lines by owner prefix any more"
        );
        assert!(SCHEMA.contains("CREATE TABLE IF NOT EXISTS leaderboard_snapshots"));
        assert!(SCHEMA.contains("UNIQUE (metric, window_days, rank)"));
        assert!(SCHEMA.contains("CREATE TABLE IF NOT EXISTS leaderboard_snapshot_state"));
    }

    /// `repo_commit_days` must carry exactly one `(repo, day)` btree.
    ///
    /// It used to carry two: `PRIMARY KEY (repo, day)` and a non-unique
    /// `idx_repo_commit_days_year` on the identical columns. The planner can
    /// never prefer the duplicate, so it served no read while being maintained
    /// on every write of every analysis pass. This asserts the duplicate is
    /// gone from the schema, that startup actively drops it rather than merely
    /// stopping to create it, and that the covering replacement is built
    /// concurrently — a plain build would lock out the analysis writers.
    #[test]
    fn repo_commit_days_has_one_key_index_and_a_covering_one() {
        assert!(
            SCHEMA.contains("PRIMARY KEY (repo, day)"),
            "the key itself must stay"
        );
        assert!(
            !SCHEMA.contains("CREATE INDEX IF NOT EXISTS idx_repo_commit_days_year"),
            "the duplicate of the primary key must not be created"
        );
        assert!(
            CONCURRENT_INDEX_DROPS.contains(&"idx_repo_commit_days_year"),
            "an index already built on a live database is only removed by dropping it"
        );
        // Every drop is concurrent: a plain DROP INDEX takes ACCESS EXCLUSIVE
        // on the table and would stall startup behind the analysis writers.
        for name in CONCURRENT_INDEX_DROPS {
            assert!(
                !SCHEMA.contains(&format!("DROP INDEX IF EXISTS {name}")),
                "{name} must be dropped concurrently, not inside the schema transaction"
            );
        }
        // The replacement carries the summed column so the profile aggregate
        // can be index-only instead of one heap fetch per matching day.
        let covering = CONCURRENT_INDEXES
            .iter()
            .find(|(name, _)| *name == "idx_repo_commit_days_covering")
            .map(|(_, sql)| *sql)
            .expect("covering index is registered");
        assert!(
            covering.contains("ON repo_commit_days(repo, day) INCLUDE (commits)"),
            "{covering}"
        );
        // A name cannot be in both lists: startup would drop and recreate it
        // on every boot.
        for name in CONCURRENT_INDEX_DROPS {
            assert!(
                !CONCURRENT_INDEXES.iter().any(|(built, _)| built == name),
                "{name} is both created and dropped"
            );
        }
    }

    /// The `repo_stargazers` secondary indexes must NOT be created inline in
    /// the schema transaction (a plain CREATE INDEX there blocks writes and
    /// stalls startup on the largest table). They move to the post-connect
    /// CONCURRENTLY step, which must:
    ///   * cover both access shapes — the GLOBAL `(starred_at, repo)`
    ///     velocity index and the PER-REPO
    ///     `(repo, starred_at)` window index (leaderboard stars LATERAL /
    ///     export+card 30-day windows / per-repo series);
    ///   * use `CONCURRENTLY` + `IF NOT EXISTS` on every statement (cannot
    ///     run in a txn; must tolerate an already-built index).
    #[test]
    fn stargazer_indexes_are_concurrent_and_out_of_schema_txn() {
        // Not inline in the schema transaction.
        assert!(!SCHEMA.contains("idx_repo_stargazers_starred_at"));
        assert!(!SCHEMA.contains("idx_repo_stargazers_repo_starred_at"));

        let names: Vec<&str> = CONCURRENT_INDEXES.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"idx_repo_stargazers_starred_at"));
        // The per-repo window index complements the position PK and the
        // (starred_at,repo) existed — no (repo, starred_at)).
        assert!(names.contains(&"idx_repo_stargazers_repo_starred_at"));

        for (_, stmt) in CONCURRENT_INDEXES {
            assert!(
                stmt.contains("CREATE INDEX CONCURRENTLY IF NOT EXISTS")
                    || stmt.contains("CREATE UNIQUE INDEX CONCURRENTLY IF NOT EXISTS"),
                "{stmt}"
            );
        }
        // Both the global (velocity GROUP BY) and per-repo shapes are present.
        assert!(
            CONCURRENT_INDEXES
                .iter()
                .any(|(_, s)| s.contains("ON repo_stargazers(starred_at, repo)"))
        );
        assert!(
            CONCURRENT_INDEXES
                .iter()
                .any(|(_, s)| s.contains("ON repo_stargazers(repo, starred_at)"))
        );
        assert!(
            CONCURRENT_INDEXES
                .iter()
                .any(|(_, s)| s.contains("ON repo_star_arrivals(starred_at, repo)"))
        );
        assert!(
            CONCURRENT_INDEXES
                .iter()
                .any(|(_, s)| s.contains("ON repo_star_arrivals(repo, starred_at)"))
        );
        assert!(
            CONCURRENT_INDEXES.iter().any(|(_, s)| {
                s.contains("ON repo_star_arrivals(repo, source_event_id)")
                    && s.contains("WHERE source_event_id IS NOT NULL")
            }),
            "hourly and BigQuery overlap must deduplicate by archive event ID"
        );
    }

    #[test]
    fn archive_history_is_source_separated_and_hidden_until_complete() {
        assert!(SCHEMA.contains("CREATE TABLE IF NOT EXISTS repo_star_arrivals"));
        assert!(SCHEMA.contains("history_complete      BOOLEAN NOT NULL DEFAULT FALSE"));
        assert!(SCHEMA.contains("archive_cursor        DATE"));
        assert!(SCHEMA.contains("source_event_id TEXT"));
        assert!(SCHEMA.contains("CREATE OR REPLACE VIEW active_repo_star_history"));
        assert!(SCHEMA.contains("repos.history_source = 'github_api'"));
        assert!(SCHEMA.contains("repos.history_source = 'gh_archive'"));
        assert!(
            !SCHEMA.contains("actor.login"),
            "archive storage must never retain stargazer identities"
        );
    }

    /// A login can be a user or an organization, and the two are listed
    /// through different GitHub endpoints. The resolved kind, the account's
    /// public-repo count, and the "page cap cut the walk short" flag must
    /// live next to the cached list — an in-memory-only kind would be
    /// re-guessed after every restart, and a truncated list with no record
    /// of the truncation reads as a complete account.
    #[test]
    fn login_lists_persist_account_kind_and_coverage() {
        for column in [
            "account_type   TEXT",
            "public_repos   BIGINT",
            "list_truncated BOOLEAN NOT NULL DEFAULT FALSE",
        ] {
            assert!(
                SCHEMA.contains(column),
                "missing login-list column: {column}"
            );
        }
        for alter in [
            "ALTER TABLE login_repo_lists ADD COLUMN IF NOT EXISTS account_type   TEXT",
            "ALTER TABLE login_repo_lists ADD COLUMN IF NOT EXISTS public_repos   BIGINT",
            "ALTER TABLE login_repo_lists ADD COLUMN IF NOT EXISTS list_truncated BOOLEAN NOT NULL DEFAULT FALSE",
        ] {
            assert!(
                SCHEMA.contains(alter),
                "existing installations need this column added idempotently: {alter}"
            );
        }
        // The completeness gate stays the reader's contract: the rows are
        // only readable once `complete` flips inside the writer's
        // transaction.
        assert!(SCHEMA.contains("complete       BOOLEAN NOT NULL DEFAULT FALSE"));
    }

    #[test]
    fn analysis_progress_and_priority_survive_restart() {
        for column in [
            "phase         TEXT NOT NULL DEFAULT 'queued'",
            "priority      BIGINT NOT NULL DEFAULT 0",
            "requested_by_user_id BIGINT REFERENCES app_users(id) ON DELETE SET NULL",
            "started_at    TIMESTAMPTZ",
            "updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()",
            "total_units   BIGINT",
            "completed_units BIGINT NOT NULL DEFAULT 0",
            "analysis_duration_ms BIGINT",
            "analysis_scope_commits BIGINT",
            "analysis_truncated   BOOLEAN NOT NULL DEFAULT FALSE",
        ] {
            assert!(
                SCHEMA.contains(column),
                "missing durable analysis column: {column}"
            );
        }
        assert!(SCHEMA.contains("idx_repo_queue_priority_available"));
        assert!(SCHEMA.contains("priority DESC, enqueued_at"));
    }
}
