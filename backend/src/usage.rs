//! "Stars vs. real usage" data pipeline.
//!
//! Given an `owner/repo`, we (1) resolve package identifiers on the major
//! registries, then (2) fetch download/usage metrics for each resolved
//! package. Both halves are *best-effort*: a failure to resolve a package,
//! or a registry that errors / times out, never fails the whole request —
//! the offending source is simply omitted (or, for downloads, falls back
//! to stale-but-cached data).
//!
//! Supported sources are npm, crates.io, PyPI, and Docker Hub. Go has no
//! public download metric and is deliberately omitted.
//!
//! Every external response is normalized to [`DownloadStats`] by a pure
//! (body → stats) function — the `normalize_*` family, unit-tested against
//! inline fixtures — and cached in Postgres (`usage_cache`) with a
//! daily-ish TTL so origin load + registry rate-limit exposure stay low.
//! All outbound calls send a descriptive
//! `User-Agent: gitdebt (+https://gitdebt.com)`.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::cache::Cache;
use crate::repo_history::RepoStorage;

/// Descriptive UA. crates.io 403s a blank/library-default UA; npm + PyPI +
/// Docker Hub are lenient but we send it everywhere for politeness +
/// traceability.
const USER_AGENT: &str = "gitdebt (+https://gitdebt.com)";

/// Cache TTL for external download data. Registries refresh download counts
/// at most daily, so anything shorter just burns rate budget. 18h gives us
/// at least one refresh per calendar day while smoothing thundering herds.
const USAGE_TTL_HOURS: i64 = 18;

/// Per-request timeout for any single registry call. Kept well under the
/// 60s global request timeout so a slow registry degrades to "source
/// omitted" rather than stalling the whole `/usage` response.
const REGISTRY_TIMEOUT_SECS: u64 = 10;

/// Max points kept in any normalized series. Mirrors the chart's downsample
/// cap so the JSON payload + rendered path stay bounded. Even index
/// sampling, always keeping first + last.
pub const MAX_USAGE_POINTS: usize = 400;

/// One daily download data point. `date` is `YYYY-MM-DD` (registry-native).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DownloadPoint {
    pub date: String,
    pub downloads: u64,
}

/// Normalized per-source download stats. `series` is empty for sources that
/// only expose a lifetime total (Docker Hub). `total` is the source's
/// lifetime total when it reports one, else the sum of the series —
/// computed over the FULL series before any downsampling, so capping the
/// point count never shrinks the reported total.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DownloadStats {
    pub total: u64,
    pub series: Vec<DownloadPoint>,
}

/// Which registries a repo resolved to. Each is `Some(package_id)` when we
/// found (or were given) an identifier, else `None`. Serialized as the
/// `resolved` object in the `/usage` response.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Resolved {
    pub npm: Option<String>,
    /// Serialized as `crate` (a Rust keyword, so the field is `crate_`).
    #[serde(rename = "crate")]
    pub crate_: Option<String>,
    pub pypi: Option<String>,
    pub docker: Option<String>,
}

impl Resolved {
    /// True if no registry resolved — the caller renders a "no package
    /// downloads found" affordance instead of an empty overlay.
    pub fn is_empty(&self) -> bool {
        self.npm.is_none() && self.crate_.is_none() && self.pypi.is_none() && self.docker.is_none()
    }
}

/// Explicit per-registry overrides parsed from query params (`?npm=`,
/// `?crate=`, `?pypi=`, `?docker=`). Any present value short-circuits
/// resolution for that registry.
#[derive(Debug, Clone, Default)]
pub struct UsageOverrides {
    pub npm: Option<String>,
    pub crate_: Option<String>,
    pub pypi: Option<String>,
    pub docker: Option<String>,
}

// Package resolution

/// Resolve package identifiers for `owner/repo`, best-effort.
///
/// Priority: explicit override → a publishable root manifest in the local
/// clone. There is deliberately no repo-name heuristic: a registry package
/// that happens to share a repository name is not proof of ownership. A
/// *present* override pins its registry: if the
/// value fails validation the registry resolves to `None` (source omitted)
/// rather than falling back — see [`assemble_resolved`]. The repo-name
/// download fetch still confirms that the declared package exists, omitting
/// the source on a 404. Docker images have no trustworthy root-manifest
/// declaration here, so they appear only through an explicit override.
pub async fn resolve_packages(
    owner: &str,
    repo: &str,
    overrides: &UsageOverrides,
    storage: &RepoStorage,
) -> Resolved {
    // Manifest probe only when at least one registry isn't overridden — no
    // point shelling to git if every field is pinned.
    let manifest = if overrides_cover_all(overrides) {
        ManifestNames::default()
    } else {
        read_manifest_names(owner, repo, storage).await
    };
    assemble_resolved(owner, repo, overrides, manifest)
}

/// Pure assembly of the final [`Resolved`] set from verified declarations.
/// Precedence per registry: explicit override > root manifest name.
///
/// Every candidate is filtered through [`valid_pkg`] / [`clean_docker`]
/// before it can reach a registry URL: overrides (attacker-controlled query
/// params) and the manifest-derived names (parsers already filter, but
/// belt-and-braces). A crafted value like
/// `react/../../admin` would otherwise traverse the registry URL path
/// (SSRF / path-traversal).
///
/// Override semantics: a *present* override pins the registry. If it fails
/// validation, the registry resolves to `None` (omitted) — we never fall
/// back to a different package under an explicit override, because
/// rendering the manifest package's numbers while the URL names another
/// package would misattribute data (and the invalid value itself must never
/// reach a registry URL).
fn assemble_resolved(
    _owner: &str,
    _repo: &str,
    overrides: &UsageOverrides,
    manifest: ManifestNames,
) -> Resolved {
    let pick = |ov: &Option<String>, manifest_name: Option<String>| match ov {
        Some(v) => clean_pkg(Some(v)),
        None => manifest_name.and_then(|name| clean_pkg(Some(&name))),
    };
    Resolved {
        npm: pick(&overrides.npm, manifest.npm),
        crate_: pick(&overrides.crate_, manifest.crate_),
        pypi: pick(&overrides.pypi, manifest.pypi),
        // Docker has no trustworthy manifest probe in this pipeline.
        docker: match &overrides.docker {
            Some(v) => clean_docker(Some(v)),
            None => None,
        },
    }
}

/// Validate an optional override/name as a registry package id; `None` (or
/// a value that fails [`valid_pkg`]) yields `None`.
fn clean_pkg(name: Option<&str>) -> Option<String> {
    let name = name?.trim();
    valid_pkg(name).then(|| name.to_string())
}

/// Validate a Docker Hub reference: `namespace/repo` or a bare `repo`
/// (mapped to `library/repo` at fetch time). Each side must be a clean
/// single segment — this blocks `react/../../admin`-style traversal in the
/// `?docker=` override.
fn clean_docker(name: Option<&str>) -> Option<String> {
    let name = name?.trim();
    if name.is_empty() || name.len() > 128 {
        return None;
    }
    let ok = match name.split_once('/') {
        Some((ns, repo)) => valid_segment(ns) && valid_segment(repo),
        None => valid_segment(name),
    };
    ok.then(|| name.to_string())
}

fn overrides_cover_all(o: &UsageOverrides) -> bool {
    o.npm.is_some() && o.crate_.is_some() && o.pypi.is_some()
}

/// Package names harvested from manifests in a repo clone.
#[derive(Debug, Default)]
struct ManifestNames {
    npm: Option<String>,
    crate_: Option<String>,
    pypi: Option<String>,
}

/// Best-effort manifest read from the bare clone the debt pipeline already
/// keeps. We never clone here — if the clone is absent (never analyzed, or
/// evicted) we return empties and omit unverifiable package associations.
/// Bare clones have no working tree, so we read blobs via `git show
/// HEAD:<path>`.
async fn read_manifest_names(owner: &str, repo: &str, storage: &RepoStorage) -> ManifestNames {
    let full = format!("{owner}/{repo}");
    let path = storage.path_for(&full);
    if !path.exists() {
        return ManifestNames::default();
    }
    let mut names = ManifestNames::default();

    if let Some(content) = git_show(&path, "package.json").await {
        names.npm = parse_package_json_name(&content);
    }
    if let Some(content) = git_show(&path, "Cargo.toml").await {
        names.crate_ = parse_cargo_toml_name(&content);
    }
    // PyPI: prefer pyproject.toml, then setup.cfg, then setup.py.
    if let Some(content) = git_show(&path, "pyproject.toml").await {
        names.pypi = parse_pyproject_name(&content);
    }
    if names.pypi.is_none()
        && let Some(content) = git_show(&path, "setup.cfg").await
    {
        names.pypi = parse_setup_cfg_name(&content);
    }
    if names.pypi.is_none()
        && let Some(content) = git_show(&path, "setup.py").await
    {
        names.pypi = parse_setup_py_name(&content);
    }
    names
}

/// `git show HEAD:<file>` against a bare clone. Returns `None` on any error
/// (missing file, git failure) — manifests are optional.
async fn git_show(repo_path: &Path, file: &str) -> Option<String> {
    use tokio::process::Command;
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["show", &format!("HEAD:{file}")])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).to_string();
    (!s.trim().is_empty()).then_some(s)
}

/// Pull the top-level `"name"` field out of a package.json. Hand-parsed
/// (no serde_json dependency on the manifest's full shape) but robust: we
/// decode the whole thing as a JSON value and read `.name`.
fn parse_package_json_name(content: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(content).ok()?;
    if v.get("private").and_then(serde_json::Value::as_bool) == Some(true) {
        return None;
    }
    let name = v.get("name")?.as_str()?.trim();
    valid_pkg(name).then(|| name.to_string())
}

/// `[package] name = "..."` from a Cargo.toml. We scan section-by-section
/// rather than pulling in a TOML parser — the manifest read is best-effort
/// and the `[package]` table's `name` is a stable, simple shape.
fn parse_cargo_toml_name(content: &str) -> Option<String> {
    let mut in_package = false;
    for line in content.lines() {
        let line = strip_toml_comment(line).trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if in_package && let Some(name) = toml_kv(line, "name") {
            return valid_pkg(&name).then_some(name);
        }
    }
    None
}

/// `[project] name = "..."` (PEP 621) or `[tool.poetry] name = "..."` from
/// a pyproject.toml.
fn parse_pyproject_name(content: &str) -> Option<String> {
    let mut section = String::new();
    for line in content.lines() {
        let line = strip_toml_comment(line).trim();
        if line.starts_with('[') {
            section = line.trim_matches(|c| c == '[' || c == ']').to_string();
            continue;
        }
        if (section == "project" || section == "tool.poetry")
            && let Some(name) = toml_kv(line, "name")
        {
            return valid_pkg(&name).then_some(name);
        }
    }
    None
}

/// `name = ...` under `[metadata]` in a setup.cfg (INI format).
fn parse_setup_cfg_name(content: &str) -> Option<String> {
    let mut in_metadata = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_metadata = trimmed == "[metadata]";
            continue;
        }
        if in_metadata && let Some(rest) = trimmed.strip_prefix("name") {
            let rest = rest.trim_start();
            if let Some(val) = rest.strip_prefix('=') {
                let name = val.trim().trim_matches(|c| c == '"' || c == '\'');
                if valid_pkg(name) {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}

/// `name="..."` / `name='...'` from a setup.py `setup(...)` call. Crude but
/// covers the overwhelmingly common single-line literal form. We scan every
/// `name` occurrence at a word boundary (so `package_name=` or a mention of
/// "name" in a comment doesn't shadow the real kwarg) and take the first
/// one followed by `=` and a quoted literal. We bail to the repo-name
/// heuristic for dynamic/computed names.
fn parse_setup_py_name(content: &str) -> Option<String> {
    let bytes = content.as_bytes();
    for (idx, _) in content.match_indices("name") {
        // Word boundary on the left: reject `package_name`, `filename`, …
        if idx > 0 {
            let prev = bytes[idx - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                continue;
            }
        }
        // `names…` / `name_of…` etc. fail the `=` check below naturally.
        let after = content[idx + 4..].trim_start();
        let Some(after) = after.strip_prefix('=') else {
            continue;
        };
        let after = after.trim_start();
        let Some(quote) = after.chars().next() else {
            continue;
        };
        if quote != '"' && quote != '\'' {
            continue;
        }
        let rest = &after[1..];
        let Some(end) = rest.find(quote) else {
            continue;
        };
        let name = &rest[..end];
        if valid_pkg(name) {
            return Some(name.to_string());
        }
    }
    None
}

/// Extract a quoted/bare TOML scalar `key = value`. Returns the unquoted
/// value when `line` is exactly `key = ...`.
fn toml_kv(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.trim_start();
    let rest = rest.strip_prefix('=')?.trim();
    let val = rest.trim_matches(|c| c == '"' || c == '\'').trim();
    (!val.is_empty()).then(|| val.to_string())
}

fn strip_toml_comment(line: &str) -> &str {
    match line.find('#') {
        Some(i) => &line[..i],
        None => line,
    }
}

/// Reject package names that could escape a registry URL path or carry
/// junk. The fetch's 404 handling is the *final* validator (it confirms the
/// package exists), but this is the SECURITY boundary: every value that
/// reaches a `format!("https://.../{pkg}/...")` URL — query overrides,
/// and manifest-derived names — passes through here
/// first. A crafted value like `react/../../admin` would otherwise traverse
/// the registry URL path (SSRF / path-traversal).
///
/// Rules (deliberately stricter than each registry's own grammar):
///   * non-empty after trim, length ≤ 128
///   * no `..` anywhere (path traversal), no control chars, no space, no `%`
///     (URL-encoding smuggling), no `{` (template placeholders)
///   * at most a SINGLE leading `@scope/` segment (npm scoped packages);
///     any other `/` is rejected so a name can't add URL path segments
///   * must not start with `.` or `-`
///   * remaining chars limited to `[A-Za-z0-9._-]` (plus the one allowed
///     scope `@`/`/`)
fn valid_pkg(name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() || name.len() > 128 {
        return false;
    }
    if name.contains("..")
        || name.contains(' ')
        || name.contains('%')
        || name.contains('{')
        || name.bytes().any(|b| b.is_ascii_control())
    {
        return false;
    }
    // Strip at most one leading `@scope/` segment; what remains must carry
    // no further slashes or `@`.
    let core = if let Some(rest) = name.strip_prefix('@') {
        let Some((scope, pkg)) = rest.split_once('/') else {
            // A bare `@something` with no `/` is not a valid scoped name.
            return false;
        };
        if scope.is_empty() || pkg.is_empty() {
            return false;
        }
        if !valid_segment(scope) {
            return false;
        }
        pkg
    } else {
        name
    };
    // The core (post-scope) must be a single clean segment: no `/`, no `@`.
    valid_segment(core)
}

/// A single path segment: `[A-Za-z0-9._-]+`, not starting with `.` or `-`,
/// not exactly `.`/`..`.
fn valid_segment(seg: &str) -> bool {
    if seg.is_empty() || seg == "." || seg == ".." {
        return false;
    }
    if seg.starts_with('.') || seg.starts_with('-') {
        return false;
    }
    seg.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

// Download fetchers (cached, best-effort)

fn registry_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(REGISTRY_TIMEOUT_SECS))
        .build()
        .unwrap_or_default()
}

/// Narrow view of the `usage_cache` table used by [`cached_or_fetch`].
/// Exists so the cache-or-fetch policy (fresh hit → skip fetch; live error
/// → stale fallback; genuine 404 → omit) is unit-testable without Postgres.
/// [`Cache`] is the production implementation; the trait stays private to
/// this module.
#[async_trait::async_trait]
trait UsageStore {
    async fn get_fresh(
        &self,
        source: &str,
        package: &str,
        ttl: chrono::Duration,
    ) -> Result<Option<String>>;
    async fn get_any(&self, source: &str, package: &str) -> Result<Option<String>>;
    async fn put(&self, source: &str, package: &str, body: &str) -> Result<()>;
}

#[async_trait::async_trait]
impl UsageStore for Cache {
    async fn get_fresh(
        &self,
        source: &str,
        package: &str,
        ttl: chrono::Duration,
    ) -> Result<Option<String>> {
        self.get_usage_fresh(source, package, ttl).await
    }
    async fn get_any(&self, source: &str, package: &str) -> Result<Option<String>> {
        self.get_usage_any(source, package).await
    }
    async fn put(&self, source: &str, package: &str, body: &str) -> Result<()> {
        self.put_usage(source, package, body).await
    }
}

/// Cache-or-fetch a single source's [`DownloadStats`]. Returns `None` when
/// the source genuinely has no data (unresolved 404) AND nothing cached;
/// returns stale cached data when a live refresh fails. Never errors — the
/// `/usage` endpoint must not fail because one registry is down.
async fn cached_or_fetch<S, F, Fut>(
    store: &S,
    source: &str,
    package: &str,
    fetch: F,
) -> Option<DownloadStats>
where
    S: UsageStore,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<Option<DownloadStats>>>,
{
    let ttl = chrono::Duration::hours(USAGE_TTL_HOURS);
    // Fresh cache hit. A corrupt (undeserializable) body falls through to a
    // live fetch, whose result overwrites the bad row.
    if let Ok(Some(body)) = store.get_fresh(source, package, ttl).await
        && let Ok(stats) = serde_json::from_str::<DownloadStats>(&body)
    {
        return Some(stats);
    }
    // Miss / stale → live fetch.
    match fetch().await {
        Ok(Some(stats)) => {
            if let Ok(body) = serde_json::to_string(&stats)
                && let Err(e) = store.put(source, package, &body).await
            {
                tracing::warn!(source, package, error = %e, "put_usage failed");
            }
            Some(stats)
        }
        Ok(None) => {
            // Source reported no data (e.g. 404 package). Don't keep a
            // stale row alive — if we previously had data and the package
            // truly vanished, returning stale is misleading. But a
            // transient 404 is rare; prefer "omit" over "stale" here.
            tracing::debug!(source, package, "no usage data");
            None
        }
        Err(e) => {
            // Live fetch failed (network/timeout/5xx). Degrade to stale
            // cached data if we have any — last-known beats nothing.
            tracing::debug!(source, package, error = %e, "usage fetch failed; trying stale");
            match store.get_any(source, package).await {
                Ok(Some(body)) => serde_json::from_str::<DownloadStats>(&body).ok(),
                _ => None,
            }
        }
    }
}

/// Fetch all resolved sources' download stats concurrently. Each is
/// independently cached + best-effort; an unresolved or failing source maps
/// to `None`.
pub async fn fetch_all(cache: &Cache, resolved: &Resolved) -> UsageDownloads {
    let client = registry_client();

    let npm_fut = async {
        match &resolved.npm {
            Some(pkg) => cached_or_fetch(cache, "npm", pkg, || fetch_npm(&client, pkg)).await,
            None => None,
        }
    };
    let crates_fut = async {
        match &resolved.crate_ {
            Some(pkg) => cached_or_fetch(cache, "crates", pkg, || fetch_crates(&client, pkg)).await,
            None => None,
        }
    };
    let pypi_fut = async {
        match &resolved.pypi {
            Some(pkg) => cached_or_fetch(cache, "pypi", pkg, || fetch_pypi(&client, pkg)).await,
            None => None,
        }
    };
    let docker_fut = async {
        match &resolved.docker {
            Some(pkg) => cached_or_fetch(cache, "docker", pkg, || fetch_docker(&client, pkg)).await,
            None => None,
        }
    };

    let (npm, crates, pypi, docker) =
        futures::future::join4(npm_fut, crates_fut, pypi_fut, docker_fut).await;
    UsageDownloads {
        npm,
        crates,
        pypi,
        docker,
    }
}

/// Per-source download results, serialized as the `downloads` object.
#[derive(Debug, Clone, Default, Serialize)]
pub struct UsageDownloads {
    pub npm: Option<DownloadStats>,
    pub crates: Option<DownloadStats>,
    pub pypi: Option<DownloadStats>,
    pub docker: Option<DownloadStats>,
}

/// Shared tail for the daily-series normalizers: an empty series is "no
/// data" (`None`); otherwise `total` is the saturating sum of the FULL
/// series (computed before the point cap so downsampling never shrinks the
/// reported total) and the series is capped at [`MAX_USAGE_POINTS`].
fn finish_series(series: Vec<DownloadPoint>) -> Option<DownloadStats> {
    if series.is_empty() {
        return None;
    }
    let total = series
        .iter()
        .fold(0u64, |acc, p| acc.saturating_add(p.downloads));
    Some(DownloadStats {
        total,
        series: downsample_points(series, MAX_USAGE_POINTS),
    })
}

// npm
// Verified shape (GET api.npmjs.org/downloads/range/{from}:{to}/{pkg}):
//   { "start": "YYYY-MM-DD", "end": "YYYY-MM-DD", "package": "react",
//     "downloads": [ { "downloads": 1267159, "day": "2024-01-01" }, ... ] }
// A single range call covers at most ~18 months; for older history we'd
// loop. We fetch the last ~18 months in one call — plenty for the overlay
// + total trend without hammering the registry. Scoped `@scope/pkg` works
// directly in the path.

#[derive(Deserialize)]
struct NpmRange {
    #[serde(default)]
    downloads: Vec<NpmDay>,
}

#[derive(Deserialize)]
struct NpmDay {
    downloads: u64,
    day: String,
}

fn npm_range_url(pkg: &str, start: chrono::NaiveDate, end: chrono::NaiveDate) -> String {
    format!("https://api.npmjs.org/downloads/range/{start}:{end}/{pkg}")
}

/// Normalize an npm range response: drop zero-download days, sort by date.
/// npm is the one source whose ordering we'd otherwise trust blindly — a
/// stable sort on ISO dates is a no-op for well-formed input and protects
/// the cumulative overlay (running totals would attach to the wrong day on
/// unordered input). Pure; unit-tested against inline fixtures.
fn normalize_npm(body: NpmRange) -> Option<DownloadStats> {
    let mut series: Vec<DownloadPoint> = body
        .downloads
        .into_iter()
        .filter(|d| d.downloads > 0)
        .map(|d| DownloadPoint {
            date: d.day,
            downloads: d.downloads,
        })
        .collect();
    series.sort_by(|a, b| a.date.cmp(&b.date));
    finish_series(series)
}

async fn fetch_npm(client: &reqwest::Client, pkg: &str) -> Result<Option<DownloadStats>> {
    // ~18 months back from today (npm's per-call ceiling).
    let end = chrono::Utc::now().date_naive();
    let start = end - chrono::Duration::days(540);
    let url = npm_range_url(pkg, start, end);
    let resp = client.get(&url).send().await?;
    if resp.status().as_u16() == 404 {
        return Ok(None);
    }
    if !resp.status().is_success() {
        anyhow::bail!("npm {} status {}", pkg, resp.status());
    }
    let body: NpmRange = resp.json().await?;
    Ok(normalize_npm(body))
}

// crates.io
// Verified shape (GET crates.io/api/v1/crates/{crate}/downloads):
//   { "version_downloads": [ { "version": <id>, "downloads": 813156,
//                              "date": "2026-03-01" }, ... ],
//     "meta": { "extra_downloads": [ { "date": "...", "downloads": ... } ] } }
// `version_downloads` is per-version per-day; `meta.extra_downloads` is the
// daily total NOT attributable to a still-listed version. To get a true
// daily total we sum version_downloads by date AND add extra_downloads by
// date. REQUIRES a non-default User-Agent (we send one) or the API 403s.

#[derive(Deserialize)]
struct CratesDownloads {
    #[serde(default)]
    version_downloads: Vec<CratesDay>,
    #[serde(default)]
    meta: CratesMeta,
}

#[derive(Deserialize, Default)]
struct CratesMeta {
    #[serde(default)]
    extra_downloads: Vec<CratesDay>,
}

#[derive(Deserialize)]
struct CratesDay {
    downloads: u64,
    date: String,
}

fn crates_downloads_url(pkg: &str) -> String {
    format!("https://crates.io/api/v1/crates/{pkg}/downloads")
}

/// Normalize a crates.io downloads response: sum `version_downloads` and
/// `meta.extra_downloads` by date into one daily-total series (the BTreeMap
/// keys — ISO dates — guarantee ascending order). Pure.
fn normalize_crates(body: CratesDownloads) -> Option<DownloadStats> {
    let mut by_date: BTreeMap<String, u64> = BTreeMap::new();
    for d in body
        .version_downloads
        .into_iter()
        .chain(body.meta.extra_downloads)
    {
        let e = by_date.entry(d.date).or_insert(0);
        *e = e.saturating_add(d.downloads);
    }
    let series: Vec<DownloadPoint> = by_date
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .map(|(date, downloads)| DownloadPoint { date, downloads })
        .collect();
    finish_series(series)
}

async fn fetch_crates(client: &reqwest::Client, pkg: &str) -> Result<Option<DownloadStats>> {
    let url = crates_downloads_url(pkg);
    let resp = client.get(&url).send().await?;
    if resp.status().as_u16() == 404 {
        return Ok(None);
    }
    if !resp.status().is_success() {
        anyhow::bail!("crates {} status {}", pkg, resp.status());
    }
    let body: CratesDownloads = resp.json().await?;
    Ok(normalize_crates(body))
}

// PyPI
// Verified shape (GET pypistats.org/api/packages/{pkg}/overall?mirrors=true):
//   { "data": [ { "category": "with_mirrors", "date": "2025-11-29",
//                 "downloads": 15578423 }, ... ],
//     "package": "numpy", "type": "overall_downloads" }
// With mirrors=true the API returns ONLY `with_mirrors` rows (the all-in
// total), so summing every row is correct and does not double-count. We
// defensively filter to `with_mirrors` in case the API ever returns both.
// Covers ~180 days.

#[derive(Deserialize)]
struct PypiOverall {
    #[serde(default)]
    data: Vec<PypiDay>,
}

#[derive(Deserialize)]
struct PypiDay {
    category: String,
    date: String,
    downloads: u64,
}

fn pypi_overall_url(pkg: &str) -> String {
    format!("https://pypistats.org/api/packages/{pkg}/overall?mirrors=true")
}

/// Normalize a pypistats overall response: keep only `with_mirrors` rows
/// (the inclusive total — never sum a with/without pair into a double
/// count), aggregated by date in ascending order. Pure.
fn normalize_pypi(body: PypiOverall) -> Option<DownloadStats> {
    let mut by_date: BTreeMap<String, u64> = BTreeMap::new();
    for d in body.data {
        if d.category != "with_mirrors" {
            continue;
        }
        let e = by_date.entry(d.date).or_insert(0);
        *e = e.saturating_add(d.downloads);
    }
    let series: Vec<DownloadPoint> = by_date
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .map(|(date, downloads)| DownloadPoint { date, downloads })
        .collect();
    finish_series(series)
}

async fn fetch_pypi(client: &reqwest::Client, pkg: &str) -> Result<Option<DownloadStats>> {
    let url = pypi_overall_url(pkg);
    let resp = client.get(&url).send().await?;
    if resp.status().as_u16() == 404 {
        return Ok(None);
    }
    if !resp.status().is_success() {
        anyhow::bail!("pypi {} status {}", pkg, resp.status());
    }
    let body: PypiOverall = resp.json().await?;
    Ok(normalize_pypi(body))
}

// Docker Hub
// Verified shape (GET hub.docker.com/v2/repositories/{namespace}/{repo}/):
//   { "name": "nginx", "namespace": "library", "pull_count": 13042883291,
//     "star_count": 21289, ... }
// TOTAL pull_count only — Docker Hub exposes no time series, so `series`
// stays empty. `pkg` is `namespace/repo`; a bare `repo` (no slash) is
// rewritten to `library/repo` (Docker's official-image namespace).

#[derive(Deserialize)]
struct DockerRepo {
    #[serde(default)]
    pull_count: u64,
}

/// Registry URL for a validated Docker reference. A bare name maps to the
/// `library` official-image namespace, matching `docker pull nginx`
/// semantics.
fn docker_repo_url(pkg: &str) -> String {
    let path = if pkg.contains('/') {
        pkg.to_string()
    } else {
        format!("library/{pkg}")
    };
    format!("https://hub.docker.com/v2/repositories/{path}/")
}

/// Normalize a Docker Hub repository response: lifetime `pull_count` only,
/// no time series. A zero count is "no data". Pure.
fn normalize_docker(body: DockerRepo) -> Option<DownloadStats> {
    (body.pull_count > 0).then_some(DownloadStats {
        total: body.pull_count,
        series: Vec::new(),
    })
}

async fn fetch_docker(client: &reqwest::Client, pkg: &str) -> Result<Option<DownloadStats>> {
    let url = docker_repo_url(pkg);
    let resp = client.get(&url).send().await?;
    if resp.status().as_u16() == 404 {
        return Ok(None);
    }
    if !resp.status().is_success() {
        anyhow::bail!("docker {} status {}", pkg, resp.status());
    }
    let body: DockerRepo = resp.json().await?;
    Ok(normalize_docker(body))
}

// Helpers

/// Build a cumulative download series (for the overlay chart's right axis)
/// from a daily-counts [`DownloadStats`]. Each `DownloadCumPoint.total` is
/// the running sum up to that day. Returns empty when there's no daily
/// series (e.g. Docker Hub, which only reports a lifetime total). Dates are
/// parsed as `YYYY-MM-DD` at UTC midnight; unparseable dates are skipped
/// (their counts are excluded from the running sum). Points are sorted by
/// date before accumulating — normalizers emit sorted series, but cached
/// blobs written before the sort was introduced may not be, and a running
/// sum over unordered days would attach the wrong totals. Pure and
/// deterministic (stable sort, no clock).
pub fn cumulative_downloads(stats: &DownloadStats) -> Vec<crate::chart::DownloadCumPoint> {
    let mut daily: Vec<(chrono::DateTime<chrono::Utc>, u64)> = stats
        .series
        .iter()
        .filter_map(|p| {
            let date = chrono::NaiveDate::parse_from_str(&p.date, "%Y-%m-%d").ok()?;
            let at = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
                date.and_hms_opt(0, 0, 0)?,
                chrono::Utc,
            );
            Some((at, p.downloads))
        })
        .collect();
    daily.sort_by_key(|(at, _)| *at);
    let mut running = 0u64;
    daily
        .into_iter()
        .map(|(at, downloads)| {
            running = running.saturating_add(downloads);
            crate::chart::DownloadCumPoint { at, total: running }
        })
        .collect()
}

/// Downsample a download series to at most `max_points` via even index
/// sampling, keeping the first + last point whenever `max_points >= 2`.
/// Degenerate caps honor the "at most" contract: `1` keeps only the last
/// point (the full-range cumulative endpoint), `0` empties the series.
/// Mirrors `chart::downsample` but for `DownloadPoint`. The series is
/// assumed date-ordered (the normalizers guarantee it).
pub fn downsample_points(series: Vec<DownloadPoint>, max_points: usize) -> Vec<DownloadPoint> {
    if series.len() <= max_points {
        return series;
    }
    match max_points {
        0 => return Vec::new(),
        1 => return vec![series[series.len() - 1].clone()],
        _ => {}
    }
    let n = series.len();
    let mut out = Vec::with_capacity(max_points);
    for i in 0..(max_points - 1) {
        let idx = i * (n - 1) / (max_points - 1);
        out.push(series[idx].clone());
    }
    out.push(series[n - 1].clone());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_json_name_parsed() {
        let c = r#"{ "name": "react", "version": "18.0.0" }"#;
        assert_eq!(parse_package_json_name(c).as_deref(), Some("react"));
    }

    #[test]
    fn package_json_scoped_name() {
        let c = r#"{ "name": "@scope/pkg" }"#;
        assert_eq!(parse_package_json_name(c).as_deref(), Some("@scope/pkg"));
    }

    #[test]
    fn package_json_no_name_is_none() {
        let c = r#"{ "version": "1.0.0" }"#;
        assert_eq!(parse_package_json_name(c), None);
    }

    #[test]
    fn private_package_json_is_not_a_published_package() {
        let c = r#"{ "name": "workspace-root", "private": true }"#;
        assert_eq!(parse_package_json_name(c), None);
    }

    #[test]
    fn package_json_malicious_name_rejected() {
        // A hostile manifest cannot smuggle a traversal into the URL path.
        let c = r#"{ "name": "../../../internal/admin" }"#;
        assert_eq!(parse_package_json_name(c), None);
        let c2 = r#"{ "name": "pkg/extra/path" }"#;
        assert_eq!(parse_package_json_name(c2), None);
    }

    #[test]
    fn cargo_toml_name_parsed() {
        let c = "[package]\nname = \"serde\"\nversion = \"1.0\"\n";
        assert_eq!(parse_cargo_toml_name(c).as_deref(), Some("serde"));
    }

    #[test]
    fn cargo_toml_name_ignores_other_sections() {
        // A `name` under `[dependencies.foo]` must not be picked up.
        let c = "[dependencies]\nname = \"wrong\"\n\n[package]\nname = \"right\"\n";
        assert_eq!(parse_cargo_toml_name(c).as_deref(), Some("right"));
    }

    #[test]
    fn cargo_toml_comment_stripped() {
        let c = "[package]\nname = \"mycrate\" # the crate name\n";
        assert_eq!(parse_cargo_toml_name(c).as_deref(), Some("mycrate"));
    }

    #[test]
    fn cargo_toml_workspace_inherited_name_is_none() {
        // `name.workspace = true` is not a literal name — fall back.
        let c = "[package]\nname.workspace = true\n";
        assert_eq!(parse_cargo_toml_name(c), None);
    }

    #[test]
    fn pyproject_pep621_name() {
        let c = "[project]\nname = \"numpy\"\nversion = \"1.0\"\n";
        assert_eq!(parse_pyproject_name(c).as_deref(), Some("numpy"));
    }

    #[test]
    fn pyproject_poetry_name() {
        let c = "[tool.poetry]\nname = \"requests\"\n";
        assert_eq!(parse_pyproject_name(c).as_deref(), Some("requests"));
    }

    #[test]
    fn setup_cfg_name() {
        let c = "[metadata]\nname = flask\nversion = 2.0\n";
        assert_eq!(parse_setup_cfg_name(c).as_deref(), Some("flask"));
    }

    #[test]
    fn setup_py_name_double_quote() {
        let c = "from setuptools import setup\nsetup(name=\"django\", version=\"4.0\")\n";
        assert_eq!(parse_setup_py_name(c).as_deref(), Some("django"));
    }

    #[test]
    fn setup_py_name_single_quote() {
        let c = "setup(\n    name='click',\n)\n";
        assert_eq!(parse_setup_py_name(c).as_deref(), Some("click"));
    }

    #[test]
    fn setup_py_name_word_boundary() {
        // `package_name=` must not shadow the real `name=` kwarg.
        let c = "setup(package_name=\"wrong\", name=\"right\")\n";
        assert_eq!(parse_setup_py_name(c).as_deref(), Some("right"));
    }

    #[test]
    fn setup_py_name_skips_comment_mentions() {
        // "name" in a comment (not followed by `= <quote>`) is skipped and
        // the scan continues to the real kwarg.
        let c = "# the name of this package is click\nsetup(name='click')\n";
        assert_eq!(parse_setup_py_name(c).as_deref(), Some("click"));
    }

    #[test]
    fn setup_py_dynamic_name_is_none() {
        // Computed names are not treated as verifiable declarations.
        let c = "setup(name=get_name(), version='1.0')\n";
        assert_eq!(parse_setup_py_name(c), None);
    }

    #[test]
    fn valid_pkg_accepts_real_names() {
        assert!(valid_pkg("react"));
        assert!(valid_pkg("@scope/pkg"));
        assert!(valid_pkg("some_crate-name.x"));
        assert!(valid_pkg("numpy"));
        assert!(valid_pkg("typescript-eslint"));
    }

    #[test]
    fn valid_pkg_rejects_junk_and_traversal() {
        assert!(!valid_pkg(""));
        assert!(!valid_pkg("../etc/passwd"));
        assert!(!valid_pkg("has space"));
        assert!(!valid_pkg("{{template}}"));
        // Path traversal via a second slash (the SSRF vector).
        assert!(!valid_pkg("react/../../admin"));
        assert!(!valid_pkg("foo/bar"));
        assert!(!valid_pkg("a/b/c"));
        // A leading scope is allowed, but only one segment after it.
        assert!(!valid_pkg("@scope/a/b"));
        assert!(!valid_pkg("@scope")); // scope with no package
        assert!(!valid_pkg("@/pkg")); // empty scope
        // URL-encoding smuggling + control chars.
        assert!(!valid_pkg("react%2f..%2fadmin"));
        assert!(!valid_pkg("ab\nc"));
        assert!(!valid_pkg("ab\tc"));
        // Leading dot/dash (registry/URL footguns).
        assert!(!valid_pkg(".hidden"));
        assert!(!valid_pkg("-flag"));
        assert!(!valid_pkg("."));
        assert!(!valid_pkg(".."));
        // Length cap.
        assert!(!valid_pkg(&"a".repeat(129)));
    }

    #[test]
    fn valid_pkg_rejects_url_injection_shapes() {
        // Values crafted to redirect or mutate the registry URL: absolute
        // URLs, authority tricks, query/fragment injection, backslashes,
        // scoped traversal, non-ASCII.
        for evil in [
            "https://evil.com/pkg",
            "http://evil.com",
            "//evil.com/pkg",
            "evil.com/pkg",
            "pkg?x=1",
            "pkg#frag",
            "pkg&y=2", // & is not in the allowed charset either
            "user:pass@host",
            "a\\b",
            "@scope/../secret",
            "@sc ope/pkg",
            "pkg\u{0}",
            "pkg\r\nHost: evil",
            "p\u{00e9}kg", // non-ASCII
            "@scope/pkg/extra",
        ] {
            assert!(!valid_pkg(evil), "must reject {evil:?}");
        }
    }

    #[test]
    fn clean_docker_validates_namespace_repo() {
        assert_eq!(
            clean_docker(Some("library/nginx")).as_deref(),
            Some("library/nginx")
        );
        assert_eq!(clean_docker(Some("nginx")).as_deref(), Some("nginx"));
        // Traversal through the Docker reference is rejected.
        assert_eq!(clean_docker(Some("a/../../b")), None);
        assert_eq!(clean_docker(Some("ns/repo/extra")), None);
        assert_eq!(clean_docker(Some("ns/..")), None);
        assert_eq!(clean_docker(None), None);
        // URL-injection shapes.
        assert_eq!(clean_docker(Some("evil.com?x=1/repo")), None);
        assert_eq!(clean_docker(Some("ns/repo#frag")), None);
        assert_eq!(clean_docker(Some("https://evil.com")), None);
        assert_eq!(clean_docker(Some(&"a".repeat(129))), None);
    }

    #[test]
    fn clean_pkg_filters_overrides() {
        assert_eq!(clean_pkg(Some("react")).as_deref(), Some("react"));
        assert_eq!(clean_pkg(Some("  react  ")).as_deref(), Some("react"));
        assert_eq!(clean_pkg(Some("react/../../admin")), None);
        assert_eq!(clean_pkg(None), None);
    }

    #[test]
    fn registry_urls_pin_host_and_path() {
        // Every URL builder must keep the registry host fixed and place the
        // (already-validated) package inside the expected path.
        let u = url::Url::parse(&crates_downloads_url("serde")).unwrap();
        assert_eq!(u.host_str(), Some("crates.io"));
        assert_eq!(u.path(), "/api/v1/crates/serde/downloads");

        let u = url::Url::parse(&pypi_overall_url("numpy")).unwrap();
        assert_eq!(u.host_str(), Some("pypistats.org"));
        assert_eq!(u.path(), "/api/packages/numpy/overall");
        assert_eq!(u.query(), Some("mirrors=true"));

        let start = chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let end = chrono::NaiveDate::from_ymd_opt(2025, 6, 24).unwrap();
        let u = url::Url::parse(&npm_range_url("react", start, end)).unwrap();
        assert_eq!(u.host_str(), Some("api.npmjs.org"));
        assert_eq!(u.path(), "/downloads/range/2024-01-01:2025-06-24/react");
        // Scoped names keep the same host.
        let u = url::Url::parse(&npm_range_url("@scope/pkg", start, end)).unwrap();
        assert_eq!(u.host_str(), Some("api.npmjs.org"));
    }

    #[test]
    fn docker_url_normalizes_bare_names_to_library() {
        let u = url::Url::parse(&docker_repo_url("nginx")).unwrap();
        assert_eq!(u.host_str(), Some("hub.docker.com"));
        assert_eq!(u.path(), "/v2/repositories/library/nginx/");
        let u = url::Url::parse(&docker_repo_url("grafana/grafana")).unwrap();
        assert_eq!(u.path(), "/v2/repositories/grafana/grafana/");
    }

    fn manifest(npm: Option<&str>, crate_: Option<&str>, pypi: Option<&str>) -> ManifestNames {
        ManifestNames {
            npm: npm.map(str::to_string),
            crate_: crate_.map(str::to_string),
            pypi: pypi.map(str::to_string),
        }
    }

    fn ov(
        npm: Option<&str>,
        crate_: Option<&str>,
        pypi: Option<&str>,
        docker: Option<&str>,
    ) -> UsageOverrides {
        UsageOverrides {
            npm: npm.map(str::to_string),
            crate_: crate_.map(str::to_string),
            pypi: pypi.map(str::to_string),
            docker: docker.map(str::to_string),
        }
    }

    #[test]
    fn precedence_override_beats_manifest_and_bare() {
        let r = assemble_resolved(
            "octo",
            "widget",
            &ov(
                Some("npm-ov"),
                Some("crate-ov"),
                Some("pypi-ov"),
                Some("ns/img"),
            ),
            manifest(Some("npm-mf"), Some("crate-mf"), Some("pypi-mf")),
        );
        assert_eq!(r.npm.as_deref(), Some("npm-ov"));
        assert_eq!(r.crate_.as_deref(), Some("crate-ov"));
        assert_eq!(r.pypi.as_deref(), Some("pypi-ov"));
        assert_eq!(r.docker.as_deref(), Some("ns/img"));
    }

    #[test]
    fn manifest_names_are_used_without_guessing_other_registries() {
        let r = assemble_resolved(
            "octo",
            "widget",
            &UsageOverrides::default(),
            manifest(Some("npm-mf"), None, Some("pypi-mf")),
        );
        assert_eq!(r.npm.as_deref(), Some("npm-mf"));
        assert_eq!(r.crate_, None);
        assert_eq!(r.pypi.as_deref(), Some("pypi-mf"));
        assert_eq!(r.docker, None);
    }

    #[test]
    fn no_manifest_means_no_guessed_package() {
        let r = assemble_resolved(
            "octo",
            "widget",
            &UsageOverrides::default(),
            ManifestNames::default(),
        );
        assert!(r.is_empty());
    }

    #[test]
    fn precedence_invalid_override_disables_registry() {
        // An explicit-but-invalid override pins the registry OFF: it must
        // not silently fall back to the manifest/bare name (that would
        // misattribute another package's numbers under an attacker-chosen
        // query param), and the hostile value must never appear anywhere.
        let r = assemble_resolved(
            "octo",
            "widget",
            &ov(
                Some("react/../../admin"),
                Some("https://evil.com"),
                Some("a b"),
                Some("ns/../../x"),
            ),
            manifest(Some("npm-mf"), Some("crate-mf"), Some("pypi-mf")),
        );
        assert_eq!(r.npm, None);
        assert_eq!(r.crate_, None);
        assert_eq!(r.pypi, None);
        assert_eq!(r.docker, None);
        assert!(r.is_empty());
    }

    #[test]
    fn no_manifest_never_uses_the_repo_name() {
        // Repository identity alone is not evidence of a registry package.
        let r = assemble_resolved(
            "octo",
            "weird~name",
            &UsageOverrides::default(),
            ManifestNames::default(),
        );
        assert_eq!(r.npm, None);
        assert_eq!(r.crate_, None);
        assert_eq!(r.pypi, None);
        assert_eq!(r.docker, None);
    }

    #[test]
    fn precedence_override_whitespace_trimmed() {
        let r = assemble_resolved(
            "octo",
            "widget",
            &ov(Some("  left-pad  "), None, None, None),
            ManifestNames::default(),
        );
        assert_eq!(r.npm.as_deref(), Some("left-pad"));
    }

    #[test]
    fn overrides_cover_all_requires_three_core() {
        // npm + crate + pypi all set → cover_all true (skip the git probe).
        let all = UsageOverrides {
            npm: Some("a".into()),
            crate_: Some("b".into()),
            pypi: Some("c".into()),
            docker: None,
        };
        assert!(overrides_cover_all(&all));
        // Missing pypi → not covered, manifest probe still runs.
        let partial = UsageOverrides {
            npm: Some("a".into()),
            crate_: Some("b".into()),
            pypi: None,
            docker: Some("ns/img".into()),
        };
        assert!(!overrides_cover_all(&partial));
    }

    #[test]
    fn resolved_is_empty_when_all_none() {
        let r = Resolved::default();
        assert!(r.is_empty());
        let r2 = Resolved {
            npm: Some("x".into()),
            ..Default::default()
        };
        assert!(!r2.is_empty());
    }

    mod git_probe {
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

        #[tokio::test]
        async fn resolve_packages_precedence_end_to_end() {
            if !git_available() {
                eprintln!("skipping: git not available");
                return;
            }
            let tmp = tempfile::tempdir().unwrap();
            let storage = RepoStorage {
                root: tmp.path().to_path_buf(),
                quota_bytes: 0,
                high_watermark_pct: 100,
            };
            let clone = storage.path_for("octo/widget");
            std::fs::create_dir_all(&clone).unwrap();
            run(&clone, &["init", "-q"]);
            run(&clone, &["config", "user.email", "t@example.com"]);
            run(&clone, &["config", "user.name", "T"]);
            std::fs::write(clone.join("package.json"), r#"{ "name": "widget-js" }"#).unwrap();
            std::fs::write(
                clone.join("Cargo.toml"),
                "[package]\nname = \"widget-rs\"\n",
            )
            .unwrap();
            std::fs::write(
                clone.join("pyproject.toml"),
                "[project]\nname = \"widget-py\"\n",
            )
            .unwrap();
            // A setup.py with a different name — pyproject must win.
            std::fs::write(clone.join("setup.py"), "setup(name=\"wrong-py\")\n").unwrap();
            run(&clone, &["add", "-A"]);
            run(&clone, &["commit", "-q", "-m", "manifests"]);

            // Root manifests are the only automatic source of identity.
            let r = resolve_packages("octo", "widget", &UsageOverrides::default(), &storage).await;
            assert_eq!(r.npm.as_deref(), Some("widget-js"));
            assert_eq!(r.crate_.as_deref(), Some("widget-rs"));
            assert_eq!(r.pypi.as_deref(), Some("widget-py"));
            assert_eq!(r.docker, None);

            // Override beats manifest; untouched registries keep manifest.
            let r = resolve_packages(
                "octo",
                "widget",
                &UsageOverrides {
                    npm: Some("override-js".into()),
                    ..Default::default()
                },
                &storage,
            )
            .await;
            assert_eq!(r.npm.as_deref(), Some("override-js"));
            assert_eq!(r.crate_.as_deref(), Some("widget-rs"));

            // Invalid override disables the registry, no manifest fallback.
            let r = resolve_packages(
                "octo",
                "widget",
                &UsageOverrides {
                    npm: Some("../../evil".into()),
                    ..Default::default()
                },
                &storage,
            )
            .await;
            assert_eq!(r.npm, None);
            assert_eq!(r.crate_.as_deref(), Some("widget-rs"));

            // Absent clone → no guessed registry identity.
            let r = resolve_packages("octo", "noclone", &UsageOverrides::default(), &storage).await;
            assert!(r.is_empty());
        }
    }

    #[test]
    fn normalize_npm_fixture() {
        // Verified npm response shape; unknown fields ignored.
        let fixture = r#"{
            "start": "2024-01-01", "end": "2024-01-04", "package": "react",
            "downloads": [
                { "downloads": 5,   "day": "2024-01-01" },
                { "downloads": 0,   "day": "2024-01-02" },
                { "downloads": 7,   "day": "2024-01-03" },
                { "downloads": 11,  "day": "2024-01-04" }
            ]
        }"#;
        let body: NpmRange = serde_json::from_str(fixture).unwrap();
        let stats = normalize_npm(body).unwrap();
        // Zero-download days are dropped from the series…
        assert_eq!(
            stats.series,
            vec![
                DownloadPoint {
                    date: "2024-01-01".into(),
                    downloads: 5
                },
                DownloadPoint {
                    date: "2024-01-03".into(),
                    downloads: 7
                },
                DownloadPoint {
                    date: "2024-01-04".into(),
                    downloads: 11
                },
            ]
        );
        // …and the total is the sum of what remains.
        assert_eq!(stats.total, 23);
    }

    #[test]
    fn normalize_npm_sorts_unordered_days() {
        // Registry ordering is not trusted: the cumulative overlay needs
        // ascending dates or running totals attach to the wrong day.
        let fixture = r#"{ "downloads": [
            { "downloads": 3, "day": "2024-01-03" },
            { "downloads": 1, "day": "2024-01-01" },
            { "downloads": 2, "day": "2024-01-02" }
        ] }"#;
        let body: NpmRange = serde_json::from_str(fixture).unwrap();
        let stats = normalize_npm(body).unwrap();
        let dates: Vec<&str> = stats.series.iter().map(|p| p.date.as_str()).collect();
        assert_eq!(dates, vec!["2024-01-01", "2024-01-02", "2024-01-03"]);
        assert_eq!(stats.total, 6);
    }

    #[test]
    fn normalize_npm_empty_or_all_zero_is_none() {
        let empty: NpmRange = serde_json::from_str(r#"{ "downloads": [] }"#).unwrap();
        assert_eq!(normalize_npm(empty), None);
        let zeros: NpmRange =
            serde_json::from_str(r#"{ "downloads": [ { "downloads": 0, "day": "2024-01-01" } ] }"#)
                .unwrap();
        assert_eq!(normalize_npm(zeros), None);
        // Missing `downloads` key entirely (serde default).
        let missing: NpmRange = serde_json::from_str(r#"{ "package": "x" }"#).unwrap();
        assert_eq!(normalize_npm(missing), None);
    }

    #[test]
    fn normalize_crates_fixture_sums_versions_and_extra() {
        // Two versions on the same day + extra_downloads must sum by date.
        let fixture = r#"{
            "version_downloads": [
                { "version": 101, "downloads": 5, "date": "2026-03-01" },
                { "version": 102, "downloads": 7, "date": "2026-03-01" },
                { "version": 102, "downloads": 3, "date": "2026-03-02" }
            ],
            "meta": { "extra_downloads": [
                { "date": "2026-03-01", "downloads": 2 },
                { "date": "2026-03-03", "downloads": 4 }
            ] }
        }"#;
        let body: CratesDownloads = serde_json::from_str(fixture).unwrap();
        let stats = normalize_crates(body).unwrap();
        assert_eq!(
            stats.series,
            vec![
                DownloadPoint {
                    date: "2026-03-01".into(),
                    downloads: 14
                },
                DownloadPoint {
                    date: "2026-03-02".into(),
                    downloads: 3
                },
                DownloadPoint {
                    date: "2026-03-03".into(),
                    downloads: 4
                },
            ]
        );
        assert_eq!(stats.total, 21);
    }

    #[test]
    fn normalize_crates_missing_meta_ok() {
        let fixture = r#"{ "version_downloads": [
            { "version": 1, "downloads": 9, "date": "2026-03-01" }
        ] }"#;
        let body: CratesDownloads = serde_json::from_str(fixture).unwrap();
        let stats = normalize_crates(body).unwrap();
        assert_eq!(stats.total, 9);
        assert_eq!(stats.series.len(), 1);
    }

    #[test]
    fn normalize_crates_empty_is_none() {
        let body: CratesDownloads = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(normalize_crates(body), None);
    }

    #[test]
    fn normalize_pypi_fixture_filters_mirror_category() {
        // If the API ever returns both categories, only the inclusive
        // `with_mirrors` rows count — never a double-summed pair.
        let fixture = r#"{
            "data": [
                { "category": "with_mirrors",    "date": "2025-11-29", "downloads": 100 },
                { "category": "without_mirrors", "date": "2025-11-29", "downloads": 60 },
                { "category": "with_mirrors",    "date": "2025-11-30", "downloads": 50 }
            ],
            "package": "numpy", "type": "overall_downloads"
        }"#;
        let body: PypiOverall = serde_json::from_str(fixture).unwrap();
        let stats = normalize_pypi(body).unwrap();
        assert_eq!(
            stats.series,
            vec![
                DownloadPoint {
                    date: "2025-11-29".into(),
                    downloads: 100
                },
                DownloadPoint {
                    date: "2025-11-30".into(),
                    downloads: 50
                },
            ]
        );
        assert_eq!(stats.total, 150);
    }

    #[test]
    fn normalize_pypi_empty_is_none() {
        let body: PypiOverall = serde_json::from_str(r#"{ "data": [] }"#).unwrap();
        assert_eq!(normalize_pypi(body), None);
        let only_without: PypiOverall = serde_json::from_str(
            r#"{ "data": [ { "category": "without_mirrors", "date": "2025-11-29", "downloads": 60 } ] }"#,
        )
        .unwrap();
        assert_eq!(normalize_pypi(only_without), None);
    }

    #[test]
    fn normalize_docker_total_only() {
        let fixture = r#"{ "name": "nginx", "namespace": "library",
                           "pull_count": 13042883291, "star_count": 21289 }"#;
        let body: DockerRepo = serde_json::from_str(fixture).unwrap();
        let stats = normalize_docker(body).unwrap();
        // Lifetime total (past u32) with NO time series.
        assert_eq!(stats.total, 13_042_883_291);
        assert!(stats.series.is_empty());
    }

    #[test]
    fn normalize_docker_zero_or_missing_is_none() {
        let zero: DockerRepo = serde_json::from_str(r#"{ "pull_count": 0 }"#).unwrap();
        assert_eq!(normalize_docker(zero), None);
        let missing: DockerRepo = serde_json::from_str(r#"{ "name": "x" }"#).unwrap();
        assert_eq!(normalize_docker(missing), None);
    }

    /// Build a big npm fixture programmatically (1000 days, downloads =
    /// day index + 1).
    fn npm_fixture_days(n: u32) -> NpmRange {
        let start = chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
        let days: Vec<String> = (0..n)
            .map(|i| {
                let d = start + chrono::Duration::days(i as i64);
                format!("{{ \"downloads\": {}, \"day\": \"{d}\" }}", i + 1)
            })
            .collect();
        serde_json::from_str(&format!("{{ \"downloads\": [{}] }}", days.join(","))).unwrap()
    }

    #[test]
    fn normalized_series_capped_at_400_points_total_preserved() {
        // Pin the public cap contract.
        assert_eq!(MAX_USAGE_POINTS, 400);
        let stats = normalize_npm(npm_fixture_days(1000)).unwrap();
        assert_eq!(stats.series.len(), MAX_USAGE_POINTS);
        // Total is computed over the FULL series, not the sampled one.
        assert_eq!(stats.total, 1000 * 1001 / 2);
        // First + last survive the cap.
        assert_eq!(stats.series.first().unwrap().date, "2020-01-01");
        assert_eq!(stats.series.first().unwrap().downloads, 1);
        assert_eq!(stats.series.last().unwrap().downloads, 1000);
        // Sampled dates stay strictly ascending (no duplicate indices).
        for w in stats.series.windows(2) {
            assert!(w[0].date < w[1].date);
        }
    }

    #[test]
    fn downsample_points_keeps_first_last() {
        let series: Vec<DownloadPoint> = (0..1000)
            .map(|i| DownloadPoint {
                date: format!("2024-01-{i:03}"),
                downloads: i,
            })
            .collect();
        let ds = downsample_points(series.clone(), 400);
        assert_eq!(ds.len(), 400);
        assert_eq!(ds.first().unwrap(), series.first().unwrap());
        assert_eq!(ds.last().unwrap(), series.last().unwrap());
    }

    #[test]
    fn downsample_points_noop_when_small() {
        let series: Vec<DownloadPoint> = (0..10)
            .map(|i| DownloadPoint {
                date: format!("d{i}"),
                downloads: i,
            })
            .collect();
        assert_eq!(downsample_points(series.clone(), 400).len(), 10);
        // Exactly at the cap → untouched.
        assert_eq!(downsample_points(series.clone(), 10), series);
    }

    #[test]
    fn downsample_points_degenerate_caps_honor_at_most() {
        let series: Vec<DownloadPoint> = (0..10)
            .map(|i| DownloadPoint {
                date: format!("d{i}"),
                downloads: i,
            })
            .collect();
        // 2 → exactly first + last.
        let two = downsample_points(series.clone(), 2);
        assert_eq!(two.len(), 2);
        assert_eq!(two[0], series[0]);
        assert_eq!(two[1], series[9]);
        // 1 → the cumulative endpoint only.
        let one = downsample_points(series.clone(), 1);
        assert_eq!(one, vec![series[9].clone()]);
        // 0 → empty.
        assert!(downsample_points(series, 0).is_empty());
    }

    #[test]
    fn cumulative_downloads_running_sum() {
        let stats = DownloadStats {
            total: 60,
            series: vec![
                DownloadPoint {
                    date: "2024-01-01".into(),
                    downloads: 10,
                },
                DownloadPoint {
                    date: "2024-01-02".into(),
                    downloads: 20,
                },
                DownloadPoint {
                    date: "2024-01-05".into(),
                    downloads: 30,
                },
            ],
        };
        let cum = cumulative_downloads(&stats);
        assert_eq!(cum.len(), 3);
        assert_eq!(cum[0].total, 10);
        assert_eq!(cum[1].total, 30);
        assert_eq!(cum[2].total, 60);
        // Dates parse to UTC midnight.
        assert_eq!(cum[0].at.to_rfc3339(), "2024-01-01T00:00:00+00:00");
        // Monotonic non-decreasing.
        for w in cum.windows(2) {
            assert!(w[0].total <= w[1].total && w[0].at < w[1].at);
        }
    }

    #[test]
    fn cumulative_downloads_skips_bad_dates() {
        let stats = DownloadStats {
            total: 15,
            series: vec![
                DownloadPoint {
                    date: "2024-01-01".into(),
                    downloads: 5,
                },
                DownloadPoint {
                    date: "not-a-date".into(),
                    downloads: 100,
                },
                DownloadPoint {
                    date: "2024-01-02".into(),
                    downloads: 10,
                },
            ],
        };
        let cum = cumulative_downloads(&stats);
        // The bad row is skipped entirely (not silently added to the sum).
        assert_eq!(cum.len(), 2);
        assert_eq!(cum[1].total, 15);
    }

    #[test]
    fn cumulative_downloads_sorts_legacy_unordered_series() {
        // Cached blobs written before normalize-time sorting may be
        // unordered; the running sum must still accumulate chronologically.
        let stats = DownloadStats {
            total: 6,
            series: vec![
                DownloadPoint {
                    date: "2024-01-03".into(),
                    downloads: 3,
                },
                DownloadPoint {
                    date: "2024-01-01".into(),
                    downloads: 1,
                },
                DownloadPoint {
                    date: "2024-01-02".into(),
                    downloads: 2,
                },
            ],
        };
        let cum = cumulative_downloads(&stats);
        let totals: Vec<u64> = cum.iter().map(|p| p.total).collect();
        assert_eq!(totals, vec![1, 3, 6]);
        assert!(cum[0].at < cum[1].at && cum[1].at < cum[2].at);
    }

    #[test]
    fn cumulative_downloads_empty_for_docker_total_only() {
        let stats = DownloadStats {
            total: 13_042_883_291,
            series: Vec::new(),
        };
        assert!(cumulative_downloads(&stats).is_empty());
    }

    #[test]
    fn cumulative_downloads_saturates_at_u64_max() {
        let stats = DownloadStats {
            total: u64::MAX,
            series: vec![
                DownloadPoint {
                    date: "2024-01-01".into(),
                    downloads: u64::MAX,
                },
                DownloadPoint {
                    date: "2024-01-02".into(),
                    downloads: 5,
                },
            ],
        };
        let cum = cumulative_downloads(&stats);
        assert_eq!(cum[0].total, u64::MAX);
        assert_eq!(cum[1].total, u64::MAX); // saturated, no overflow panic
    }

    use crate::chart::{self, ChartConfig, ChartOpts, OverlayConfig, TimeAxis};
    use crate::theme::LIGHT;

    fn day(d: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_naive_utc_and_offset(
            chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            chrono::Utc,
        )
    }

    /// Extract the `(x, y)` points of the first `<path>` whose attributes
    /// contain `marker` (e.g. a stroke color or `stroke-dasharray`).
    fn path_points(svg: &str, marker: &str) -> Vec<(f32, f32)> {
        let seg = svg
            .split("<path ")
            .find(|s| s.contains(marker))
            .unwrap_or_else(|| panic!("no path with marker {marker:?}"));
        let start = seg.find("d=\"").expect("d attr") + 3;
        let rest = &seg[start..];
        let end = rest.find('"').expect("closing quote");
        let nums: Vec<f32> = rest[..end]
            .split_whitespace()
            .filter(|t| *t != "M" && *t != "L")
            .map(|t| t.parse().unwrap())
            .collect();
        nums.chunks(2).map(|c| (c[0], c[1])).collect()
    }

    /// Fixture pipeline: npm JSON → normalize → cumulative → overlay SVG.
    fn overlay_from_fixture(log_y: bool) -> String {
        let fixture = r#"{ "downloads": [
            { "downloads": 1,      "day": "2024-01-01" },
            { "downloads": 999,    "day": "2024-01-02" },
            { "downloads": 999000, "day": "2024-01-03" }
        ] }"#;
        let body: NpmRange = serde_json::from_str(fixture).unwrap();
        let stats = normalize_npm(body).unwrap();
        let dl = cumulative_downloads(&stats);
        let stars =
            chart::cumulative_series(&[day("2024-01-01"), day("2024-01-02"), day("2024-01-03")]);
        chart::render_overlay_svg(
            &stars,
            &dl,
            &ChartConfig::default(),
            &OverlayConfig {
                repo: "octo/widget".into(),
                downloads_label: Some("npm downloads".into()),
            },
            &LIGHT,
            &ChartOpts {
                axis: TimeAxis::Date,
                log_y,
                animate: false,
            },
        )
    }

    #[test]
    fn overlay_linear_vs_log_right_axis_math() {
        // Cumulative downloads: 1, 1_000, 1_000_000 (dl_max = 1e6).
        // Default geometry: pad = 56, plot_h = 600 - 2*56 - 24 = 464.
        let lin = overlay_from_fixture(false);
        let log = overlay_from_fixture(true);
        // The downloads line is the only dashed path.
        let lin_pts = path_points(&lin, "stroke-dasharray");
        let log_pts = path_points(&log, "stroke-dasharray");
        assert_eq!(lin_pts.len(), 3);
        assert_eq!(log_pts.len(), 3);
        // Both scales pin the max (last point) to the top of the plot.
        assert!((lin_pts[2].1 - 56.0).abs() < 0.2, "linear max at top");
        assert!((log_pts[2].1 - 56.0).abs() < 0.2, "log max at top");
        // Mid point (1_000 of 1_000_000): linear ≈ bottom
        // (y ≈ 56 + 464·(1 − 0.001) ≈ 519.5); log ≈ half height
        // (ln(1001)/ln(1e6+1) ≈ 0.5 → y ≈ 288).
        let y_lin_mid = lin_pts[1].1;
        let y_log_mid = log_pts[1].1;
        assert!(y_lin_mid > 500.0, "linear mid near bottom, got {y_lin_mid}");
        assert!(
            y_log_mid < 300.0,
            "log mid near half height, got {y_log_mid}"
        );
        assert!(y_lin_mid - y_log_mid > 200.0);
        // Shared x-axis: downloads and stars span the same x range.
        let star_pts = path_points(&lin, "stroke=\"#087fea\"");
        assert!((star_pts[0].0 - lin_pts[0].0).abs() < 0.2);
        assert!((star_pts[2].0 - lin_pts[2].0).abs() < 0.2);
    }

    #[test]
    fn overlay_docker_total_only_renders_stars_only() {
        // Docker has no series → cumulative is empty → the overlay must
        // fall back to a stars-only chart with the "not found" note, and
        // the docker legend entry must not render.
        let docker: DockerRepo = serde_json::from_str(r#"{ "pull_count": 13042883291 }"#).unwrap();
        let stats = normalize_docker(docker).unwrap();
        let dl = cumulative_downloads(&stats);
        assert!(dl.is_empty());
        let stars = chart::cumulative_series(&[day("2024-01-01"), day("2024-01-02")]);
        let svg = chart::render_overlay_svg(
            &stars,
            &dl,
            &ChartConfig::default(),
            &OverlayConfig {
                repo: "octo/widget".into(),
                downloads_label: Some("docker pulls".into()),
            },
            &LIGHT,
            &ChartOpts::default(),
        );
        assert!(svg.contains("no package downloads found"));
        assert!(!svg.contains("docker pulls"));
        assert!(!svg.contains("stroke-dasharray"));
    }

    #[test]
    fn overlay_pipeline_deterministic_and_static() {
        // Same fixture through the whole pipeline twice → identical bytes
        // (ETag/CDN contract), and embeddable-static (no SMIL/CSS anim).
        let a = overlay_from_fixture(false);
        let b = overlay_from_fixture(false);
        assert_eq!(a, b);
        assert!(!a.contains("<animate"));
        assert!(!a.contains("@keyframes"));
        assert!(!a.contains("var(--"));
    }

    #[test]
    fn overlay_handles_past_u32_download_totals() {
        // Docker-scale numbers (10B+) must not panic the axis math and the
        // right axis must label in billions.
        let stats = DownloadStats {
            total: 10_000_000_000,
            series: vec![
                DownloadPoint {
                    date: "2024-01-01".into(),
                    downloads: 2_000_000_000,
                },
                DownloadPoint {
                    date: "2024-01-02".into(),
                    downloads: 3_000_000_000,
                },
                DownloadPoint {
                    date: "2024-01-03".into(),
                    downloads: 5_000_000_000,
                },
            ],
        };
        let dl = cumulative_downloads(&stats);
        assert_eq!(dl.last().unwrap().total, 10_000_000_000);
        let stars =
            chart::cumulative_series(&[day("2024-01-01"), day("2024-01-02"), day("2024-01-03")]);
        let svg = chart::render_overlay_svg(
            &stars,
            &dl,
            &ChartConfig::default(),
            &OverlayConfig {
                repo: "octo/widget".into(),
                downloads_label: Some("npm downloads".into()),
            },
            &LIGHT,
            &ChartOpts::default(),
        );
        assert!(svg.contains("B</text>"), "right axis labels in billions");
    }

    mod cache_policy {
        use super::*;
        use std::collections::HashMap;
        use std::sync::Mutex;
        use std::sync::atomic::{AtomicBool, Ordering};

        /// In-memory [`UsageStore`]: rows carry an explicit freshness flag
        /// (standing in for `fetched_at` vs TTL), `puts` records writes,
        /// `last_ttl` captures the TTL the policy passed down.
        #[derive(Default)]
        struct MemStore {
            rows: Mutex<HashMap<(String, String), (String, bool)>>,
            puts: Mutex<Vec<(String, String, String)>>,
            last_ttl: Mutex<Option<chrono::Duration>>,
            fail_puts: bool,
        }

        impl MemStore {
            fn with_row(source: &str, package: &str, body: &str, fresh: bool) -> Self {
                let store = Self::default();
                store
                    .rows
                    .lock()
                    .unwrap()
                    .insert((source.into(), package.into()), (body.into(), fresh));
                store
            }
        }

        #[async_trait::async_trait]
        impl UsageStore for MemStore {
            async fn get_fresh(
                &self,
                source: &str,
                package: &str,
                ttl: chrono::Duration,
            ) -> Result<Option<String>> {
                *self.last_ttl.lock().unwrap() = Some(ttl);
                Ok(self
                    .rows
                    .lock()
                    .unwrap()
                    .get(&(source.into(), package.into()))
                    .filter(|(_, fresh)| *fresh)
                    .map(|(body, _)| body.clone()))
            }
            async fn get_any(&self, source: &str, package: &str) -> Result<Option<String>> {
                Ok(self
                    .rows
                    .lock()
                    .unwrap()
                    .get(&(source.into(), package.into()))
                    .map(|(body, _)| body.clone()))
            }
            async fn put(&self, source: &str, package: &str, body: &str) -> Result<()> {
                if self.fail_puts {
                    anyhow::bail!("disk full");
                }
                self.puts
                    .lock()
                    .unwrap()
                    .push((source.into(), package.into(), body.into()));
                self.rows
                    .lock()
                    .unwrap()
                    .insert((source.into(), package.into()), (body.into(), true));
                Ok(())
            }
        }

        fn stats(total: u64) -> DownloadStats {
            DownloadStats {
                total,
                series: vec![DownloadPoint {
                    date: "2024-01-01".into(),
                    downloads: total,
                }],
            }
        }

        #[tokio::test]
        async fn fresh_hit_short_circuits_fetch() {
            let cached = stats(42);
            let body = serde_json::to_string(&cached).unwrap();
            let store = MemStore::with_row("npm", "react", &body, true);
            let called = AtomicBool::new(false);
            let out = cached_or_fetch(&store, "npm", "react", || {
                called.store(true, Ordering::SeqCst);
                async { Ok(Some(stats(999))) }
            })
            .await;
            assert_eq!(out, Some(cached));
            assert!(!called.load(Ordering::SeqCst), "fetch must not run");
            assert!(store.puts.lock().unwrap().is_empty());
        }

        #[tokio::test]
        async fn policy_uses_the_18h_ttl() {
            let store = MemStore::default();
            let _ = cached_or_fetch(&store, "npm", "react", || async { Ok(None) }).await;
            assert_eq!(
                *store.last_ttl.lock().unwrap(),
                Some(chrono::Duration::hours(USAGE_TTL_HOURS))
            );
            assert_eq!(USAGE_TTL_HOURS, 18);
        }

        #[tokio::test]
        async fn corrupt_fresh_row_falls_through_to_fetch() {
            let store = MemStore::with_row("npm", "react", "{not json", true);
            let fetched = stats(7);
            let expect = fetched.clone();
            let out = cached_or_fetch(
                &store,
                "npm",
                "react",
                move || async move { Ok(Some(fetched)) },
            )
            .await;
            assert_eq!(out, Some(expect));
            // The refetch overwrites the corrupt row.
            assert_eq!(store.puts.lock().unwrap().len(), 1);
        }

        #[tokio::test]
        async fn miss_then_success_persists_normalized_blob() {
            let store = MemStore::default();
            let fetched = stats(7);
            let expect = fetched.clone();
            let out = cached_or_fetch(&store, "crates", "serde", move || async move {
                Ok(Some(fetched))
            })
            .await;
            assert_eq!(out, Some(expect.clone()));
            let puts = store.puts.lock().unwrap();
            assert_eq!(puts.len(), 1);
            assert_eq!(puts[0].0, "crates");
            assert_eq!(puts[0].1, "serde");
            // The stored blob round-trips to the same stats.
            let stored: DownloadStats = serde_json::from_str(&puts[0].2).unwrap();
            assert_eq!(stored, expect);
        }

        #[tokio::test]
        async fn miss_then_404_returns_none_and_writes_nothing() {
            let store = MemStore::default();
            let out = cached_or_fetch(&store, "pypi", "ghost", || async { Ok(None) }).await;
            assert_eq!(out, None);
            assert!(store.puts.lock().unwrap().is_empty());
        }

        #[tokio::test]
        async fn fetch_error_falls_back_to_stale_row() {
            let stale = stats(3);
            let body = serde_json::to_string(&stale).unwrap();
            // fresh=false → invisible to get_fresh, visible to get_any.
            let store = MemStore::with_row("npm", "react", &body, false);
            let out = cached_or_fetch(&store, "npm", "react", || async {
                Err(anyhow::anyhow!("registry down"))
            })
            .await;
            assert_eq!(out, Some(stale), "stale beats nothing");
        }

        #[tokio::test]
        async fn fetch_error_with_no_cache_is_none() {
            let store = MemStore::default();
            let out = cached_or_fetch(&store, "npm", "react", || async {
                Err(anyhow::anyhow!("timeout"))
            })
            .await;
            assert_eq!(out, None);
        }

        #[tokio::test]
        async fn stale_row_refreshed_by_successful_fetch() {
            let stale = stats(3);
            let body = serde_json::to_string(&stale).unwrap();
            let store = MemStore::with_row("npm", "react", &body, false);
            let fetched = stats(9);
            let expect = fetched.clone();
            let out = cached_or_fetch(
                &store,
                "npm",
                "react",
                move || async move { Ok(Some(fetched)) },
            )
            .await;
            // Fresh fetch wins over the stale row and is persisted.
            assert_eq!(out, Some(expect));
            assert_eq!(store.puts.lock().unwrap().len(), 1);
        }

        #[tokio::test]
        async fn put_failure_is_swallowed() {
            // A cache-write failure must not fail the request (best-effort
            // contract: the endpoint never 5xxs because Postgres hiccuped).
            let store = MemStore {
                fail_puts: true,
                ..Default::default()
            };
            let fetched = stats(5);
            let expect = fetched.clone();
            let out = cached_or_fetch(
                &store,
                "npm",
                "react",
                move || async move { Ok(Some(fetched)) },
            )
            .await;
            assert_eq!(out, Some(expect));
        }
    }

    #[test]
    fn download_stats_json_shape() {
        let stats = DownloadStats {
            total: 999,
            series: vec![DownloadPoint {
                date: "2021-01-01".into(),
                downloads: 10,
            }],
        };
        let v = serde_json::to_value(&stats).unwrap();
        assert_eq!(v["total"], 999);
        assert_eq!(v["series"][0]["date"], "2021-01-01");
        assert_eq!(v["series"][0]["downloads"], 10);
    }

    #[test]
    fn resolved_json_uses_crate_key() {
        // The `crate_` field must serialize as `crate` in the API response.
        let r = Resolved {
            npm: Some("react".into()),
            crate_: Some("serde".into()),
            pypi: None,
            docker: None,
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["npm"], "react");
        assert_eq!(v["crate"], "serde");
        assert!(v["pypi"].is_null());
        assert!(v["docker"].is_null());
        assert!(v.get("crate_").is_none());
    }
}
