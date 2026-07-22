//! In-house lines-of-code aggregator.
//!
//! Walks an extracted HEAD tree, classifies each file by extension /
//! basename, and counts blank / comment / code lines per language.
//!
//! Why hand-rolled instead of `tokei`:
//!   1. Tokei is a 100+ language behemoth pulling in clap, regex, and a
//!      heap of language definitions we don't need. We only render the
//!      top ~12 in the chart, so most of that surface is dead weight.
//!   2. Tokei follows symlinks by default. Hostile repos with a symlink
//!      to `/etc` or a runaway loop would walk host filesystem state.
//!      `walkdir::WalkDir::new(...).follow_links(false)` makes that
//!      impossible — we never traverse anything outside the extracted
//!      tarball.
//!
//! Comment classification is a per-language single-pass state machine.
//! No string-literal tracking (tokei has it, we skip it). The error
//! mode is "occasionally classifying a line as comment when a string
//! contains `//`", which moves a handful of lines between the comment
//! and code buckets — irrelevant at the order of magnitude this chart
//! shows.

use std::path::Path;
use std::process::Stdio;
use std::{collections::HashMap, time::Duration};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use tempfile::TempDir;
use tokio::process::Command;
use tokio::task;
use walkdir::WalkDir;

use crate::db::Db;

/// Cap per-file size. Anything bigger is almost always generated
/// (minified bundles, vendored snapshots, fixture data) and would
/// dominate counts without representing human-written code.
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// Probe size for binary detection. We sniff the first chunk for a NUL
/// byte and skip the file if found — text files in any encoding we
/// support don't contain NULs.
const BINARY_SNIFF_BYTES: usize = 8 * 1024;

/// Avoid hydrating the complete HEAD of very large blobless clones. The tree
/// still gives us an exact, current language/file census without downloading
/// gigabytes of blobs; the UI and embed renderer explicitly label that
/// fallback in files rather than pretending it is a line count.
const DEFAULT_EXACT_LINE_COUNT_MAX_FILES: usize = 20_000;
const DEFAULT_EXACT_LINE_COUNT_TIMEOUT_SECS: u64 = 20;

#[derive(Debug, Clone)]
pub struct LanguageCount {
    pub language: String,
    pub files: i64,
    pub lines_code: i64,
    pub lines_blank: i64,
    pub lines_comment: i64,
}

/// Walk HEAD of a bare clone, count lines per language. Returns
/// languages with non-zero total lines, sorted by `lines_code` desc.
pub async fn count_lines(repo_path: &Path) -> Result<Vec<LanguageCount>> {
    let extracted = extract_head(repo_path).await?;
    let extracted_path = extracted.path().to_path_buf();
    // The walk + parse is sync + cpu-bound; spawn_blocking keeps it off
    // the runtime so we don't starve workers during a big repo.
    let counts = task::spawn_blocking(move || walk_and_count(&extracted_path))
        .await
        .context("line-count task")??;
    drop(extracted);
    Ok(counts)
}

/// Return exact language file counts from the committed HEAD tree without
/// materializing blobs. The second value is the total number of files in the
/// tree, including file types that gitdebt does not classify.
pub async fn language_file_census(repo_path: &Path) -> Result<(Vec<LanguageCount>, usize)> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["ls-tree", "-rz", "--name-only", "HEAD"])
        .kill_on_drop(true)
        .output()
        .await
        .context("git ls-tree")?;
    if !output.status.success() {
        bail!("git ls-tree exited {}", output.status);
    }

    Ok(language_file_census_from_paths(&output.stdout))
}

fn language_file_census_from_paths(paths: &[u8]) -> (Vec<LanguageCount>, usize) {
    let mut total_files = 0usize;
    let mut files_by_language: HashMap<&'static str, i64> = HashMap::new();
    for raw_path in paths.split(|byte| *byte == 0) {
        if raw_path.is_empty() {
            continue;
        }
        total_files = total_files.saturating_add(1);
        let Ok(path) = std::str::from_utf8(raw_path) else {
            continue;
        };
        let path = Path::new(path);
        if path
            .components()
            .any(|component| component.as_os_str().to_str().is_some_and(is_ignored_dir))
        {
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
    (counts, total_files)
}

pub fn exact_line_count_max_files() -> usize {
    std::env::var("REPO_LINE_COUNT_MAX_FILES")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_EXACT_LINE_COUNT_MAX_FILES)
}

pub fn exact_line_count_timeout() -> Duration {
    Duration::from_secs(
        std::env::var("REPO_LINE_COUNT_TIMEOUT_SECONDS")
            .ok()
            .and_then(|value| value.parse().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_EXACT_LINE_COUNT_TIMEOUT_SECS),
    )
}

/// Materialize HEAD into a tempdir as `git archive HEAD > tar; tar -xf tar`.
/// Two-step (write tar, extract tar) instead of a piped one-shot — keeps
/// us off tokio's pipe-stdio dance and the intermediate `.tar` is
/// removed before the walker sees the dir.
async fn extract_head(repo_path: &Path) -> Result<TempDir> {
    let tmp = tempfile::Builder::new()
        .prefix("gitdebt-loc-")
        .tempdir()
        .context("create tempdir")?;
    let tar_path = tmp.path().join(".gitdebt-HEAD.tar");

    let archive_out = std::fs::File::create(&tar_path).context("create tar file")?;
    let archive_status = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["archive", "--format=tar", "HEAD"])
        .stdout(archive_out)
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .status()
        .await
        .context("git archive")?;
    if !archive_status.success() {
        bail!("git archive exited {archive_status}");
    }

    let tar_status = Command::new("tar")
        .arg("-xf")
        .arg(&tar_path)
        .arg("-C")
        .arg(tmp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .status()
        .await
        .context("tar -xf")?;
    if !tar_status.success() {
        bail!("tar exited {tar_status}");
    }
    let _ = tokio::fs::remove_file(&tar_path).await;
    Ok(tmp)
}

fn walk_and_count(dir: &Path) -> Result<Vec<LanguageCount>> {
    use std::collections::HashMap;
    let mut totals: HashMap<&'static str, LanguageCount> = HashMap::new();

    // `follow_links(false)` is the security boundary: hostile repos
    // could include a symlink to `/etc/passwd` or a self-referential
    // loop, both of which would let us walk outside the extracted tar
    // if we honored them. We don't.
    let walker = WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !is_ignored_dir(e.file_name().to_str().unwrap_or("")));

    for entry in walker.filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Some(lang) = detect_language(path) else {
            continue;
        };
        let Ok(meta) = entry.metadata() else { continue };
        if meta.len() > MAX_FILE_BYTES {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        if looks_binary(&bytes) {
            continue;
        }
        let text = match std::str::from_utf8(&bytes) {
            Ok(s) => s,
            Err(_) => continue, // not UTF-8; treat as binary/foreign
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

    let mut out: Vec<LanguageCount> = totals
        .into_values()
        .filter(|l| l.lines_code + l.lines_comment + l.lines_blank > 0)
        .collect();
    out.sort_by_key(|row| std::cmp::Reverse(row.lines_code));
    Ok(out)
}

/// Replace the persisted line counts for `repo` with `counts`. Single
/// transaction: visible state goes from "old data" → "new data" without
/// any half-empty intermediate.
pub async fn save(db: &Db, repo: &str, counts: &[LanguageCount]) -> Result<()> {
    let mut tx = db.pool.begin().await?;
    sqlx::query("DELETE FROM repo_lines WHERE repo = $1")
        .bind(repo)
        .execute(&mut *tx)
        .await?;
    for c in counts {
        sqlx::query(
            "INSERT INTO repo_lines (repo, language, files, lines_code, lines_blank, lines_comment) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(repo)
        .bind(&c.language)
        .bind(c.files)
        .bind(c.lines_code)
        .bind(c.lines_blank)
        .bind(c.lines_comment)
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
/// frequently changing tool caches.
fn is_ignored_dir(name: &str) -> bool {
    matches!(
        name,
        "node_modules"
            | "target"
            | "dist"
            | "build"
            | "out"
            | "vendor"
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
            | "bin"
            | "obj"
            | ".idea"
            | ".vscode"
            | "DerivedData"
            | "Pods"
            | ".gradle"
    )
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

    #[test]
    fn language_file_census_is_exact_and_ignores_unknown_types() {
        let (rows, total_files) = language_file_census_from_paths(
            b"src/main.rs\0src/lib.rs\0web/app.ts\0assets/logo.bin\0Makefile\0node_modules/vendor.js\0",
        );
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
}
