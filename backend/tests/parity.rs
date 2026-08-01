//! The Rust half of the cross-language goldens.
//!
//! Two documents are checked in under `tests/fixtures/`:
//!
//! - `embed-parity.md` — every embeddable asset, its catalog metadata, and the
//!   snippet it produces.
//! - `prompt-parity.md` — the full "Ask an agent" prompt in every state that
//!   changes it.
//!
//! This file renders both from `agent_embeds` and `agent_prompt` and asserts
//! byte equality; `frontend/scripts/embed-parity.test.mjs` asserts the same of
//! `src/lib/readme-embeds.ts` and `src/lib/agent-prompt.ts`. The API serves the
//! Markdown an agent reads while those TypeScript modules still render the
//! `/badges` page and the "Ask an agent" clipboard button in the browser, so
//! without a shared golden the two could quietly hand out different snippets —
//! and a differently worded prompt — for the same repository.
//!
//! Lives out here rather than in the renderer modules because the goldens
//! belong to the pair of implementations, not to either one of them.
//!
//! Everything below is pure: no database, no network, no wall clock. It runs
//! with a plain `cargo test --test parity`.

use chrono::{Duration, NaiveDate};
use gitdebt::agent_embeds::{
    EMBED_RULES, EmbedAsset, EmbedGroup, asset_url, best_embed, best_embed_language,
    profile_embed_assets, readme_link, repo_embed_assets,
};
use gitdebt::agent_prompt::{
    PLACEHOLDER_SLUG, StarSummary, profile_agent_prompt, repo_agent_prompt,
};

const SITE: &str = "https://gitdebt.com";
const API: &str = "https://api.gitdebt.com";
const SLUG: &str = "OWNER/REPO";
const LOGIN: &str = "LOGIN";
/// `OWNER/REPO` is the placeholder slug, so it always selects the "resolve the
/// repository" opening. Reaching the other branch needs a slug that is not the
/// placeholder; everything else about the two renders is identical.
const RESOLVED_SLUG: &str = "owner/repo";

const EMBED_FIXTURE: &str = include_str!("fixtures/embed-parity.md");
const PROMPT_FIXTURE: &str = include_str!("fixtures/prompt-parity.md");

const EMBED_HEADER: &str = r###"<!--
embed-parity.md — the cross-language golden for gitdebt's README embed catalog.

What this is: every asset gitdebt can put in somebody else's README — for a
repository and for a profile — with the catalog metadata the /badges page and
the API's Markdown both read off it, and the exact snippet each one produces.

Two implementations render it: backend/src/agent_embeds.rs, asserted by
backend/tests/parity.rs, and frontend/src/lib/readme-embeds.ts, asserted by
frontend/scripts/embed-parity.test.mjs. Both compare byte for byte.

A diff here means the two implementations have drifted. Fix the drift: work out
which side is wrong and change that code. Do not regenerate this file to make a
test pass — that only lets the API and the /badges page disagree quietly.

Fixed inputs: slug OWNER/REPO, login LOGIN, site https://gitdebt.com,
api https://api.gitdebt.com.

Format, deliberately trivial to reproduce in any language:
  "# Repository assets — OWNER/REPO", then one section per asset in catalog
  order, then "# Profile assets — LOGIN" and its sections, then "# Rules" with
  EMBED_RULES as "- " bullets, one line each. An asset section is "## " + the
  asset id, a blank line, one "- key: value" line per catalog field, one
  "- url(FORMAT): " line per advertised format, a blank line, then the
  published snippet fenced in its own language. Sections are separated by one
  blank line and the file ends with a single newline.
-->"###;

const PROMPT_HEADER: &str = r###"<!--
prompt-parity.md — the cross-language golden for the "Ask an agent" prompt.

What this is: the complete prompt gitdebt hands a coding agent, rendered in
every state that changes it — a repository with nothing measured, one with a
complete star history, one whose curve is GH Archive star activity (with and
without a resolved total), and a profile with and without measured totals.

Two implementations render it: backend/src/agent_prompt.rs, asserted by
backend/tests/parity.rs, and frontend/src/lib/agent-prompt.ts, asserted by
frontend/scripts/embed-parity.test.mjs. Both compare byte for byte. The
frontend copy still backs the clipboard button while the Rust copy is what the
API serves, so a sentence reworded on one side and not the other would ship two
different prompts for the same repository.

A diff here means the two implementations have drifted. Fix the drift: work out
which side is wrong and change that code. Do not regenerate this file to make a
test pass.

Fixed inputs, no wall clock anywhere: site https://gitdebt.com,
api https://api.gitdebt.com, login LOGIN, and a synthetic star history of 600
daily points of +3 stars from 2013-03-09 followed by 90 daily points of +30.
OWNER/REPO is the placeholder slug, which is what selects the "resolve the
repository" opening; owner/repo is a resolved slug, the only way to reach the
other branch.

Format: one "===== BEGIN <label> =====" / "===== END <label> =====" pair per
rendered prompt with the prompt verbatim in between, blocks separated by one
blank line, single trailing newline.
-->"###;

/// The string form of an asset's group, matching the TypeScript union.
fn group_label(group: EmbedGroup) -> &'static str {
    match group {
        EmbedGroup::Headline => "headline",
        EmbedGroup::Health => "health",
        EmbedGroup::Social => "social",
    }
}

/// A star series with a lifetime pace and a much faster trailing quarter, so
/// the derived summary exercises both windows, the "accelerating" verdict, and
/// the first-star month label rather than leaving them null.
fn fixed_history() -> Vec<(NaiveDate, i64)> {
    let start = NaiveDate::from_ymd_opt(2013, 3, 9).expect("valid date");
    let mut history = Vec::new();
    for index in 0..600i64 {
        history.push((start + Duration::days(index), (index + 1) * 3));
    }
    for index in 1..=90i64 {
        history.push((start + Duration::days(599 + index), 1_800 + index * 30));
    }
    history
}

/// One asset: every catalog field, every advertised URL, and its snippet.
fn asset_section(api: &str, asset: &EmbedAsset, link: &str) -> String {
    let mut lines = vec![
        format!("## {}", asset.id),
        String::new(),
        format!("- name: {}", asset.name),
        format!("- purpose: {}", asset.purpose),
        format!("- placement: {}", asset.placement),
        format!("- group: {}", group_label(asset.group)),
        format!("- themed: {}", asset.themed),
        format!("- formats: {}", asset.formats.join(", ")),
    ];
    for &encoding in asset.formats {
        lines.push(format!(
            "- url({encoding}): {}",
            asset_url(api, asset, None, Some(encoding))
        ));
    }
    lines.extend([
        String::new(),
        format!("```{}", best_embed_language(asset)),
        best_embed(api, asset, link),
        "```".to_string(),
    ]);
    lines.join("\n")
}

/// The mirror of `embedParityDocument` in `embed-parity.test.mjs`.
fn embed_parity_document(slug: &str, login: &str, site: &str, api: &str) -> String {
    let mut sections = vec![
        EMBED_HEADER.to_string(),
        format!("# Repository assets — {slug}"),
    ];

    let repo_link = readme_link(site, &format!("/{slug}"));
    for asset in repo_embed_assets(slug) {
        sections.push(asset_section(api, &asset, &repo_link));
    }

    sections.push(format!("# Profile assets — {login}"));
    let profile_link = readme_link(site, &format!("/{login}"));
    for asset in profile_embed_assets(login) {
        sections.push(asset_section(api, &asset, &profile_link));
    }

    sections.push(format!(
        "# Rules\n\n{}",
        EMBED_RULES
            .iter()
            .map(|rule| format!("- {rule}"))
            .collect::<Vec<_>>()
            .join("\n")
    ));
    format!("{}\n", sections.join("\n\n"))
}

/// One rendered prompt, delimited so the prompt's own headings stay readable.
fn prompt_section(label: &str, body: &str) -> String {
    format!("===== BEGIN {label} =====\n{body}===== END {label} =====")
}

/// The mirror of `promptParityDocument` in `embed-parity.test.mjs`.
fn prompt_parity_document(site: &str, api: &str) -> String {
    let history = fixed_history();
    let complete = StarSummary::from_history(&history, Some(4_500), false);
    let approximate = StarSummary::from_history(&history, Some(4_500), true);
    let approximate_without_total = StarSummary::from_history(&history, None, true);

    let sections = [
        PROMPT_HEADER.to_string(),
        prompt_section(
            &format!("repo {PLACEHOLDER_SLUG} — nothing measured"),
            &repo_agent_prompt(PLACEHOLDER_SLUG, site, api, None),
        ),
        prompt_section(
            &format!("repo {RESOLVED_SLUG} — complete star history"),
            &repo_agent_prompt(RESOLVED_SLUG, site, api, Some(&complete)),
        ),
        prompt_section(
            &format!("repo {RESOLVED_SLUG} — approximate star history"),
            &repo_agent_prompt(RESOLVED_SLUG, site, api, Some(&approximate)),
        ),
        prompt_section(
            &format!("repo {RESOLVED_SLUG} — approximate star history, total not resolved"),
            &repo_agent_prompt(RESOLVED_SLUG, site, api, Some(&approximate_without_total)),
        ),
        prompt_section(
            &format!("profile {LOGIN} — measured"),
            &profile_agent_prompt(LOGIN, site, api, Some(90_120), Some(42)),
        ),
        prompt_section(
            &format!("profile {LOGIN} — nothing measured"),
            &profile_agent_prompt(LOGIN, site, api, None, None),
        ),
    ];
    format!("{}\n", sections.join("\n\n"))
}

/// A 20 KB `assert_eq!` is unreadable in a terminal, and the useful next step
/// is always a diff, so the rendered bytes land on disk and the panic says
/// where.
fn assert_matches_golden(name: &str, actual: &str, expected: &str) {
    if actual == expected {
        return;
    }
    let dump = std::env::temp_dir().join(name);
    std::fs::write(&dump, actual).expect("write the rendered golden for diffing");
    panic!(
        "{name} drifted from the checked-in golden.\n  \
         diff {} backend/tests/fixtures/{name}\n\
         Then fix whichever implementation is wrong. Do not regenerate the \
         fixture to make this pass.",
        dump.display()
    );
}

#[test]
fn agent_embeds_reproduces_the_embed_golden_byte_for_byte() {
    assert_matches_golden(
        "embed-parity.md",
        &embed_parity_document(SLUG, LOGIN, SITE, API),
        EMBED_FIXTURE,
    );
}

#[test]
fn agent_prompt_reproduces_the_prompt_golden_byte_for_byte() {
    assert_matches_golden(
        "prompt-parity.md",
        &prompt_parity_document(SITE, API),
        PROMPT_FIXTURE,
    );
}

#[test]
fn the_goldens_are_rendered_not_memoized() {
    assert_eq!(
        embed_parity_document(SLUG, LOGIN, SITE, API),
        embed_parity_document(SLUG, LOGIN, SITE, API)
    );
    assert_eq!(
        prompt_parity_document(SITE, API),
        prompt_parity_document(SITE, API)
    );
}

/// The goldens are only worth their bytes if they cover the whole catalog. A
/// new asset that nobody added a section for has to fail here rather than sit
/// unguarded.
#[test]
fn the_embed_golden_covers_every_asset_in_the_catalog() {
    let assets: Vec<EmbedAsset> = repo_embed_assets(SLUG)
        .into_iter()
        .chain(profile_embed_assets(LOGIN))
        .collect();
    assert_eq!(EMBED_FIXTURE.matches("\n## ").count(), assets.len());
    for asset in &assets {
        assert!(
            EMBED_FIXTURE.contains(&format!("\n- purpose: {}\n", asset.purpose)),
            "{} is missing from the golden",
            asset.id
        );
        for &encoding in asset.formats {
            assert!(
                EMBED_FIXTURE.contains(&format!(
                    "- url({encoding}): {}\n",
                    asset_url(API, asset, None, Some(encoding))
                )),
                "{} does not publish its {encoding} URL in the golden",
                asset.id
            );
        }
    }
}

/// The prompt golden is only worth its bytes if it reaches every branch that
/// changes the prompt. Each of these strings comes from exactly one of them.
#[test]
fn the_prompt_golden_covers_every_branch_that_changes_the_prompt() {
    for marker in [
        // The placeholder slug's "resolve the repository" opening.
        "Run `git remote get-url origin`",
        // The named-repository heading it replaces.
        "# Add gitdebt analytics to the owner/repo README",
        // Nothing measured: no figure is invented.
        "## Numbers",
        // Something measured: the windows and the lifetime-pace verdict.
        "- 4,500 GitHub stars (+2,700 in 90 days, +900 in 30), running ahead of its lifetime pace.",
        // A GH Archive series, described as activity rather than net stars.
        "never as net stars.",
        // Profile totals, and the repository count that is only printed above zero.
        "- 90,120 stars across LOGIN's public repositories (42 repositories counted).",
    ] {
        assert!(
            PROMPT_FIXTURE.contains(marker),
            "the prompt golden never reaches the branch that emits {marker:?}"
        );
    }
    // The profile prompt with nothing measured claims nothing.
    assert_eq!(
        PROMPT_FIXTURE
            .matches("# Add gitdebt profile analytics")
            .count(),
        2
    );
    assert_eq!(
        PROMPT_FIXTURE
            .matches("## What gitdebt has measured")
            .count(),
        4
    );
}
