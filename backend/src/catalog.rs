//! Deployment bootstrap for the curated comparison catalog.
//!
//! `frontend/src/data/categories.ts` remains the single catalog source. The
//! worker embeds it at compile time, extracts only exact `owner/repo` string
//! literals, and offers those repositories to both durable queues on every
//! startup. Queue deduplication and freshness checks make this safe across
//! rolling deploys and multiple worker replicas.

use std::collections::BTreeSet;

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
}
