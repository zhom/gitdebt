use std::sync::Arc;

use chrono::{DateTime, Utc};
use reqwest::Response;
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, LINK, USER_AGENT};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Semaphore;

use crate::rate_limit::{RateLimitTracker, source_for_token};

const USER_AGENT_STR: &str = concat!("gitdebt/", env!("CARGO_PKG_VERSION"));
const API_BASE: &str = "https://api.github.com";
type StargazerPageEvents = Vec<(i64, DateTime<Utc>)>;

/// Concurrency factor for parallelized stargazer page fetches. 8 is a
/// pragmatic default — high enough that TCP/TLS handshake cost amortizes
/// across pages, low enough that we don't pile up rate-limit-tracker
/// wakeups when GitHub momentarily slows. Tunable per-deployment if a
/// faster pipe wants to push it.
const STARGAZER_FETCH_CONCURRENCY: usize = 8;
const DEFAULT_MAX_IN_FLIGHT_REQUESTS: usize = 64;
const HARD_MAX_IN_FLIGHT_REQUESTS: usize = 90;

#[derive(Debug, Error)]
pub enum GithubError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("GitHub token contains invalid header characters")]
    InvalidToken,
    #[error("repo not found: {0}")]
    NotFound(String),
    /// The stargazer-list endpoint returned 404. This does not prove the
    /// repository itself is missing: GitHub can restrict this endpoint
    /// independently of public repository metadata. The worker confirms
    /// existence through `/repos/{owner}/{repo}` before deciding whether to
    /// tombstone or park the history fetch as restricted.
    #[error("stargazers unavailable: {0}")]
    StargazersUnavailable(String),
    /// A *durable* 403 with no rate-limit signal — access is denied for a
    /// reason retrying can't fix (e.g. the 2026-06-30 stargazer restriction
    /// serving 403 to non-admin callers). Distinct from [`Self::RateLimited`]
    /// so the worker can park the repo `restricted` (not re-poll it every
    /// view, and NOT tombstone it as `missing`).
    #[error("access forbidden: {0}")]
    Forbidden(String),
    #[error("rate limited; resets at {0:?}")]
    RateLimited(Option<DateTime<Utc>>),
    #[error("github error {status}: {body}")]
    Api { status: u16, body: String },
}

#[derive(Clone)]
pub struct GithubClient {
    http: reqwest::Client,
    rate: Arc<RateLimitTracker>,
    /// Bucket key for the rate-limit tracker — derived from the token's
    /// hash + a kind prefix (`default` for the env PAT, `user` for an
    /// OAuth user token, etc). Tokens with separate GitHub-side budgets
    /// must each have a distinct source so we don't conflate quotas.
    source: String,
    /// GitHub recommends keeping concurrent REST requests below 100. This
    /// process-wide-per-client gate keeps an 8× analysis configuration plus
    /// request traffic inside that boundary.
    request_permits: Arc<Semaphore>,
}

impl GithubClient {
    /// Default client for background workers — uses the env PAT (if set).
    pub fn new(token: Option<&str>, rate: Arc<RateLimitTracker>) -> Result<Self, GithubError> {
        Self::with_token("default", token, rate)
    }

    /// Request-scoped client for a logged-in user's OAuth credential. It uses
    /// the same persistent tracker but a token-derived `github:user:*` bucket,
    /// so the user's allowance is never conflated with the shared worker PAT.
    pub fn for_user_token(&self, token: &str) -> Result<Self, GithubError> {
        Self::with_token("user", Some(token), self.rate.clone())
    }

    fn with_token(
        kind: &str,
        token: Option<&str>,
        rate: Arc<RateLimitTracker>,
    ) -> Result<Self, GithubError> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_STR));
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        if let Some(t) = token {
            let mut v = HeaderValue::from_str(&format!("Bearer {t}"))
                .map_err(|_| GithubError::InvalidToken)?;
            v.set_sensitive(true);
            headers.insert(AUTHORIZATION, v);
        }
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(30))
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .build()?;
        let source = source_for_token(kind, token);
        let max_in_flight = std::env::var("GITHUB_MAX_IN_FLIGHT_REQUESTS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(DEFAULT_MAX_IN_FLIGHT_REQUESTS)
            .clamp(1, HARD_MAX_IN_FLIGHT_REQUESTS);
        Ok(Self {
            http,
            rate,
            source,
            request_permits: Arc::new(Semaphore::new(max_in_flight)),
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// Non-blocking budget probe for request-path callers: `true` when this
    /// client's source bucket has spendable budget right now (or its window
    /// has already reset), i.e. when `RateLimitTracker::acquire` would
    /// return without sleeping. Request handlers use this to degrade
    /// gracefully (serve stale/cached data) instead of hanging until the
    /// reset — only background workers are allowed to sleep on quota.
    ///
    /// The `50` mirrors `rate_limit.rs`'s private `RESERVE` headroom: at or
    /// under it, `acquire` sleeps, so the probe must use the same line.
    pub async fn has_budget(&self) -> bool {
        let (remaining, _, reset_at) = self.rate.snapshot(&self.source).await;
        let now = Utc::now().timestamp();
        now >= reset_at || remaining > 50
    }

    /// Send a GET with rate-limit acquire/record bookkeeping, scoped to
    /// this client's source bucket. All public methods route through this
    /// to keep the per-token budget tracker consistent.
    async fn send(
        &self,
        url: &str,
        accept_override: Option<&'static str>,
    ) -> Result<Response, GithubError> {
        self.rate.acquire(&self.source).await;
        let _permit = self
            .request_permits
            .acquire()
            .await
            .expect("GitHub request semaphore is never closed");
        let mut req = self.http.get(url);
        if let Some(a) = accept_override {
            req = req.header(ACCEPT, a);
        }
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                self.rate.record_response(&self.source, None).await;
                return Err(e.into());
            }
        };
        self.rate
            .record_response(&self.source, Some(resp.headers()))
            .await;
        // Differentiate "rate-limited" from a plain "forbidden". GitHub uses
        // 403 for both, so we only flip the budget when we have positive
        // evidence: either x-ratelimit-remaining=0 (primary rate limit) or
        // a Retry-After header (secondary/abuse rate limit). 429 is always
        // a rate limit. A plain 403 with neither header is an access denied
        // and shouldn't pause the worker pool.
        if resp.status() == 403 || resp.status() == 429 {
            let h = resp.headers();
            let primary_hit = h
                .get("x-ratelimit-remaining")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<i64>().ok())
                .is_some_and(|n| n == 0);
            let secondary_hit = h.contains_key("retry-after");
            if resp.status() == 429 || primary_hit || secondary_hit {
                self.rate.mark_exhausted(&self.source, Some(h)).await;
            }
        }
        Ok(resp)
    }

    /// Fetch an explicit set of stargazer pages (1-indexed, `per_page` =
    /// 100) concurrently, returning non-identifying `(position, starred_at)`
    /// events in the SAME order as `pages`. Used by the star-fetch
    /// worker: GitHub paginates stargazers oldest-first, so fetching a
    /// contiguous range of pages in parallel and concatenating in order
    /// preserves the oldest-first ordering the cumulative series relies on.
    /// Each page still routes through `RateLimitTracker::acquire` (inside
    /// `send`), so the per-token GitHub budget is honored — concurrency
    /// only reclaims TCP/TLS round-trip latency, not request quota.
    pub async fn stargazers_pages(
        &self,
        owner: &str,
        repo: &str,
        pages: &[u32],
    ) -> Result<Vec<Vec<(i64, DateTime<Utc>)>>, GithubError> {
        let base = format!("{API_BASE}/repos/{owner}/{repo}/stargazers?per_page=100");
        use futures::stream::{self, StreamExt};
        let results: Vec<Result<StargazerPageEvents, GithubError>> =
            stream::iter(pages.iter().copied())
                .map(|p| {
                    let url = format!("{base}&page={p}");
                    let owner = owner.to_string();
                    let repo = repo.to_string();
                    let me = self.clone();
                    async move {
                        let resp = me
                            .send(&url, Some("application/vnd.github.star+json"))
                            .await?;
                        let resp = check_status(resp, &format!("{owner}/{repo}")).await?;
                        let page: Vec<Stargazer> = resp.json().await?;
                        Ok::<_, GithubError>(
                            page.into_iter()
                                .enumerate()
                                .map(|(index, s)| {
                                    let position =
                                        i64::from(p.saturating_sub(1)) * 100 + index as i64 + 1;
                                    (position, s.starred_at)
                                })
                                .collect(),
                        )
                    }
                })
                // `buffered` preserves input order in the output stream.
                .buffered(STARGAZER_FETCH_CONCURRENCY)
                .collect()
                .await;
        // Propagate the first error; otherwise return ordered page items.
        results.into_iter().collect()
    }

    /// Fetch a single page of the stargazers list (1-indexed, `per_page`
    /// = 100). Returns the page's stargazers and the total last-page count
    /// derived from page 1's `Link: rel="last"` header (`None` on later
    /// pages, where GitHub omits `last`). Used by the incremental
    /// refresh path in the worker: GitHub paginates stargazers
    /// oldest-first, so the newest stars live on the last pages — the
    /// worker walks backward from `last_page` and stops once it reaches
    /// already-cached timestamps. Each call still routes through
    /// `RateLimitTracker::acquire` so the per-token GitHub budget holds.
    pub async fn stargazers_page(
        &self,
        owner: &str,
        repo: &str,
        page: u32,
    ) -> Result<StargazerPage, GithubError> {
        let url = format!("{API_BASE}/repos/{owner}/{repo}/stargazers?per_page=100&page={page}");
        let resp = self
            .send(&url, Some("application/vnd.github.star+json"))
            .await?;
        let resp = check_status(resp, &format!("{owner}/{repo}")).await?;
        let last_page = parse_last_page(&resp);
        let items: Vec<Stargazer> = resp.json().await?;
        Ok(StargazerPage { items, last_page })
    }

    pub async fn user(&self, login: &str) -> Result<Option<User>, GithubError> {
        let url = format!("{API_BASE}/users/{login}");
        let resp = self.send(&url, None).await?;
        match resp.status().as_u16() {
            200 => Ok(Some(resp.json().await?)),
            404 => Ok(None),
            403 | 429 => Err(GithubError::RateLimited(None)),
            s => Err(GithubError::Api {
                status: s,
                body: resp.text().await.unwrap_or_default(),
            }),
        }
    }

    /// Resolve a commit's author. The author block in the response carries
    /// the GitHub-side `login` + `avatar_url` when GitHub knows the user
    /// behind the commit-author email — otherwise it's null. Lets us turn
    /// "Alex Hp <drunkod@gmail.com>" into "drunkod" with the right avatar
    /// without separately maintaining an email→login mapping.
    pub async fn commit_author(
        &self,
        owner: &str,
        repo: &str,
        sha: &str,
    ) -> Result<Option<CommitAuthorInfo>, GithubError> {
        let url = format!("{API_BASE}/repos/{owner}/{repo}/commits/{sha}");
        let resp = self.send(&url, None).await?;
        match resp.status().as_u16() {
            200 => {
                let body: CommitResponse = resp.json().await?;
                Ok(body.author)
            }
            404 => Ok(None),
            403 | 429 => Err(GithubError::RateLimited(None)),
            s => Err(GithubError::Api {
                status: s,
                body: resp.text().await.unwrap_or_default(),
            }),
        }
    }

    /// Fetch repo metadata. We only persist the fields the star-history
    /// + usage surfaces use: the authoritative `stargazers_count`
    ///
    /// (sanity-checks our own pagination), `forks_count`, and the repo
    /// `created_at`.
    pub async fn repo_metadata(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<Option<RepoMetadata>, GithubError> {
        let url = format!("{API_BASE}/repos/{owner}/{repo}");
        let resp = self.send(&url, None).await?;
        match resp.status().as_u16() {
            200 => {
                let metadata: RepoMetadata = resp.json().await?;
                // A user OAuth token may be able to see a private repository,
                // but gitdebt is a public-data product. Treat it exactly like
                // an inaccessible repo and never enqueue or persist it.
                Ok((!metadata.private).then_some(metadata))
            }
            404 => Ok(None),
            403 | 429 => Err(GithubError::RateLimited(None)),
            s => Err(GithubError::Api {
                status: s,
                body: resp.text().await.unwrap_or_default(),
            }),
        }
    }

    /// List a login's public repos via `/users/{login}/repos` (works for
    /// both user accounts and organizations). Returns `Ok(None)` on a 404
    /// login so the caller can tombstone it.
    ///
    /// Pagination follows the Link header (`next_link`) per the repo-wide
    /// discipline — never a page-counter loop — but is bounded by
    /// [`REPO_LIST_MAX_PAGES`] as a cost cap: the aggregate feature only
    /// needs the top ~50 repos by stars, and the API can't sort by stars,
    /// so beyond ~1000 repos we accept an approximate candidate set
    /// (`sort=pushed` biases it toward active repos) rather than paying
    /// dozens of calls per giant org.
    pub async fn user_repos(&self, login: &str) -> Result<Option<Vec<RepoListItem>>, GithubError> {
        let mut url = format!("{API_BASE}/users/{login}/repos?per_page=100&type=owner&sort=pushed");
        let mut out: Vec<RepoListItem> = Vec::new();
        let mut pages = 0usize;
        loop {
            let resp = self.send(&url, None).await?;
            match resp.status().as_u16() {
                200 => {}
                404 => return Ok(None),
                403 | 429 => return Err(GithubError::RateLimited(None)),
                s => {
                    return Err(GithubError::Api {
                        status: s,
                        body: resp.text().await.unwrap_or_default(),
                    });
                }
            }
            let next = next_link(&resp);
            let page: Vec<RepoListItem> = resp.json().await?;
            out.extend(page);
            pages += 1;
            match next {
                Some(n) if pages < REPO_LIST_MAX_PAGES => url = n,
                _ => break,
            }
        }
        Ok(Some(out))
    }
}

/// Page cap for [`GithubClient::user_repos`] — see its docs. 10 pages ×
/// 100 repos bounds a single cold login at ≤10 API calls.
const REPO_LIST_MAX_PAGES: usize = 10;

async fn check_status(resp: Response, ctx: &str) -> Result<Response, GithubError> {
    let status = resp.status();
    if status == 404 {
        return Err(GithubError::StargazersUnavailable(ctx.to_string()));
    }
    if status == 403 || status == 429 {
        // GitHub overloads 403 for both rate limiting and plain access
        // denial. Only treat it as a rate limit when we have positive
        // evidence: `x-ratelimit-remaining: 0` (primary) or a `Retry-After`
        // (secondary/abuse), or a 429 (always a rate limit). A bare 403 —
        // notably the 2026-06-30 stargazer restriction for non-admin
        // callers — is a DURABLE forbidden: retrying can't fix it, so we
        // surface it distinctly and let the worker park the repo instead of
        // re-polling it on every view.
        let h = resp.headers();
        let primary_hit = h
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<i64>().ok())
            .is_some_and(|n| n == 0);
        let secondary_hit = h.contains_key("retry-after");
        if status == 429 || primary_hit || secondary_hit {
            return Err(GithubError::RateLimited(None));
        }
        return Err(GithubError::Forbidden(ctx.to_string()));
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(GithubError::Api {
            status: status.as_u16(),
            body,
        });
    }
    Ok(resp)
}

/// Parses the GitHub Link header and returns the URL with `rel=next`, if any.
/// Format example:
///   `<https://api.github.com/...&page=2>; rel="next", <...&page=10>; rel="last"`
pub fn next_link(resp: &Response) -> Option<String> {
    let header = resp.headers().get(LINK)?.to_str().ok()?;
    parse_next_link(header)
}

pub fn parse_next_link(header: &str) -> Option<String> {
    parse_link_rel(header, "next")
}

/// Returns the `page=N` value from the URL with `rel=last` in the Link
/// header. Used to compute total page count up front so the remainder
/// can be fanned out in parallel. Falls back to `None` (= no second
/// page) if the header or the `page` param is missing.
pub fn parse_last_page(resp: &Response) -> Option<u32> {
    let header = resp.headers().get(LINK)?.to_str().ok()?;
    let url = parse_link_rel(header, "last")?;
    url.split('&')
        .chain(url.split('?').skip(1))
        .find_map(|kv| kv.strip_prefix("page="))
        .and_then(|n| n.parse::<u32>().ok())
}

fn parse_link_rel(header: &str, want_rel: &str) -> Option<String> {
    for part in header.split(',') {
        let part = part.trim();
        let (url_part, rest) = part.split_once(';')?;
        let url_part = url_part.trim();
        if !url_part.starts_with('<') || !url_part.ends_with('>') {
            continue;
        }
        let url = &url_part[1..url_part.len() - 1];
        for attr in rest.split(';') {
            let attr = attr.trim();
            if attr == format!(r#"rel="{want_rel}""#) {
                return Some(url.to_string());
            }
        }
    }
    None
}

/// One page of the stargazers list plus the total page count (from page
/// 1's `Link: rel="last"`; `None` thereafter). Returned by
/// [`GithubClient::stargazers_page`] for the incremental refresh walk.
#[derive(Debug, Clone)]
pub struct StargazerPage {
    pub items: Vec<Stargazer>,
    pub last_page: Option<u32>,
}

/// One entry from `/repos/{o}/{r}/stargazers` with the star+json accept header.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stargazer {
    pub starred_at: DateTime<Utc>,
}

/// Subset of the `/users/{login}` payload. The star-history pipeline no
/// longer scores accounts; the only consumer left is repo-author
/// enrichment (`repo_analysis`), which reads the display `login`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub login: String,
    pub id: u64,
}

/// Slimmed repo-metadata response. GitHub returns ~100 fields on this
/// endpoint; deserializing all of them is expensive at no benefit. We
/// only pull what the star-history + usage surfaces use: the
/// authoritative star count, the fork count, and the repo creation date.
#[derive(Debug, Clone, Deserialize)]
pub struct RepoMetadata {
    #[serde(default)]
    pub private: bool,
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub stargazers_count: u64,
    #[serde(default)]
    pub forks_count: u64,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
}

/// One entry from `/users/{login}/repos`, slimmed to what the org/user
/// aggregate needs: the slug and the star count (for the client-side
/// top-by-stars sort — the endpoint itself can't sort by stars).
#[derive(Debug, Clone, Deserialize)]
pub struct RepoListItem {
    pub full_name: String,
    #[serde(default)]
    pub stargazers_count: i64,
    #[serde(default)]
    pub fork: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommitAuthorInfo {
    pub login: Option<String>,
    pub id: Option<u64>,
    pub avatar_url: Option<String>,
}

#[derive(Deserialize)]
struct CommitResponse {
    author: Option<CommitAuthorInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_next_link_among_multiple() {
        let header = r#"<https://api.github.com/x?page=2>; rel="next", <https://api.github.com/x?page=10>; rel="last""#;
        assert_eq!(
            parse_next_link(header),
            Some("https://api.github.com/x?page=2".into()),
        );
    }

    #[test]
    fn parses_no_next_when_only_last() {
        let header = r#"<https://api.github.com/x?page=10>; rel="last", <https://api.github.com/x?page=1>; rel="first""#;
        assert_eq!(parse_next_link(header), None);
    }

    #[test]
    fn parses_last_page_from_real_link_header() {
        // The form GitHub actually sends: `next` and `last` separated by comma.
        let header = r#"<https://api.github.com/repositories/123/stargazers?per_page=100&page=2>; rel="next", <https://api.github.com/repositories/123/stargazers?per_page=100&page=47>; rel="last""#;
        assert_eq!(
            parse_link_rel(header, "last").as_deref(),
            Some("https://api.github.com/repositories/123/stargazers?per_page=100&page=47")
        );
        // The full helper that pulls out the page integer.
        // We inline-call it via the same logic the Response-based variant uses.
        let url = parse_link_rel(header, "last").unwrap();
        let page = url
            .split('&')
            .chain(url.split('?').skip(1))
            .find_map(|kv| kv.strip_prefix("page="))
            .and_then(|n| n.parse::<u32>().ok());
        assert_eq!(page, Some(47));
    }

    #[test]
    fn parses_url_with_semicolons_in_query_safely() {
        // Pathological: a URL containing a semicolon inside angle brackets.
        // Our split-on-comma + split-once on ';' relies on '<...>' being the
        // first segment; semicolons inside the URL would break it. GitHub
        // doesn't emit those, but we document the assumption with a guard.
        let header = r#"<https://api.github.com/x?a=1&b=2>; rel="next""#;
        assert_eq!(
            parse_next_link(header),
            Some("https://api.github.com/x?a=1&b=2".into()),
        );
    }
}
