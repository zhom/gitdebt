//! The curated comparison catalog: deployment bootstrap, and the editorial
//! metadata the `/compare/{category}` surfaces read.
//!
//! `frontend/src/data/categories.ts` remains the single catalog source. The
//! worker embeds it at compile time, extracts only exact `owner/repo` string
//! literals, and offers those repositories to both durable queues on every
//! startup. Queue deduplication and freshness checks make this safe across
//! rolling deploys and multiple worker replicas.
//!
//! [`categories`] parses the same embedded file for slug, name, short
//! description and ordered members, so the API never carries a second copy of
//! hand-written editorial text that could drift from the pages it describes.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use anyhow::{Context, Result};

use crate::db::Db;

const CATEGORIES_SOURCE: &str = include_str!("../../frontend/src/data/categories.ts");

/// Sorted, deduplicated repository slugs from the curated comparison source.
pub fn curated_repos() -> Vec<String> {
    CATEGORIES_SOURCE
        .lines()
        .filter_map(|line| {
            let value = line.trim().strip_prefix('"')?.strip_suffix("\",")?;
            is_repo_slug(value).then(|| value.to_string())
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn is_repo_slug(value: &str) -> bool {
    let Some((owner, repo)) = value.split_once('/') else {
        return false;
    };
    !repo.contains('/') && valid_segment(owner) && valid_segment(repo)
}

fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// One curated comparison category, as authored in the frontend source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    /// URL slug under `/compare/`. Lowercase, hyphenated.
    pub slug: String,
    /// Display name, e.g. "Frontend frameworks".
    pub name: String,
    /// One-line summary for hubs, link lists, and meta descriptions.
    pub short: String,
    /// Member repositories in the authored order, which is editorial.
    pub repos: Vec<String>,
}

/// Every curated category, in source order.
pub fn categories() -> Vec<Category> {
    static PARSED: OnceLock<Vec<Category>> = OnceLock::new();
    PARSED
        .get_or_init(|| parse_categories(CATEGORIES_SOURCE))
        .clone()
}

/// One category by exact slug.
pub fn category(slug: &str) -> Option<Category> {
    categories().into_iter().find(|entry| entry.slug == slug)
}

/// Extract the categories from the embedded TypeScript.
///
/// Scoped to the `CATEGORIES` array literal: `CATEGORY_GROUPS` further down the
/// file carries its own `name:` keys, and a parser that ran past the closing
/// bracket would quietly overwrite the last category with a group heading.
/// Anything the parse cannot recognize is dropped rather than guessed, and the
/// tests below assert a floor on what must survive — a silently empty catalog
/// is the failure mode this file exists to prevent.
fn parse_categories(source: &str) -> Vec<Category> {
    let Some((_, body)) = source.split_once("export const CATEGORIES") else {
        return Vec::new();
    };

    let mut out: Vec<Category> = Vec::new();
    let mut current: Option<Category> = None;
    let mut in_repos = false;
    // A key whose value a formatter pushed onto the following line.
    let mut wrapped: Option<&str> = None;

    for line in body.lines() {
        let line = line.trim();
        if line == "];" {
            break;
        }
        if in_repos {
            if line.starts_with(']') {
                in_repos = false;
            } else if let (Some(entry), Some(value)) = (current.as_mut(), quoted(line))
                && is_repo_slug(&value)
            {
                entry.repos.push(value);
            }
            continue;
        }
        if let Some(key) = wrapped.take()
            && let (Some(entry), Some(value)) = (current.as_mut(), quoted(line))
        {
            assign(entry, key, value);
            continue;
        }
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let rest = rest.trim();
        match key {
            "slug" => {
                let Some(value) = quoted(rest) else { continue };
                out.extend(current.take());
                current = Some(Category {
                    slug: value,
                    name: String::new(),
                    short: String::new(),
                    repos: Vec::new(),
                });
            }
            "name" | "short" => match (current.as_mut(), quoted(rest)) {
                (Some(entry), Some(value)) => assign(entry, key, value),
                (Some(_), None) if rest.is_empty() => wrapped = Some(key),
                _ => {}
            },
            "repos" if rest.starts_with('[') => in_repos = current.is_some(),
            _ => {}
        }
    }
    out.extend(current);
    out
}

fn assign(entry: &mut Category, key: &str, value: String) {
    match key {
        "name" => entry.name = value,
        "short" => entry.short = value,
        _ => {}
    }
}

/// The contents of a trailing TypeScript string literal, comma included or not.
fn quoted(value: &str) -> Option<String> {
    let value = value.trim_end().trim_end_matches(',');
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.replace("\\\"", "\"").replace("\\\\", "\\"))
}

/// Offer every curated repository to both pipelines.
///
/// Star-history insertion is one set-based Postgres statement and only
/// selects cold/stale repositories. Repo-health enqueueing reuses the shared
/// freshness predicate and global capacity contract.
pub async fn enqueue_curated(db: &Db) -> Result<(u64, usize)> {
    let repos = curated_repos();
    if repos.is_empty() {
        anyhow::bail!("curated comparison catalog unexpectedly contains no repositories");
    }
    let star_jobs = crate::queue::enqueue_cold_or_stale_many(db, &repos, 0)
        .await
        .context("enqueue curated star histories")?;
    let analysis_jobs = crate::repo_analysis::enqueue_many(db, &repos, repos.len())
        .await
        .context("enqueue curated repo analyses")?;
    Ok((star_jobs, analysis_jobs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_is_large_valid_and_deduplicated() {
        let repos = curated_repos();
        assert!(
            repos.len() >= 100,
            "comparison catalog unexpectedly shrank to {} repos",
            repos.len()
        );
        assert!(repos.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(repos.iter().all(|repo| is_repo_slug(repo)));
        assert!(repos.iter().any(|repo| repo == "facebook/react"));
        assert!(repos.iter().any(|repo| repo == "vuejs/vue"));
    }

    /// The parse reads hand-written TypeScript, so a reformat of the source is
    /// the realistic way it breaks. Every assertion here fails loudly on an
    /// empty or thinned catalog rather than letting the API publish comparison
    /// pages with no members.
    #[test]
    fn parsed_categories_are_complete_and_cover_the_curated_repos() {
        let categories = categories();
        assert!(
            categories.len() >= 18,
            "comparison catalog unexpectedly shrank to {} categories",
            categories.len()
        );

        let mut slugs: Vec<&str> = categories.iter().map(|entry| entry.slug.as_str()).collect();
        let total = slugs.len();
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(slugs.len(), total, "duplicate category slug");

        for entry in &categories {
            assert!(!entry.slug.is_empty());
            assert!(!entry.name.is_empty(), "{} has no name", entry.slug);
            assert!(!entry.short.is_empty(), "{} has no summary", entry.slug);
            assert!(
                entry.repos.len() >= 4,
                "{} carries only {} repos",
                entry.slug,
                entry.repos.len()
            );
            assert!(entry.repos.iter().all(|repo| is_repo_slug(repo)));
        }

        // Two independent parses of the same file: the line scan that feeds the
        // queues and the structured parse that feeds the pages must agree, or
        // one of them is reading a stale shape.
        let members: Vec<String> = categories
            .iter()
            .flat_map(|entry| entry.repos.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        assert_eq!(members, curated_repos());
    }

    #[test]
    fn category_lookup_is_exact() {
        let frontend = category("frontend-frameworks").expect("curated category");
        assert_eq!(frontend.name, "Frontend frameworks");
        assert!(frontend.short.contains("React"));
        assert_eq!(
            frontend.repos.first().map(String::as_str),
            Some("facebook/react")
        );
        // Slugs are lowercase by convention; the lookup does not normalize.
        assert!(category("Frontend-Frameworks").is_none());
        assert!(category("no-such-category").is_none());
    }

    /// `CATEGORY_GROUPS` sits below the array and carries its own `name:` keys.
    /// Running past the closing bracket would overwrite the last category.
    #[test]
    fn parse_stops_at_the_end_of_the_categories_array() {
        let categories = categories();
        let last = categories.last().expect("at least one category");
        assert_ne!(last.name, "Web development");
        assert!(!categories.iter().any(|entry| entry.slug.is_empty()));

        // A source whose declaration moved or was renamed parses to nothing,
        // which the assertions above turn into a red test rather than an empty
        // catalog shipped to production.
        assert!(parse_categories("export const OTHER = [];").is_empty());
    }
}
