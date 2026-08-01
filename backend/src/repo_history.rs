//! Local bare-clone management + incremental commit-history walking.
//!
//! Storage layout: `<REPOS_DIR>/<owner>/<repo>.git` (bare, complete). Default
//! `REPOS_DIR=~/.cache/gitdebt/repos` for dev; set to your mounted volume
//! path for container deployments. Disk usage is tracked in
//! `repo_history.clone_size_bytes` and trimmed via `evict_to_quota`.
//!
//! Clones carry every object, and that single decision is the performance
//! story of this module. `--filter=blob:none` makes the *clone* cheaper and
//! everything after it ruinous: `--numstat`, which every churn, ownership and
//! cadence signal is derived from, can only be computed by diffing blob
//! content, so a blobless clone re-buys the very bytes it skipped one promisor
//! round trip at a time. Measured on this host: hexo (3,751 commits) clones
//! complete in 10.6 s and walks its entire history in 0.41 s, against 55 s for
//! the filtered pipeline locally and 132 s in production; django (34,838
//! commits) clones complete in 134 s and walks all of it in 15.2 s, against a
//! filtered run that burned a twenty-minute budget without finishing.
//!
//! A commit is therefore not the expensive dimension — 0.44 ms of local walk
//! each — and there is no cap on how many of them an analysis covers. The
//! clone is paid once; later runs fetch only new objects and walk only the
//! commits between the stored cursor and the new head.

use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// Patch bodies are substantially more expensive than commit metadata: `-p`
/// makes git generate, and this process decode, the full text of every diff
/// rather than three integers per file. Contributor, cadence, churn and fix
/// signals cover every commit; TODO/FIXME churn is one auxiliary marker count
/// measured over the newest commits so it cannot hold the primary analysis
/// hostage. This bounds one signal's *input*, never the history that is
/// analyzed.
pub(crate) const TODO_PATCH_COMMIT_LIMIT: usize = 100;
/// Stable cursor for a valid repository whose default branch has no commits.
/// It cannot collide with a Git object id and lets the normal freshness/cache
/// contract complete instead of retrying an empty repository forever.
pub(crate) const EMPTY_REPOSITORY_HEAD: &str = "empty-repository";
/// Bumped from `2` when the blob filter was dropped. Every cache built by an
/// older release is a partial clone whose historical blobs are promisor stubs,
/// and relaxing a filter on a later fetch does not reliably backfill them, so
/// those caches are rebuilt once instead of being walked forever at network
/// speed.
const CACHE_FORMAT_VERSION: &str = "3";

/// Wall-clock ceilings, all env-tunable, with defaults sized for a
/// 12 vCPU / 32 GB host running the default analysis pool. Each one bounds a
/// single phase, so a repository that is slow in one of them no longer spends
/// another job's worth of wall clock discovering that: a lapsed refresh
/// analyzes the revision already on disk, a lapsed TODO scan drops one
/// auxiliary signal. Only the clone gives up on the run, because there is
/// nothing to analyze without one.
///
/// The transfer ceilings are large on purpose. A complete clone is the one
/// unavoidable network cost, it is paid once per repository, and the
/// repositories that need it most are the ones it must not cut off: the
/// largest failing catalog entries are 1.8 GB (godot), 2.5 GB (next.js) and
/// 6.1 GB (linux), and at the ~1.8 MB/s the measured django clone sustained
/// the last of those needs the better part of an hour. Every phase after the
/// clone is local and correspondingly tight.
///
/// The clone ceiling is only *reachable* because the transfer phases report
/// liveness through [`Progress`]. The caller kills a run after a much shorter
/// silence, so one hour-long await with no signal would have parked the
/// largest repositories long before this budget ever applied.
const DEFAULT_CLONE_TIMEOUT_SECS: u64 = 3_600;
const DEFAULT_FETCH_TIMEOUT_SECS: u64 = 1_800;
const DEFAULT_MAINTENANCE_TIMEOUT_SECS: u64 = 600;
const DEFAULT_COMMIT_COUNT_TIMEOUT_SECS: u64 = 300;
const DEFAULT_PLAN_TIMEOUT_SECS: u64 = 300;
const DEFAULT_WALK_TIMEOUT_SECS: u64 = 300;
const DEFAULT_PATCH_WALK_TIMEOUT_SECS: u64 = 120;

/// Smallest slice of commits worth its own `git log` subprocess. Below this
/// the fan-out in [`walk_commit_metadata_batch`] would spend more on process
/// startup than it saves on parallelism.
const MIN_WALK_CHUNK_COMMITS: usize = 50;

/// Concurrent `git log --numstat` subprocesses one commit walk may run.
///
/// The walk was a single subprocess on a single core — 13.8 s of user CPU for
/// django's 34,838 commits — even though its batches are independent and, now
/// that the clone is complete, entirely local. The share is the cores this
/// analysis is entitled to rather than every core on the host, for the same
/// reason [`pack_threads_config`] divides them: the pool runs this many
/// analyses, and therefore this many walks, at once.
fn walk_concurrency() -> usize {
    let cores = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(2);
    let share = (cores / crate::repo_analysis::configured_analysis_workers().max(1)).max(1);
    usize_from_env("REPO_ANALYSIS_WALK_CONCURRENCY", share).clamp(1, 16)
}

fn usize_from_env(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

/// Wall-clock ceiling read from `name`, in seconds. Zero and unparseable
/// values fall back to the default: a ceiling of zero would mean "every
/// repository is too slow", which is never what an operator means.
fn budget_from_env(name: &str, default_seconds: u64) -> Duration {
    Duration::from_secs(
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value| *value > 0)
            .unwrap_or(default_seconds),
    )
}

/// Run one git subprocess under a wall-clock ceiling. `Ok(None)` means the
/// ceiling lapsed: the future is dropped, and the `kill_on_drop` set by
/// [`git`] reaps the subprocess instead of leaving it pulling a pack into a
/// clone nobody is waiting for any more.
async fn output_within(mut command: Command, budget: Duration) -> Result<Option<Output>> {
    match tokio::time::timeout(budget, command.output()).await {
        Ok(result) => Ok(Some(result?)),
        Err(_) => Ok(None),
    }
}

/// Liveness callback handed to the long-running transfer phases.
///
/// Invoked at least every [`PROGRESS_BEAT_INTERVAL`] while the operation is
/// demonstrably making progress, and never invoked while it is wedged. That
/// distinction is the whole contract: the caller's stall guard converts
/// silence into a killed and eventually parked job, so a beat must be evidence
/// and not a timer.
pub(crate) type Progress<'a> = &'a (dyn Fn() + Send + Sync);

/// Floor between two liveness beats. The stall guard measures silence in
/// half-hours, so anything in the seconds range is ample, and coalescing keeps
/// a chatty transfer from turning git's per-percent progress lines into a
/// write per line on whatever the consumer does with the beat.
const PROGRESS_BEAT_INTERVAL: Duration = Duration::from_secs(5);

/// Retained tail of a progress-enabled subprocess's stderr. Enough to hold any
/// realistic git failure message, small enough that an hour of progress lines
/// cannot grow the process.
const STDERR_TAIL_BYTES: usize = 16 * 1024;

/// [`output_within`] for the phases that can legitimately run for an hour.
///
/// Liveness is taken from the bytes git writes to its own `--progress` stream,
/// not from a timer beside the future. A ticker would beat just as steadily
/// through a TCP connection that has stopped delivering data, which is exactly
/// the state the caller's stall guard exists to catch; git's progress stream
/// only advances when objects are being counted, received, or resolved, so a
/// genuinely hung transfer stays silent and is still killed. The wall-clock
/// ceiling is unchanged and still bounds the whole run.
async fn output_within_progress(
    mut command: Command,
    budget: Duration,
    progress: Option<Progress<'_>>,
) -> Result<Option<Output>> {
    let Some(progress) = progress else {
        return output_within(command, budget).await;
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let run = async move {
        let mut child = command.spawn().context("spawn git")?;
        let mut stdout = child.stdout.take().context("git stdout pipe")?;
        let mut stderr = child.stderr.take().context("git stderr pipe")?;
        // Both pipes are drained concurrently on this task rather than in a
        // spawned one: the progress callback is borrowed, and draining only
        // stderr would let a subprocess that does write to stdout block on a
        // full pipe and look exactly like a wedged transfer.
        let (stdout_bytes, stderr_tail) = tokio::join!(
            async {
                let mut bytes = Vec::new();
                stdout.read_to_end(&mut bytes).await.map(|_| bytes)
            },
            drain_with_progress(&mut stderr, progress),
        );
        let status = child.wait().await.context("wait for git")?;
        Ok::<Output, anyhow::Error>(Output {
            status,
            stdout: stdout_bytes.context("read git stdout")?,
            stderr: stderr_tail.context("read git stderr")?,
        })
    };
    // Dropping the future on timeout drops the `Child`, and the `kill_on_drop`
    // set by [`git`] reaps the subprocess.
    match tokio::time::timeout(budget, run).await {
        Ok(result) => Ok(Some(result?)),
        Err(_) => Ok(None),
    }
}

/// Read a progress stream to EOF, beating liveness as bytes arrive and keeping
/// only the last [`STDERR_TAIL_BYTES`] for error reporting.
async fn drain_with_progress<R>(reader: &mut R, progress: Progress<'_>) -> std::io::Result<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut tail: Vec<u8> = Vec::new();
    let mut buffer = [0u8; 8 * 1024];
    let mut last_beat: Option<Instant> = None;
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            return Ok(tail);
        }
        tail.extend_from_slice(&buffer[..read]);
        if tail.len() > STDERR_TAIL_BYTES {
            tail.drain(..tail.len() - STDERR_TAIL_BYTES);
        }
        if last_beat.is_none_or(|beat| beat.elapsed() >= PROGRESS_BEAT_INTERVAL) {
            progress();
            last_beat = Some(Instant::now());
        }
    }
}

/// Human-readable failure detail from a `--progress` stderr stream.
///
/// Progress is written as carriage-return-separated redraws of the same line,
/// so the raw bytes are mostly `Receiving objects:  41% (…)` frames that would
/// bury the one line an operator needs. Keep the last few non-progress lines.
fn git_failure_detail(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text
        .split(['\r', '\n'])
        .map(str::trim)
        .filter(|line| !line.is_empty() && !is_progress_line(line))
        .collect();
    if lines.is_empty() {
        return "no diagnostic output".to_string();
    }
    let kept = lines.len().saturating_sub(4);
    lines[kept..].join("; ")
}

/// `Counting objects:  73% (…)` and friends, in either the local or the
/// `remote: `-prefixed form.
fn is_progress_line(line: &str) -> bool {
    line.contains("% (") || line.ends_with('%')
}

/// `owner/name` for a clone directory, for logs. Analysis code below only ever
/// holds a path, and an operator reading "which repositories are hitting the
/// ceilings" needs the slug, not a volume-relative directory.
fn repo_label(path: &Path) -> String {
    let name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    match path
        .parent()
        .and_then(|parent| parent.file_name())
        .map(|owner| owner.to_string_lossy().into_owned())
    {
        Some(owner) if !owner.is_empty() && !name.is_empty() => format!("{owner}/{name}"),
        _ => name,
    }
}

/// Bytes of clone storage `REPOS_DIR` may hold before [`evict_clone`] starts
/// trimming, when `REPOS_QUOTA_BYTES` is unset.
///
/// **This default assumes a 500 GB data volume mounted for `REPOS_DIR`, with
/// Postgres and the OS living outside it.** That is an assumption, not a
/// measurement — the operator has not stated a disk budget — so it is written
/// here as one number to change rather than spread across the module.
///
/// Sizing, at the clone sizes this module now produces: a few hundred catalog
/// repositories average a few hundred megabytes each (django is 275 MB), which
/// is roughly 75-100 GB, and the handful of giants that must also stay warm add
/// about 20 GB more (godot 1.8 GB, next.js 2.5 GB, linux 6.1 GB). 250 GiB
/// therefore holds the whole intended working set with headroom of the same
/// order again — which the sweep needs, because a repack transiently doubles
/// one repository on disk and an in-flight clone is not yet counted against
/// anything. The other half of the volume is deliberately left unclaimed.
const DEFAULT_REPOS_QUOTA_BYTES: u64 = 250 * 1024 * 1024 * 1024;

/// Where clones live and how much of the volume they may hold.
///
/// Complete clones are the whole point of this module and they are large:
/// 275 MB for django, 1.8 GB for godot, 2.5 GB for next.js, 6.1 GB for linux,
/// against tens of megabytes each when the same repositories were cloned
/// blobless. Disk is now the only ceiling on what gitdebt can analyze, so
/// `REPOS_QUOTA_BYTES` and the volume behind it are the knob that decides how
/// many repositories stay warm — set it from your mount rather than inheriting
/// [`DEFAULT_REPOS_QUOTA_BYTES`], whose assumed volume is written down there.
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
            .unwrap_or(DEFAULT_REPOS_QUOTA_BYTES);
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
///
/// Auto-maintenance is disabled for the same reason. Each incremental fetch
/// writes another packfile, and git repacks the whole object store once
/// `gc.autoPackLimit` (50 by default) is crossed — a repository refreshed
/// fifty times would then repack multiple gigabytes in the middle of an
/// analysis, on every worker at once. Repacking belongs to the eviction sweep,
/// not to a request-facing job.
fn git() -> Command {
    let mut command = Command::new("git");
    command
        .args([
            "-c",
            pack_threads_config(),
            "-c",
            "gc.auto=0",
            "-c",
            "maintenance.auto=false",
        ])
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
/// Idempotent — repeated calls fetch only the objects that appeared since the
/// last run rather than re-cloning, which is what keeps a complete-history
/// analysis cheap in steady state.
///
/// `progress` is the liveness contract described on [`Progress`]. A cold clone
/// of the largest catalog entries runs for the better part of an hour, which is
/// longer than the caller's stall patience, so a caller that passes `None` is
/// asserting that nothing is watching this run for silence.
pub(crate) async fn open_or_clone(
    storage: &RepoStorage,
    repo: &str,
    _since_sha: Option<&str>,
    progress: Option<Progress<'_>>,
) -> Result<RepoHandle> {
    let path = storage.path_for(repo);
    tokio::fs::create_dir_all(path.parent().unwrap_or_else(|| Path::new("."))).await?;

    if path.exists() && !cache_format_is_current(&path).await {
        // Older caches were cloned under an object filter, so they hold the
        // commit graph without the blobs (and, older still, without the trees)
        // that a diff needs. Relaxing a filter on a later fetch does not
        // reliably backfill those promisor objects, so walks degrade into
        // thousands of serial lazy fetches. A versioned one-time rebuild is
        // faster and makes every later scan fully local.
        tracing::info!(repo, "rebuilding legacy repository cache");
        tokio::fs::remove_dir_all(&path)
            .await
            .context("remove legacy bare clone")?;
        if let Err(clone_error) = clone_bare(repo, &path, progress).await {
            let _ = tokio::fs::remove_dir_all(&path).await;
            return Err(clone_error);
        }
    } else if path.exists() {
        if let Err(error) = fetch_updates(&path, progress).await {
            if budget_lapsed(&error) {
                // A refresh that cannot finish inside its ceiling is not a
                // reason to produce nothing: the clone on disk still resolves
                // a real revision, and a report one push behind is worth far
                // more than a failed job that re-downloads the same objects
                // on the next attempt.
                tracing::info!(
                    repo,
                    "repository refresh exceeded its budget; analyzing the cached revision"
                );
                let head_sha = rev_parse_head(&path).await?;
                return Ok(RepoHandle { path, head_sha });
            }
            if !fetch_requires_reclone(&error) {
                return Err(error);
            }
            tracing::warn!(repo, %error, "cached default branch disappeared; recloning");
            tokio::fs::remove_dir_all(&path)
                .await
                .context("remove stale bare clone")?;
            if let Err(clone_error) = clone_bare(repo, &path, progress).await {
                // A failed clone can leave a non-repository directory that
                // would otherwise turn every later retry into a fetch error.
                let _ = tokio::fs::remove_dir_all(&path).await;
                return Err(clone_error);
            }
        }
    } else {
        clone_bare(repo, &path, progress).await?;
    }
    let head_sha = rev_parse_head(&path).await?;
    Ok(RepoHandle { path, head_sha })
}

fn fetch_requires_reclone(error: &anyhow::Error) -> bool {
    let detail = error.to_string().to_ascii_lowercase();
    detail.contains("couldn't find remote ref refs/heads/")
        || detail.contains("could not find remote ref refs/heads/")
}

/// Marker embedded in the errors this module raises when one of its own
/// wall-clock ceilings lapsed, as opposed to git reporting a real failure.
/// Callers degrade on the former and surface the latter.
pub(crate) const BUDGET_MARKER: &str = "gitdebt budget exceeded";

/// Did this error come from one of *this process's own* wall-clock ceilings
/// lapsing, rather than from git or the remote reporting a real failure?
///
/// The whole error chain is searched, not just its outermost frame: every
/// caller wraps these errors in `.context(...)` before the queue ever sees
/// them, and `Error::to_string` renders only the last context added.
///
/// The distinction is load-bearing outside this module. A lapse is a
/// statement about the ceiling an operator configured, never about the
/// repository, so it must never be allowed to retire a repository
/// permanently — raising the ceiling and redeploying has to bring the row
/// back. See `repo_analysis::Failure::terminal`.
pub(crate) fn budget_lapsed(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.to_string().contains(BUDGET_MARKER))
}

/// Marks a failure caused by the state of the clone on THIS host's disk —
/// unreadable objects, a pack damaged by a full volume, a child killed by the
/// OOM reaper — rather than by anything true of the repository.
pub(crate) const LOCAL_CLONE_MARKER: &str = "gitdebt local clone unusable";

/// Is this error about our own copy rather than about the repository?
///
/// Same reasoning as [`budget_lapsed`], and the same consequence: it must
/// never retire a repository permanently. It carries one extra obligation,
/// though. A lapse can be fixed by raising a ceiling, but an unreadable clone
/// is fixed only by getting a new one — so every attempt would otherwise meet
/// the identical bytes on disk and fail identically until the attempt ceiling
/// retired a perfectly healthy repository. Callers must discard the clone.
pub(crate) fn local_clone_unusable(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.to_string().contains(LOCAL_CLONE_MARKER))
}

/// Delete a clone so the next attempt fetches a fresh one. Best-effort: if the
/// directory cannot be removed, the retry re-reads the same bytes and fails the
/// same way, which is the behaviour this exists to end — so it is logged.
pub(crate) async fn discard_clone(path: &Path) {
    if let Err(error) = tokio::fs::remove_dir_all(path).await {
        tracing::warn!(
            path = %path.display(),
            %error,
            "could not discard an unusable clone; the next attempt will re-read it"
        );
    }
}

async fn clone_bare(repo: &str, path: &Path, progress: Option<Progress<'_>>) -> Result<()> {
    let url = format!("https://github.com/{repo}.git");
    // No object filter. One bulk pack transfer of everything the default
    // branch reaches is cheaper end to end than a filtered clone plus the
    // thousands of promisor round trips the diff walks would then need, and it
    // is what makes every phase after this one local. `--no-tags` and
    // `--single-branch` still apply: gitdebt only ever analyzes the default
    // branch, so release tags and other branches are bytes nothing reads.
    let mut command = git();
    // `--progress` forces the counting/receiving/resolving stream even though
    // stderr is a pipe. It is the evidence [`output_within_progress`] reads to
    // tell a multi-gigabyte transfer apart from a wedged one.
    command
        .args([
            "clone",
            "--bare",
            "--no-tags",
            "--single-branch",
            "--progress",
            "--",
        ])
        .arg(&url)
        .arg(path);
    let budget = budget_from_env(
        "REPO_ANALYSIS_CLONE_TIMEOUT_SECONDS",
        DEFAULT_CLONE_TIMEOUT_SECS,
    );
    let Some(output) = output_within_progress(command, budget, progress)
        .await
        .context("spawn full git clone")?
    else {
        // git cleans up after itself when it fails, but not when it is killed:
        // the half-written object store left behind would be read as a cached
        // clone by the next attempt. Nothing about it is resumable — git has
        // no resumable clone — so the honest move is to discard it and let the
        // operator see the ceiling in the log.
        let _ = tokio::fs::remove_dir_all(path).await;
        tracing::info!(
            repo,
            budget_seconds = budget.as_secs(),
            "clone exceeded its budget; discarding the partial clone"
        );
        bail!(
            "{BUDGET_MARKER}: clone did not finish in {}s",
            budget.as_secs()
        );
    };
    if !output.status.success() {
        bail!("git clone failed: {}", git_failure_detail(&output.stderr));
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
    write_commit_graph(path, progress).await;
    Ok(())
}

/// Write (or extend) the commit-graph.
///
/// Every analysis run counts reachable commits, which without a commit-graph
/// parses every commit object out of the pack — seconds of CPU per run on a
/// large repository, paid to refresh one integer. Best-effort: a failure only
/// means the next count is slower.
///
/// This runs immediately after a clone that may already have consumed most of
/// the caller's stall patience, so it takes the same liveness callback rather
/// than adding its own budget's worth of silence on top.
async fn write_commit_graph(path: &Path, progress: Option<Progress<'_>>) {
    let mut command = git_in(path);
    command.args([
        "commit-graph",
        "write",
        "--reachable",
        "--split",
        "--progress",
    ]);
    let budget = budget_from_env(
        "REPO_ANALYSIS_GIT_MAINTENANCE_TIMEOUT_SECONDS",
        DEFAULT_MAINTENANCE_TIMEOUT_SECS,
    );
    match output_within_progress(command, budget, progress).await {
        Ok(Some(output)) if output.status.success() => {}
        Ok(Some(output)) => tracing::debug!(
            stderr = %String::from_utf8_lossy(&output.stderr),
            "commit-graph write failed"
        ),
        // The graph is an accelerator for the reachable-commit count, so a
        // lapse costs a slower count rather than a wrong one. Logged at info
        // because the count's own ceiling is the next thing to lapse.
        Ok(None) => tracing::info!(
            repo = repo_label(path),
            budget_seconds = budget.as_secs(),
            "commit-graph write exceeded its budget"
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

async fn fetch_updates(path: &Path, progress: Option<Progress<'_>>) -> Result<()> {
    // Resolve the clone's default branch from its HEAD symref. A bare
    // single-branch clone has NO `remote.origin.fetch` refspec, so a plain
    // `git fetch origin` only writes `FETCH_HEAD` and never advances the
    // branch ref — `rev-parse HEAD` would then keep returning the stale
    // SHA and incremental analysis would never see new commits. We fetch an
    // *explicit* refspec (`+refs/heads/<branch>:refs/heads/<branch>`) so the
    // local branch (and therefore HEAD) actually moves forward.
    let branch = default_branch(path).await?;
    let refspec = format!("+refs/heads/{branch}:refs/heads/{branch}");
    // No filter and no `--unshallow`/`--refetch` repair: [`open_or_clone`]
    // rebuilds any cache not stamped with the current [`CACHE_FORMAT_VERSION`]
    // before reaching here, so every clone this runs against is already
    // complete and unfiltered. This transfers exactly the objects pushed since
    // the last run — the whole reason a one-time full clone pays for itself.
    let mut command = git_in(path);
    // A steady-state fetch is a year's commits at most, but the first fetch
    // after an eviction or a re-clone is another full transfer, so it reports
    // liveness on exactly the same terms as the clone.
    command.args(["fetch", "--no-tags", "--progress"]);
    // `--` before the positional remote name: defense-in-depth so a future
    // unvalidated positional arg can't be parsed as a flag.
    command.args(["--", "origin", &refspec]);
    let budget = budget_from_env(
        "REPO_ANALYSIS_FETCH_TIMEOUT_SECONDS",
        DEFAULT_FETCH_TIMEOUT_SECS,
    );
    let Some(output) = output_within_progress(command, budget, progress)
        .await
        .context("spawn git fetch")?
    else {
        bail!(
            "{BUDGET_MARKER}: fetch did not finish in {}s",
            budget.as_secs()
        );
    };
    if !output.status.success() {
        bail!("git fetch failed: {}", git_failure_detail(&output.stderr));
    }
    write_commit_graph(path, progress).await;
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
// Keep progress and cancellation responsive. A large `git log -p` batch can
// run for a long time without emitting a durable progress update; 100 still
// amortizes process startup while giving the UI measured checkpoints.
pub(crate) const LOG_BATCH_COMMITS: usize = 100;
/// Metadata-only walks retain far less output than patch walks, so larger
/// batches reduce process startup cost without hiding progress. This is the
/// unit the caller reports progress in; [`walk_commit_metadata_batch`] splits
/// each batch further across cores internally.
pub(crate) const METADATA_BATCH_COMMITS: usize = 500;

/// The commits one analysis run will walk, oldest-first.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CommitWalkPlan {
    pub shas: Vec<String>,
    /// Always `false`. Analyzing complete history is the product, so no plan
    /// this module produces omits reachable commits. The field stays because
    /// `analysis_truncated` is a persisted column that the overview API and
    /// the report page still read, and "this window covers everything" is the
    /// honest value to keep writing into it — not a reason to drop the column
    /// out from under its readers.
    pub truncated: bool,
}

/// Why a stored cursor could not be used to plan an incremental run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CursorRejection {
    /// `git cat-file -e <sha>^{commit}` failed: the object is not in the local
    /// store. The clone was evicted and re-created, or the commit was garbage
    /// collected after a force-push. `<sha>..HEAD` would fail outright.
    Missing,
    /// The object exists but `git merge-base --is-ancestor <sha> HEAD` says it
    /// is not on the branch any more — a rebase or force-push rewrote history
    /// under the stored aggregates. This is the dangerous one: `<sha>..HEAD`
    /// still exits 0 and still prints a plausible commit list, which appended
    /// on top of aggregates that already counted the rewritten commits drifts
    /// commit counts, per-file churn and per-author totals upward forever.
    Diverged,
}

impl CursorRejection {
    /// Stable label for logs and for the caller's own reporting.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "cursor object absent from the local clone",
            Self::Diverged => "cursor is not an ancestor of HEAD",
        }
    }
}

/// The outcome of planning one analysis run.
///
/// Both variants carry a complete, walkable plan; the variant states how the
/// caller must persist the result. `Rebuild` is expensive and deliberately
/// loud — it means the stored aggregates describe a history that no longer
/// exists, so appending to them is what would be wrong.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CommitPlan {
    /// The cursor was usable, or the caller asked for full history to begin
    /// with. Persist however the caller already intended.
    Planned(CommitWalkPlan),
    /// The cursor failed validation. `plan` is complete history from HEAD, and
    /// the caller MUST replace the stored aggregates rather than append.
    Rebuild {
        plan: CommitWalkPlan,
        reason: CursorRejection,
    },
}

impl CommitPlan {
    pub(crate) fn plan(&self) -> &CommitWalkPlan {
        match self {
            Self::Planned(plan) | Self::Rebuild { plan, .. } => plan,
        }
    }

    /// True when this run must replace stored aggregates even if the caller
    /// intended to append. Never the other way round: a caller that already
    /// decided to rebuild (revision bump, empty-repository cursor) still does.
    pub(crate) fn requires_full_rebuild(&self) -> bool {
        matches!(self, Self::Rebuild { .. })
    }

    pub(crate) fn rejection(&self) -> Option<CursorRejection> {
        match self {
            Self::Planned(_) => None,
            Self::Rebuild { reason, .. } => Some(*reason),
        }
    }

    pub(crate) fn into_plan(self) -> CommitWalkPlan {
        match self {
            Self::Planned(plan) | Self::Rebuild { plan, .. } => plan,
        }
    }
}

/// Every non-merge commit reachable from HEAD that `since_sha` does not
/// already cover, oldest-first.
///
/// There is no cap and no sampling window. A commit costs about 0.44 ms to
/// walk against a complete local clone, so the dimension that used to make
/// full history unaffordable was never the commit count — it was the promisor
/// round trip per commit that a blob-filtered clone forced, and that is gone.
/// `since_sha` is what keeps steady state cheap: on a repository already
/// analyzed at an older head this enumerates only the commits pushed since.
///
/// A cursor is only trusted after [`validate_cursor`] proves it still names a
/// commit on this branch. Rebases and force-pushes are routine, and the range
/// syntax reports neither failure mode usefully on its own.
pub(crate) async fn plan_commits(
    handle: &RepoHandle,
    since_sha: Option<&str>,
) -> Result<CommitPlan> {
    if handle.is_empty() {
        return Ok(CommitPlan::Planned(CommitWalkPlan {
            shas: Vec::new(),
            truncated: false,
        }));
    }
    let mut rejection = None;
    if let Some(sha) = since_sha {
        rejection = validate_cursor(&handle.path, sha).await?;
        if let Some(reason) = rejection {
            // A rebuild is correct but costs the whole history again, so it is
            // never silent: an operator seeing these repeatedly for one
            // repository is seeing a rewritten branch, not a gitdebt bug.
            tracing::info!(
                repo = repo_label(&handle.path),
                cursor = sha,
                head = handle.head_sha,
                reason = reason.as_str(),
                "stored analysis cursor rejected; rebuilding complete history"
            );
        }
    }
    let range = match since_sha {
        Some(sha) if rejection.is_none() => format!("{sha}..HEAD"),
        _ => "HEAD".to_string(),
    };
    let mut command = git_in(&handle.path);
    command.args(["rev-list", "--reverse", "--no-merges", &range]);
    let budget = budget_from_env(
        "REPO_ANALYSIS_PLAN_TIMEOUT_SECONDS",
        DEFAULT_PLAN_TIMEOUT_SECS,
    );
    let Some(output) = output_within(command, budget)
        .await
        .context("git rev-list")?
    else {
        bail!(
            "{BUDGET_MARKER}: commit selection did not finish in {}s",
            budget.as_secs()
        );
    };
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
    let plan = CommitWalkPlan {
        shas,
        truncated: false,
    };
    Ok(match rejection {
        Some(reason) => CommitPlan::Rebuild { plan, reason },
        None => CommitPlan::Planned(plan),
    })
}

/// Prove a stored cursor can still anchor an incremental walk. `Ok(None)` is
/// the usable case.
///
/// Both probes are graph reads against a local clone and cost milliseconds
/// even on linux, which is the entire argument for running them on every
/// incremental plan rather than trying to guess when history was rewritten.
async fn validate_cursor(path: &Path, sha: &str) -> Result<Option<CursorRejection>> {
    // A cursor that is not an object id never reaches git. `EMPTY_REPOSITORY_HEAD`
    // is a real stored value, and beyond it this keeps any future caller from
    // handing a revision expression — or an argument — to the commands below.
    if !(40..=64).contains(&sha.len()) || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Ok(Some(CursorRejection::Missing));
    }
    let exists = git_in(path)
        .args(["cat-file", "-e", &format!("{sha}^{{commit}}")])
        .output()
        .await
        .context("probe cursor object")?;
    if !exists.status.success() {
        return Ok(Some(CursorRejection::Missing));
    }
    let ancestor = git_in(path)
        .args(["merge-base", "--is-ancestor", sha, "HEAD"])
        .output()
        .await
        .context("probe cursor ancestry")?;
    if ancestor.status.success() {
        return Ok(None);
    }
    // Exit 1 is the documented "not an ancestor". Anything else means git
    // could not answer, and an unanswerable cursor is not a trustworthy one
    // either — a rebuild is expensive but it is never wrong.
    if ancestor.status.code() != Some(1) {
        tracing::info!(
            repo = repo_label(path),
            detail = %git_failure_detail(&ancestor.stderr),
            "cursor ancestry probe failed; treating the cursor as diverged"
        );
    }
    Ok(Some(CursorRejection::Diverged))
}

/// Exact number of commits reachable from the default branch, including
/// merges — the denominator the analyzed non-merge total is reported against.
/// The clone always carries the complete commit graph, so this is a cheap
/// local graph walk.
pub(crate) async fn reachable_commit_count(handle: &RepoHandle) -> Result<usize> {
    if handle.is_empty() {
        return Ok(0);
    }
    let mut command = git_in(&handle.path);
    command.args(["rev-list", "--count", "HEAD"]);
    let budget = budget_from_env(
        "REPO_ANALYSIS_COMMIT_COUNT_TIMEOUT_SECONDS",
        DEFAULT_COMMIT_COUNT_TIMEOUT_SECS,
    );
    // This is the one phase with no honest degradation. The number is the
    // repository's exact commit total; substituting the analyzed non-merge
    // count, or zero, would state something false about the repository. A
    // ceiling here therefore converts an unbounded graph walk into a bounded,
    // explicitly named failure — and with
    // the commit-graph written above it is a sub-second read even at a million
    // commits, so lapsing means the clone itself is damaged.
    let Some(output) = output_within(command, budget)
        .await
        .context("git reachable commit count")?
    else {
        bail!(
            "{BUDGET_MARKER}: reachable commit count did not finish in {}s",
            budget.as_secs()
        );
    };
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
    // not only noise control: producing "Binary files ... differ" still costs
    // reading and hashing both sides of a file that may be hundreds of
    // megabytes, for a line the TODO scanner then throws away.
    command.args(NON_TEXT_PATHSPECS);
    let budget = budget_from_env(
        "REPO_ANALYSIS_PATCH_WALK_TIMEOUT_SECONDS",
        DEFAULT_PATCH_WALK_TIMEOUT_SECS,
    );
    let Some(output) = output_within(command, budget)
        .await
        .context("batched git log")?
    else {
        tracing::info!(
            repo = repo_label(&handle.path),
            commits = shas.len(),
            budget_seconds = budget.as_secs(),
            "TODO patch scan exceeded its budget; TODO deltas omitted for this batch"
        );
        return Ok(Vec::new());
    };
    if !output.status.success() {
        bail!(
            "batched git log failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(parse_log_records(&output.stdout))
}

/// One batch of walked commits plus whether any of it lost line movement.
///
/// `incomplete_objects` means at least one commit in `commits` came from the
/// tree-only fallback: its paths are exact but its `lines_added`/
/// `lines_deleted` are zero because no blob could be read, which is
/// indistinguishable from a commit that genuinely moved no lines. Persisting
/// such a batch as exact is permanent: an incremental run never re-walks a
/// commit behind the cursor, so the churn and ownership charts would stay
/// wrong forever. Worse, the chunk size derives from [`walk_concurrency`], so
/// under the same fault two replicas zero *different* commits for the same
/// repository.
///
/// It is deliberately narrower than the old `degraded` flag, which also fired
/// when a chunk merely lapsed [`DEFAULT_WALK_TIMEOUT_SECS`]. Conflating the
/// two meant one slow chunk on a busy host threw away a walk that had already
/// completed a million commits, and the retry started from zero. Slowness is
/// now answered where it happens — see [`walk_with_retry`] — and never
/// reported as damage.
#[derive(Clone, Debug)]
pub(crate) struct CommitWalk {
    pub commits: Vec<CommitInfo>,
    pub incomplete_objects: bool,
}

/// What one `git log --numstat` subprocess over an explicit SHA list learned.
///
/// The three arms are the three distinct events that used to collapse into a
/// single `degraded` boolean, and only one of them says anything about the
/// repository.
enum ChunkOutcome {
    /// Exact: line movement for every commit in the slice.
    Exact(Vec<CommitInfo>),
    /// git exited non-zero. Lazy fetching is off and the clone is complete,
    /// so the reachable cause is an object git could not read; the slice was
    /// re-walked from trees alone, which keeps every path-shaped signal exact
    /// and reports zero line movement.
    PathsOnly(Vec<CommitInfo>),
    /// The wall-clock ceiling lapsed and nothing at all was learned — about
    /// the objects least of all.
    Lapsed,
}

/// How many times one lapsed slice may be halved before the walk gives up.
///
/// The budget is per subprocess, so halving a slow slice hands each half the
/// full budget again: the retry is a real second chance rather than the same
/// race re-run. Five halvings take a [`METADATA_BATCH_COMMITS`] batch down to
/// ~16 commits per subprocess. A repository that still cannot walk sixteen
/// commits inside the ceiling surfaces a [`BUDGET_MARKER`] lapse, which the
/// queue classifies as non-terminal and retries against a now-warm clone —
/// slow must stay convergent, and it never earns the zeroed churn that a
/// damaged object store gets.
const MAX_WALK_CHUNK_SPLITS: u32 = 5;

/// Total lapsed subprocesses one chunk walk may absorb before it gives up.
///
/// Depth alone does not bound the wall clock: five levels of halving is up to
/// 63 slices, and if every one of them lapsed the chunk would sit silent for
/// 63 budgets. A batch boundary is the only place this walk beats its
/// heartbeat, so anything longer than `REPO_ANALYSIS_STALL_SECONDS` gets the
/// run killed as wedged — throwing away the completed prefix that the retry
/// ladder exists to protect. Eight lapses at the default 300 s ceiling is
/// 40 minutes of retrying inside a 60-minute stall window, which is far more
/// slack than a transiently busy host ever needs and still leaves the guard
/// its margin.
const MAX_WALK_CHUNK_LAPSES: u32 = 8;

/// Where to halve a lapsed slice, or `None` when the ladder is spent.
fn retry_split(len: usize, splits: u32) -> Option<usize> {
    (len > 1 && splits < MAX_WALK_CHUNK_SPLITS).then(|| len.div_ceil(2))
}

/// Walk `shas` through `run`, halving and retrying any slice that lapsed.
///
/// Generic over the subprocess so the retry ladder is testable without a
/// repository that happens to be slow on the day the test runs. What it must
/// preserve is the ordering contract every caller depends on: records come
/// back in `shas` order no matter how many times a slice was split, which is
/// why the halves go back on the *front* of the worklist, left before right.
async fn walk_with_retry<'a, R, F>(shas: &'a [String], run: R) -> Result<CommitWalk>
where
    R: Fn(&'a [String]) -> F,
    F: std::future::Future<Output = Result<ChunkOutcome>>,
{
    let mut commits = Vec::new();
    let mut incomplete_objects = false;
    let mut lapses = 0;
    let mut pending: std::collections::VecDeque<(&'a [String], u32)> =
        std::collections::VecDeque::new();
    pending.push_back((shas, 0));
    while let Some((slice, splits)) = pending.pop_front() {
        match run(slice).await? {
            ChunkOutcome::Exact(walked) => commits.extend(walked),
            ChunkOutcome::PathsOnly(walked) => {
                commits.extend(walked);
                incomplete_objects = true;
            }
            ChunkOutcome::Lapsed => {
                lapses += 1;
                let split =
                    retry_split(slice.len(), splits).filter(|_| lapses < MAX_WALK_CHUNK_LAPSES);
                let Some(mid) = split else {
                    bail!(
                        "{BUDGET_MARKER}: commit walk did not finish for a slice of {} commits \
                         after {lapses} lapsed attempts",
                        slice.len()
                    );
                };
                let (left, right) = slice.split_at(mid);
                pending.push_front((right, splits + 1));
                pending.push_front((left, splits + 1));
            }
        }
    }
    Ok(CommitWalk {
        commits,
        incomplete_objects,
    })
}

/// Read author, date, message, changed paths, and line movement for a batch
/// of commits.
///
/// `--numstat` is what makes `lines_added`/`lines_deleted` — and therefore
/// `repo_commit_days` and every churn chart — possible, and git can only
/// produce those counts by diffing blob content. Against a complete clone that
/// is pure local work: 15.2 s for all 34,838 of django's commits, of which
/// 13.8 s is a single core's user time. So the batch is split across cores
/// here rather than handed to one subprocess.
///
/// The split cannot change the result. Each chunk is an explicit, ordered SHA
/// list walked with `--no-walk=unsorted`, git emits one record per named
/// commit in the order given, and the chunks are re-concatenated in plan
/// order — so the bytes this returns are identical to the sequential walk's,
/// whatever the completion order was.
///
/// `--no-renames` preserves the old path-set contract for renames (delete +
/// add); rename-only commits can therefore report line movement, which is
/// intentional and documented as Git numstat churn rather than edit distance.
pub(crate) async fn walk_commit_metadata_batch(
    handle: &RepoHandle,
    shas: &[String],
) -> Result<CommitWalk> {
    if shas.is_empty() {
        return Ok(CommitWalk {
            commits: Vec::new(),
            incomplete_objects: false,
        });
    }
    validate_shas(shas)?;
    let budget = budget_from_env(
        "REPO_ANALYSIS_WALK_TIMEOUT_SECONDS",
        DEFAULT_WALK_TIMEOUT_SECS,
    );
    let chunk = shas
        .len()
        .div_ceil(walk_concurrency())
        .max(MIN_WALK_CHUNK_COMMITS);
    let path = handle.path.as_path();
    let walks = shas
        .chunks(chunk)
        .map(|chunk| walk_metadata_chunk(path, chunk, budget));
    // `try_join_all` drives every chunk on this one task: the work being
    // parallelized lives in the git child processes and in the blocking pool,
    // so nothing here needs a `'static` spawn or a clone of the SHA list.
    let chunks = futures::future::try_join_all(walks).await?;
    // One unreadable chunk makes the whole batch inexact. The caller cannot
    // tell which commits lost their line counts from the records alone, so
    // the honest report is that this batch is not exact.
    let incomplete_objects = chunks.iter().any(|chunk| chunk.incomplete_objects);
    Ok(CommitWalk {
        commits: chunks.into_iter().flat_map(|chunk| chunk.commits).collect(),
        incomplete_objects,
    })
}

/// One `git log --numstat` slice, retried on slowness and downgraded only on
/// damage.
///
/// Lazy fetching is disabled. On a complete clone nothing is missing, so this
/// costs nothing and is the guard that keeps it that way: were an object ever
/// absent — a damaged pack, a cache built by some future filtered path — git
/// left to itself would download the missing ones one object per round trip,
/// and the walk would again cost what the *repository* churns rather than what
/// it contains. Instead the slice degrades to [`walk_commit_paths_chunk`]:
/// exact changed paths, no line movement, one info log naming the repository,
/// and — the part the caller cannot reconstruct afterwards —
/// `incomplete_objects` set on the [`CommitWalk`] it returns.
///
/// A lapsed ceiling gets none of that. It is not evidence about any object,
/// so it is answered by halving the slice and walking it again
/// ([`walk_with_retry`]).
async fn walk_metadata_chunk(path: &Path, shas: &[String], budget: Duration) -> Result<CommitWalk> {
    walk_with_retry(shas, |slice| numstat_chunk(path, slice, budget)).await
}

/// One `git log --numstat` subprocess over an explicit SHA list.
async fn numstat_chunk(path: &Path, shas: &[String], budget: Duration) -> Result<ChunkOutcome> {
    let log_format = "%x00%x00GDCOMMIT%x00%H%x00%P%x00%ae%x00%an%x00%aI%x00%s%x00";
    let mut command = git_in(path);
    command
        .args([
            "log",
            "--no-walk=unsorted",
            "--numstat",
            "-z",
            "--no-renames",
            &format!("--format={log_format}"),
        ])
        .env("GIT_NO_LAZY_FETCH", "1");
    command.args(shas).arg("--");
    let detail = match output_within(command, budget)
        .await
        .context("batched metadata git log")?
    {
        Some(output) if output.status.success() => {
            return Ok(ChunkOutcome::Exact(
                parse_off_thread(output.stdout, parse_metadata_records).await?,
            ));
        }
        Some(output) => String::from_utf8_lossy(&output.stderr)
            .lines()
            .next_back()
            .unwrap_or("git log --numstat failed")
            .to_string(),
        None => {
            tracing::info!(
                repo = repo_label(path),
                commits = shas.len(),
                budget_seconds = budget.as_secs(),
                "commit walk slice exceeded its budget; retrying it in halves"
            );
            return Ok(ChunkOutcome::Lapsed);
        }
    };
    tracing::info!(
        repo = repo_label(path),
        commits = shas.len(),
        detail,
        "commit walk could not read every changed blob locally; \
         recording changed paths without line movement"
    );
    Ok(ChunkOutcome::PathsOnly(
        walk_commit_paths_chunk(path, shas).await?,
    ))
}

/// Decode one subprocess's stdout on the blocking pool.
///
/// A full-history walk parses millions of records, and the analysis pool runs
/// several of these at once; leaving that on the runtime threads starves every
/// other task in the process, including the ones writing progress rows.
async fn parse_off_thread(
    stdout: Vec<u8>,
    parse: fn(&[u8]) -> Vec<CommitInfo>,
) -> Result<Vec<CommitInfo>> {
    tokio::task::spawn_blocking(move || parse(&stdout))
        .await
        .context("parse commit records")
}

/// Changed paths for a chunk of commits, read from trees alone.
///
/// The degraded form of [`walk_metadata_chunk`]: `--raw` compares the object
/// ids recorded in each commit's trees, so it produces a changed-path set
/// without reading a single byte of file content. Ownership, coupling, and
/// change-frequency signals stay exact; line movement is reported as zero for
/// these commits. On a complete clone this should never run — it is the
/// answer to a damaged object store, not to a missing download.
pub(crate) async fn walk_commit_paths_chunk(
    path: &Path,
    shas: &[String],
) -> Result<Vec<CommitInfo>> {
    if shas.is_empty() {
        return Ok(Vec::new());
    }
    validate_shas(shas)?;
    let log_format = "%x00%x00GDCOMMIT%x00%H%x00%P%x00%ae%x00%an%x00%aI%x00%s%x00";
    let mut command = git_in(path);
    command
        .args([
            "log",
            "--no-walk=unsorted",
            "--raw",
            "-z",
            "--no-renames",
            "--no-abbrev",
            &format!("--format={log_format}"),
        ])
        .env("GIT_NO_LAZY_FETCH", "1");
    command.args(shas).arg("--");
    let budget = budget_from_env(
        "REPO_ANALYSIS_WALK_TIMEOUT_SECONDS",
        DEFAULT_WALK_TIMEOUT_SECS,
    );
    let Some(output) = output_within(command, budget)
        .await
        .context("batched path-only git log")?
    else {
        bail!(
            "{BUDGET_MARKER}: path-only commit walk did not finish in {}s",
            budget.as_secs()
        );
    };
    if !output.status.success() {
        bail!(
            "batched path-only git log failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    parse_off_thread(output.stdout, parse_raw_metadata_records).await
}

/// Pure parser for the `git log --raw -z` stream the path-only walk produces.
/// The header is identical to the numstat walk's; each changed file is one
/// `:<modes> <oids> <status>` segment followed by its path segment, and line
/// movement is unknowable from trees alone.
fn parse_raw_metadata_records(stdout: &[u8]) -> Vec<CommitInfo> {
    split_on(stdout, COMMIT_SENTINEL)
        .into_iter()
        .filter(|record| !record.is_empty())
        .filter_map(parse_raw_metadata_record)
        .collect()
}

fn parse_raw_metadata_record(record: &[u8]) -> Option<CommitInfo> {
    let segments: Vec<&[u8]> = record.split(|&byte| byte == 0).collect();
    let mut header = segments.iter();
    let sha = String::from_utf8_lossy(header.next()?).trim().to_string();
    if sha.is_empty() {
        return None;
    }
    let is_root = String::from_utf8_lossy(header.next()?).trim().is_empty();
    let author_email = String::from_utf8_lossy(header.next()?).to_lowercase();
    let author_name = String::from_utf8_lossy(header.next()?).to_string();
    let iso = String::from_utf8_lossy(header.next()?).to_string();
    let message_first_line = String::from_utf8_lossy(header.next()?).to_string();
    let committed_at = DateTime::parse_from_rfc3339(iso.trim())
        .map(|date| date.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    let mut file_changes = Vec::new();
    // The first six segments are the header; entries and their paths follow in
    // fixed pairs, so paths are consumed by position and never re-parsed as
    // entries (a committed file may be named like one).
    let mut index = 6;
    while index < segments.len() {
        let segment = String::from_utf8_lossy(segments[index]);
        let Some(entry) = parse_raw_diff_entry(segment.as_ref()) else {
            index += 1;
            continue;
        };
        if !is_root {
            for offset in 1..=entry.path_segments {
                let Some(path) = segments.get(index + offset) else {
                    break;
                };
                let path = String::from_utf8_lossy(path).trim().to_string();
                if !path.is_empty() {
                    file_changes.push(FileChange {
                        path,
                        lines_added: 0,
                        lines_deleted: 0,
                        binary: false,
                    });
                }
            }
        }
        index += 1 + entry.path_segments;
    }
    let paths_changed = file_changes
        .iter()
        .map(|change| change.path.clone())
        .collect();

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
        lines_added: 0,
        lines_deleted: 0,
        binary_files: 0,
        todo_added: 0,
        todo_removed: 0,
    })
}

/// How many path segments follow one `--raw -z` entry header, or `None` when
/// the segment is not an entry header at all.
///
/// An entry is `:<srcmode> <dstmode> <srcoid> <dstoid> <status>` in one NUL
/// segment, followed by its path in the next. Recognizing the shape is what
/// lets the caller consume paths by position: `-z` emits paths raw and
/// unquoted, so a committed file can be named byte-for-byte like an entry
/// header, and re-parsing segments would read that path as a diff entry.
fn parse_raw_diff_entry(segment: &str) -> Option<RawDiffEntry> {
    // The first entry of each commit is preceded by the newline that follows
    // the `--format` block.
    let rest = segment.trim_start_matches(['\n', '\r']).strip_prefix(':')?;
    let mut fields = rest.split(' ');
    let src_mode = fields.next()?;
    let dst_mode = fields.next()?;
    let _src_oid = fields.next()?;
    let _dst_oid = fields.next()?;
    let status = fields.next()?;
    if fields.next().is_some() || !is_diff_mode(src_mode) || !is_diff_mode(dst_mode) {
        return None;
    }
    let mut status = status.chars();
    let letter = status.next()?;
    if !letter.is_ascii_uppercase() || !status.all(|char| char.is_ascii_digit()) {
        return None;
    }
    Some(RawDiffEntry {
        // Rename/copy statuses carry two path segments instead of one. `--raw`
        // is always invoked with `--no-renames`, so this is defense against a
        // caller that forgets rather than a shape we expect.
        path_segments: usize::from(matches!(letter, 'R' | 'C')) + 1,
    })
}

struct RawDiffEntry {
    path_segments: usize,
}

fn is_diff_mode(mode: &str) -> bool {
    mode.len() == 6 && mode.bytes().all(|byte| byte.is_ascii_digit())
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
/// stat of every loose object). Loose objects are summed too via the
/// `objects/<xx>/` shards, since an incremental fetch of a handful of new
/// objects may write them loose rather than packed. The eviction scorer only
/// needs a relative ranking, so approximate-but-cheap is the right trade.
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

    /// Cheap stand-in for a walked record: the retry ladder only ever moves
    /// these around, so identity by SHA is the whole contract under test.
    fn stub_commit(sha: &str) -> CommitInfo {
        CommitInfo {
            sha: sha.to_string(),
            author_email: "a@example.com".into(),
            author_name: "A".into(),
            committed_at: Utc::now(),
            committed_day: Utc::now().date_naive(),
            message_first_line: String::new(),
            is_fix: false,
            paths_changed: Vec::new(),
            file_changes: Vec::new(),
            lines_added: 0,
            lines_deleted: 0,
            binary_files: 0,
            todo_added: 0,
            todo_removed: 0,
        }
    }

    fn stub_shas(count: usize) -> Vec<String> {
        (0..count).map(|index| format!("{index:040x}")).collect()
    }

    /// The failure the operator spent a session fighting: one slow slice used
    /// to mark the whole batch degraded, which discarded a walk that had
    /// already completed a million commits. A lapse must cost the slice, not
    /// the run — and it must never be reported as a damaged object store.
    #[tokio::test]
    async fn a_slow_slice_is_retried_and_the_completed_prefix_survives() {
        let shas = stub_shas(400);
        let walk = walk_with_retry(&shas, |slice| {
            let commits: Vec<CommitInfo> = slice.iter().map(|sha| stub_commit(sha)).collect();
            // Stands for a host that can walk a hundred commits inside the
            // ceiling but not four hundred.
            let lapsed = slice.len() > 100;
            async move {
                Ok(if lapsed {
                    ChunkOutcome::Lapsed
                } else {
                    ChunkOutcome::Exact(commits)
                })
            }
        })
        .await
        .expect("a slice that is merely slow must not fail the walk");

        assert!(
            !walk.incomplete_objects,
            "slowness says nothing about the object store"
        );
        let walked: Vec<String> = walk.commits.into_iter().map(|commit| commit.sha).collect();
        assert_eq!(
            walked, shas,
            "records stay in plan order however often a slice was halved"
        );
    }

    /// When the ladder really is spent the run still must not be retired: the
    /// error carries [`BUDGET_MARKER`], which is what keeps the queue row
    /// revivable after the operator raises the ceiling.
    #[tokio::test]
    async fn an_exhausted_retry_ladder_reports_a_budget_lapse_not_damage() {
        let shas = stub_shas(64);
        let error = walk_with_retry(&shas, |_| async { Ok(ChunkOutcome::Lapsed) })
            .await
            .expect_err("a walk that never completes a slice cannot succeed");
        assert!(
            budget_lapsed(&error),
            "a lapse must be recognisable as this process's own ceiling: {error:#}"
        );
    }

    /// Depth bounds one branch; nothing bounds the tree. Enough independently
    /// slow siblings would keep the chunk retrying for dozens of budgets, and
    /// a batch boundary is the only place this walk beats its heartbeat — so
    /// the stall guard would kill the run and discard the very prefix the
    /// ladder exists to keep.
    #[tokio::test]
    async fn total_retry_time_is_bounded_so_the_stall_guard_never_fires_first() {
        let shas = stub_shas(2_000);
        let lapses = std::cell::Cell::new(0_u32);
        let error = walk_with_retry(&shas, |slice| {
            let commits: Vec<CommitInfo> = slice.iter().map(|sha| stub_commit(sha)).collect();
            // Slow for anything wider than a hundred commits, so the ladder
            // reaches its depth limit on several sibling branches rather than
            // on one.
            let lapsed = slice.len() > 100;
            if lapsed {
                lapses.set(lapses.get() + 1);
            }
            async move {
                Ok(if lapsed {
                    ChunkOutcome::Lapsed
                } else {
                    ChunkOutcome::Exact(commits)
                })
            }
        })
        .await
        .expect_err("this fixture cannot finish and must say so");
        assert!(budget_lapsed(&error), "{error:#}");
        assert_eq!(
            lapses.get(),
            MAX_WALK_CHUNK_LAPSES,
            "the walk must stop at its lapse budget, not run the whole tree"
        );
    }

    /// The one case that still has to abandon exactness, unchanged.
    #[tokio::test]
    async fn an_unreadable_slice_still_marks_the_batch_inexact() {
        let shas = stub_shas(8);
        let walk = walk_with_retry(&shas, |slice| {
            let commits: Vec<CommitInfo> = slice.iter().map(|sha| stub_commit(sha)).collect();
            async move { Ok(ChunkOutcome::PathsOnly(commits)) }
        })
        .await
        .unwrap();
        assert!(walk.incomplete_objects);
        assert_eq!(walk.commits.len(), shas.len());
    }

    /// The ladder has to shrink and then stop; an unbounded one would let a
    /// pathological repository spawn a subprocess per commit forever.
    #[test]
    fn the_retry_ladder_halves_and_terminates() {
        let mut len = METADATA_BATCH_COMMITS;
        let mut splits = 0;
        while let Some(mid) = retry_split(len, splits) {
            assert!(mid < len);
            // `split_at(mid)` leaves the larger half at `mid`, so that is what
            // bounds the worst slice the ladder ever hands a subprocess.
            len = mid;
            splits += 1;
        }
        assert_eq!(splits, MAX_WALK_CHUNK_SPLITS);
        assert!(len <= METADATA_BATCH_COMMITS / 16);
        assert_eq!(retry_split(1, 0), None, "a single commit cannot be halved");
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
    fn repo_label_reads_owner_and_name_from_the_clone_path() {
        assert_eq!(
            repo_label(Path::new("/data/repos/facebook/react.git")),
            "facebook/react"
        );
        assert_eq!(repo_label(Path::new("react.git")), "react");
    }

    /// Build the byte stream `git log --raw -z --no-renames --no-abbrev
    /// --format=<header>` produces for one commit: the header block, then the
    /// newline git writes before the first raw entry, then `entry\0path\0`
    /// pairs. Entries are `(src_oid, dst_oid, status, path)`.
    fn raw_metadata_record(
        sha: &str,
        parents: &str,
        subject: &str,
        entries: &[(&str, &str, &str, &str)],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(COMMIT_SENTINEL);
        for field in [
            sha,
            parents,
            "Alice@Example.com",
            "Alice",
            "2021-01-01T00:00:00+00:00",
            subject,
        ] {
            bytes.extend_from_slice(field.as_bytes());
            bytes.push(0);
        }
        for (index, (src, dst, status, path)) in entries.iter().enumerate() {
            if index == 0 {
                bytes.push(b'\n');
            }
            bytes.extend_from_slice(format!(":100644 100644 {src} {dst} {status}").as_bytes());
            bytes.push(0);
            bytes.extend_from_slice(path.as_bytes());
            bytes.push(0);
        }
        bytes
    }

    /// The degraded walk must keep every changed path exact — ownership,
    /// coupling and change-frequency all read the path set — and must not
    /// invent line movement it cannot know without downloading the blobs.
    #[test]
    fn path_only_walk_keeps_paths_and_reports_no_line_movement() {
        let before = "a".repeat(40);
        let after = "b".repeat(40);
        let zero = "0".repeat(40);
        let added = "c".repeat(40);
        let mut stream = raw_metadata_record(
            "abc123",
            "p0",
            "fix: parser",
            &[
                (&before, &after, "M", "src/lib.rs"),
                (&zero, &added, "A", "src/new file.rs"),
            ],
        );
        stream.extend_from_slice(&raw_metadata_record("def456", "p1", "docs", &[]));

        let parsed = parse_raw_metadata_records(&stream);
        assert_eq!(parsed.len(), 2);
        let first = &parsed[0];
        assert_eq!(first.sha, "abc123");
        assert_eq!(first.author_email, "alice@example.com");
        assert!(first.is_fix);
        assert_eq!(first.paths_changed, vec!["src/lib.rs", "src/new file.rs"]);
        assert_eq!((first.lines_added, first.lines_deleted), (0, 0));
        assert_eq!(
            first.binary_files, 0,
            "a path whose blob was never read is not evidence of a binary file"
        );
        assert!(parsed[1].paths_changed.is_empty());
    }

    /// A root commit's whole tree is not a change set, exactly as in the
    /// numstat walk.
    #[test]
    fn path_only_walk_suppresses_root_commit_paths() {
        let zero = "0".repeat(40);
        let added = "c".repeat(40);
        let stream = raw_metadata_record("root1", "", "init", &[(&zero, &added, "A", "a.txt")]);
        let parsed = parse_raw_metadata_records(&stream);
        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].paths_changed.is_empty());
    }

    /// `-z` paths are raw and unquoted, so a committed file can be named like
    /// a raw entry header. Path segments are consumed by position.
    #[test]
    fn path_only_walk_ignores_paths_that_look_like_entries() {
        let before = "a".repeat(40);
        let after = "b".repeat(40);
        let decoy = format!(":100644 100644 {} {} M", "c".repeat(40), "d".repeat(40));
        let stream = raw_metadata_record("abc123", "p0", "edit", &[(&before, &after, "M", &decoy)]);
        let parsed = parse_raw_metadata_records(&stream);
        assert_eq!(parsed[0].paths_changed, vec![decoy]);
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
        let plan = plan_commits(&handle, None).await.unwrap();
        assert!(!plan.requires_full_rebuild());
        assert!(plan.plan().shas.is_empty());
        assert!(!plan.plan().truncated);
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

    /// Progress redraws must not bury the one line that says what went wrong.
    #[test]
    fn git_failure_detail_drops_progress_frames() {
        let stderr = b"Counting objects:  42% (100/238)\rCounting objects: 100% (238/238), done.\n\
            remote: Enumerating objects: 55%\r\
            fatal: couldn't find remote ref refs/heads/main\n";
        let detail = git_failure_detail(stderr);
        assert!(detail.contains("couldn't find remote ref refs/heads/main"));
        assert!(!detail.contains("42%"));
        assert!(!detail.contains("55%"));
    }

    /// The liveness contract, both halves: a subprocess that is writing
    /// progress beats, and one that has gone silent does not.
    #[cfg(test)]
    mod liveness {
        use super::*;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        fn shell(script: &str) -> Command {
            let mut command = Command::new("sh");
            command.args(["-c", script]).kill_on_drop(true);
            command
        }

        #[tokio::test]
        async fn bytes_on_the_progress_stream_beat_liveness() {
            let beats = Arc::new(AtomicUsize::new(0));
            let counter = beats.clone();
            let tick = move || {
                counter.fetch_add(1, Ordering::Relaxed);
            };
            let output = output_within_progress(
                shell("printf 'Receiving objects: 50%%\\r' >&2; printf 'done.\\n' >&2"),
                Duration::from_secs(30),
                Some(&tick),
            )
            .await
            .unwrap()
            .expect("well inside the budget");
            assert!(output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("done."));
            assert!(beats.load(Ordering::Relaxed) >= 1);
        }

        /// The entire point of reading real evidence rather than running a
        /// ticker: a transfer that has stopped delivering data must stay
        /// silent so the caller's stall guard can still kill it.
        #[tokio::test]
        async fn a_wedged_subprocess_never_beats() {
            let beats = Arc::new(AtomicUsize::new(0));
            let counter = beats.clone();
            let tick = move || {
                counter.fetch_add(1, Ordering::Relaxed);
            };
            let lapsed =
                output_within_progress(shell("sleep 30"), Duration::from_millis(250), Some(&tick))
                    .await
                    .unwrap();
            assert!(lapsed.is_none(), "the wall-clock ceiling still applies");
            assert_eq!(beats.load(Ordering::Relaxed), 0);
        }
    }

    /// Incremental-cursor validation against a real repository.
    ///
    /// `{cursor}..HEAD` misreports both of the ways a stored cursor goes bad,
    /// and the second one misreports it *silently*: after a force-push or a
    /// rebase the range still exits 0 with a plausible commit list, which
    /// appended to aggregates that already counted the rewritten commits
    /// drifts every total upward permanently.
    #[cfg(test)]
    mod cursor {
        use super::*;
        use std::process::Command as SyncCommand;

        fn git_available() -> bool {
            SyncCommand::new("git")
                .arg("--version")
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        }

        fn git_stdout(dir: &Path, args: &[&str]) -> String {
            let output = SyncCommand::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .output()
                .expect("spawn git");
            assert!(
                output.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }

        /// Three commits on `main`, returned with its handle.
        fn fixture(dir: &Path) -> RepoHandle {
            git_stdout(dir, &["init", "-q"]);
            git_stdout(dir, &["config", "user.email", "a@example.com"]);
            git_stdout(dir, &["config", "user.name", "Alice"]);
            git_stdout(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
            for step in 0..3 {
                std::fs::write(dir.join("a.txt"), format!("line {step}\n")).unwrap();
                git_stdout(dir, &["add", "-A"]);
                git_stdout(dir, &["commit", "-q", "-m", &format!("commit {step}")]);
            }
            RepoHandle {
                path: dir.to_path_buf(),
                head_sha: git_stdout(dir, &["rev-parse", "HEAD"]),
            }
        }

        #[tokio::test]
        async fn a_genuine_ancestor_yields_exactly_the_commits_in_between() {
            if !git_available() {
                eprintln!("skipping: git not available");
                return;
            }
            let tmp = tempfile::tempdir().unwrap();
            let handle = fixture(tmp.path());
            let full = plan_commits(&handle, None).await.unwrap().into_plan();
            assert_eq!(full.shas.len(), 3);

            let cursor = full.shas[0].clone();
            let planned = plan_commits(&handle, Some(&cursor)).await.unwrap();
            assert_eq!(planned.rejection(), None);
            assert!(!planned.requires_full_rebuild());
            assert_eq!(planned.plan().shas, full.shas[1..].to_vec());
        }

        #[tokio::test]
        async fn a_cursor_whose_object_is_absent_is_rejected() {
            if !git_available() {
                eprintln!("skipping: git not available");
                return;
            }
            let tmp = tempfile::tempdir().unwrap();
            let handle = fixture(tmp.path());
            let full = plan_commits(&handle, None).await.unwrap().into_plan();

            // The shape a quota eviction or a post-force-push gc leaves behind:
            // a well-formed object id that names nothing locally. `rev-list`
            // would fail outright on `{sha}..HEAD`.
            let evicted = "0".repeat(40);
            let plan = plan_commits(&handle, Some(&evicted)).await.unwrap();
            assert_eq!(plan.rejection(), Some(CursorRejection::Missing));
            assert!(plan.requires_full_rebuild());
            assert_eq!(
                plan.plan().shas,
                full.shas,
                "a rejected cursor plans complete history, not an empty append"
            );

            // A cursor that is not an object id at all never reaches git.
            let sentinel = plan_commits(&handle, Some(EMPTY_REPOSITORY_HEAD))
                .await
                .unwrap();
            assert_eq!(sentinel.rejection(), Some(CursorRejection::Missing));
        }

        #[tokio::test]
        async fn a_cursor_that_is_not_an_ancestor_of_head_is_rejected() {
            if !git_available() {
                eprintln!("skipping: git not available");
                return;
            }
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path();
            let handle = fixture(dir);
            let full = plan_commits(&handle, None).await.unwrap().into_plan();

            // A rewritten commit, exactly as a rebase or force-push leaves one:
            // a real object, reachable from nothing, parented on the branch's
            // first commit. `commit-tree` builds it without moving any ref.
            let tree = git_stdout(dir, &["rev-parse", "HEAD^{tree}"]);
            let diverged = git_stdout(
                dir,
                &["commit-tree", &tree, "-p", &full.shas[0], "-m", "rewritten"],
            );
            assert_ne!(diverged, handle.head_sha);
            // The trap this guards: the range is not an error, so nothing
            // downstream would ever notice.
            let range = SyncCommand::new("git")
                .arg("-C")
                .arg(dir)
                .args(["rev-list", &format!("{diverged}..HEAD")])
                .output()
                .unwrap();
            assert!(
                range.status.success(),
                "a diverged cursor still produces a plausible commit list"
            );

            let plan = plan_commits(&handle, Some(&diverged)).await.unwrap();
            assert_eq!(plan.rejection(), Some(CursorRejection::Diverged));
            assert!(plan.requires_full_rebuild());
            assert_eq!(plan.plan().shas, full.shas);
        }
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

            // Complete history, never a window: every non-merge commit, in
            // order, and nothing reported as truncated.
            let full = plan_commits(&handle, None).await.unwrap().into_plan();
            assert_eq!(full.shas.len(), 7);
            assert!(!full.truncated);
            assert_eq!(full.shas.last(), Some(&handle.head_sha));

            // Re-analysis walks only what the cursor does not already cover.
            // This is the whole steady-state argument for a complete clone: a
            // repository analyzed at an older head costs the new commits, not
            // the repository.
            let fourth = full.shas[3].clone();
            let incremental = plan_commits(&handle, Some(&fourth)).await.unwrap();
            assert!(!incremental.requires_full_rebuild());
            assert_eq!(incremental.plan().shas, full.shas[4..].to_vec());
            let unchanged = plan_commits(&handle, Some(&handle.head_sha)).await.unwrap();
            assert!(!unchanged.requires_full_rebuild());
            assert!(unchanged.plan().shas.is_empty());

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

            let walk = walk_commit_metadata_batch(&handle, &shas).await.unwrap();
            assert!(
                !walk.incomplete_objects,
                "a complete clone reads every blob it needs locally"
            );
            let metadata = walk.commits;
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
                // TODO markers, and diffing a binary reads both sides of it
                // for a line it discards). So its path set is a subset.
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

            // Splitting the walk across cores is only safe because the chunks
            // are explicit ordered SHA lists. The fixture is far below
            // `MIN_WALK_CHUNK_COMMITS`, so drive the chunk walker directly at
            // a size that forces several of them and compare the whole
            // serialized result, not a chosen field.
            let mut chunked = Vec::new();
            for chunk in shas.chunks(2) {
                chunked.extend(
                    walk_metadata_chunk(dir, chunk, Duration::from_secs(300))
                        .await
                        .unwrap()
                        .commits,
                );
            }
            assert_eq!(
                serde_json::to_string(&chunked).unwrap(),
                serde_json::to_string(&metadata).unwrap(),
                "a walk split across cores must be byte-identical to a serial one"
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

            // The degraded walk is what a damaged object store gets instead of
            // a failed analysis. Every path-shaped signal must survive it
            // exactly; only line movement is given up.
            let path_only = walk_commit_paths_chunk(dir, &shas).await.unwrap();
            assert_eq!(path_only.len(), metadata.len());
            for (degraded, exact) in path_only.iter().zip(metadata.iter()) {
                assert_eq!(degraded.sha, exact.sha);
                assert_eq!(degraded.author_email, exact.author_email);
                assert_eq!(degraded.committed_at, exact.committed_at);
                assert_eq!(degraded.is_fix, exact.is_fix);
                let mut degraded_paths = degraded.paths_changed.clone();
                let mut exact_paths = exact.paths_changed.clone();
                degraded_paths.sort();
                exact_paths.sort();
                assert_eq!(degraded_paths, exact_paths);
                assert_eq!((degraded.lines_added, degraded.lines_deleted), (0, 0));
            }
            assert!(
                metadata.iter().any(|commit| commit.lines_added > 0),
                "the exact walk it degrades from does report line movement"
            );
        }
    }
}
