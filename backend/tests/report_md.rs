//! Integration tests for the Markdown surfaces — `GET /api/md/{*path}` and its
//! repository alias `GET /api/repos/{owner}/{repo}/report.md` — against a real
//! Postgres and the real router.
//!
//! The renderer's unit tests cover the bytes; these cover the contract an
//! agent actually sees — status, cache policy, `Retry-After`, the canonical
//! `Link`, and the rule that a permanently-parked analysis is never revived
//! by a poll.
//!
//! Gated on `GITDEBT_TEST_DATABASE_URL` so `cargo test` stays green in
//! environments without a database. To run them:
//!
//! ```bash
//! scripts/db.sh up
//! GITDEBT_TEST_DATABASE_URL=postgres://gitdebt:gitdebt@localhost:5432/gitdebt \
//!   cargo test --test report_md
//! ```
//!
//! Each test namespaces its repos under a unique owner and cleans up after
//! itself, so the suite is safe to run repeatedly against a shared dev
//! database.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use chrono::{TimeZone, Utc};
use gitdebt::{
    analyzer::AnalyzerCtx,
    api::{ApiState, router},
    cache::Cache,
    db::Db,
    github::RepoMetadata,
    repo_history::RepoStorage,
};
use tower::ServiceExt;

/// Deployment settings the assertions read back out of the served bodies and
/// headers: nothing here may be inferred from the request or hardcoded in the
/// handler.
const SITE_ORIGIN: &str = "https://frontend.test";
const API_ORIGIN: &str = "https://api.test";

/// A runtime-local pool on the test database, or `None` when
/// `GITDEBT_TEST_DATABASE_URL` is unset and the test no-ops.
async fn test_db() -> Option<Db> {
    gitdebt::test_db::shared().await
}

/// Delete every row a test's owner namespace holds.
///
/// Both patterns are terminated — repository rows match `{owner}/%`, login rows
/// match the owner exactly — because the default harness runs these tests
/// concurrently. An open `{owner}%` pattern would let `…-md-vs-ready`'s cleanup
/// delete the rows a hypothetical `…-md-vs` had just seeded, mid-request, and
/// the victim would then fail its completeness assertion intermittently. Owner
/// names are also kept distinct rather than merely prefix-free, so neither
/// guard alone carries the isolation.
async fn cleanup(db: &Db, owner: &str) {
    let repos = format!("{owner}/%");
    for statement in [
        "DELETE FROM star_fetch_queue WHERE repo LIKE $1",
        "DELETE FROM repo_analysis_queue WHERE repo LIKE $1",
        "DELETE FROM repo_stargazers WHERE repo LIKE $1",
        "DELETE FROM repo_history WHERE repo LIKE $1",
        "DELETE FROM repos WHERE repo LIKE $1",
    ] {
        let _ = sqlx::query(statement).bind(&repos).execute(&db.pool).await;
    }
    for statement in [
        "DELETE FROM login_repos WHERE login = $1",
        "DELETE FROM login_repo_lists WHERE login = $1",
    ] {
        let _ = sqlx::query(statement).bind(owner).execute(&db.pool).await;
    }
}

/// An `ApiState` built exactly as the deployment does, but with no
/// `PUBLIC_API_BASE` — the state a container gets when the variable is missing
/// from its service environment.
async fn api_state_without_api_origin(db: Db) -> ApiState {
    let mut state = api_state(db).await;
    state.api_origin = None;
    state
}

async fn api_state(db: Db) -> ApiState {
    let rate = std::sync::Arc::new(
        gitdebt::rate_limit::RateLimitTracker::load(db.clone())
            .await
            .expect("load rate tracker"),
    );
    let github =
        std::sync::Arc::new(gitdebt::github::GithubClient::new(None, rate).expect("github client"));
    ApiState::with_settings(
        AnalyzerCtx {
            github,
            cache: Cache::new(db),
        },
        None,
        std::sync::Arc::new(RepoStorage::from_env()),
        None,
        SITE_ORIGIN.to_string(),
        Some(API_ORIGIN.to_string()),
        None,
    )
    .expect("api state")
}

async fn get(state: ApiState, uri: &str) -> Response {
    router(state)
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response")
}

fn header_value(response: &Response, name: header::HeaderName) -> String {
    response
        .headers()
        .get(&name)
        .unwrap_or_else(|| panic!("{name} header"))
        .to_str()
        .expect("ascii header")
        .to_string()
}

/// The headers every report answer carries, whatever its status: the type the
/// agent asked for, no indexing, and a canonical link built from the
/// configured frontend origin rather than the request or a baked-in host.
fn assert_report_headers(response: &Response, canonical_path: &str) {
    assert_eq!(
        header_value(response, header::CONTENT_TYPE),
        "text/markdown; charset=utf-8"
    );
    assert_eq!(
        header_value(response, header::HeaderName::from_static("x-robots-tag")),
        "noindex, follow"
    );
    assert_eq!(
        header_value(response, header::LINK),
        format!("<{SITE_ORIGIN}{canonical_path}>; rel=\"canonical\"")
    );
}

async fn body_text(response: Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("read body");
    String::from_utf8(bytes.to_vec()).expect("utf-8 body")
}

/// Seed a public repository with fresh metadata and no star history.
async fn seed_public_repo(state: &ApiState, repo: &str, stars: u64) {
    state
        .analyzer
        .cache
        .put_repo_metadata(
            repo,
            &RepoMetadata {
                id: Some(1),
                stargazers_count: stars,
                ..RepoMetadata::default()
            },
        )
        .await
        .expect("seed metadata");
}

/// Seed a complete star history: `stars` arrivals a second apart, which is
/// what flips the `history_complete` gate the read surfaces check.
async fn seed_complete_history(state: &ApiState, repo: &str, stars: i64) {
    seed_public_repo(state, repo, stars as u64).await;
    let base = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
    let stargazers: Vec<(i64, _)> = (0..stars)
        .map(|index| (index + 1, base + chrono::Duration::seconds(index)))
        .collect();
    state
        .analyzer
        .cache
        .put_repo_stargazers(repo, &stargazers)
        .await
        .expect("seed complete history");
}

#[tokio::test]
async fn invalid_slug_is_rejected_without_echoing_it() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let state = api_state(db).await;

    let response = get(state, "/api/repos/owner/ev~il/report.md").await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    // The canonical link points at the live discovery route, never at a URL
    // built from the rejected input.
    assert_report_headers(&response, "/report");
    assert_eq!(header_value(&response, header::CACHE_CONTROL), "no-store");
    let link = header_value(&response, header::LINK);
    let body = body_text(response).await;
    for echoed in ["ev~il", "owner/ev~il"] {
        assert!(!body.contains(echoed), "rejected slug must not be echoed");
        assert!(!link.contains(echoed), "rejected slug must not be echoed");
    }
}

#[tokio::test]
async fn tombstoned_repository_answers_a_cacheable_404() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let owner = "gitdebt-test-report-missing";
    cleanup(&db, owner).await;
    let repo = format!("{owner}/gone");
    let state = api_state(db.clone()).await;
    state
        .analyzer
        .cache
        .mark_repo_missing(&repo)
        .await
        .expect("tombstone");

    let response = get(state, &format!("/api/repos/{repo}/report.md")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_report_headers(&response, &format!("/{repo}"));
    assert_eq!(
        header_value(&response, header::CACHE_CONTROL),
        "public, s-maxage=86400"
    );
    let body = body_text(response).await;
    assert!(body.contains("not a public GitHub repository"));

    cleanup(&db, owner).await;
}

#[tokio::test]
async fn incomplete_history_answers_202_with_a_retry_and_no_star_figures() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let owner = "gitdebt-test-report-queued";
    cleanup(&db, owner).await;
    let repo = format!("{owner}/cold");
    let state = api_state(db.clone()).await;
    // A star count GitHub already reported, with no history behind it: the
    // 202 must print neither that figure nor a zero.
    seed_public_repo(&state, &repo, 4_210).await;

    // `enqueue=0` keeps the assertions about the response, not about what the
    // queues did with it.
    let response = get(state, &format!("/api/repos/{repo}/report.md?enqueue=0")).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_report_headers(&response, &format!("/{repo}"));
    assert_eq!(header_value(&response, header::RETRY_AFTER), "30");
    // Never `no-store`: a poll loop following `Retry-After` must not reach the
    // origin, and so the enqueue paths, more than once per window per edge.
    assert_eq!(
        header_value(&response, header::CACHE_CONTROL),
        "public, s-maxage=30"
    );
    let body = body_text(response).await;
    assert!(!body.contains("## Star snapshot"));
    assert!(!body.contains("4,210"));
    assert!(body.contains(&format!("Poll {API_ORIGIN}/api/repos/{repo}/progress.json")));

    cleanup(&db, owner).await;
}

/// Star history is the core product: a complete series is served the moment
/// Postgres holds it, whether or not the repository-health analysis has run.
#[tokio::test]
async fn complete_star_history_answers_200_without_repository_health() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let owner = "gitdebt-test-report-ready";
    cleanup(&db, owner).await;
    let repo = format!("{owner}/charted");
    let state = api_state(db.clone()).await;
    seed_public_repo(&state, &repo, 5).await;
    let base = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
    let stargazers: Vec<(i64, _)> = (0..5)
        .map(|index| (index + 1, base + chrono::Duration::seconds(index)))
        .collect();
    state
        .analyzer
        .cache
        .put_repo_stargazers(&repo, &stargazers)
        .await
        .expect("seed complete history");

    let response = get(state, &format!("/api/repos/{repo}/report.md?enqueue=0")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_report_headers(&response, &format!("/{repo}"));
    assert_eq!(
        header_value(&response, header::CACHE_CONTROL),
        "public, s-maxage=300, max-age=60"
    );
    let body = body_text(response).await;
    assert!(body.contains("## Star snapshot"));
    assert!(body.contains("| GitHub stars | 5 |"));
    // No health figures, and an explicit line saying so plus where to watch.
    assert!(!body.contains("| Reading | Figures |"));
    assert!(body.contains("The repository-health analysis has not finished"));
    assert!(body.contains(&format!("{API_ORIGIN}/api/repos/{repo}/progress.json")));
    // Asset URLs come from the configured API origin, not from the request.
    assert!(body.contains(&format!("{API_ORIGIN}/api/repos/{repo}/chart.svg")));

    cleanup(&db, owner).await;
}

/// A job parked by the analysis attempt ceiling stays parked. `enqueue`'s
/// `ON CONFLICT` resets `attempts` and clears `last_error` on a `dead` row,
/// so an agent obeying `Retry-After` would otherwise re-arm a permanently
/// failing clone every 30 seconds, each time taking a capacity slot.
#[tokio::test]
async fn a_poll_never_resurrects_a_terminally_parked_analysis() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let owner = "gitdebt-test-report-parked";
    cleanup(&db, owner).await;
    let repo = format!("{owner}/unclonable");
    let state = api_state(db.clone()).await;
    seed_public_repo(&state, &repo, 9).await;
    let last_error = format!("{} clone failed", gitdebt::repo_analysis::TERMINAL_MARKER);
    sqlx::query(
        "INSERT INTO repo_analysis_queue \
            (repo, status, phase, attempts, last_error, enqueued_at, updated_at) \
         VALUES ($1, 'dead', 'retrying', 8, $2, NOW(), NOW())",
    )
    .bind(&repo)
    .bind(&last_error)
    .execute(&db.pool)
    .await
    .expect("park the analysis job");

    let response = get(state, &format!("/api/repos/{repo}/report.md")).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body = body_text(response).await;
    assert!(body.contains("unavailable — repeated attempts failed"));

    let (status, attempts, error): (String, i32, Option<String>) = sqlx::query_as(
        "SELECT status, attempts, last_error FROM repo_analysis_queue WHERE repo = $1",
    )
    .bind(&repo)
    .fetch_one(&db.pool)
    .await
    .expect("read the parked row");
    assert_eq!(status, "dead", "a poll must not revive a parked analysis");
    assert_eq!(attempts, 8);
    assert_eq!(error.as_deref(), Some(last_error.as_str()));

    cleanup(&db, owner).await;
}

// `GET /api/md/{*path}` — the universal surface

/// The shared-cache lifetime of a page with no live data behind it. These
/// bodies are compiled-in text plus the configured origins, so they change
/// only on redeploy.
const COMPILED_CACHE_CONTROL: &str = "public, s-maxage=86400, max-age=3600";

/// Pages that read nothing: the home document, the static pages, the embed
/// catalog, and a curated category. All 200, all cacheable for a day, each
/// canonical to the site page it represents.
#[tokio::test]
async fn compiled_pages_answer_200_at_the_site_path_that_backs_them() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let state = api_state(db).await;

    for (uri, canonical) in [
        ("/api/md", "/"),
        ("/api/md/", "/"),
        ("/api/md/about", "/about"),
        ("/api/md/badges", "/badges"),
        (
            "/api/md/compare/frontend-frameworks",
            "/compare/frontend-frameworks",
        ),
    ] {
        let response = get(state.clone(), uri).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        assert_report_headers(&response, canonical);
        assert_eq!(
            header_value(&response, header::CACHE_CONTROL),
            COMPILED_CACHE_CONTROL,
            "{uri}"
        );
        let body = body_text(response).await;
        assert!(body.starts_with("# "), "{uri} is not a Markdown document");
        // Asset and data-surface URLs come from the configured API origin, not
        // from the request or a baked-in host.
        assert!(!body.contains("gitdebt.com"), "{uri} hardcodes an origin");
    }

    // `/badges` is a static page AND the embed catalog; the Markdown
    // representation is the catalog, which the landing page's prose is not.
    let body = body_text(get(state, "/api/md/badges").await).await;
    assert!(body.contains(&format!("{API_ORIGIN}/api/repos/")));
}

/// A category slug nobody authored is a settled 404, not a guess.
#[tokio::test]
async fn an_unknown_category_answers_a_cacheable_markdown_404() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let state = api_state(db).await;

    let response = get(state, "/api/md/compare/no-such-category").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_report_headers(&response, "/");
    assert_eq!(
        header_value(&response, header::CACHE_CONTROL),
        "public, s-maxage=86400"
    );
    let body = body_text(response).await;
    assert!(body.starts_with("# No Markdown at this path"));
    // The 404 is still Markdown, and still tells an agent the route shapes.
    assert!(body.contains(&format!("{API_ORIGIN}/api/md/compare/")));
}

/// The segments the site owns can never resolve as a login or as a repository
/// owner. `frontend/src/lib/static-routing.mjs` decides this for the site; the
/// unit test beside the list keeps the two spellings identical.
#[tokio::test]
async fn a_reserved_segment_is_never_served_as_a_profile() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let state = api_state(db).await;

    // `/u/{login}` is a redirect on the site, so neither `u` nor `u/{login}`
    // is a page here.
    for uri in ["/api/md/u", "/api/md/u/octocat", "/api/md/api"] {
        let response = get(state.clone(), uri).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        let body = body_text(response).await;
        assert!(body.starts_with("# No Markdown at this path"), "{uri}");
    }

    // A reserved segment that IS a page still answers as that page.
    let response = get(state, "/api/md/leaderboard").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_report_headers(&response, "/leaderboard");
}

#[tokio::test]
async fn an_invalid_path_is_rejected_without_echoing_it() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let state = api_state(db).await;

    for uri in [
        "/api/md/ev~il",
        "/api/md/owner/ev~il",
        "/api/md/compare/ev~il",
    ] {
        let response = get(state.clone(), uri).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{uri}");
        // The canonical link points at the live discovery route, never at a URL
        // built from the rejected input.
        assert_report_headers(&response, "/report");
        assert_eq!(header_value(&response, header::CACHE_CONTROL), "no-store");
        let link = header_value(&response, header::LINK);
        let body = body_text(response).await;
        assert!(!body.contains("ev~il"), "{uri} echoed its input");
        assert!(!link.contains("ev~il"), "{uri} echoed its input");
    }
}

/// The two URLs for a repository report are one document. They share a
/// renderer, a memo and a canonical link, so the bytes must be identical.
#[tokio::test]
async fn the_repository_alias_and_the_universal_route_are_the_same_document() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let owner = "gitdebt-test-md-alias";
    cleanup(&db, owner).await;
    let repo = format!("{owner}/charted");
    let state = api_state(db.clone()).await;
    seed_complete_history(&state, &repo, 5).await;

    let alias = get(
        state.clone(),
        &format!("/api/repos/{repo}/report.md?enqueue=0"),
    )
    .await;
    let universal = get(state, &format!("/api/md/{repo}?enqueue=0")).await;

    assert_eq!(alias.status(), StatusCode::OK);
    assert_eq!(universal.status(), alias.status());
    for name in [
        header::CACHE_CONTROL,
        header::CONTENT_TYPE,
        header::LINK,
        header::HeaderName::from_static("x-robots-tag"),
    ] {
        assert_eq!(
            header_value(&universal, name.clone()),
            header_value(&alias, name.clone()),
            "{name} differs between the two URLs"
        );
    }
    assert_eq!(body_text(universal).await, body_text(alias).await);

    cleanup(&db, owner).await;
}

/// A profile answers from the account's cached repository list. Nothing
/// measured means no figures at all and a 202, not a zero.
#[tokio::test]
async fn a_profile_reports_only_repositories_with_a_complete_history() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let owner = "gitdebt-test-md-profile";
    cleanup(&db, owner).await;
    let state = api_state(db.clone()).await;
    let charted = format!("{owner}/charted");
    let cold = format!("{owner}/cold");
    seed_complete_history(&state, &charted, 5).await;
    // Metadata GitHub already reported, with no history behind it: its stars
    // must not reach the sum, and its absence must not read as a zero.
    seed_public_repo(&state, &cold, 4_210).await;
    state
        .analyzer
        .cache
        .put_login_repos(
            owner,
            &[(cold.clone(), 4_210), (charted.clone(), 5)],
            gitdebt::cache::LoginListFacts {
                kind: gitdebt::github::AccountKind::User,
                public_repos: Some(2),
                truncated: false,
            },
        )
        .await
        .expect("seed the account's repository list");

    let response = get(state, &format!("/api/md/{owner}?enqueue=0")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_report_headers(&response, &format!("/{owner}"));
    assert_eq!(
        header_value(&response, header::CACHE_CONTROL),
        "public, s-maxage=300, max-age=60"
    );
    let body = body_text(response).await;
    assert!(body.starts_with(&format!("# {owner} —")));
    // Only the complete history is summed, and the incomplete repository's
    // GitHub-reported total is nowhere in the document.
    assert!(!body.contains("4,210"));
    assert!(body.contains(&format!("{API_ORIGIN}/api/users/{owner}/")));

    cleanup(&db, owner).await;
}

/// A large account answers 200 the moment anything is measured, so the body has
/// to say what the sum covers. Three measured repositories out of four hundred
/// is a floor, and printing the floor without the denominator — or without the
/// count still draining — is the one reading of this page that is actually
/// wrong.
#[tokio::test]
async fn a_partially_measured_profile_states_its_coverage() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let owner = "gitdebt-test-md-coverage";
    cleanup(&db, owner).await;
    let state = api_state(db.clone()).await;

    // Distinctive figures with no shared digits: every assertion below is a
    // substring search over the whole document.
    let mut listed: Vec<(String, i64)> = Vec::new();
    for (name, stars) in [("charted-a", 60_i64), ("charted-b", 80), ("charted-c", 101)] {
        let repo = format!("{owner}/{name}");
        seed_complete_history(&state, &repo, stars).await;
        listed.push((repo, stars));
    }
    // Cold repositories need no metadata at all: an account's list entry with
    // no complete history behind it is exactly what `repos_pending` counts.
    for name in [
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m",
    ] {
        listed.push((format!("{owner}/cold-{name}"), 0));
    }
    state
        .analyzer
        .cache
        .put_login_repos(
            owner,
            &listed,
            gitdebt::cache::LoginListFacts {
                kind: gitdebt::github::AccountKind::Organization,
                public_repos: Some(407),
                truncated: false,
            },
        )
        .await
        .expect("seed the account's repository list");

    let response = get(state, &format!("/api/md/{owner}?enqueue=0")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        header_value(&response, header::CACHE_CONTROL),
        "public, s-maxage=300, max-age=60"
    );
    let body = body_text(response).await;
    assert!(body.contains("241"), "the measured sum is missing");
    assert!(
        body.contains("407"),
        "the public-repository denominator is missing, so the sum reads as a \
         settled total"
    );
    assert!(
        body.contains("13"),
        "the still-measuring count is missing, so a reader cannot tell the \
         total will keep growing"
    );

    cleanup(&db, owner).await;
}

/// A tombstoned login is a settled 404 with a Markdown body.
#[tokio::test]
async fn a_missing_login_answers_a_cacheable_markdown_404() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let owner = "gitdebt-test-md-gone";
    cleanup(&db, owner).await;
    let state = api_state(db.clone()).await;
    state
        .analyzer
        .cache
        .mark_login_missing(owner)
        .await
        .expect("tombstone the login");

    let response = get(state, &format!("/api/md/{owner}?enqueue=0")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_report_headers(&response, &format!("/{owner}"));
    assert_eq!(
        header_value(&response, header::CACHE_CONTROL),
        "public, s-maxage=86400"
    );
    let body = body_text(response).await;
    assert!(body.contains("not a public GitHub account"));

    cleanup(&db, owner).await;
}

/// A comparison carries each leg's readiness separately: a curated pair
/// routinely has one analyzed repository and one nobody has opened. The
/// unmeasured leg prints nothing — an empty history is not a zero.
#[tokio::test]
async fn a_comparison_withholds_every_figure_for_an_incomplete_leg() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let owner = "gitdebt-test-md-vs-mixed";
    cleanup(&db, owner).await;
    let state = api_state(db.clone()).await;
    let charted = format!("{owner}/charted");
    let cold = format!("{owner}/cold");
    seed_complete_history(&state, &charted, 5).await;
    seed_public_repo(&state, &cold, 4_210).await;

    let response = get(state, &format!("/api/md/vs/{charted}/{cold}?enqueue=0")).await;
    // One leg still running: 202 with the same poll contract as a report.
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_report_headers(&response, &format!("/vs/{charted}/{cold}"));
    assert_eq!(header_value(&response, header::RETRY_AFTER), "30");
    assert_eq!(
        header_value(&response, header::CACHE_CONTROL),
        "public, s-maxage=30"
    );
    let body = body_text(response).await;
    assert!(body.contains(&charted));
    assert!(body.contains(&cold));
    assert!(
        !body.contains("4,210"),
        "an incomplete leg printed a figure"
    );

    cleanup(&db, owner).await;
}

/// Both legs complete is a settled comparison.
#[tokio::test]
async fn a_comparison_of_two_complete_histories_answers_200() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let owner = "gitdebt-test-md-vs-ready";
    cleanup(&db, owner).await;
    let state = api_state(db.clone()).await;
    let first = format!("{owner}/first");
    let second = format!("{owner}/second");
    seed_complete_history(&state, &first, 5).await;
    seed_complete_history(&state, &second, 7).await;

    let response = get(state, &format!("/api/md/vs/{first}/{second}?enqueue=0")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_report_headers(&response, &format!("/vs/{first}/{second}"));
    assert_eq!(
        header_value(&response, header::CACHE_CONTROL),
        "public, s-maxage=300, max-age=60"
    );
    let body = body_text(response).await;
    assert!(body.contains(&first));
    assert!(body.contains(&second));

    cleanup(&db, owner).await;
}

/// A missing `PUBLIC_API_BASE` used to abort startup, which took an entire
/// deployment offline over a variable only these routes read. It must now cost
/// exactly the Markdown surfaces and nothing else: every other endpoint keeps
/// answering, and the ones that cannot are honest about why rather than
/// inventing a plausible-looking wrong host.
#[tokio::test]
async fn markdown_routes_degrade_alone_when_the_api_origin_is_unconfigured() {
    let Some(db) = test_db().await else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };
    let state = api_state_without_api_origin(db).await;

    for path in [
        "/api/md/",
        "/api/md/about",
        "/api/md/badges",
        "/api/md/facebook/react",
        "/api/repos/facebook/react/report.md",
    ] {
        let response = get(state.clone(), path).await;
        assert_eq!(
            response.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "{path} should degrade, not guess an origin"
        );
        let body = body_text(response).await;
        assert!(
            !body.contains("http"),
            "{path} must not name a host it had to invent"
        );
    }

    // The rest of the API is untouched: this is one feature degrading, not an
    // outage.
    let healthy = get(state, "/health").await;
    assert_eq!(healthy.status(), StatusCode::OK);
}
