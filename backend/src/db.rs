use anyhow::{Context, Result};
use sqlx::Connection;
use sqlx::PgConnection;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

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
CREATE INDEX IF NOT EXISTS idx_repos_history_star_count
    ON repos (star_count DESC, repo ASC)
    WHERE history_complete AND NOT missing AND star_count IS NOT NULL;

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
-- (MAX_STARGAZER_PAGES) and was re-enqueued to continue later — the
-- stargazer cache stays `*_complete = FALSE` until a job finishes the
-- whole list.
CREATE TABLE IF NOT EXISTS star_fetch_queue (
    repo         TEXT PRIMARY KEY NOT NULL,
    status       TEXT NOT NULL DEFAULT 'pending',
    attempts     BIGINT NOT NULL DEFAULT 0,
    partial      BOOLEAN NOT NULL DEFAULT FALSE,
    next_page    BIGINT NOT NULL DEFAULT 1,
    priority     BIGINT NOT NULL DEFAULT 0,
    last_error   TEXT,
    enqueued_at  TIMESTAMPTZ NOT NULL,
    claimed_at   TIMESTAMPTZ,
    worker_id    TEXT
);
ALTER TABLE star_fetch_queue ADD COLUMN IF NOT EXISTS next_page BIGINT NOT NULL DEFAULT 1;
CREATE INDEX IF NOT EXISTS idx_star_fetch_queue_status
    ON star_fetch_queue(status, priority DESC, enqueued_at);

CREATE TABLE IF NOT EXISTS api_quota (
    source       TEXT PRIMARY KEY NOT NULL,
    remaining    BIGINT NOT NULL,
    limit_total  BIGINT NOT NULL,
    reset_at     BIGINT NOT NULL,
    updated_at   TIMESTAMPTZ NOT NULL
);

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
    total_commits        BIGINT NOT NULL DEFAULT 0
);

-- Per-file aggregates. fix_commits = commit count where the message
-- matches /\b(fix|bug|hotfix|patch)\b/i; commits = total commits touching
-- that path. Stays bounded by unique paths (chromium ~700k).
CREATE TABLE IF NOT EXISTS repo_file_stats (
    repo               TEXT NOT NULL,
    path               TEXT NOT NULL,
    commits            BIGINT NOT NULL DEFAULT 0,
    fix_commits        BIGINT NOT NULL DEFAULT 0,
    last_modified_at   TIMESTAMPTZ,
    PRIMARY KEY (repo, path)
);
CREATE INDEX IF NOT EXISTS idx_repo_file_fix ON repo_file_stats(repo, fix_commits DESC);
CREATE INDEX IF NOT EXISTS idx_repo_file_recent ON repo_file_stats(repo, last_modified_at DESC);

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

-- Per-day commit count for the heatmap.
CREATE TABLE IF NOT EXISTS repo_commit_days (
    repo     TEXT NOT NULL,
    day      DATE NOT NULL,
    commits  BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (repo, day)
);
CREATE INDEX IF NOT EXISTS idx_repo_commit_days_year ON repo_commit_days(repo, day);

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
    enqueued_at   TIMESTAMPTZ NOT NULL,
    claimed_at    TIMESTAMPTZ,
    last_error    TEXT,
    attempts      INT NOT NULL DEFAULT 0,
    worker_id     TEXT
);
CREATE INDEX IF NOT EXISTS idx_repo_queue_status ON repo_analysis_queue(status, enqueued_at);

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
CREATE INDEX IF NOT EXISTS idx_repo_lines_code ON repo_lines(repo, lines_code DESC);
-- Owner-prefix language totals for the user profile card (`WHERE repo
-- LIKE $1 || '/%'` in api.rs::load_top_langs) — same non-C-collation LIKE
-- rationale as idx_repos_repo_prefix above.
CREATE INDEX IF NOT EXISTS idx_repo_lines_repo_prefix ON repo_lines (repo text_pattern_ops);

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
CREATE TABLE IF NOT EXISTS login_repo_lists (
    login        TEXT PRIMARY KEY NOT NULL,
    fetched_at   TIMESTAMPTZ,
    complete     BOOLEAN NOT NULL DEFAULT FALSE,
    missing      BOOLEAN NOT NULL DEFAULT FALSE
);
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
        "idx_repo_star_arrivals_source_event",
        "CREATE UNIQUE INDEX CONCURRENTLY IF NOT EXISTS idx_repo_star_arrivals_source_event \
         ON repo_star_arrivals(repo, source_event_id) WHERE source_event_id IS NOT NULL",
    ),
];

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
        let max_connections: u32 = std::env::var("DB_POOL_MAX")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(60);
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(database_url)
            .await
            .context("connect postgres")?;
        let me = Self { pool };
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

        let migration_result = me.migrate(&mut connection).await;
        if migration_result.is_ok() {
            // Complete index maintenance before accepting traffic so
            // expensive public queries never launch without their required
            // indexes.
            me.ensure_concurrent_indexes(&mut connection).await;
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
        Ok(me)
    }

    async fn migrate(&self, connection: &mut PgConnection) -> Result<()> {
        let mut transaction = connection
            .begin()
            .await
            .context("begin schema transaction")?;
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
    use super::{CONCURRENT_INDEXES, SCHEMA, SCHEMA_MIGRATION_LOCK_ID};

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
        assert!(
            SCHEMA.contains("IF NOT EXISTS"),
            "schema must stay idempotent"
        );
    }

    #[test]
    fn schema_migration_uses_a_stable_advisory_lock_key() {
        assert_ne!(
            SCHEMA_MIGRATION_LOCK_ID, 0,
            "the startup schema lock must use a dedicated non-zero key"
        );
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
        assert!(!SCHEMA.contains("subscribers_count"));
        assert!(SCHEMA.contains("CREATE TABLE IF NOT EXISTS repo_stargazers"));
        assert!(SCHEMA.contains("position    BIGINT NOT NULL"));
        assert!(!SCHEMA.contains("login       TEXT NOT NULL"));
        assert!(SCHEMA.contains("ALTER TABLE repo_stargazers DROP COLUMN login"));
        assert!(SCHEMA.contains("stargazers_complete   BOOLEAN NOT NULL DEFAULT FALSE"));
        assert!(SCHEMA.contains("next_page    BIGINT NOT NULL DEFAULT 1"));
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
    }

    /// The public leaderboard + profile-card queries must never regress to
    /// sequential scans of the append-heavy tables. The small-table indexes
    /// stay inline in the schema; the owner-prefix LIKE lookups need
    /// `text_pattern_ops` indexes (a plain PK btree can't serve a LIKE
    /// prefix under a non-C collation). Purely additive — no completeness
    /// flag or reader/writer semantics change.
    #[test]
    fn schema_keeps_leaderboard_and_card_indexes() {
        assert!(SCHEMA.contains("idx_repos_repo_prefix"));
        assert!(SCHEMA.contains("ON repos (repo text_pattern_ops)"));
        assert!(SCHEMA.contains("idx_repos_history_star_count"));
        assert!(SCHEMA.contains("idx_repo_lines_repo_prefix"));
        assert!(SCHEMA.contains("ON repo_lines (repo text_pattern_ops)"));
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
}
