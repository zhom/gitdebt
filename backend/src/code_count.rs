//! In-house lines-of-code aggregator.
//!
//! Reads HEAD's tree listing, classifies each blob by extension / basename,
//! and counts blank / comment / code lines per language for the blobs that
//! can hold code.
//!
//! **Nothing materializes the working tree.** Clones are bare and complete, so
//! every object is already local and there is no download to avoid — but a
//! checkout would still write a second full copy of the repository onto the
//! volume the quota accountant is trying to keep under control, and `ls-tree
//! --long` would read every blob header in the tree to report sizes this
//! module does not need. So paths worth counting are selected from the tree
//! listing alone, and their contents are streamed out of the local object
//! store through one `cat-file --batch`.
//!
//! Why hand-rolled instead of `tokei`: it is a 100+ language dependency for
//! the ~12 languages this renders, and it counts a working directory, which
//! is the thing this module exists not to create.
//!
//! Comment classification is a per-language single-pass state machine.
//! No string-literal tracking (tokei has it, we skip it). The error
//! mode is "occasionally classifying a line as comment when a string
//! contains `//`", which moves a handful of lines between the comment
//! and code buckets — irrelevant at the order of magnitude this chart
//! shows.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use tokio::process::Command;
use tokio::task;

use crate::db::Db;

/// Cap per-file size. Anything bigger is almost always generated
/// (minified bundles, vendored snapshots, fixture data) and would
/// dominate counts without representing human-written code.
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// Probe size for binary detection. We sniff the first chunk for a NUL
/// byte and skip the file if found — text files in any encoding we
/// support don't contain NULs.
const BINARY_SNIFF_BYTES: usize = 8 * 1024;

/// Countable files above which the exact count is skipped in favour of the
/// census. A property of the tree, so the same repository always resolves the
/// same way — the choice must never depend on how a particular run raced a
/// clock, or one repository's stored metric flips between runs.
///
/// It was twenty thousand when every one of those files had to be downloaded
/// from a promisor remote first, which put the largest repositories — the ones
/// whose language breakdown is most worth having — permanently on the census.
/// Reading them out of a local pack is a different order of cost, so the
/// ceiling is now sized to cover a kernel-scale tree rather than to bound a
/// network transfer.
const DEFAULT_EXACT_LINE_COUNT_MAX_FILES: usize = 200_000;
// Exact lines are a refinement over the cheap, always-saved language census,
// so this stays a ceiling rather than an open-ended phase — but it has to be
// larger than the work it bounds or it decides the outcome by itself, which is
// exactly what happened when it was eight seconds and the files had to be
// fetched. The read phase below carries its own deadline; this one is the
// backstop over the whole call.
const DEFAULT_EXACT_LINE_COUNT_TIMEOUT_SECS: u64 = 180;

/// Wall-clock ceiling for the read phase. It degrades to the file census,
/// which is always saved, so it cannot fail an analysis.
const DEFAULT_EXACT_LINE_COUNT_READ_TIMEOUT_SECS: u64 = 120;
/// Ceiling on the tree listing itself. Local and cheap even on a monorepo;
/// this exists so a damaged object store cannot stall the phase forever.
const DEFAULT_TREE_LISTING_TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Clone)]
pub struct LanguageCount {
    pub language: String,
    pub files: i64,
    pub lines_code: i64,
    pub lines_blank: i64,
    pub lines_comment: i64,
}

/// Contributor-facing project guides and automation present in the committed
/// HEAD tree. This is a factual checklist, not an achievement or quality
/// score: repositories can intentionally omit any item.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepositoryReadiness {
    pub readme: bool,
    pub security: bool,
    pub cla: bool,
    pub code_of_conduct: bool,
    pub contributing: bool,
    pub license: bool,
    pub codeowners: bool,
    pub changelog: bool,
    pub issue_templates: bool,
    pub pr_template: bool,
    pub ci: bool,
    pub tests: bool,
    pub dependency_updates: bool,
}

/// One blob of HEAD's tree.
#[derive(Debug, Clone)]
pub(crate) struct TreeBlob {
    oid: String,
    path: String,
}

/// HEAD's blobs, straight from the tree listing.
///
/// `--long` is deliberately not passed: a blob's size lives in its object
/// header rather than in the tree, so asking for it reads every object in the
/// repository to answer a question the selection below does not ask. Symlinks
/// (mode 120000) and submodule gitlinks (type `commit`) are dropped here.
///
/// Callers hoist one listing and feed it to every consumer: readiness, the
/// census, and the exact count all describe the same tree, so three separate
/// `ls-tree` passes would be three walks of it for one answer.
pub(crate) async fn head_blobs(repo_path: &Path) -> Result<Vec<TreeBlob>> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(repo_path)
        .args(["ls-tree", "-rz", "HEAD"])
        .kill_on_drop(true);
    let budget = budget_from_env(
        "REPO_LINE_COUNT_TREE_TIMEOUT_SECONDS",
        DEFAULT_TREE_LISTING_TIMEOUT_SECS,
    );
    let Ok(output) = tokio::time::timeout(budget, command.output()).await else {
        bail!("git ls-tree did not finish in {}s", budget.as_secs());
    };
    let output = output.context("git ls-tree")?;
    if !output.status.success() {
        bail!("git ls-tree exited {}", output.status);
    }
    Ok(parse_tree_listing(&output.stdout))
}

/// Wall-clock ceiling read from `name`, in seconds. Zero and unparseable
/// values fall back to the default: a ceiling of zero would mean "no
/// repository is ever counted exactly", which is never what an operator means.
fn budget_from_env(name: &str, default_seconds: u64) -> std::time::Duration {
    std::time::Duration::from_secs(
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value| *value > 0)
            .unwrap_or(default_seconds),
    )
}

/// Inspect contributor-facing files from their paths alone, reading no blob.
pub(crate) fn readiness_from_blobs(blobs: &[TreeBlob]) -> RepositoryReadiness {
    let mut readiness = RepositoryReadiness::default();
    for blob in blobs {
        let path = blob.path.replace('\\', "/").to_ascii_lowercase();
        let basename = path.rsplit('/').next().unwrap_or(path.as_str());
        let root_or_meta =
            !path.contains('/') || path.starts_with(".github/") || path.starts_with("docs/");

        readiness.readme |= root_or_meta && basename.starts_with("readme");
        readiness.security |= root_or_meta && basename.starts_with("security.");
        readiness.cla |= matches!(
            basename,
            "cla.md"
                | "cla.txt"
                | "contributor_license_agreement.md"
                | "contributor-license-agreement.md"
        );
        readiness.code_of_conduct |= root_or_meta && basename.starts_with("code_of_conduct.");
        readiness.contributing |= root_or_meta && basename.starts_with("contributing.");
        readiness.license |= root_or_meta
            && (basename.starts_with("license")
                || basename.starts_with("licence")
                || basename.starts_with("copying"));
        readiness.codeowners |= basename == "codeowners"
            && (!path.contains('/') || path.starts_with(".github/") || path.starts_with("docs/"));
        readiness.changelog |=
            root_or_meta && (basename.starts_with("changelog") || basename.starts_with("changes."));
        readiness.issue_templates |= path.starts_with(".github/issue_template/");
        readiness.pr_template |= basename.starts_with("pull_request_template.")
            && (!path.contains('/') || path.starts_with(".github/"));
        readiness.ci |= path.starts_with(".github/workflows/")
            || path == ".gitlab-ci.yml"
            || path == "azure-pipelines.yml"
            || path.starts_with(".circleci/");
        readiness.tests |= path.starts_with("test/")
            || path.starts_with("tests/")
            || path.contains("/tests/")
            || basename.ends_with("_test.rs")
            || basename.ends_with("_test.py")
            || basename.ends_with(".test.ts")
            || basename.ends_with(".test.tsx")
            || basename.ends_with(".spec.ts")
            || basename.ends_with(".spec.tsx");
        readiness.dependency_updates |= path == ".github/dependabot.yml"
            || path == ".github/dependabot.yaml"
            || basename == "renovate.json"
            || basename == "renovate.json5"
            || basename == ".renovaterc";
    }
    readiness
}

/// Store the exact HEAD checklist under the same head SHA used by the commit
/// aggregates, so badges never imply that stale files describe a newer tree.
pub async fn save_repository_readiness(
    db: &Db,
    repo: &str,
    head_sha: &str,
    value: &RepositoryReadiness,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO repo_readiness \
            (repo, head_sha, readme, security, cla, code_of_conduct, contributing, license, \
             codeowners, changelog, issue_templates, pr_template, ci, tests, \
             dependency_updates, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, NOW()) \
         ON CONFLICT (repo) DO UPDATE SET \
            head_sha = EXCLUDED.head_sha, readme = EXCLUDED.readme, \
            security = EXCLUDED.security, cla = EXCLUDED.cla, \
            code_of_conduct = EXCLUDED.code_of_conduct, \
            contributing = EXCLUDED.contributing, license = EXCLUDED.license, \
            codeowners = EXCLUDED.codeowners, changelog = EXCLUDED.changelog, \
            issue_templates = EXCLUDED.issue_templates, pr_template = EXCLUDED.pr_template, \
            ci = EXCLUDED.ci, tests = EXCLUDED.tests, \
            dependency_updates = EXCLUDED.dependency_updates, updated_at = NOW()",
    )
    .bind(repo)
    .bind(head_sha)
    .bind(value.readme)
    .bind(value.security)
    .bind(value.cla)
    .bind(value.code_of_conduct)
    .bind(value.contributing)
    .bind(value.license)
    .bind(value.codeowners)
    .bind(value.changelog)
    .bind(value.issue_templates)
    .bind(value.pr_template)
    .bind(value.ci)
    .bind(value.tests)
    .bind(value.dependency_updates)
    .execute(&db.pool)
    .await
    .context("save repository readiness")?;
    Ok(())
}

/// Parse `git ls-tree -rz` records: `<mode> SP <type> SP <oid> TAB <path>`,
/// NUL-separated.
fn parse_tree_listing(stdout: &[u8]) -> Vec<TreeBlob> {
    let mut blobs = Vec::new();
    for record in stdout.split(|byte| *byte == 0) {
        if record.is_empty() {
            continue;
        }
        let Ok(record) = std::str::from_utf8(record) else {
            continue;
        };
        let Some((meta, path)) = record.split_once('\t') else {
            continue;
        };
        let mut fields = meta.split_whitespace();
        let (Some(mode), Some(kind), Some(oid)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if kind != "blob" || mode == "120000" {
            continue;
        }
        blobs.push(TreeBlob {
            oid: oid.to_string(),
            path: path.to_string(),
        });
    }
    blobs
}

/// Language file counts over the committed HEAD tree. The second value is the
/// total number of files in the tree, including types gitdebt cannot classify
/// — the denominator that makes the classified count honest.
pub(crate) fn census_from_blobs(blobs: &[TreeBlob]) -> (Vec<LanguageCount>, usize) {
    let mut files_by_language: HashMap<&'static str, i64> = HashMap::new();
    for blob in blobs {
        let path = Path::new(&blob.path);
        if is_excluded_path(path) {
            continue;
        }
        let Some(hit) = detect_language(path) else {
            continue;
        };
        *files_by_language.entry(hit.name).or_default() += 1;
    }

    let mut counts: Vec<LanguageCount> = files_by_language
        .into_iter()
        .map(|(language, files)| LanguageCount {
            language: language.to_string(),
            files,
            lines_code: 0,
            lines_blank: 0,
            lines_comment: 0,
        })
        .collect();
    counts.sort_by_key(|row| std::cmp::Reverse(row.files));
    (counts, blobs.len())
}

/// Exact per-language line counts over HEAD.
///
/// `None` means the repository's countable content is outside the budget and
/// the caller should persist the census instead. That decision is a pure
/// function of the tree, so a repository does not switch metrics between runs.
pub(crate) async fn count_lines_for(
    repo_path: &Path,
    blobs: &[TreeBlob],
) -> Result<Option<Vec<LanguageCount>>> {
    let candidates: Vec<TreeBlob> = blobs
        .iter()
        .filter(|blob| {
            let path = Path::new(&blob.path);
            !is_excluded_path(path) && detect_language(path).is_some()
        })
        .cloned()
        .collect();
    if candidates.is_empty() || candidates.len() > exact_line_count_max_files() {
        return Ok(None);
    }
    let repo_path = repo_path.to_path_buf();
    // Reading and counting is local I/O plus a per-file scan; keep it off the
    // runtime threads. It carries its own deadline because a blocking task is
    // not cancellable: an outer `tokio::time::timeout` returns while the
    // thread — and the `git cat-file` child it is feeding — keep running.
    let deadline = std::time::Instant::now()
        + budget_from_env(
            "REPO_LINE_COUNT_READ_TIMEOUT_SECONDS",
            DEFAULT_EXACT_LINE_COUNT_READ_TIMEOUT_SECS,
        );
    task::spawn_blocking(move || read_and_count(&repo_path, &candidates, deadline))
        .await
        .context("line-count task")?
        .map(Some)
}

/// Stream the selected blobs out of the local object store and count them.
/// Lazy fetching is disabled. The clone is complete, so every object is
/// already present; the env var is the guard that keeps an unexpected miss —
/// a damaged pack — surfacing as an error rather than turning a local read
/// into one network round trip per file.
///
/// `deadline` is enforced here rather than by the caller because this runs on
/// a blocking thread, which `tokio::time::timeout` cannot cancel: without it a
/// lapsed timeout returned to the caller while this thread and its `cat-file`
/// child kept reading a repository nobody was waiting for. A lapse is an error
/// so the caller stores the file census — a truncated count persisted as exact
/// is the one outcome worse than not counting at all.
fn read_and_count(
    repo_path: &Path,
    blobs: &[TreeBlob],
    deadline: std::time::Instant,
) -> Result<Vec<LanguageCount>> {
    let mut child = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["cat-file", "--batch"])
        .env("GIT_NO_LAZY_FETCH", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("spawn git cat-file --batch")?;
    let mut stdin = child.stdin.take().context("cat-file stdin")?;
    let requested: Vec<TreeBlob> = blobs.to_vec();
    // The writer thread needs owned data, but only the object ids: copying the
    // whole blob list would duplicate every path string a second time, and the
    // tree this may be handed is now an order of magnitude larger than when
    // the ceiling assumed a network transfer.
    let feed: Vec<String> = requested.iter().map(|blob| blob.oid.clone()).collect();
    let writer = std::thread::spawn(move || {
        for oid in &feed {
            if writeln!(stdin, "{oid}").is_err() {
                break;
            }
        }
    });

    let mut reader = BufReader::new(child.stdout.take().context("cat-file stdout")?);
    let mut totals: HashMap<&'static str, LanguageCount> = HashMap::new();
    let mut header = String::new();
    let mut content = Vec::new();
    let mut lapsed = false;
    for (index, blob) in requested.iter().enumerate() {
        // Checking every file would put a clock read in front of every blob;
        // a batch of 256 bounds the overshoot to a fraction of a second.
        if index % 256 == 0 && std::time::Instant::now() >= deadline {
            lapsed = true;
            break;
        }
        header.clear();
        if reader.read_line(&mut header)? == 0 {
            break;
        }
        let mut fields = header.split_whitespace();
        let (_oid, kind, size) = (fields.next(), fields.next(), fields.next());
        let Some(size) = kind
            .filter(|kind| *kind == "blob")
            .and(size)
            .and_then(|size| size.parse::<usize>().ok())
        else {
            // `<oid> missing` carries no body to skip.
            continue;
        };
        content.clear();
        content.resize(size, 0);
        reader.read_exact(&mut content)?;
        // cat-file writes a trailing newline after each object body.
        let mut trailer = [0u8; 1];
        reader.read_exact(&mut trailer)?;
        if size > MAX_FILE_BYTES as usize || looks_binary(&content) {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&content) else {
            continue;
        };
        let Some(lang) = detect_language(Path::new(&blob.path)) else {
            continue;
        };
        let spec = LANG_SPECS[lang.spec_idx];
        let (blank, comment, code) = count_lines_in_text(spec, text);
        let row = totals.entry(lang.name).or_insert_with(|| LanguageCount {
            language: lang.name.to_string(),
            files: 0,
            lines_code: 0,
            lines_blank: 0,
            lines_comment: 0,
        });
        row.files += 1;
        row.lines_blank += blank;
        row.lines_comment += comment;
        row.lines_code += code;
    }
    if lapsed {
        // Killing the child is what unblocks the writer thread: it is parked
        // in a `writeln!` on a pipe nobody is draining any more.
        let _ = child.kill();
    }
    let _ = writer.join();
    // A mid-stream death ends the loop above at EOF with whatever it had, and
    // the result is persisted as an *exact* count. Truncated-but-confident is
    // the one outcome worse than falling back to the file census, so a failed
    // probe becomes an error the caller can degrade on.
    let status = child.wait().context("wait for git cat-file --batch")?;
    if lapsed {
        tracing::info!(
            files = requested.len(),
            "exact line count exceeded its read budget; using the file census"
        );
        bail!("exact line count exceeded its read budget");
    }
    if !status.success() {
        bail!("git cat-file --batch exited with {status}");
    }

    let mut out: Vec<LanguageCount> = totals
        .into_values()
        .filter(|lang| lang.lines_code + lang.lines_comment + lang.lines_blank > 0)
        .collect();
    out.sort_by_key(|row| std::cmp::Reverse(row.lines_code));
    Ok(out)
}

pub fn exact_line_count_max_files() -> usize {
    std::env::var("REPO_LINE_COUNT_MAX_FILES")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_EXACT_LINE_COUNT_MAX_FILES)
}

pub fn exact_line_count_timeout() -> std::time::Duration {
    std::time::Duration::from_secs(
        std::env::var("REPO_LINE_COUNT_TIMEOUT_SECONDS")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_EXACT_LINE_COUNT_TIMEOUT_SECS),
    )
}

/// Replace the persisted line counts for `repo` with `counts`. Single
/// transaction: visible state goes from "old data" → "new data" without
/// any half-empty intermediate.
///
/// `exact` records which metric these rows are. A census carries file counts
/// with zero lines, and readers must be able to tell that apart from a
/// repository that genuinely has no code.
pub async fn save(db: &Db, repo: &str, counts: &[LanguageCount], exact: bool) -> Result<()> {
    let mut tx = db.pool.begin().await?;
    sqlx::query("DELETE FROM repo_lines WHERE repo = $1")
        .bind(repo)
        .execute(&mut *tx)
        .await?;
    for c in counts {
        sqlx::query(
            "INSERT INTO repo_lines \
                (repo, language, files, lines_code, lines_blank, lines_comment, lines_exact) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(repo)
        .bind(&c.language)
        .bind(c.files)
        .bind(c.lines_code)
        .bind(c.lines_blank)
        .bind(c.lines_comment)
        .bind(exact)
        .execute(&mut *tx)
        .await?;
    }
    let _ = Utc::now();
    tx.commit().await?;
    Ok(())
}

// Language detection

struct LangHit {
    name: &'static str,
    spec_idx: usize,
}

fn detect_language(path: &Path) -> Option<LangHit> {
    let basename = path.file_name()?.to_str()?;
    // Special-case file names without extensions.
    if let Some((name, idx)) = match_basename(basename) {
        return Some(LangHit {
            name,
            spec_idx: idx,
        });
    }
    let ext = path.extension()?.to_str()?;
    match_extension(ext).map(|(name, spec_idx)| LangHit { name, spec_idx })
}

fn match_basename(name: &str) -> Option<(&'static str, usize)> {
    match name {
        "Makefile" | "makefile" | "GNUmakefile" => Some(("Makefile", SPEC_HASH)),
        "Dockerfile" | "Containerfile" => Some(("Dockerfile", SPEC_HASH)),
        "CMakeLists.txt" => Some(("CMake", SPEC_HASH)),
        ".gitignore" | ".gitattributes" | ".dockerignore" | ".npmignore" => {
            Some(("Config", SPEC_HASH))
        }
        _ => None,
    }
}

fn match_extension(ext: &str) -> Option<(&'static str, usize)> {
    // Match is preferred over a HashMap so the table is one cache-line
    // walk and stays inlinable. Order: most-common-first within each
    // family for branch prediction friendliness, but correctness is
    // identical regardless.
    Some(match ext {
        // C-family (// and /* */)
        "rs" => ("Rust", SPEC_C),
        "ts" | "mts" | "cts" => ("TypeScript", SPEC_C),
        "tsx" => ("TSX", SPEC_C),
        "js" | "mjs" | "cjs" => ("JavaScript", SPEC_C),
        "jsx" => ("JSX", SPEC_C),
        "go" => ("Go", SPEC_C),
        "java" => ("Java", SPEC_C),
        "kt" | "kts" => ("Kotlin", SPEC_C),
        "swift" => ("Swift", SPEC_C),
        "m" | "mm" => ("Objective-C", SPEC_C),
        "c" | "h" => ("C", SPEC_C),
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" | "ipp" => ("C++", SPEC_C),
        "cs" => ("C#", SPEC_C),
        "scala" | "sc" => ("Scala", SPEC_C),
        "dart" => ("Dart", SPEC_C),
        "zig" => ("Zig", SPEC_C),
        "css" => ("CSS", SPEC_C),
        "scss" | "sass" => ("SCSS", SPEC_C),
        "less" => ("Less", SPEC_C),
        "groovy" | "gradle" => ("Groovy", SPEC_C),
        "rs.in" => ("Rust", SPEC_C),
        "v" | "sv" => ("Verilog", SPEC_C),
        "php" | "phtml" => ("PHP", SPEC_C),

        // Hash-comment family
        "py" | "pyw" | "pyi" => ("Python", SPEC_HASH),
        "rb" => ("Ruby", SPEC_HASH),
        "sh" | "bash" | "zsh" | "ksh" => ("Shell", SPEC_HASH),
        "fish" => ("Shell", SPEC_HASH),
        "pl" | "pm" => ("Perl", SPEC_HASH),
        "r" | "rmd" => ("R", SPEC_HASH),
        "jl" => ("Julia", SPEC_HASH),
        "ex" | "exs" => ("Elixir", SPEC_HASH),
        "tf" | "tfvars" => ("Terraform", SPEC_HASH),
        "hcl" => ("HCL", SPEC_HASH),
        "toml" => ("TOML", SPEC_HASH),
        "yaml" | "yml" => ("YAML", SPEC_HASH),
        "nix" => ("Nix", SPEC_HASH),
        "conf" | "ini" | "cfg" => ("Config", SPEC_HASH),

        // PowerShell — # line + <# #> block
        "ps1" | "psm1" | "psd1" => ("PowerShell", SPEC_POWERSHELL),

        // Lua — -- line + --[[ ]] block
        "lua" => ("Lua", SPEC_LUA),

        // SQL — -- line + /* */ block
        "sql" => ("SQL", SPEC_SQL),

        // Haskell — -- line + {- -} block
        "hs" | "lhs" => ("Haskell", SPEC_HASKELL),

        // Erlang — % only
        "erl" | "hrl" => ("Erlang", SPEC_ERLANG),

        // OCaml — (* *) only
        "ml" | "mli" => ("OCaml", SPEC_OCAML),

        // HTML/XML — <!-- --> only
        "html" | "htm" => ("HTML", SPEC_HTML),
        "xml" | "xsd" | "xsl" => ("XML", SPEC_HTML),
        "svg" => ("SVG", SPEC_HTML),
        "md" | "markdown" | "mdx" => ("Markdown", SPEC_HTML),

        // Mixed — Vue/Svelte/Astro have <!-- --> and // (script blocks),
        // but the dominant tag-based shell wraps everything. SPEC_MIXED
        // recognizes both forms; the bias toward classifying ambiguous
        // lines as code matches user expectations for these formats.
        "vue" => ("Vue", SPEC_MIXED),
        "svelte" => ("Svelte", SPEC_MIXED),
        "astro" => ("Astro", SPEC_MIXED),

        // Comment-less / data
        "json" | "jsonc" => ("JSON", SPEC_NONE),
        "graphql" | "gql" => ("GraphQL", SPEC_HASH),

        // Templating
        "tex" | "ltx" => ("TeX", SPEC_TEX),

        _ => return None,
    })
}

/// Directories containing vendored output, generated artifacts, or
/// frequently changing tool caches, at any depth.
fn is_ignored_dir(name: &str) -> bool {
    matches!(
        name,
        "node_modules"
            | "target"
            | "dist"
            | "build"
            | "out"
            | "vendor"
            | "third_party"
            | "thirdparty"
            | ".git"
            | ".svn"
            | ".hg"
            | "__pycache__"
            | ".venv"
            | "venv"
            | ".tox"
            | ".next"
            | ".nuxt"
            | ".svelte-kit"
            | ".astro"
            | ".cache"
            | ".turbo"
            | ".parcel-cache"
            | "obj"
            | ".idea"
            | ".vscode"
            | "DerivedData"
            | "Pods"
            | ".gradle"
    )
}

/// Directories ignored only at the repository root. `bin` is the reason this
/// distinction exists: it is a build-output directory at the root and an
/// ordinary source directory anywhere else — `src/bin/*.rs` is where Cargo
/// keeps a crate's extra binaries, and pruning it by name at any depth
/// deleted real source from every such repository's counts.
fn is_root_only_ignored_dir(name: &str) -> bool {
    matches!(name, "bin")
}

/// Committed but not authored: lockfiles, minified bundles, source maps, and
/// generated client code. Counting them makes a language breakdown describe a
/// package manager rather than the project.
fn is_generated_file(name: &str) -> bool {
    const LOCKFILES: &[&str] = &[
        "package-lock.json",
        "npm-shrinkwrap.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "bun.lockb",
        "Cargo.lock",
        "composer.lock",
        "Gemfile.lock",
        "poetry.lock",
        "Pipfile.lock",
        "go.sum",
        "pubspec.lock",
        "packages.lock.json",
        "mix.lock",
        "flake.lock",
    ];
    const GENERATED_SUFFIXES: &[&str] = &[
        ".min.js",
        ".min.css",
        ".map",
        ".pb.go",
        "_pb2.py",
        "_pb2_grpc.py",
        ".pb.cc",
        ".pb.h",
        ".g.dart",
        ".freezed.dart",
        ".generated.ts",
        ".generated.cs",
        ".designer.cs",
    ];
    LOCKFILES.contains(&name)
        || GENERATED_SUFFIXES
            .iter()
            .any(|suffix| name.ends_with(suffix))
}

/// The single exclusion policy for both the census and the exact count. They
/// must agree: the two numbers are rendered under the same labels, so a path
/// counted by one and not the other silently changes what a repository's
/// language breakdown means between runs.
fn is_excluded_path(path: &Path) -> bool {
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_generated_file)
    {
        return true;
    }
    let directories: Vec<&str> = path
        .parent()
        .map(|parent| {
            parent
                .components()
                .filter_map(|component| component.as_os_str().to_str())
                .collect()
        })
        .unwrap_or_default();
    if directories
        .first()
        .is_some_and(|name| is_root_only_ignored_dir(name))
    {
        return true;
    }
    directories.iter().any(|name| is_ignored_dir(name))
}

fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(BINARY_SNIFF_BYTES).any(|&b| b == 0)
}

// Comment-spec table

#[derive(Copy, Clone)]
struct LangSpec {
    line_comment: &'static [&'static str],
    block_comment: &'static [(&'static str, &'static str)],
}

// Indexed by the SPEC_* constants below — kept as a flat array so the
// per-file hot path is a `LANG_SPECS[idx]` instead of a match cascade.
const LANG_SPECS: &[LangSpec] = &[
    LangSpec {
        // 0: SPEC_NONE — no comments (JSON proper).
        line_comment: &[],
        block_comment: &[],
    },
    LangSpec {
        // 1: SPEC_C — //, /* */
        line_comment: &["//"],
        block_comment: &[("/*", "*/")],
    },
    LangSpec {
        // 2: SPEC_HASH — #
        line_comment: &["#"],
        block_comment: &[],
    },
    LangSpec {
        // 3: SPEC_HTML — <!-- -->
        line_comment: &[],
        block_comment: &[("<!--", "-->")],
    },
    LangSpec {
        // 4: SPEC_LUA — --, --[[ ]]
        line_comment: &["--"],
        block_comment: &[("--[[", "]]")],
    },
    LangSpec {
        // 5: SPEC_HASKELL — --, {- -}
        line_comment: &["--"],
        block_comment: &[("{-", "-}")],
    },
    LangSpec {
        // 6: SPEC_SQL — --, /* */
        line_comment: &["--"],
        block_comment: &[("/*", "*/")],
    },
    LangSpec {
        // 7: SPEC_ERLANG — %
        line_comment: &["%"],
        block_comment: &[],
    },
    LangSpec {
        // 8: SPEC_OCAML — (* *)
        line_comment: &[],
        block_comment: &[("(*", "*)")],
    },
    LangSpec {
        // 9: SPEC_POWERSHELL — #, <# #>
        line_comment: &["#"],
        block_comment: &[("<#", "#>")],
    },
    LangSpec {
        // 10: SPEC_MIXED — //, /* */, <!-- --> (Vue/Svelte/Astro)
        line_comment: &["//"],
        block_comment: &[("/*", "*/"), ("<!--", "-->")],
    },
    LangSpec {
        // 11: SPEC_TEX — %
        line_comment: &["%"],
        block_comment: &[],
    },
];

const SPEC_NONE: usize = 0;
const SPEC_C: usize = 1;
const SPEC_HASH: usize = 2;
const SPEC_HTML: usize = 3;
const SPEC_LUA: usize = 4;
const SPEC_HASKELL: usize = 5;
const SPEC_SQL: usize = 6;
const SPEC_ERLANG: usize = 7;
const SPEC_OCAML: usize = 8;
const SPEC_POWERSHELL: usize = 9;
const SPEC_MIXED: usize = 10;
const SPEC_TEX: usize = 11;

// Line counter

/// Per-line classification:
///   blank   = no non-whitespace, no comment chars.
///   comment = at least one comment char, no code chars.
///   code    = at least one non-comment, non-whitespace char.
///
/// Block-comment state carries across line boundaries via `in_block`.
/// String-literal awareness is intentionally absent — see the module
/// header for the cost/benefit reasoning.
fn count_lines_in_text(spec: LangSpec, text: &str) -> (i64, i64, i64) {
    let mut blank = 0i64;
    let mut comment = 0i64;
    let mut code = 0i64;
    let mut in_block: Option<&'static str> = None;

    for line in text.lines() {
        if line.is_empty() && in_block.is_none() {
            blank += 1;
            continue;
        }
        let (has_code, has_comment, new_block) = classify_line(spec, line, in_block);
        in_block = new_block;
        match (has_code, has_comment) {
            (true, _) => code += 1,
            (false, true) => comment += 1,
            (false, false) => blank += 1,
        }
    }
    (blank, comment, code)
}

fn classify_line(
    spec: LangSpec,
    line: &str,
    mut in_block: Option<&'static str>,
) -> (bool, bool, Option<&'static str>) {
    let bytes = line.as_bytes();
    let mut i = 0usize;
    let mut has_code = false;
    let mut has_comment = false;

    while i < bytes.len() {
        if let Some(close) = in_block {
            if bytes[i..].starts_with(close.as_bytes()) {
                has_comment = true;
                i += close.len();
                in_block = None;
                continue;
            }
            if !bytes[i].is_ascii_whitespace() {
                has_comment = true;
            }
            i += 1;
            continue;
        }

        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Line comments — match longest-prefix-first so e.g. Lua's `--[[`
        // wins over `--`. Specs are written with the longer form already
        // in `block_comment`, so checking blocks before line markers
        // gives us the right precedence without explicit length sort.
        let mut started = false;
        for &(open, close) in spec.block_comment {
            if bytes[i..].starts_with(open.as_bytes()) {
                has_comment = true;
                in_block = Some(close);
                i += open.len();
                started = true;
                break;
            }
        }
        if started {
            continue;
        }
        for &lc in spec.line_comment {
            if bytes[i..].starts_with(lc.as_bytes()) {
                return (has_code, true, in_block); // rest of line is comment
            }
        }
        has_code = true;
        i += 1;
    }
    (has_code, has_comment, in_block)
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;

    fn rust() -> LangSpec {
        LANG_SPECS[SPEC_C]
    }
    fn py() -> LangSpec {
        LANG_SPECS[SPEC_HASH]
    }
    fn html() -> LangSpec {
        LANG_SPECS[SPEC_HTML]
    }
    fn lua() -> LangSpec {
        LANG_SPECS[SPEC_LUA]
    }
    fn json() -> LangSpec {
        LANG_SPECS[SPEC_NONE]
    }

    #[test]
    fn rust_basic() {
        let src = "fn main() {\n    // hi\n    println!(\"x\");\n\n}\n";
        let (b, c, code) = count_lines_in_text(rust(), src);
        assert_eq!((b, c, code), (1, 1, 3));
    }

    #[test]
    fn unicode_code_does_not_slice_inside_utf8() {
        let src = "let café = \"Հայերեն\";\nprintln!(\"你好\");\n";
        let (blank, comment, code) = count_lines_in_text(rust(), src);
        assert_eq!((blank, comment, code), (0, 0, 2));
    }

    #[test]
    fn unicode_inside_block_comment_is_counted_safely() {
        let src = "/* Հայերեն\n你好 */\nfn main() {}\n";
        let (blank, comment, code) = count_lines_in_text(rust(), src);
        assert_eq!((blank, comment, code), (0, 2, 1));
    }

    #[test]
    fn rust_block_comment_across_lines() {
        let src = "/*\nfoo\n*/\nfn main() {}\n";
        let (b, c, code) = count_lines_in_text(rust(), src);
        // /* and */ on their own lines + middle "foo" → 3 comment.
        // fn main() {} → 1 code.
        assert_eq!((b, c, code), (0, 3, 1));
    }

    #[test]
    fn rust_block_with_code_after_close() {
        let src = "let x /* note */ = 1;\n";
        let (b, c, code) = count_lines_in_text(rust(), src);
        // Mixed line counts as code (code wins).
        assert_eq!((b, c, code), (0, 0, 1));
    }

    #[test]
    fn python_hash_only() {
        let src = "# comment\nx = 1\n\n# another\n";
        let (b, c, code) = count_lines_in_text(py(), src);
        assert_eq!((b, c, code), (1, 2, 1));
    }

    #[test]
    fn python_triple_quoted_string_is_code_not_comment() {
        // Tokei classifies this as comment; we don't try to be that
        // smart. Confirm our predictable behavior.
        let src = "\"\"\"docstring\"\"\"\n";
        let (b, c, code) = count_lines_in_text(py(), src);
        assert_eq!((b, c, code), (0, 0, 1));
    }

    #[test]
    fn html_block_comment() {
        let src = "<!-- a -->\n<p>x</p>\n";
        let (b, c, code) = count_lines_in_text(html(), src);
        assert_eq!((b, c, code), (0, 1, 1));
    }

    #[test]
    fn lua_block_comment_takes_precedence_over_line() {
        let src = "--[[\nx\n]]\nfoo()\n";
        let (b, c, code) = count_lines_in_text(lua(), src);
        assert_eq!((b, c, code), (0, 3, 1));
    }

    #[test]
    fn json_has_no_comments() {
        let src = "{\n  \"a\": 1\n}\n";
        let (b, c, code) = count_lines_in_text(json(), src);
        assert_eq!((b, c, code), (0, 0, 3));
    }

    #[test]
    fn detects_rust_by_extension() {
        let p = std::path::Path::new("foo/bar.rs");
        let hit = detect_language(p).expect("hit");
        assert_eq!(hit.name, "Rust");
    }

    #[test]
    fn detects_makefile_by_basename() {
        let p = std::path::Path::new("Makefile");
        let hit = detect_language(p).expect("hit");
        assert_eq!(hit.name, "Makefile");
    }

    #[test]
    fn ignored_dirs_includes_node_modules_and_git() {
        assert!(is_ignored_dir("node_modules"));
        assert!(is_ignored_dir(".git"));
        assert!(is_ignored_dir("target"));
        assert!(!is_ignored_dir("src"));
    }

    #[test]
    fn binary_sniffer_flags_nul_byte() {
        let mut buf = vec![b'A'; 100];
        buf[10] = 0;
        assert!(looks_binary(&buf));
        assert!(!looks_binary(b"hello world"));
    }

    #[test]
    fn carriage_return_only_line_is_blank() {
        let src = "\r\n\r\nfoo\n";
        let (b, c, code) = count_lines_in_text(rust(), src);
        assert_eq!((b, c, code), (2, 0, 1));
    }

    /// Build a tree listing the way `git ls-tree -rz HEAD` emits it.
    fn listing(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (mode_and_kind, path) in entries {
            out.extend_from_slice(
                format!("{mode_and_kind} 0000000000000000000000000000000000000000\t{path}")
                    .as_bytes(),
            );
            out.push(0);
        }
        out
    }

    #[test]
    fn tree_listing_keeps_blobs_and_drops_symlinks_and_submodules() {
        let blobs = parse_tree_listing(&listing(&[
            ("100644 blob", "src/main.rs"),
            ("120000 blob", "link-to-elsewhere"),
            ("160000 commit", "vendored-submodule"),
            ("100755 blob", "scripts/run.sh"),
        ]));
        let paths: Vec<&str> = blobs.iter().map(|blob| blob.path.as_str()).collect();
        assert_eq!(paths, vec!["src/main.rs", "scripts/run.sh"]);
    }

    #[test]
    fn readiness_is_derived_from_head_paths_without_blob_hydration() {
        let blobs = parse_tree_listing(&listing(&[
            ("100644 blob", "README.md"),
            ("100644 blob", "LICENCE"),
            ("100644 blob", ".github/SECURITY.md"),
            ("100644 blob", ".github/CODE_OF_CONDUCT.md"),
            ("100644 blob", ".github/ISSUE_TEMPLATE/bug.yml"),
            ("100644 blob", ".github/PULL_REQUEST_TEMPLATE.md"),
            ("100644 blob", ".github/workflows/test.yml"),
            ("100644 blob", "docs/CONTRIBUTING.md"),
            ("100644 blob", "tests/smoke_test.py"),
            ("100644 blob", "src/main.rs"),
        ]));
        let value = readiness_from_blobs(&blobs);
        assert!(value.readme);
        assert!(value.license);
        assert!(value.security);
        assert!(value.code_of_conduct);
        assert!(value.contributing);
        assert!(value.issue_templates);
        assert!(value.pr_template);
        assert!(value.ci);
        assert!(value.tests);
        assert!(!value.cla);
        assert!(!value.dependency_updates);
    }

    #[test]
    fn census_counts_classified_files_and_reports_the_whole_tree() {
        let blobs = parse_tree_listing(&listing(&[
            ("100644 blob", "src/main.rs"),
            ("100644 blob", "src/lib.rs"),
            ("100644 blob", "web/app.ts"),
            ("100644 blob", "assets/logo.bin"),
            ("100644 blob", "Makefile"),
            ("100644 blob", "node_modules/vendor.js"),
        ]));
        let (rows, total_files) = census_from_blobs(&blobs);
        assert_eq!(total_files, 6);
        assert_eq!(rows.iter().map(|row| row.files).sum::<i64>(), 4);
        assert_eq!(rows[0].language, "Rust");
        assert_eq!(rows[0].files, 2);
        assert!(
            rows.iter().all(|row| {
                row.lines_code == 0 && row.lines_blank == 0 && row.lines_comment == 0
            })
        );
    }

    /// `bin` is a build-output directory at the root and a source directory
    /// anywhere else; Cargo keeps a crate's extra binaries in `src/bin`.
    #[test]
    fn bin_is_ignored_only_at_the_repository_root() {
        assert!(is_excluded_path(Path::new("bin/tool.rs")));
        assert!(!is_excluded_path(Path::new("src/bin/server.rs")));
        assert!(is_excluded_path(Path::new("web/node_modules/pkg/index.js")));
        assert!(!is_excluded_path(Path::new("src/main.rs")));
    }

    /// The read phase runs on a blocking thread, which no outer timeout can
    /// cancel, so the deadline has to be enforced inside it — and a lapse must
    /// leave the caller with an error to degrade on rather than a partial
    /// count presented as exact.
    #[tokio::test]
    async fn the_read_phase_enforces_its_own_deadline() {
        if std::process::Command::new("git")
            .arg("--version")
            .status()
            .is_err()
        {
            eprintln!("skipping: git not available");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "a@example.com"],
            vec!["config", "user.name", "Alice"],
        ] {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(dir)
                    .args(&args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(dir.join("main.rs"), "fn main() {}\n// note\n\n").unwrap();
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["add", "-A"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["commit", "-qm", "init"])
                .status()
                .unwrap()
                .success()
        );

        let blobs = head_blobs(dir).await.unwrap();
        let counted = read_and_count(
            dir,
            &blobs,
            std::time::Instant::now() + std::time::Duration::from_secs(30),
        )
        .unwrap();
        assert_eq!(counted.len(), 1);
        assert_eq!(counted[0].language, "Rust");
        assert_eq!(counted[0].lines_code, 1);

        let lapsed = read_and_count(dir, &blobs, std::time::Instant::now());
        assert!(
            lapsed.is_err(),
            "a lapsed read budget degrades to the census instead of reporting \
             a partial count as exact"
        );
    }

    #[test]
    fn generated_and_locked_files_are_excluded() {
        assert!(is_excluded_path(Path::new("package-lock.json")));
        assert!(is_excluded_path(Path::new("web/static/app.min.js")));
        assert!(is_excluded_path(Path::new("api/service.pb.go")));
        assert!(!is_excluded_path(Path::new("web/src/app.js")));
    }
}
