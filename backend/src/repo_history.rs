//! Local bare-clone management + incremental commit-history walking.
//!
//! Storage layout: `<REPOS_DIR>/<owner>/<repo>.git` (bare). Default
//! `REPOS_DIR=~/.cache/gitdebt/repos` for dev; set to your mounted volume
//! path for container deployments. Disk usage is tracked in
//! `repo_history.clone_size_bytes` and trimmed via `evict_to_quota`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use tokio::process::Command;

// Five thousand recent non-merge commits is enough to stabilize ownership,
// churn, cadence, and fix-concentration signals while keeping a cold
// interactive report bounded. The exact reachable commit total is still
// computed from the full graph and the UI labels a capped analysis window.
const DEFAULT_ANALYSIS_COMMIT_LIMIT: usize = 5_000;
const MIN_ANALYSIS_COMMIT_LIMIT: usize = 5_000;
const HARD_ANALYSIS_COMMIT_LIMIT: usize = 50_000;
/// Patch bodies are substantially more expensive than commit metadata because
/// partial clones must hydrate historical blobs. Contributor, cadence, churn,
/// and fix signals use the full bounded window; TODO/FIXME churn uses only the
/// newest commits so it cannot hold the primary analysis hostage.
pub(crate) const TODO_PATCH_COMMIT_LIMIT: usize = 100;
/// Stable cursor for a valid repository whose default branch has no commits.
/// It cannot collide with a Git object id and lets the normal freshness/cache
/// contract complete instead of retrying an empty repository forever.
pub(crate) const EMPTY_REPOSITORY_HEAD: &str = "empty-repository";
const CACHE_FORMAT_VERSION: &str = "2";

pub(crate) fn analysis_commit_limit() -> usize {
    std::env::var("REPO_ANALYSIS_COMMIT_LIMIT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_ANALYSIS_COMMIT_LIMIT)
        .clamp(MIN_ANALYSIS_COMMIT_LIMIT, HARD_ANALYSIS_COMMIT_LIMIT)
}

#[derive(Clone, Debug)]
pub struct RepoStorage {
    pub root: PathBuf,
    pub quota_bytes: u64,
    pub high_watermark_pct: u8,
}

impl RepoStorage {
    pub fn from_env() -> Self {
        let root = std::env::var("REPOS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
                PathBuf::from(home).join(".cache/gitdebt/repos")
            });
        let quota_bytes: u64 = std::env::var("REPOS_QUOTA_BYTES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(80 * 1024 * 1024 * 1024);
        let high_watermark_pct: u8 = std::env::var("REPOS_HIGH_WATERMARK_PCT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(80);
        Self {
            root,
            quota_bytes,
            high_watermark_pct,
        }
    }

    pub fn path_for(&self, repo: &str) -> PathBuf {
        let safe: String = repo
            .chars()
            .map(|c| match c {
                '/' | '-' | '_' | '.' => c,
                c if c.is_ascii_alphanumeric() => c,
                _ => '_',
            })
            .collect();
        self.root.join(format!("{safe}.git"))
    }
}

pub struct RepoHandle {
    pub path: PathBuf,
    pub head_sha: String,
}

impl RepoHandle {
    pub fn is_empty(&self) -> bool {
        self.head_sha == EMPTY_REPOSITORY_HEAD
    }
}

/// Thread cap handed to every git invocation. `index-pack` defaults to one
/// thread per visible core, so N concurrent clones oversubscribe the host by a
/// factor of N while Postgres runs beside them. Dividing the cores by the pool
/// size keeps the whole pool inside one host's CPU budget.
fn pack_threads_config() -> &'static str {
    static CONFIG: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CONFIG.get_or_init(|| {
        let cores = std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(2);
        let threads =
            (cores / crate::repo_analysis::configured_analysis_workers().max(1)).clamp(1, 8);
        format!("pack.threads={threads}")
    })
}

/// Every git invocation goes through here. `kill_on_drop` is the load-bearing
/// part: a dropped future (job timeout, shutdown) must reap the subprocess
/// instead of orphaning a clone that keeps writing to the volume.
fn git() -> Command {
    let mut command = Command::new("git");
    command
        .args(["-c", pack_threads_config()])
        .kill_on_drop(true);
    command
}

/// [`git`] scoped to a repository directory.
fn git_in(path: &Path) -> Command {
    let mut command = git();
    command.arg("-C").arg(path);
    command
}

/// Open the bare clone if present, otherwise clone fresh from GitHub.
/// Idempotent — repeated calls fast-fetch updates rather than re-cloning.
/// The complete commit graph is retained even after aggregate analysis is
/// bounded, so exact repository totals never depend on the sampling window.
pub async fn open_or_clone(
    storage: &RepoStorage,
    repo: &str,
    _last_analyzed_sha: Option<&str>,
) -> Result<RepoHandle> {
    let path = storage.path_for(repo);
    tokio::fs::create_dir_all(path.parent().unwrap_or_else(|| Path::new("."))).await?;

    if path.exists() && !cache_format_is_current(&path).await {
        // Older tree-filtered clones can contain the commit graph without the
        // historical trees required by `--name-only`. Relaxing the filter on
        // a later fetch does not reliably backfill those promisor objects, so
        // metadata walks degrade into thousands of serial lazy fetches. A
        // versioned one-time rebuild is faster and makes every later scan
        // fully local.
        tracing::info!(repo, "rebuilding legacy repository cache");
        tokio::fs::remove_dir_all(&path)
            .await
            .context("remove legacy bare clone")?;
        if let Err(clone_error) = clone_bare(repo, &path).await {
            let _ = tokio::fs::remove_dir_all(&path).await;
            return Err(clone_error);
        }
    } else if path.exists() {
        if let Err(error) = fetch_updates(&path).await {
            if !fetch_requires_reclone(&error) {
                return Err(error);
            }
            tracing::warn!(repo, %error, "cached default branch disappeared; recloning");
            tokio::fs::remove_dir_all(&path)
                .await
                .context("remove stale bare clone")?;
            if let Err(clone_error) = clone_bare(repo, &path).await {
                // A failed clone can leave a non-repository directory that
                // would otherwise turn every later retry into a fetch error.
                let _ = tokio::fs::remove_dir_all(&path).await;
                return Err(clone_error);
            }
        }
    } else {
        clone_bare(repo, &path).await?;
    }
    let head_sha = rev_parse_head(&path).await?;
    Ok(RepoHandle { path, head_sha })
}

fn fetch_requires_reclone(error: &anyhow::Error) -> bool {
    let detail = error.to_string().to_ascii_lowercase();
    detail.contains("couldn't find remote ref refs/heads/")
        || detail.contains("could not find remote ref refs/heads/")
}

async fn clone_bare(repo: &str, path: &Path) -> Result<()> {
    let url = format!("https://github.com/{repo}.git");
    // Fetch the complete commit graph and trees so exact totals and path-only
    // history scans stay local. Historical file bodies remain promisor objects
    // and are hydrated only for the small TODO patch window.
    let output = git()
        .args([
            "clone",
            "--bare",
            "--no-tags",
            "--single-branch",
            "--filter=blob:none",
            "--",
        ])
        .arg(&url)
        .arg(path)
        .output()
        .await
        .context("spawn full git clone")?;
    if !output.status.success() {
        bail!(
            "git clone failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let config = git_in(path)
        .args(["config", "gitdebt.cacheFormat", CACHE_FORMAT_VERSION])
        .output()
        .await
        .context("mark repository cache format")?;
    if !config.status.success() {
        bail!(
            "git cache format failed: {}",
            String::from_utf8_lossy(&config.stderr)
        );
    }
    write_commit_graph(path).await;
    Ok(())
}

/// Write (or extend) the commit-graph.
///
/// Every analysis run counts reachable commits, which without a commit-graph
/// parses every commit object out of the pack — seconds of CPU per run on a
/// large repository, paid to refresh one integer. Best-effort: a failure only
/// means the next count is slower.
async fn write_commit_graph(path: &Path) {
    match git_in(path)
        .args(["commit-graph", "write", "--reachable", "--split"])
        .output()
        .await
    {
        Ok(output) if output.status.success() => {}
        Ok(output) => tracing::debug!(
            stderr = %String::from_utf8_lossy(&output.stderr),
            "commit-graph write failed"
        ),
        Err(error) => tracing::debug!(%error, "commit-graph write could not run"),
    }
}

async fn cache_format_is_current(path: &Path) -> bool {
    let Ok(output) = git_in(path)
        .args(["config", "--get", "gitdebt.cacheFormat"])
        .output()
        .await
    else {
        return false;
    };
    output.status.success()
        && String::from_utf8_lossy(&output.stdout).trim() == CACHE_FORMAT_VERSION
}

async fn fetch_updates(path: &Path) -> Result<()> {
    // Resolve the clone's default branch from its HEAD symref. A bare
    // single-branch clone has NO `remote.origin.fetch` refspec, so a plain
    // `git fetch origin` only writes `FETCH_HEAD` and never advances the
    // branch ref — `rev-parse HEAD` would then keep returning the stale
    // SHA and incremental analysis would never see new commits. We fetch an
    // *explicit* refspec (`+refs/heads/<branch>:refs/heads/<branch>`) so the
    // local branch (and therefore HEAD) actually moves forward.
    let branch = default_branch(path).await?;
    let refspec = format!("+refs/heads/{branch}:refs/heads/{branch}");
    let shallow = is_shallow_repository(path).await?;
    let tree_less = partial_clone_filter(path)
        .await?
        .is_some_and(|filter| filter == "tree:0");
    let mut command = git_in(path);
    command.args(["fetch", "--no-tags", "--filter=blob:none"]);
    if shallow {
        command.arg("--unshallow");
    } else if tree_less {
        // Existing tree:0 caches have the full graph but omit historical
        // trees. Reapply the less restrictive filter once so later metadata
        // walks do not degrade into hundreds of lazy network fetches.
        command.arg("--refetch");
    }
    let output = command
        // `--` before the positional remote name: defense-in-depth so a
        // future unvalidated positional arg can't be parsed as a flag.
        .args(["--", "origin", &refspec])
        .output()
        .await
        .context("spawn git fetch")?;
    if !output.status.success() {
        bail!(
            "git fetch failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    write_commit_graph(path).await;
    Ok(())
}

/// The default branch name of a bare clone, read from its `HEAD` symref
/// (e.g. `main`). Used to build the explicit fetch refspec so a
/// single-branch bare clone's branch ref actually advances on refresh.
async fn default_branch(path: &Path) -> Result<String> {
    let output = git_in(path)
        .args(["symbolic-ref", "--short", "HEAD"])
        .output()
        .await
        .context("spawn symbolic-ref")?;
    if !output.status.success() {
        bail!(
            "git symbolic-ref HEAD failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        bail!("empty default branch from symbolic-ref HEAD");
    }
    Ok(branch)
}

async fn rev_parse_head(path: &Path) -> Result<String> {
    let output = git_in(path)
        // Plain `rev-parse HEAD` prints the literal string `HEAD` with a zero
        // exit status in an unborn repository. Verification is required to
        // prove that the name resolves to a commit object.
        .args(["rev-parse", "--verify", "HEAD^{commit}"])
        .output()
        .await
        .context("spawn rev-parse")?;
    if !output.status.success() && repository_has_any_commit(path).await? {
        bail!(
            "git rev-parse failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    if !output.status.success() {
        return Ok(EMPTY_REPOSITORY_HEAD.to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Distinguish a genuinely empty repository from a corrupt clone when HEAD
/// cannot be resolved. `rev-list --all` succeeds with no output for an empty
/// repo, while malformed object databases still surface as an error.
async fn repository_has_any_commit(path: &Path) -> Result<bool> {
    let output = git_in(path)
        .args(["rev-list", "--all", "--max-count=1"])
        .output()
        .await
        .context("probe repository commits")?;
    if !output.status.success() {
        bail!(
            "git commit probe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(!output.stdout.is_empty())
}

/// Per-commit aggregated facts. Built from a single streaming `git log`.
#[derive(Clone, Debug, Serialize)]
pub struct CommitInfo {
    pub sha: String,
    pub author_email: String,
    pub author_name: String,
    pub committed_at: DateTime<Utc>,
    pub committed_day: NaiveDate,
    pub message_first_line: String,
    pub is_fix: bool,
    pub paths_changed: Vec<String>,
    /// Per-path line movement from `git --numstat`. Binary files carry zero
    /// line counts and set `binary = true`. Root commits stay excluded from
    /// change-frequency/churn aggregates so an initial import cannot dominate
    /// every later repository signal.
    pub file_changes: Vec<FileChange>,
    pub lines_added: u64,
    pub lines_deleted: u64,
    pub binary_files: u32,
    pub todo_added: u32,
    pub todo_removed: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct FileChange {
    pub path: String,
    pub lines_added: u64,
    pub lines_deleted: u64,
    pub binary: bool,
}

/// Record-separator sentinel emitted at the start of each commit's
/// `--format` block. The two leading NULs make it impossible to appear
/// inside a `--numstat -z` field (which is single-NUL-terminated) or a
/// `-p` text patch (which contains no NUL bytes at all), so splitting the
/// raw stdout on this byte string cleanly delimits commits even when a
/// patch body or commit subject contains arbitrary text.
const COMMIT_SENTINEL: &[u8] = b"\x00\x00GDCOMMIT\x00";

/// Pathspecs excluded from the TODO/FIXME patch walk: media, archives,
/// fonts, compiled artifacts, and generated or vendored text. None of them
/// can contain a TODO comment worth counting, and every one of them is
/// expensive to diff — a partial clone must download the whole blob first.
pub(crate) const NON_TEXT_PATHSPECS: &[&str] = &[
    ":(exclude,icase)*.png",
    ":(exclude,icase)*.jpg",
    ":(exclude,icase)*.jpeg",
    ":(exclude,icase)*.gif",
    ":(exclude,icase)*.webp",
    ":(exclude,icase)*.avif",
    ":(exclude,icase)*.bmp",
    ":(exclude,icase)*.tiff",
    ":(exclude,icase)*.psd",
    ":(exclude,icase)*.ico",
    ":(exclude,icase)*.icns",
    ":(exclude,icase)*.mp4",
    ":(exclude,icase)*.mov",
    ":(exclude,icase)*.webm",
    ":(exclude,icase)*.avi",
    ":(exclude,icase)*.mp3",
    ":(exclude,icase)*.wav",
    ":(exclude,icase)*.ogg",
    ":(exclude,icase)*.flac",
    ":(exclude,icase)*.pdf",
    ":(exclude,icase)*.zip",
    ":(exclude,icase)*.gz",
    ":(exclude,icase)*.tar",
    ":(exclude,icase)*.bz2",
    ":(exclude,icase)*.xz",
    ":(exclude,icase)*.7z",
    ":(exclude,icase)*.rar",
    ":(exclude,icase)*.jar",
    ":(exclude,icase)*.war",
    ":(exclude,icase)*.wasm",
    ":(exclude,icase)*.exe",
    ":(exclude,icase)*.dll",
    ":(exclude,icase)*.so",
    ":(exclude,icase)*.dylib",
    ":(exclude,icase)*.a",
    ":(exclude,icase)*.o",
    ":(exclude,icase)*.class",
    ":(exclude,icase)*.pyc",
    ":(exclude,icase)*.woff",
    ":(exclude,icase)*.woff2",
    ":(exclude,icase)*.ttf",
    ":(exclude,icase)*.otf",
    ":(exclude,icase)*.eot",
    ":(exclude,icase)*.bin",
    ":(exclude,icase)*.dat",
    ":(exclude,icase)*.parquet",
    ":(exclude,icase)*.min.js",
    ":(exclude,icase)*.min.css",
    ":(exclude,icase)*.map",
    ":(exclude)package-lock.json",
    ":(exclude)pnpm-lock.yaml",
    ":(exclude)yarn.lock",
    ":(exclude)Cargo.lock",
    ":(exclude)go.sum",
    ":(exclude)composer.lock",
    ":(exclude)Gemfile.lock",
    ":(exclude)poetry.lock",
    ":(exclude)node_modules/**",
    ":(exclude)vendor/**",
    ":(exclude)third_party/**",
];

/// Cap on the per-commit patch bytes scanned for TODO/FIXME deltas. A
/// chromium-merge-class commit with 100k changed lines would otherwise
/// blow up the scan; 4 MB is plenty to capture the realistic TODO churn.
const MAX_PATCH_BYTES: usize = 4 * 1024 * 1024;

/// Bound the raw `git log -p` output retained at once. Large repositories can
/// produce gigabytes of patch text; the parsed commit facts are much smaller,
/// so walk a fixed number of explicitly listed commits per subprocess and
/// discard each raw batch before continuing.
// Keep progress and cancellation responsive on large repositories. A large
// `git log -p` batch can run for several minutes without emitting a durable
// progress update; 100 still amortizes process startup while giving the UI
// five measured checkpoints across the production first-pass window.
pub(crate) const LOG_BATCH_COMMITS: usize = 100;
/// Metadata-only walks retain far less output and do not hydrate file bodies,
/// so larger batches reduce git/network startup cost without hiding progress
/// for minutes at a time.
pub(crate) const METADATA_BATCH_COMMITS: usize = 500;

/// Bounded newest-history plan used by the production worker. Large projects
/// such as Linux have more than a million reachable commits; repository-health
/// signals are useful over a recent, explicitly reported window and must not
/// require an unbounded full-history walk before anything becomes visible.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CommitWalkPlan {
    pub shas: Vec<String>,
    pub truncated: bool,
}

/// Select at most `limit` newest non-merge commits, returned oldest-first.
/// Asking git for `limit + 1` lets us report that the analysis window was
/// capped without ever materializing a million-SHA rev-list in memory.
pub(crate) async fn plan_recent_commits(
    handle: &RepoHandle,
    since_sha: Option<&str>,
    limit: usize,
) -> Result<CommitWalkPlan> {
    if handle.is_empty() {
        return Ok(CommitWalkPlan {
            shas: Vec::new(),
            truncated: false,
        });
    }
    let range = match since_sha {
        Some(sha) => format!("{sha}..HEAD"),
        None => "HEAD".to_string(),
    };
    let bounded = limit.max(1);
    let probe = bounded.saturating_add(1);
    let max_count = format!("--max-count={probe}");
    let output = git_in(&handle.path)
        .args(["rev-list", "--no-merges", &max_count, &range])
        .output()
        .await
        .context("git bounded rev-list")?;
    if !output.status.success() {
        bail!(
            "git rev-list failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let sha_output = std::str::from_utf8(&output.stdout).context("git rev-list non-UTF-8")?;
    let mut shas: Vec<String> = sha_output
        .lines()
        .map(str::trim)
        .filter(|sha| !sha.is_empty())
        .map(str::to_string)
        .collect();
    validate_shas(&shas)?;
    let truncated = shas.len() > bounded;
    shas.truncate(bounded);
    shas.reverse();
    Ok(CommitWalkPlan { shas, truncated })
}

async fn is_shallow_repository(path: &Path) -> Result<bool> {
    let output = git_in(path)
        .args(["rev-parse", "--is-shallow-repository"])
        .output()
        .await
        .context("git shallow-repository probe")?;
    if !output.status.success() {
        bail!(
            "git shallow-repository probe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim() == "true")
}

async fn partial_clone_filter(path: &Path) -> Result<Option<String>> {
    let output = git_in(path)
        .args(["config", "--get", "remote.origin.partialclonefilter"])
        .output()
        .await
        .context("git partial clone filter")?;
    if output.status.success() {
        let filter = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Ok((!filter.is_empty()).then_some(filter));
    }
    if output.status.code() == Some(1) {
        return Ok(None);
    }
    bail!(
        "git partial clone filter failed: {}",
        String::from_utf8_lossy(&output.stderr)
    )
}

/// Exact number of commits reachable from the default branch, including
/// merges. The clone always carries the complete commit graph, so this is a
/// cheap local graph walk even when patch-level health analysis is bounded.
pub(crate) async fn reachable_commit_count(handle: &RepoHandle) -> Result<usize> {
    if handle.is_empty() {
        return Ok(0);
    }
    let output = git_in(&handle.path)
        .args(["rev-list", "--count", "HEAD"])
        .output()
        .await
        .context("git reachable commit count")?;
    if !output.status.success() {
        bail!(
            "git rev-list --count failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<usize>()
        .context("parse reachable commit count")
}

/// Iterate every commit reachable from HEAD that isn't an ancestor of
/// `since_sha`. None = walk entire history. Yields oldest-first.
///
/// Each bounded `git log` emits, per commit:
///   * the `--format` header (NUL-delimited sha/email/name/date/subject),
///   * `--numstat -z` changed-path triples (NUL-terminated; the path is
///     the 3rd tab-field, so paths with spaces are exact), and
///   * the `-p --unified=0` patch (for the TODO/FIXME scan).
///
/// Rename numstat entries are expanded to both paths so per-file commit
/// counts remain correct while pure renames add no TODO churn.
///
/// We drop `--first-parent` so PR commits merged via "Create a merge
/// commit" are visible, and keep `--no-merges` so the merge commits
/// themselves don't appear (their author is whoever clicked merge; their
/// first-parent diff is the wrong shape for our file-change counters).
pub async fn walk_new_commits(
    handle: &RepoHandle,
    since_sha: Option<&str>,
) -> Result<Vec<CommitInfo>> {
    walk_new_commits_batched(handle, since_sha, LOG_BATCH_COMMITS).await
}

async fn walk_new_commits_batched(
    handle: &RepoHandle,
    since_sha: Option<&str>,
    batch_size: usize,
) -> Result<Vec<CommitInfo>> {
    let range = match since_sha {
        Some(sha) => format!("{sha}..HEAD"),
        None => "HEAD".to_string(),
    };

    let output = git_in(&handle.path)
        .args(["rev-list", "--reverse", "--no-merges", &range])
        .output()
        .await
        .context("git rev-list")?;
    if !output.status.success() {
        bail!(
            "git rev-list failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let sha_output = std::str::from_utf8(&output.stdout).context("git rev-list non-UTF-8")?;
    let shas: Vec<String> = sha_output
        .lines()
        .map(str::trim)
        .filter(|sha| !sha.is_empty())
        .map(str::to_string)
        .collect();
    validate_shas(&shas)?;

    let mut commits = Vec::with_capacity(shas.len());
    for batch in shas.chunks(batch_size.max(1)) {
        commits.extend(walk_commit_batch(handle, batch).await?);
    }
    Ok(commits)
}

/// Patch-level walk for the TODO/FIXME signal.
///
/// Non-text and generated paths are excluded (see [`NON_TEXT_PATHSPECS`]), so
/// the `paths_changed` this returns is deliberately narrower than the commit's
/// real path set. The per-file and per-author aggregates take their paths from
/// [`walk_commit_metadata_batch`], which applies no pathspec.
pub(crate) async fn walk_commit_batch(
    handle: &RepoHandle,
    shas: &[String],
) -> Result<Vec<CommitInfo>> {
    if shas.is_empty() {
        return Ok(Vec::new());
    }

    // %H sha · %P parent hashes · %ae author email · %an name · %aI
    // iso8601 · %s subject. `%P` identifies the root commit, whose content
    // contributes TODO deltas but no changed-path aggregate.
    let log_format = "%x00%x00GDCOMMIT%x00%H%x00%P%x00%ae%x00%an%x00%aI%x00%s%x00";
    let mut command = git_in(&handle.path);
    command.args([
        "log",
        "--no-walk=unsorted",
        "--numstat",
        "-z",
        "--unified=0",
        "-p",
        &format!("--format={log_format}"),
    ]);
    command.args(shas).arg("--");
    // Exclude paths that cannot carry a TODO before git diffs them. This is
    // not only noise control: on a `--filter=blob:none` clone `git log -p`
    // hydrates both sides of every changed file from the remote, so without
    // this a commit that touches a video or a generated bundle downloads it
    // in full to produce the line "Binary files ... differ".
    command.args(NON_TEXT_PATHSPECS);
    let output = command.output().await.context("batched git log")?;
    if !output.status.success() {
        bail!(
            "batched git log failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(parse_log_records(&output.stdout))
}

/// Read author, date, message, changed paths, and line movement without
/// materializing historical file bodies. `--numstat --no-renames` needs commit
/// trees but not blobs and preserves the old path-set contract for renames
/// (delete + add). Rename-only commits can therefore report line movement;
/// this is intentional and documented as Git numstat churn rather than edit
/// distance.
pub(crate) async fn walk_commit_metadata_batch(
    handle: &RepoHandle,
    shas: &[String],
) -> Result<Vec<CommitInfo>> {
    if shas.is_empty() {
        return Ok(Vec::new());
    }
    validate_shas(shas)?;
    let log_format = "%x00%x00GDCOMMIT%x00%H%x00%P%x00%ae%x00%an%x00%aI%x00%s%x00";
    let mut command = git_in(&handle.path);
    command.args([
        "log",
        "--no-walk=unsorted",
        "--numstat",
        "-z",
        "--no-renames",
        &format!("--format={log_format}"),
    ]);
    command.args(shas).arg("--");
    let output = command.output().await.context("batched metadata git log")?;
    if !output.status.success() {
        bail!(
            "batched metadata git log failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(parse_metadata_records(&output.stdout))
}

fn parse_metadata_records(stdout: &[u8]) -> Vec<CommitInfo> {
    split_on(stdout, COMMIT_SENTINEL)
        .into_iter()
        .filter(|record| !record.is_empty())
        .filter_map(parse_metadata_record)
        .collect()
}

fn parse_metadata_record(record: &[u8]) -> Option<CommitInfo> {
    let mut segments = record.split(|&byte| byte == 0);
    let sha = String::from_utf8_lossy(segments.next()?).trim().to_string();
    if sha.is_empty() {
        return None;
    }
    let is_root = String::from_utf8_lossy(segments.next()?).trim().is_empty();
    let author_email = String::from_utf8_lossy(segments.next()?).to_lowercase();
    let author_name = String::from_utf8_lossy(segments.next()?).to_string();
    let iso = String::from_utf8_lossy(segments.next()?).to_string();
    let message_first_line = String::from_utf8_lossy(segments.next()?).to_string();
    let committed_at = DateTime::parse_from_rfc3339(iso.trim())
        .map(|date| date.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let mut file_changes = Vec::new();
    if !is_root {
        for segment in segments {
            if segment.is_empty() {
                continue;
            }
            if let NumstatEntry::Path {
                path,
                lines_added,
                lines_deleted,
                binary,
            } = numstat_entry(segment)
            {
                file_changes.push(FileChange {
                    path,
                    lines_added,
                    lines_deleted,
                    binary,
                });
            }
        }
    }
    let paths_changed = file_changes
        .iter()
        .map(|change| change.path.clone())
        .collect();
    let (lines_added, lines_deleted, binary_files) = summarize_file_changes(&file_changes);

    Some(CommitInfo {
        sha,
        author_email,
        author_name,
        committed_day: committed_at.date_naive(),
        committed_at,
        is_fix: is_fix_message(&message_first_line),
        message_first_line,
        paths_changed,
        file_changes,
        lines_added,
        lines_deleted,
        binary_files,
        todo_added: 0,
        todo_removed: 0,
    })
}

fn validate_shas(shas: &[String]) -> Result<()> {
    if let Some(invalid) = shas.iter().find(|sha| {
        !(40..=64).contains(&sha.len()) || !sha.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) {
        bail!("git rev-list returned invalid object id: {invalid}");
    }
    Ok(())
}

/// Pure parser for the streamed `git log --numstat -z -p` output. Splits
/// the raw bytes on [`COMMIT_SENTINEL`] and decodes each commit's header,
/// changed paths, and TODO/FIXME deltas.
fn parse_log_records(stdout: &[u8]) -> Vec<CommitInfo> {
    let mut out = Vec::new();
    for record in split_on(stdout, COMMIT_SENTINEL) {
        if record.is_empty() {
            continue;
        }
        if let Some(info) = parse_one_record(record) {
            out.push(info);
        }
    }
    out
}

/// Parse a single commit record (the bytes between two sentinels).
/// Layout (NUL = `\0`):
///   `sha\0parents\0email\0name\0iso\0subject\0` `\0` `<numstat triples,
///   each \0>` `\0` `<patch bytes>`
/// i.e. NUL-splitting gives: \[sha, parents, email, name, iso, subject,
/// "", triples…, "", patch\]. The patch is the final segment and contains
/// no NULs (text diffs only; binary files render as "Binary files …
/// differ").
fn parse_one_record(record: &[u8]) -> Option<CommitInfo> {
    let mut segs = record.split(|&b| b == 0);
    let sha = String::from_utf8_lossy(segs.next()?).trim().to_string();
    if sha.is_empty() {
        return None;
    }
    // `%P`: space-separated parent hashes. Empty ⇒ root commit, for which
    // the OLD `diff-tree` (no `--root`) reported no changed paths.
    let parents = String::from_utf8_lossy(segs.next()?).trim().to_string();
    let is_root = parents.is_empty();
    let email = String::from_utf8_lossy(segs.next()?).to_lowercase();
    let name = String::from_utf8_lossy(segs.next()?).to_string();
    let iso = String::from_utf8_lossy(segs.next()?).to_string();
    let subject = String::from_utf8_lossy(segs.next()?).to_string();

    let committed_at = DateTime::parse_from_rfc3339(iso.trim())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let committed_day = committed_at.date_naive();
    let is_fix = is_fix_message(&subject);

    // After the subject's trailing format-NUL comes an empty segment
    // (seg[5]), then the numstat entries, then an empty separator, then the
    // patch (the final segment). A commit with no file changes has neither
    // numstat entries nor a patch.
    //
    // Numstat entry forms (each is one NUL segment unless noted):
    //   * normal:  `<add>\t<rem>\t<path>`            — path in the segment.
    //   * rename/copy (renames are ON): `<add>\t<rem>\t` with an EMPTY path
    //     field, followed by TWO extra NUL segments: <oldpath>, <newpath>.
    //     We push BOTH so the path set matches the OLD rename-OFF
    //     `diff-tree --name-only` (which listed add + delete). The first
    //     numstat entry carries a leading `\n` from the -z formatting.
    let mut file_changes = Vec::new();
    let mut patch: &[u8] = &[];
    // Skip the leading empty segment that always follows the subject NUL.
    let mut rest: Vec<&[u8]> = segs.collect();
    if rest.first().is_some_and(|s| s.is_empty()) {
        rest.remove(0);
    }
    let mut i = 0;
    while i < rest.len() {
        let seg = rest[i];
        if seg.is_empty() {
            // Empty separator → the patch is the next (final) segment.
            patch = rest.get(i + 1).copied().unwrap_or(&[]);
            break;
        }
        match numstat_entry(seg) {
            // Normal entry with an inline path.
            NumstatEntry::Path {
                path,
                lines_added,
                lines_deleted,
                binary,
            } => {
                if !is_root {
                    file_changes.push(FileChange {
                        path,
                        lines_added,
                        lines_deleted,
                        binary,
                    });
                }
                i += 1;
            }
            // Rename/copy: the old + new paths are the next two segments.
            NumstatEntry::RenamePair {
                lines_added,
                lines_deleted,
                binary,
            } => {
                if let (Some(old), Some(new)) = (rest.get(i + 1), rest.get(i + 2)) {
                    if !is_root {
                        let old = String::from_utf8_lossy(old).trim().to_string();
                        let new = String::from_utf8_lossy(new).trim().to_string();
                        if !old.is_empty() {
                            // Preserve the historical old+new touch set without
                            // double-counting the rename's numstat movement.
                            file_changes.push(FileChange {
                                path: old,
                                lines_added: 0,
                                lines_deleted: 0,
                                binary,
                            });
                        }
                        if !new.is_empty() {
                            file_changes.push(FileChange {
                                path: new,
                                lines_added,
                                lines_deleted,
                                binary,
                            });
                        }
                    }
                    i += 3; // entry + two path segments
                } else {
                    // Malformed (truncated) — stop consuming entries.
                    i += 1;
                }
            }
            // Not a recognizable numstat entry (defensive) — skip it.
            NumstatEntry::Skip => {
                i += 1;
            }
        }
    }

    // TODO deltas are scanned from the patch unconditionally — including
    // the root commit, whose full content the OLD `git show` path scanned.
    let (todo_added, todo_removed) = scan_todos(patch);
    let paths_changed = file_changes
        .iter()
        .map(|change| change.path.clone())
        .collect();
    let (lines_added, lines_deleted, binary_files) = summarize_file_changes(&file_changes);

    Some(CommitInfo {
        sha,
        author_email: email,
        author_name: name,
        committed_at,
        committed_day,
        message_first_line: subject,
        is_fix,
        paths_changed,
        file_changes,
        lines_added,
        lines_deleted,
        binary_files,
        todo_added,
        todo_removed,
    })
}

/// Classification of one `--numstat -z` segment.
enum NumstatEntry {
    /// `<add>\t<rem>\t<path>` — a normal change with an inline path.
    Path {
        path: String,
        lines_added: u64,
        lines_deleted: u64,
        binary: bool,
    },
    /// `<add>\t<rem>\t` with an empty path field — a rename/copy whose old
    /// and new paths are the next two NUL segments.
    RenamePair {
        lines_added: u64,
        lines_deleted: u64,
        binary: bool,
    },
    /// Not a recognizable numstat entry (no two tabs) — ignore defensively.
    Skip,
}

/// Classify a `--numstat -z` segment. A normal entry is
/// `<added>\t<removed>\t<path>` (the first entry of a commit carries a
/// leading `\n` from -z mode, trimmed off the path). A rename/copy entry
/// has an *empty* path field (`<add>\t<rem>\t`) and the old+new paths
/// follow as separate NUL segments. We require exactly two tabs; anything
/// else is treated as `Skip` so a stray segment can't be misread as a path
/// (mirrors the old `diff-tree --name-only` line-`trim()` semantics).
fn numstat_entry(seg: &[u8]) -> NumstatEntry {
    let s = String::from_utf8_lossy(seg);
    let mut it = s.splitn(3, '\t');
    let Some(added) = it.next() else {
        return NumstatEntry::Skip;
    };
    let Some(removed) = it.next() else {
        return NumstatEntry::Skip;
    };
    let Some(path) = it.next() else {
        // Fewer than two tabs ⇒ not a numstat entry.
        return NumstatEntry::Skip;
    };
    let added = added.trim_start_matches(['\n', '\r']);
    let binary = added == "-" || removed == "-";
    let lines_added = if binary {
        0
    } else {
        added.parse().unwrap_or(0)
    };
    let lines_deleted = if binary {
        0
    } else {
        removed.parse().unwrap_or(0)
    };
    let path = path.trim();
    if path.is_empty() {
        NumstatEntry::RenamePair {
            lines_added,
            lines_deleted,
            binary,
        }
    } else {
        NumstatEntry::Path {
            path: path.to_string(),
            lines_added,
            lines_deleted,
            binary,
        }
    }
}

fn summarize_file_changes(changes: &[FileChange]) -> (u64, u64, u32) {
    changes.iter().fold((0, 0, 0), |acc, change| {
        (
            acc.0.saturating_add(change.lines_added),
            acc.1.saturating_add(change.lines_deleted),
            acc.2.saturating_add(u32::from(change.binary)),
        )
    })
}

/// Count TODO/FIXME additions/removals in a `-p --unified=0` patch body,
/// applying the same 4 MB cap and UTF-8 fallback as the old per-`git show`
/// path so the per-commit deltas are byte-for-byte unchanged: cap the
/// bytes, decode with `from_utf8(...).unwrap_or("")` (an invalid-UTF-8
/// patch counts as zero), then scan `+`/`-` lines (skipping `+++`/`---`
/// diff headers).
fn scan_todos(patch: &[u8]) -> (u32, u32) {
    let bytes = if patch.len() > MAX_PATCH_BYTES {
        &patch[..MAX_PATCH_BYTES]
    } else {
        patch
    };
    // Lossy, not strict: a patch that touches one Latin-1 or UTF-16 source
    // file alongside twenty UTF-8 ones is not UTF-8 as a whole, and treating
    // that as "no TODO churn in this commit" silently zeroed the signal for
    // every other file in the same commit.
    let text = String::from_utf8_lossy(bytes);
    let text = text.as_ref();
    let (mut todo_added, mut todo_removed) = (0u32, 0u32);
    for line in text.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        if let Some(rest) = line.strip_prefix('+') {
            todo_added = todo_added.saturating_add(count_todo_words(rest));
        } else if let Some(rest) = line.strip_prefix('-') {
            todo_removed = todo_removed.saturating_add(count_todo_words(rest));
        }
    }
    (todo_added, todo_removed)
}

/// Split a byte slice on a multi-byte separator, yielding the slices
/// between separators (like `str::split` but for `&[u8]`). Used to cut the
/// `git log` stream into per-commit records on [`COMMIT_SENTINEL`].
fn split_on<'a>(haystack: &'a [u8], sep: &[u8]) -> Vec<&'a [u8]> {
    if sep.is_empty() {
        return vec![haystack];
    }
    let mut parts = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i + sep.len() <= haystack.len() {
        if &haystack[i..i + sep.len()] == sep {
            parts.push(&haystack[start..i]);
            i += sep.len();
            start = i;
        } else {
            i += 1;
        }
    }
    parts.push(&haystack[start..]);
    parts
}

fn count_todo_words(s: &str) -> u32 {
    let mut n = 0u32;
    for needle in ["TODO", "FIXME"] {
        let mut start = 0usize;
        while let Some(idx) = s[start..].find(needle) {
            let abs = start + idx;
            let before_ok = abs
                .checked_sub(1)
                .and_then(|i| s.as_bytes().get(i).copied())
                .map(|b| !b.is_ascii_alphanumeric())
                .unwrap_or(true);
            let after_ok = s
                .as_bytes()
                .get(abs + needle.len())
                .copied()
                .map(|b| !b.is_ascii_alphanumeric())
                .unwrap_or(true);
            if before_ok && after_ok {
                n = n.saturating_add(1);
            }
            start = abs + needle.len();
        }
    }
    n
}

fn is_fix_message(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    for needle in ["fix", "bug", "hotfix", "patch"] {
        // Every occurrence, not just the first: `find` returning a hit that
        // happens to sit inside a longer word ("prefix", "debug") classified
        // the whole subject as not-a-fix even when a standalone occurrence
        // followed it. This is the only input to the bug-magnet chart.
        let mut start = 0;
        while let Some(offset) = lower[start..].find(needle) {
            let idx = start + offset;
            let before_ok = idx
                .checked_sub(1)
                .and_then(|i| lower.as_bytes().get(i).copied())
                .map(|b| !(b as char).is_ascii_alphanumeric())
                .unwrap_or(true);
            let after_ok = lower
                .as_bytes()
                .get(idx + needle.len())
                .copied()
                .map(|b| !(b as char).is_ascii_alphanumeric())
                .unwrap_or(true);
            if before_ok && after_ok {
                return true;
            }
            start = idx + needle.len();
        }
    }
    false
}

/// On-disk size of a clone in bytes. Called after each analysis run for
/// the eviction scorer.
///
/// We sum the `objects/pack/` directory (the packfiles + their `.idx`,
/// plus any `.rev`/`.mtimes` siblings) instead of a full recursive
/// `walkdir` of the whole `.git`. For a bare clone essentially all of the
/// on-disk bytes live in the packs — refs, config, and the commit-graph
/// are kilobytes — so this is within a rounding error of the true size at
/// a fraction of the syscalls (one shallow `read_dir`, not a recursive
/// stat of every loose object). Lazily-backfilled loose blobs (from
/// `git show`/`git archive` on a blobless clone) are added in too via the
/// `objects/<xx>/` shards, keeping the scorer honest after a HEAD
/// materialization. The eviction scorer only needs a relative ranking, so
/// approximate-but-cheap is the right trade.
pub fn clone_size_bytes(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    let objects = path.join("objects");
    let mut total = 0u64;
    // Packed objects: the bulk of a bare clone.
    if let Ok(read) = std::fs::read_dir(objects.join("pack")) {
        for entry in read.flatten() {
            if let Ok(meta) = entry.metadata()
                && meta.is_file()
            {
                total = total.saturating_add(meta.len());
            }
        }
    }
    // Loose objects backfilled on demand. Each lives in a two-hex-char
    // shard dir; sum the files one shard deep (loose objects are never
    // nested deeper than that). Cheap relative to a full recursive walk.
    if let Ok(shards) = std::fs::read_dir(&objects) {
        for shard in shards.flatten() {
            let name = shard.file_name();
            let name = name.to_string_lossy();
            // Skip `pack` (counted above) and `info`; only 2-hex shard dirs
            // hold loose objects.
            if name.len() != 2 || !name.bytes().all(|b| b.is_ascii_hexdigit()) {
                continue;
            }
            if let Ok(files) = std::fs::read_dir(shard.path()) {
                for f in files.flatten() {
                    if let Ok(meta) = f.metadata()
                        && meta.is_file()
                    {
                        total = total.saturating_add(meta.len());
                    }
                }
            }
        }
    }
    total
}

/// Recursively delete the bare clone for an evicted repo. Caller is
/// responsible for null-ing out `clone_path` and `clone_size_bytes` in
/// `repo_history`.
pub async fn evict_clone(path: &Path) -> Result<()> {
    if path.exists() {
        tokio::fs::remove_dir_all(path)
            .await
            .with_context(|| format!("remove clone at {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A keyword buried in a longer word must not hide a real one later in
    /// the same subject.
    #[test]
    fn fix_detection_scans_every_occurrence() {
        assert!(is_fix_message("prefix rewrite: fix the parser"));
        assert!(is_fix_message("debug tooling and bug repro"));
        assert!(!is_fix_message("prefix rewrite for the debugger"));
        assert!(is_fix_message("fix: off-by-one"));
    }

    #[test]
    fn fix_message_word_boundary() {
        assert!(is_fix_message("fix: typo"));
        assert!(is_fix_message("Fix logging"));
        assert!(is_fix_message("Hotfix release"));
        assert!(is_fix_message("revert: bug in parser"));
        assert!(!is_fix_message("prefix the api"));
        assert!(!is_fix_message("affix the label"));
    }

    #[test]
    fn count_todo_words_basic() {
        assert_eq!(count_todo_words("// TODO: fix this"), 1);
        assert_eq!(count_todo_words("// FIXME later TODO maybe"), 2);
        assert_eq!(count_todo_words("prefix should not match"), 0);
        assert_eq!(count_todo_words(""), 0);
    }

    #[test]
    fn only_a_disappeared_remote_branch_discards_the_cached_clone() {
        let missing =
            anyhow::anyhow!("git fetch failed: fatal: couldn't find remote ref refs/heads/main");
        let transient = anyhow::anyhow!("git fetch failed: connection reset by peer");
        assert!(fetch_requires_reclone(&missing));
        assert!(!fetch_requires_reclone(&transient));
    }

    #[tokio::test]
    async fn empty_repository_completes_with_zero_history() {
        if std::process::Command::new("git")
            .arg("--version")
            .status()
            .is_err()
        {
            eprintln!("skipping: git not available");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(tmp.path())
            .args(["init", "--bare", "-q"])
            .status()
            .unwrap();
        assert!(status.success());

        let head_sha = rev_parse_head(tmp.path()).await.unwrap();
        assert_eq!(head_sha, EMPTY_REPOSITORY_HEAD);
        let handle = RepoHandle {
            path: tmp.path().to_path_buf(),
            head_sha,
        };
        let plan = plan_recent_commits(&handle, None, 5_000).await.unwrap();
        assert!(plan.shas.is_empty());
        assert!(!plan.truncated);
        assert_eq!(reachable_commit_count(&handle).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn repository_cache_requires_the_current_explicit_format() {
        let tmp = tempfile::tempdir().unwrap();
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(tmp.path())
            .args(["init", "--bare", "-q"])
            .status()
            .unwrap();
        assert!(status.success());
        assert!(!cache_format_is_current(tmp.path()).await);

        let stale = std::process::Command::new("git")
            .arg("-C")
            .arg(tmp.path())
            .args(["config", "gitdebt.cacheFormat", "1"])
            .status()
            .unwrap();
        assert!(stale.success());
        assert!(!cache_format_is_current(tmp.path()).await);

        let current = std::process::Command::new("git")
            .arg("-C")
            .arg(tmp.path())
            .args(["config", "gitdebt.cacheFormat", CACHE_FORMAT_VERSION])
            .status()
            .unwrap();
        assert!(current.success());
        assert!(cache_format_is_current(tmp.path()).await);
    }

    // #3 streaming-parser equivalence tests.

    /// Build the exact byte stream `git log --numstat -z --unified=0 -p
    /// --format='<sentinel + header>'` produces for one commit, so the pure
    /// parser can be exercised without a git binary. `triples` are
    /// `(added, removed, path)` numstat rows; `patch` is the raw patch body.
    fn record(
        sha: &str,
        email: &str,
        name: &str,
        iso: &str,
        subject: &str,
        triples: &[(&str, &str, &str)],
        patch: &str,
    ) -> Vec<u8> {
        // Default to a non-root commit (a synthetic single parent) so paths
        // are emitted; the root-commit case is covered separately.
        record_with_parent(sha, "p0", email, name, iso, subject, triples, patch)
    }

    #[allow(clippy::too_many_arguments)]
    fn record_with_parent(
        sha: &str,
        parents: &str,
        email: &str,
        name: &str,
        iso: &str,
        subject: &str,
        triples: &[(&str, &str, &str)],
        patch: &str,
    ) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(COMMIT_SENTINEL);
        // header: sha\0parents\0email\0name\0iso\0subject\0
        for field in [sha, parents, email, name, iso, subject] {
            v.extend_from_slice(field.as_bytes());
            v.push(0);
        }
        // numstat block: leading empty seg (the \0 we just pushed acts as
        // the boundary), then \0, then each triple \0-terminated. The first
        // triple carries a leading \n in real output; replicate it.
        v.push(0); // empty separator segment after the subject's trailing NUL
        for (i, (a, r, p)) in triples.iter().enumerate() {
            if i == 0 {
                v.push(b'\n');
            }
            v.extend_from_slice(format!("{a}\t{r}\t{p}").as_bytes());
            v.push(0);
        }
        // separator before patch, then the patch body (no trailing NUL).
        v.push(0);
        v.extend_from_slice(patch.as_bytes());
        v
    }

    #[test]
    fn parser_basic_single_commit() {
        let patch = "diff --git a/a.txt b/a.txt\n\
            new file mode 100644\n\
            --- /dev/null\n\
            +++ b/a.txt\n\
            @@ -0,0 +1,2 @@\n\
            +line1\n\
            +// TODO: x\n";
        let stream = record(
            "abc123",
            "Alice@Example.com",
            "Alice",
            "2021-01-01T00:00:00+00:00",
            "init",
            &[("2", "0", "a.txt")],
            patch,
        );
        let parsed = parse_log_records(&stream);
        assert_eq!(parsed.len(), 1);
        let c = &parsed[0];
        assert_eq!(c.sha, "abc123");
        // email is lowercased, matching the old %ae .to_lowercase().
        assert_eq!(c.author_email, "alice@example.com");
        assert_eq!(c.author_name, "Alice");
        assert_eq!(c.message_first_line, "init");
        assert!(!c.is_fix);
        assert_eq!(c.paths_changed, vec!["a.txt"]);
        assert_eq!(c.lines_added, 2);
        assert_eq!(c.lines_deleted, 0);
        assert_eq!(c.binary_files, 0);
        assert_eq!(c.file_changes.len(), 1);
        assert_eq!(c.todo_added, 1);
        assert_eq!(c.todo_removed, 0);
        assert_eq!(c.committed_day.to_string(), "2021-01-01");
    }

    #[test]
    fn parser_multi_file_binary_and_spaces_and_fix() {
        // A "fix" commit touching a binary file (numstat `-\t-`), a text
        // file, and a path with a space. Patch removes one TODO and adds a
        // FIXME. Binary file must still appear in paths_changed (matches the
        // old diff-tree --name-only, which lists binaries).
        let patch = "diff --git a/bin.dat b/bin.dat\n\
            Binary files /dev/null and b/bin.dat differ\n\
            diff --git a/t.txt b/t.txt\n\
            --- a/t.txt\n\
            +++ b/t.txt\n\
            @@ -1 +1 @@\n\
            -old TODO line\n\
            +new FIXME line\n";
        let stream = record(
            "def456",
            "bob@example.com",
            "Bob",
            "2021-02-03T04:05:06+00:00",
            "fix: stuff",
            &[
                ("-", "-", "bin.dat"),
                ("1", "1", "t.txt"),
                ("1", "0", "with space.txt"),
            ],
            patch,
        );
        let parsed = parse_log_records(&stream);
        assert_eq!(parsed.len(), 1);
        let c = &parsed[0];
        assert!(c.is_fix, "subject 'fix: stuff' is a fix");
        assert_eq!(
            c.paths_changed,
            vec!["bin.dat", "t.txt", "with space.txt"],
            "binary + spaced paths preserved exactly"
        );
        assert_eq!(c.lines_added, 2);
        assert_eq!(c.lines_deleted, 1);
        assert_eq!(c.binary_files, 1);
        assert!(
            c.file_changes
                .iter()
                .any(|change| change.path == "bin.dat" && change.binary)
        );
        assert_eq!(c.todo_added, 1, "one FIXME added");
        assert_eq!(c.todo_removed, 1, "one TODO removed");
    }

    #[test]
    fn parser_multiple_commits_in_one_stream() {
        let mut stream = Vec::new();
        stream.extend_from_slice(&record(
            "c1",
            "a@b.c",
            "A",
            "2021-01-01T00:00:00+00:00",
            "init",
            &[("1", "0", "f.txt")],
            "diff --git a/f.txt b/f.txt\n+++ b/f.txt\n@@ -0,0 +1 @@\n+// TODO one\n",
        ));
        stream.extend_from_slice(&record(
            "c2",
            "a@b.c",
            "A",
            "2021-01-02T00:00:00+00:00",
            "bug: squash",
            &[("0", "1", "f.txt")],
            "diff --git a/f.txt b/f.txt\n--- a/f.txt\n@@ -1 +0,0 @@\n-// TODO one\n",
        ));
        let parsed = parse_log_records(&stream);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].sha, "c1");
        assert_eq!(parsed[0].todo_added, 1);
        assert_eq!(parsed[0].todo_removed, 0);
        assert_eq!(parsed[1].sha, "c2");
        assert!(parsed[1].is_fix, "'bug:' is a fix");
        assert_eq!(parsed[1].todo_added, 0);
        assert_eq!(parsed[1].todo_removed, 1);
    }

    #[test]
    fn parser_skips_plusplusplus_and_minusminusminus_headers() {
        // `+++ b/x` and `--- a/x` must NOT count even though they start with
        // +/- and could contain the word TODO in a path. Matches the old
        // header-skip rule exactly.
        let patch = "diff --git a/TODO b/TODO\n\
            --- a/TODO\n\
            +++ b/TODO\n\
            @@ -1 +1 @@\n\
            -gone\n\
            +here\n";
        let stream = record(
            "h1",
            "a@b.c",
            "A",
            "2021-01-01T00:00:00+00:00",
            "edit",
            &[("1", "1", "TODO")],
            patch,
        );
        let parsed = parse_log_records(&stream);
        assert_eq!(parsed[0].todo_added, 0, "+++ b/TODO header not counted");
        assert_eq!(parsed[0].todo_removed, 0, "--- a/TODO header not counted");
        assert_eq!(parsed[0].paths_changed, vec!["TODO"]);
    }

    #[test]
    fn parser_empty_commit_has_no_paths_or_todos() {
        // A commit with no file changes: header (with a parent), then the
        // empty numstat + empty patch. Must yield empty paths and zero todos.
        let mut v = Vec::new();
        v.extend_from_slice(COMMIT_SENTINEL);
        for field in [
            "e1",
            "p0",
            "a@b.c",
            "A",
            "2021-01-01T00:00:00+00:00",
            "empty",
        ] {
            v.extend_from_slice(field.as_bytes());
            v.push(0);
        }
        // empty numstat block + empty patch separator.
        v.push(0);
        let parsed = parse_log_records(&v);
        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].paths_changed.is_empty());
        assert_eq!(parsed[0].todo_added, 0);
        assert_eq!(parsed[0].todo_removed, 0);
    }

    #[test]
    fn parser_rename_entry_yields_both_old_and_new_paths() {
        // A rename numstat entry (renames ON) is `<add>\t<rem>\t` with an
        // EMPTY path field, followed by two NUL segments: oldpath, newpath.
        // Both must land in paths_changed (matching the old rename-OFF
        // diff-tree, which listed add + delete). Build the bytes by hand to
        // mirror the real `-z` layout: header, empty seg, an extra-add
        // entry, the rename entry + its two path segments, separator, patch.
        let mut v = Vec::new();
        v.extend_from_slice(COMMIT_SENTINEL);
        for field in [
            "r1",
            "p0",
            "a@b.c",
            "A",
            "2021-01-01T00:00:00+00:00",
            "rename + add",
        ] {
            v.extend_from_slice(field.as_bytes());
            v.push(0);
        }
        v.push(0); // empty seg after subject NUL
        // normal entry (first one carries a leading \n)
        v.extend_from_slice(b"\n1\t0\textra.txt");
        v.push(0);
        // rename entry: empty path field, then two path segments
        v.extend_from_slice(b"1\t0\t");
        v.push(0);
        v.extend_from_slice(b"old/name.txt");
        v.push(0);
        v.extend_from_slice(b"new/name.txt");
        v.push(0);
        // patch separator + a pure-rename patch (no +/- content lines)
        v.push(0);
        v.extend_from_slice(
            b"diff --git a/old/name.txt b/new/name.txt\nsimilarity index 100%\nrename from old/name.txt\nrename to new/name.txt\n",
        );
        let parsed = parse_log_records(&v);
        assert_eq!(parsed.len(), 1);
        let mut got = parsed[0].paths_changed.clone();
        got.sort();
        assert_eq!(
            got,
            vec![
                "extra.txt".to_string(),
                "new/name.txt".to_string(),
                "old/name.txt".to_string()
            ],
            "rename lists both old + new paths plus the normal add"
        );
        // A pure rename has no +/- content lines → no TODO churn (matches
        // old `git show` with rename detection on).
        assert_eq!(parsed[0].todo_added, 0);
        assert_eq!(parsed[0].todo_removed, 0);
    }

    #[test]
    fn parser_root_commit_suppresses_paths_but_keeps_todos() {
        // The root commit (empty `%P`) must contribute NO changed paths
        // (matching the OLD diff-tree without --root) yet still count its
        // TODO deltas (matching the OLD git show, which showed root content).
        let patch = "diff --git a/a.txt b/a.txt\n\
            new file mode 100644\n\
            --- /dev/null\n\
            +++ b/a.txt\n\
            @@ -0,0 +1,2 @@\n\
            +line1\n\
            +// TODO: x\n";
        let stream = record_with_parent(
            "root1",
            "",
            "a@b.c",
            "A",
            "2021-01-01T00:00:00+00:00",
            "init",
            &[("2", "0", "a.txt")],
            patch,
        );
        let parsed = parse_log_records(&stream);
        assert_eq!(parsed.len(), 1);
        assert!(
            parsed[0].paths_changed.is_empty(),
            "root commit emits no paths (matches old diff-tree)"
        );
        assert_eq!(parsed[0].todo_added, 1, "root commit TODOs still counted");
        assert_eq!(parsed[0].todo_removed, 0);
    }

    // #3 end-to-end equivalence against the OLD per-commit commands.
    //
    // Builds a real git repo, then compares the bounded batched walker
    // against an oracle that reproduces the OLD code path
    // exactly: `git log` for the sha list + per-commit `git diff-tree
    // --name-only` (paths) + `git show --unified=0 --format=` (TODO scan,
    // 4 MB cap). Asserts identical per-file, per-day, fix, and TODO
    // aggregates. Skipped (not failed) if `git` is unavailable.

    #[cfg(test)]
    mod equivalence {
        use super::*;
        use std::process::Command as SyncCommand;

        fn git_available() -> bool {
            SyncCommand::new("git")
                .arg("--version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }

        fn run(dir: &Path, args: &[&str]) {
            let status = SyncCommand::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .status()
                .expect("spawn git");
            assert!(status.success(), "git {args:?} failed");
        }

        fn commit(dir: &Path, msg: &str, date: &str) {
            run(dir, &["add", "-A"]);
            let status = SyncCommand::new("git")
                .arg("-C")
                .arg(dir)
                .args(["commit", "-q", "-m", msg, "--date", date])
                .env("GIT_COMMITTER_DATE", date)
                .status()
                .expect("spawn git commit");
            assert!(status.success(), "git commit failed");
        }

        /// The OLD code path, reproduced exactly for one commit: paths from
        /// `diff-tree --no-commit-id --name-only -r`, TODO deltas from
        /// `git show --no-color --unified=0 --format=` capped at 4 MB.
        fn old_paths_and_todos(dir: &Path, sha: &str) -> (Vec<String>, u32, u32) {
            let paths_out = SyncCommand::new("git")
                .arg("-C")
                .arg(dir)
                .args(["diff-tree", "--no-commit-id", "--name-only", "-r", sha])
                .output()
                .unwrap();
            let mut paths = Vec::new();
            for line in String::from_utf8_lossy(&paths_out.stdout).lines() {
                let p = line.trim();
                if !p.is_empty() {
                    paths.push(p.to_string());
                }
            }
            let show_out = SyncCommand::new("git")
                .arg("-C")
                .arg(dir)
                .args(["show", "--no-color", "--unified=0", "--format=", sha])
                .output()
                .unwrap();
            let bytes = if show_out.stdout.len() > MAX_PATCH_BYTES {
                &show_out.stdout[..MAX_PATCH_BYTES]
            } else {
                &show_out.stdout[..]
            };
            let text = std::str::from_utf8(bytes).unwrap_or("");
            let (mut added, mut removed) = (0u32, 0u32);
            for line in text.lines() {
                if line.starts_with("+++") || line.starts_with("---") {
                    continue;
                }
                if let Some(rest) = line.strip_prefix('+') {
                    added = added.saturating_add(count_todo_words(rest));
                } else if let Some(rest) = line.strip_prefix('-') {
                    removed = removed.saturating_add(count_todo_words(rest));
                }
            }
            (paths, added, removed)
        }

        #[tokio::test]
        async fn new_walk_matches_old_per_commit_path() {
            if !git_available() {
                eprintln!("skipping: git not available");
                return;
            }
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path();
            run(dir, &["init", "-q"]);
            run(dir, &["config", "user.email", "a@example.com"]);
            run(dir, &["config", "user.name", "Alice"]);
            run(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);

            // Commit 1: add a text file with a TODO.
            std::fs::write(dir.join("a.txt"), "line1\n// TODO: x\n").unwrap();
            commit(dir, "init", "2021-01-01T00:00:00");
            // Commit 2 ("fix:"): add a FIXME + a second file.
            std::fs::write(dir.join("a.txt"), "line1\nFIXME here\n// TODO: x\n").unwrap();
            std::fs::write(dir.join("b.txt"), "new\n").unwrap();
            commit(dir, "fix: stuff", "2021-01-02T00:00:00");
            // Commit 3: rename a.txt -> c.txt (no -M: shows as add+delete).
            std::fs::rename(dir.join("a.txt"), dir.join("c.txt")).unwrap();
            commit(dir, "rename a to c", "2021-01-03T00:00:00");
            // Commit 4: a binary file + a text file with a TODO, same commit.
            std::fs::write(dir.join("bin.dat"), [0u8, 1, 2, 3, 0, 9]).unwrap();
            std::fs::write(dir.join("t.txt"), "text TODO\n").unwrap();
            commit(dir, "add binary and text", "2021-01-04T00:00:00");
            // Commit 5: a path with a space + remove a TODO.
            std::fs::write(dir.join("with space.txt"), "spacey\n").unwrap();
            std::fs::write(dir.join("c.txt"), "line1\nFIXME here\n").unwrap(); // drop the TODO line
            commit(dir, "file with space", "2021-01-05T00:00:00");
            // Commit 6: a pure rename (rename detection ON in git show →
            // no +/- lines → no TODO churn; diff-tree without -M lists both
            // paths). Exercises the rename-pair numstat decode.
            std::fs::rename(dir.join("c.txt"), dir.join("renamed-c.txt")).unwrap();
            commit(dir, "rename c to renamed-c", "2021-01-06T00:00:00");
            // Commit 7: rename WITH modification (still detected as a rename
            // when similar enough) + an independent file add with a TODO.
            std::fs::write(
                dir.join("renamed-c.txt"),
                "line1\nFIXME here\nextra body line\n",
            )
            .unwrap();
            std::fs::rename(dir.join("renamed-c.txt"), dir.join("final-c.txt")).unwrap();
            std::fs::write(dir.join("added.txt"), "// TODO new\n").unwrap();
            commit(dir, "rename+modify and add", "2021-01-07T00:00:00");

            let head = SyncCommand::new("git")
                .arg("-C")
                .arg(dir)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap();
            let head_sha = String::from_utf8_lossy(&head.stdout).trim().to_string();
            let handle = RepoHandle {
                path: dir.to_path_buf(),
                head_sha,
            };

            // Force several subprocess batches so order and aggregate
            // equivalence are covered without creating 500+ fixtures.
            let new_commits = walk_new_commits_batched(&handle, None, 2).await.unwrap();

            let recent = plan_recent_commits(&handle, None, 3).await.unwrap();
            assert_eq!(recent.shas.len(), 3);
            assert!(recent.truncated);
            assert_eq!(recent.shas.last(), Some(&handle.head_sha));
            let complete = plan_recent_commits(&handle, None, 20).await.unwrap();
            assert_eq!(complete.shas.len(), 7);
            assert!(!complete.truncated);

            // OLD oracle: sha list (oldest-first, no-merges) then per-commit.
            let log_out = SyncCommand::new("git")
                .arg("-C")
                .arg(dir)
                .args(["log", "--reverse", "--no-merges", "--format=%H"])
                .output()
                .unwrap();
            let shas: Vec<String> = String::from_utf8_lossy(&log_out.stdout)
                .lines()
                .map(|s| s.to_string())
                .collect();

            let metadata = walk_commit_metadata_batch(&handle, &shas).await.unwrap();
            assert_eq!(metadata.len(), new_commits.len());
            let mut saw_binary_in_metadata = false;
            for (fast, complete) in metadata.iter().zip(new_commits.iter()) {
                assert_eq!(fast.sha, complete.sha);
                assert_eq!(fast.author_email, complete.author_email);
                assert_eq!(fast.author_name, complete.author_name);
                assert_eq!(fast.committed_at, complete.committed_at);
                assert_eq!(fast.message_first_line, complete.message_first_line);
                assert_eq!(fast.is_fix, complete.is_fix);
                // The metadata walk sees every changed path; the patch walk
                // excludes non-text paths (it exists only to scan diffs for
                // TODO markers, and diffing a binary on a partial clone means
                // downloading it). So the patch walk's path set is a subset.
                let mut fast_paths = fast.paths_changed.clone();
                let mut complete_paths = complete.paths_changed.clone();
                fast_paths.sort();
                complete_paths.sort();
                assert!(
                    complete_paths.iter().all(|path| fast_paths.contains(path)),
                    "patch-walk paths {complete_paths:?} must be a subset of {fast_paths:?}"
                );
                assert!(
                    !complete_paths.iter().any(|path| path.ends_with(".dat")),
                    "non-text paths are excluded from the patch walk"
                );
                saw_binary_in_metadata |= fast_paths.iter().any(|path| path.ends_with(".dat"));
                assert_eq!((fast.todo_added, fast.todo_removed), (0, 0));
            }

            assert!(
                saw_binary_in_metadata,
                "the metadata walk still reports non-text changed paths"
            );

            assert_eq!(
                new_commits.len(),
                shas.len(),
                "same number of commits as the old log walk"
            );
            for (new, sha) in new_commits.iter().zip(shas.iter()) {
                assert_eq!(&new.sha, sha, "commit order matches (oldest-first)");
                let (old_paths, old_add, old_rem) = old_paths_and_todos(dir, sha);
                // The per-file aggregate depends only on the path SET (each
                // path gets +1 commit; `apply_commits` is order-independent),
                // so compare sorted sets: rename detection (ON in the new
                // single pass to match the old `git show` TODO scan) lists a
                // rename's old+new paths in a different order than the old
                // rename-OFF `diff-tree`, but the SET is identical.
                let mut new_sorted = new.paths_changed.clone();
                new_sorted.sort();
                let mut old_sorted: Vec<String> = old_paths
                    .iter()
                    .filter(|path| !path.ends_with(".dat"))
                    .cloned()
                    .collect();
                old_sorted.sort();
                assert_eq!(
                    new_sorted, old_sorted,
                    "path set for {sha} matches old diff-tree --name-only, \
                     minus the non-text paths the patch walk excludes"
                );
                assert_eq!(new.todo_added, old_add, "todo_added for {sha}");
                assert_eq!(new.todo_removed, old_rem, "todo_removed for {sha}");
            }

            // Spot-check the derived aggregates that feed apply_commits.
            // Commit 2 is the only "fix" commit.
            let fix_count = new_commits.iter().filter(|c| c.is_fix).count();
            assert_eq!(fix_count, 1, "exactly one fix commit");
            // The patch walk skips the binary and keeps the text file it was
            // committed with, so a commit that mixes the two still yields its
            // real TODO churn without downloading the binary.
            let c4 = new_commits
                .iter()
                .find(|c| c.message_first_line == "add binary and text")
                .unwrap();
            assert!(!c4.paths_changed.contains(&"bin.dat".to_string()));
            assert!(c4.paths_changed.contains(&"t.txt".to_string()));
            // Rename commit shows BOTH a.txt and c.txt (no rename detection).
            let c3 = new_commits
                .iter()
                .find(|c| c.message_first_line == "rename a to c")
                .unwrap();
            assert!(c3.paths_changed.contains(&"a.txt".to_string()));
            assert!(c3.paths_changed.contains(&"c.txt".to_string()));
        }
    }
}
