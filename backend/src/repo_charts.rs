//! Animated SVG renderers for the per-repo history stats. Pure functions
//! from query-result rows + Theme → SVG string. Bytes-deterministic so
//! edge caches collapse identical request bursts.
//!
//! Theme handling: each renderer takes a `&Theme` and substitutes
//! concrete hex colors directly into the output. No CSS variables, no
//! `prefers-color-scheme` — that approach is fragile in `<img>`-embedded
//! README contexts (see `theme.rs` for the why). Embedders combine a
//! `?theme=light` + `?theme=dark` pair via `<picture>` for theme-aware
//! README rendering.
//!
//! Static attributes always contain the finished chart because README
//! sanitizers may remove SMIL. Animation only enhances renderers that
//! retain it.

use chrono::{Datelike, NaiveDate};
use serde::Serialize;

use crate::brand;
use crate::theme::{Theme, contrast_on};

#[derive(Debug, Clone, Serialize)]
pub struct FileRow {
    pub path: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContributorRow {
    pub login: Option<String>,
    pub name: String,
    pub avatar_url: Option<String>,
    pub commits: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DayCount {
    pub day: NaiveDate,
    pub commits: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TodoPoint {
    pub day: NaiveDate,
    pub running_total: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LanguageBar {
    pub language: String,
    pub files: i64,
    pub lines_code: i64,
    pub lines_blank: i64,
    pub lines_comment: i64,
}

fn reveal_begin(index: usize) -> f32 {
    (index as f32 * 0.04).min(0.08)
}

// Bug-magnet files

pub fn render_bug_magnets(repo: &str, rows: &[FileRow], theme: &Theme) -> String {
    crate::texture::decorate(
        horizontal_bar_chart(BarChartConfig {
            repo,
            title: "Bug-magnet files",
            subtitle: "Files with the most fix commits",
            rows,
            accent: theme.bug,
            accent_dim: theme.bug_dim,
            theme,
        }),
        theme,
    )
}

// Top changed files

pub fn render_top_changed(repo: &str, rows: &[FileRow], theme: &Theme) -> String {
    crate::texture::decorate(
        horizontal_bar_chart(BarChartConfig {
            repo,
            title: "Most-changed files",
            subtitle: "Files with the most commits",
            rows,
            accent: theme.accent,
            accent_dim: theme.accent_dim,
            theme,
        }),
        theme,
    )
}

struct BarChartConfig<'a> {
    repo: &'a str,
    title: &'a str,
    subtitle: &'a str,
    rows: &'a [FileRow],
    accent: &'a str,
    accent_dim: &'a str,
    theme: &'a Theme,
}

fn horizontal_bar_chart(cfg: BarChartConfig<'_>) -> String {
    let width = 900u32;
    let row_h = 32u32;
    let header_h = 90u32;
    let footer_h = 32u32;
    let padding = 56u32;
    let bar_h = 18u32;
    let n = cfg.rows.len() as u32;
    let height = header_h + n * row_h + footer_h;
    let max_count = cfg.rows.iter().map(|r| r.count).max().unwrap_or(1).max(1);
    let label_w = 380.0_f32;
    let bar_max_w = (width as f32) - padding as f32 - label_w - padding as f32;

    let mut bars = String::new();
    for (i, row) in cfg.rows.iter().enumerate() {
        let y = header_h as f32 + (i as f32) * row_h as f32 + (row_h - bar_h) as f32 / 2.0;
        let bar_w = (row.count as f32 / max_count as f32) * bar_max_w;
        let label = truncate_tail(&row.path, 50);
        let href = format!(
            "https://github.com/{repo}/blob/HEAD/{path}",
            repo = cfg.repo,
            path = row.path,
        );
        let count_str = row.count.to_string();
        let (count_x, count_anchor, count_color) =
            count_placement(label_w, bar_w, &count_str, cfg.accent, cfg.theme.muted);
        bars.push_str(&format!(
            r##"<g transform="translate({padding}, {y:.1})" opacity="1">
  <animate class="motion" attributeName="opacity" from="0" to="1" dur="0.2s" begin="{begin:.2}s" fill="freeze" />
  <a class="bar-link" href="{href}" target="_blank" rel="noopener">
    <title>{full_path}</title>
    <text class="bar-label" x="0" y="{label_y:.1}">{label}</text>
  </a>
  <rect class="bar-track" x="{label_w}" y="0" width="{bar_max_w:.1}" height="{bar_h}" rx="3" />
  <rect class="bar-fill" x="{label_w}" y="0" width="{bar_w:.1}" height="{bar_h}" rx="3" />
  <text class="bar-count" x="{count_x:.1}" y="{label_y:.1}" text-anchor="{count_anchor}" fill="{count_color}">
    {count}
  </text>
</g>
"##,
            padding = padding,
            y = y,
            begin = reveal_begin(i),
            href = escape_xml(&href),
            full_path = escape_xml(&row.path),
            label = escape_xml(&label),
            label_w = label_w,
            label_y = bar_h as f32 / 2.0 + 5.0,
            bar_max_w = bar_max_w,
            bar_h = bar_h,
            bar_w = bar_w,
            count = row.count,
            count_x = count_x,
            count_anchor = count_anchor,
            count_color = count_color,
        ));
    }

    let footer_y = (height - 12) as f32;
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" role="img" aria-label="{title} for {repo}">
  <style><![CDATA[
    .title {{ fill: {fg}; font: 600 18px ui-sans-serif, system-ui, sans-serif; }}
    .subtitle {{ fill: {muted}; font: 13px ui-sans-serif, system-ui, sans-serif; }}
    .bar-label {{ fill: {fg}; font: 12px ui-monospace, SFMono-Regular, monospace; }}
    .bar-link {{ cursor: pointer; }}
    .bar-link:hover .bar-label {{ fill: {accent}; text-decoration: underline; }}
    .bar-count {{ font: 600 12px ui-sans-serif, system-ui, sans-serif; }}
    .bar-track {{ fill: {track}; }}
    .bar-fill {{ fill: url(#gd-pixel-fill); stroke: {accent}; stroke-width: 1; shape-rendering: crispEdges; }}
    .footer-link {{ fill: {muted}; font: 600 11px ui-sans-serif, system-ui, sans-serif; text-decoration: none; letter-spacing: 0.02em; }}
    .footer-link:hover {{ fill: {fg}; }}
    @media (prefers-reduced-motion: reduce) {{
      .motion {{ display: none; }}
    }}
    g:hover .bar-fill {{ stroke: {accent_dim}; opacity: 0.72; }}
  ]]></style>
  <text class="title" x="{padding}" y="36">{title}</text>
  <text class="subtitle" x="{padding}" y="58">{subtitle} · {repo}</text>
{bars}
{footer}
</svg>"##,
        width = width,
        height = height,
        repo = escape_xml(cfg.repo),
        title = escape_xml(cfg.title),
        subtitle = escape_xml(cfg.subtitle),
        fg = cfg.theme.fg,
        muted = cfg.theme.muted,
        track = cfg.theme.track,
        accent = cfg.accent,
        accent_dim = cfg.accent_dim,
        padding = padding,
        bars = bars,
        footer = brand::footer_lockup((width as f32) - padding as f32, footer_y, cfg.theme,),
    )
}

/// Decide where to draw the count number relative to the bar:
///   - if the bar is wide enough to swallow the text → render INSIDE the
///     bar, right-anchored, with a contrasting (white/black) fill;
///   - otherwise render OUTSIDE to the right of the bar, in the muted
///     foreground color.
///
/// Returns `(x, text-anchor, fill)` ready to drop into the `<text>` tag.
fn count_placement<'a>(
    label_w: f32,
    bar_w: f32,
    text: &str,
    bar_color: &str,
    muted: &'a str,
) -> (f32, &'static str, &'a str) {
    // 7.5 px/char is a reasonable estimate for a 12px sans-serif weight 600.
    let estimated = (text.chars().count() as f32) * 7.5;
    if bar_w >= estimated + 16.0 {
        (label_w + bar_w - 8.0, "end", contrast_on(bar_color))
    } else {
        (label_w + bar_w + 8.0, "start", muted)
    }
}

// Commit heatmap

pub fn render_heatmap(
    repo: &str,
    subtitle_label: &str,
    start: NaiveDate,
    end: NaiveDate,
    days: &[DayCount],
    theme: &Theme,
) -> String {
    crate::texture::decorate(
        render_heatmap_inner(repo, subtitle_label, start, end, days, theme),
        theme,
    )
}

fn render_heatmap_inner(
    repo: &str,
    subtitle_label: &str,
    start: NaiveDate,
    end: NaiveDate,
    days: &[DayCount],
    theme: &Theme,
) -> String {
    use std::collections::BTreeMap;
    let counts: BTreeMap<NaiveDate, i64> = days.iter().map(|d| (d.day, d.commits)).collect();
    let mut sorted: Vec<i64> = counts.values().copied().filter(|c| *c > 0).collect();
    sorted.sort();
    let q = |p: f32| -> i64 {
        if sorted.is_empty() {
            return 0;
        }
        let idx = ((sorted.len() as f32 - 1.0) * p).round() as usize;
        sorted[idx]
    };
    let q1 = q(0.25);
    let q2 = q(0.5);
    let q3 = q(0.75);

    let cell = 14u32;
    let gap = 3u32;
    let pad_left = 32u32;
    let pad_top = 70u32;
    let pad_bottom = 50u32;
    let rows = 7u32;
    let aligned_start = first_monday_on_or_before(start);
    let total_days = (end - aligned_start).num_days().max(0) as u32 + 1;
    let cols = total_days.div_ceil(7);
    let plot_w = cols * (cell + gap);
    let plot_h = rows * (cell + gap);
    let width = plot_w + pad_left + 32;
    let height = plot_h + pad_top + pad_bottom;

    let mut cells = String::new();
    let mut total = 0i64;
    let mut max_seen = 0i64;
    let mut peak_day: Option<NaiveDate> = None;
    let mut day_iter = start;
    while day_iter <= end {
        let weekday = day_iter.weekday().num_days_from_monday();
        let days_from_aligned = (day_iter - aligned_start).num_days();
        let col = (days_from_aligned / 7) as u32;
        let count = counts.get(&day_iter).copied().unwrap_or(0);
        total = total.saturating_add(count);
        if count > max_seen {
            max_seen = count;
            peak_day = Some(day_iter);
        }
        let level = if count == 0 {
            0
        } else if count <= q1 {
            1
        } else if count <= q2 {
            2
        } else if count <= q3 {
            3
        } else {
            4
        };
        let x = pad_left + col * (cell + gap);
        let y = pad_top + weekday * (cell + gap);
        let href = format!(
            "https://github.com/{repo}/commits?since={day}T00%3A00%3A00Z&amp;until={day}T23%3A59%3A59Z",
            day = day_iter,
        );
        cells.push_str(&format!(
            r##"<a class="day-link" href="{href}" target="_blank" rel="noopener" aria-label="Open commits from {day}">
  <rect class="cell" x="{x}" y="{y}" width="{cell}" height="{cell}" rx="2" fill="{track}">
    <title>{day} · {count} commit{plural} · open on GitHub</title>
  </rect>{ink}
</a>
"##,
            href = href,
            x = x,
            y = y,
            cell = cell,
            track = theme.track,
            ink = heat_ink(x as f32, y as f32, cell as f32, level),
            day = day_iter,
            count = count,
            plural = if count == 1 { "" } else { "s" },
        ));
        let Some(next) = day_iter.succ_opt() else {
            break;
        };
        day_iter = next;
    }

    let legend_y = (height - 30) as f32;
    let legend_x = pad_left as f32;
    let mut legend_cells = String::new();
    for level in 0..HEAT_TIERS.len() {
        let x = legend_x + 60.0 + (level as f32) * (cell as f32 + 2.0);
        legend_cells.push_str(&format!(
            r##"<rect class="cell" x="{x:.1}" y="{legend_y:.1}" width="{cell}" height="{cell}" rx="2" fill="{track}" />{ink}"##,
            legend_y = legend_y,
            cell = cell,
            track = theme.track,
            ink = heat_ink(x, legend_y, cell as f32, level),
        ));
    }

    let mut dow_labels = String::new();
    for (idx, label) in ["Mon", "Wed", "Fri"].iter().enumerate() {
        let row = match idx {
            0 => 0u32,
            1 => 2,
            _ => 4,
        };
        let y = pad_top + row * (cell + gap) + cell - 2;
        dow_labels.push_str(&format!(
            r##"<text class="dow" x="0" y="{y}">{label}</text>"##,
            y = y,
            label = label,
        ));
    }

    let peak_text = peak_day
        .map(|d| format!("Peak: {} ({} commits)", d, max_seen))
        .unwrap_or_else(|| "No commits in this range".into());

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" role="img" aria-label="Commit activity for {repo}">
  <defs data-gitdebt-heat-defs="true">{heat_defs}</defs>
  <style><![CDATA[
    .title {{ fill: {fg}; font: 600 18px ui-sans-serif, system-ui, sans-serif; }}
    .subtitle {{ fill: {muted}; font: 13px ui-sans-serif, system-ui, sans-serif; }}
    .dow, .legend {{ fill: {muted}; font: 10px ui-sans-serif, system-ui, sans-serif; }}
    .cell {{ stroke: {border}; stroke-width: 0.5; stroke-opacity: 0.4; }}
    .day-link {{ cursor: pointer; }}
    .footer-link {{ fill: {muted}; font: 600 11px ui-sans-serif, system-ui, sans-serif; text-decoration: none; letter-spacing: 0.02em; }}
    .footer-link:hover {{ fill: {fg}; }}
    .cell:hover {{ stroke: {fg}; stroke-width: 1.5; stroke-opacity: 1; }}
    @media (prefers-reduced-motion: reduce) {{
      .motion {{ display: none; }}
    }}
  ]]></style>
  <text class="title" x="{pad_left}" y="32">{repo}</text>
  <text class="subtitle" x="{pad_left}" y="52">{subtitle_label} · {total} total · {peak_text}</text>
  {dow_labels}
  <g class="heat-cells" opacity="1">
    <animate class="motion" attributeName="opacity" from="0" to="1" dur="0.2s" begin="0s" fill="freeze" />
    {cells}
  </g>
  <text class="legend" x="{legend_x:.0}" y="{legend_label_y:.0}">Less</text>
  {legend_cells}
  <text class="legend" x="{legend_more_x:.0}" y="{legend_label_y:.0}">More</text>
{footer}
</svg>"##,
        width = width,
        height = height,
        repo = escape_xml(repo),
        subtitle_label = escape_xml(subtitle_label),
        fg = theme.fg,
        muted = theme.muted,
        border = theme.border,
        heat_defs = heat_defs(theme),
        pad_left = pad_left,
        total = total,
        peak_text = escape_xml(&peak_text),
        dow_labels = dow_labels,
        cells = cells,
        legend_x = legend_x,
        legend_label_y = legend_y + cell as f32 - 2.0,
        legend_cells = legend_cells,
        legend_more_x = legend_x + 60.0 + 5.0 * (cell as f32 + 2.0) + 4.0,
        footer = brand::footer_lockup((width as f32) - 16.0, (height - 10) as f32, theme,),
    )
}

// Commit-activity heat levels
//
// The heatmap is one series, so it gets one ink and modulates ALPHA, the
// same contract as every other dithered surface here. Each intensity
// level is a Bayer density tier: level 0 leaves the empty track showing,
// levels 1–4 stack a denser lit-cell pattern on top. Nothing is shaded —
// a flat five-step gray ramp is what made this chart the odd one out.

/// Bayer density tier per heat level (`0` = no commits, `4` = top
/// quartile). The top tier stops at 13/16 so even the busiest day still
/// shows grain instead of flattening back into a solid swatch.
const HEAT_TIERS: [usize; 5] = [0, 2, 6, 10, 13];
/// Lit-cell alpha per heat level, following `0.3 + 0.7 · density`.
const HEAT_ALPHA: [&str; 5] = ["0", "0.48", "0.65", "0.83", "1"];
/// Id namespace for the heat tier ladder.
const HEAT_NS: &str = "gd-heat";

/// The tier ladder the heatmap actually references, in the theme's ink.
fn heat_defs(theme: &Theme) -> String {
    let mut out = String::new();
    for tier in HEAT_TIERS.iter().skip(1) {
        out.push_str(&crate::texture::tier_pattern_ns(
            HEAT_NS, theme.fg, 2.0, *tier,
        ));
    }
    out
}

/// Dithered ink overlay for one heat cell. Level 0 renders nothing so the
/// empty track shows through untouched.
fn heat_ink(x: f32, y: f32, cell: f32, level: usize) -> String {
    let level = level.min(HEAT_TIERS.len() - 1);
    if level == 0 {
        return String::new();
    }
    format!(
        r##"<rect class="heat-ink" x="{x:.0}" y="{y:.0}" width="{cell:.0}" height="{cell:.0}" rx="2" fill="{fill}" fill-opacity="{alpha}" shape-rendering="crispEdges" pointer-events="none" />"##,
        fill = crate::texture::tier_fill_ns(HEAT_NS, HEAT_TIERS[level]),
        alpha = HEAT_ALPHA[level],
    )
}

fn first_monday_on_or_before(d: NaiveDate) -> NaiveDate {
    let offset = d.weekday().num_days_from_monday() as i64;
    d - chrono::Duration::days(offset)
}

// Contributors

pub fn render_contributors(repo: &str, contributors: &[ContributorRow], theme: &Theme) -> String {
    crate::texture::decorate(render_contributors_inner(repo, contributors, theme), theme)
}

fn render_contributors_inner(repo: &str, contributors: &[ContributorRow], theme: &Theme) -> String {
    let width = 1100u32;
    let pad = 44u32;
    let height = 208u32;
    let avatar_y = 86u32;
    let avatar_size = 78u32;
    let step = 61u32;
    let shown: Vec<&ContributorRow> = contributors
        .iter()
        .filter(|row| row.commits > 0)
        .take(16)
        .collect();

    let mut rows = String::new();
    for (i, c) in shown.iter().enumerate() {
        let x = pad + i as u32 * step;
        let y = avatar_y;
        let label = c.login.clone().unwrap_or_else(|| c.name.clone());
        let profile = c
            .login
            .as_ref()
            .map(|login| format!("https://github.com/{login}"));
        let avatar = c.avatar_url.as_ref().map_or_else(
            || {
                let initial = label
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .to_string();
                format!(
                    r#"<circle class="avatar-fallback-bg" cx="{r}" cy="{r}" r="{r}" /><text class="avatar-fallback" x="{r}" y="{y}" text-anchor="middle">{initial}</text>"#,
                    r = avatar_size / 2,
                    y = avatar_size / 2 + 8,
                    initial = escape_xml(&initial),
                )
            },
            |url| {
                format!(
                    r#"<image href="{url}" x="0" y="0" width="{avatar_size}" height="{avatar_size}" clip-path="url(#contributor-clip-{i})" preserveAspectRatio="xMidYMid slice" />"#,
                    url = escape_xml(url),
                )
            },
        );
        let content = format!(
            r##"<title>{label}</title>
      <g class="avatar-pos">
        <circle class="avatar-pixels" cx="{r}" cy="{r}" r="{ring_r}" />
        <clipPath id="contributor-clip-{i}"><circle cx="{r}" cy="{r}" r="{r}" /></clipPath>
        {avatar}
        <circle class="avatar-outline" cx="{r}" cy="{r}" r="{r}" />
      </g>"##,
            label = escape_xml(&label),
            r = avatar_size / 2,
            ring_r = avatar_size / 2 + 5,
        );
        let linked = profile.map_or(content.clone(), |href| {
            format!(
                r##"<a class="contributor-link" href="{href}" target="_blank" rel="noopener">{content}</a>"##,
                href = escape_xml(&href),
            )
        });
        rows.push_str(&format!(
            r##"<g class="contributor-node" transform="translate({x}, {y})" opacity="1">
    <animate class="motion" attributeName="opacity" from="0" to="1" dur="0.2s" begin="{begin:.2}s" fill="freeze" />
    {linked}
</g>
"##,
            y = y,
            x = x,
            begin = reveal_begin(i),
            linked = linked,
        ));
    }

    let footer_y = (height - 12) as f32;
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" role="img" aria-label="Contributors of {repo}">
  <style><![CDATA[
    .title {{ fill: {fg}; font: 600 18px ui-sans-serif, system-ui, sans-serif; }}
    .subtitle {{ fill: {muted}; font: 13px ui-sans-serif, system-ui, sans-serif; }}
    .avatar-pixels {{ fill: url(#gd-pixel-fill); stroke: {bg}; stroke-width: 3; shape-rendering: crispEdges; }}
    .avatar-outline {{ fill: none; stroke: {border}; stroke-width: 1.5; }}
    .avatar-fallback-bg {{ fill: {track}; }}
    .avatar-fallback {{ fill: {fg}; font: 700 22px ui-monospace, SFMono-Regular, monospace; }}
    .avatar-pos {{ transform-box: fill-box; transform-origin: center; transition: transform 320ms cubic-bezier(0.2, 0.8, 0.2, 1); }}
    .contributor-link {{ cursor: pointer; }}
    @media (hover: hover) and (pointer: fine) {{
      .contributor-link:hover .avatar-pos {{ transform: translateY(-10px) scale(1.08); }}
      .contributor-link:hover .avatar-outline {{ stroke: {fg}; stroke-width: 2; }}
    }}
    .footer-link {{ fill: {muted}; font: 600 11px ui-sans-serif, system-ui, sans-serif; text-decoration: none; letter-spacing: 0.02em; }}
    .footer-link:hover {{ fill: {fg}; }}
    @media (prefers-reduced-motion: reduce) {{
      .motion {{ display: none; }}
      .avatar-pos {{ transition: none; }}
    }}
  ]]></style>
  <text class="title" x="{pad}" y="36">Contributors</text>
  <text class="subtitle" x="{pad}" y="58">Public commit authors · {repo}</text>
{rows}
{footer}
</svg>"##,
        width = width,
        height = height,
        repo = escape_xml(repo),
        fg = theme.fg,
        bg = theme.bg,
        muted = theme.muted,
        border = theme.border,
        track = theme.track,
        pad = pad,
        rows = rows,
        footer = brand::footer_lockup((width as f32) - pad as f32, footer_y, theme,),
    )
}

// Lines of code by language

pub fn render_languages(repo: &str, rows: &[LanguageBar], theme: &Theme) -> String {
    crate::texture::decorate(render_languages_inner(repo, rows, theme), theme)
}

fn render_languages_inner(repo: &str, rows: &[LanguageBar], theme: &Theme) -> String {
    let width = 1100u32;
    let row_h = 32u32;
    let header_h = 96u32;
    let footer_h = 32u32;
    let padding = 56u32;
    let bar_h = 18u32;
    let n = rows.len() as u32;
    let height = header_h + n * row_h + footer_h;

    let line_totals: Vec<i64> = rows
        .iter()
        .map(|r| r.lines_code + r.lines_blank + r.lines_comment)
        .collect();
    let file_census = line_totals.iter().all(|total| *total == 0);
    let totals: Vec<i64> = if file_census {
        rows.iter().map(|row| row.files).collect()
    } else {
        line_totals.clone()
    };
    let max_total = totals.iter().copied().max().unwrap_or(1).max(1);
    let label_w = 220.0_f32;
    let meta_col_w = 200.0_f32;
    let bar_max_w = (width as f32) - padding as f32 - label_w - meta_col_w - padding as f32;

    let total_total: i64 = totals.iter().sum();
    let total_code: i64 = rows.iter().map(|r| r.lines_code).sum();
    let total_files: i64 = rows.iter().map(|r| r.files).sum();

    let mut bars = String::new();
    let mut lang_defs = String::new();
    for (i, row) in rows.iter().enumerate() {
        let total = totals[i];
        let y = header_h as f32 + (i as f32) * row_h as f32 + (row_h - bar_h) as f32 / 2.0;
        let bar_w = (total as f32 / max_total as f32) * bar_max_w;
        let color = language_color(&row.language, theme);
        // One ordered-dither ladder per row, inked in that language's own
        // color. The bar then varies ALPHA (unlit tier under lit tier),
        // never shade, so the grain matches every other gitdebt surface.
        let ns = format!("gd-lang{i}");
        lang_defs.push_str(&crate::texture::tier_pattern_ns(
            &ns,
            &color,
            2.0,
            LANGUAGE_TIER,
        ));
        let count_text = humanize(total);
        // Contrast is judged against what the bar actually LOOKS like —
        // the dither's average coverage over the track — not against the
        // raw ink, or a count printed inside a light bar goes invisible.
        let bar_tone = dithered_tone(&color, theme.track);
        let (count_x, count_anchor, count_color) =
            count_placement(label_w, bar_w, &count_text, &bar_tone, theme.muted);
        let meta_text = if file_census {
            format!(
                "{} file{}",
                row.files,
                if row.files == 1 { "" } else { "s" }
            )
        } else {
            format!(
                "{} file{} · {} code",
                row.files,
                if row.files == 1 { "" } else { "s" },
                humanize(row.lines_code),
            )
        };
        let title = if file_census {
            format!("{} files in current HEAD", row.files)
        } else {
            format!(
                "{total} total · {} code · {} comments · {} blank",
                row.lines_code, row.lines_comment, row.lines_blank
            )
        };
        bars.push_str(&format!(
            r##"<g transform="translate({padding}, {y:.1})" opacity="1">
  <animate class="motion" attributeName="opacity" from="0" to="1" dur="0.2s" begin="{begin:.2}s" fill="freeze" />
  <circle cx="6" cy="{dot_y:.1}" r="6" fill="{color}" />
  <text class="bar-label" x="20" y="{label_y:.1}">{language}</text>
  <rect class="bar-track" x="{label_w}" y="0" width="{bar_max_w:.1}" height="{bar_h}" rx="3" />
  <rect x="{label_w}" y="0" width="{bar_w:.1}" height="{bar_h}" rx="3" fill="{color}" fill-opacity="{off_alpha}" />
  <rect x="{label_w}" y="0" width="{bar_w:.1}" height="{bar_h}" rx="3" fill="{lang_fill}" fill-opacity="{lit_alpha}" shape-rendering="crispEdges" />
  <rect x="{label_w}" y="0" width="{bar_w:.1}" height="{bar_h}" rx="3" fill="none" stroke="{color}" stroke-width="1" />
  <text class="bar-count" x="{count_x:.1}" y="{label_y:.1}" text-anchor="{count_anchor}" fill="{count_color}">
    <title>{title}</title>
    {count_text}
  </text>
  <text class="bar-meta" x="{meta_x:.1}" y="{label_y:.1}">
    {meta_text}
  </text>
</g>
"##,
            padding = padding,
            y = y,
            begin = reveal_begin(i),
            dot_y = bar_h as f32 / 2.0,
            color = color,
            lang_fill = crate::texture::tier_fill_ns(&ns, LANGUAGE_TIER),
            off_alpha = LANGUAGE_OFF_ALPHA,
            lit_alpha = LANGUAGE_LIT_ALPHA,
            language = escape_xml(&row.language),
            label_w = label_w,
            label_y = bar_h as f32 / 2.0 + 5.0,
            bar_max_w = bar_max_w,
            bar_h = bar_h,
            bar_w = bar_w,
            title = escape_xml(&title),
            count_x = count_x,
            count_anchor = count_anchor,
            count_color = count_color,
            count_text = count_text,
            meta_x = label_w + bar_max_w + 14.0,
            meta_text = escape_xml(&meta_text),
        ));
    }

    let footer_y = (height - 12) as f32;
    let right_edge_x = (label_w + bar_max_w + meta_col_w) + padding as f32 - 4.0;
    let subtitle = if file_census {
        format!(
            "{} · {} files in {} languages · current HEAD tree",
            repo,
            humanize(total_files),
            rows.len()
        )
    } else {
        format!(
            "{} · {} lines · {} code · {} files in {} languages",
            repo,
            humanize(total_total),
            humanize(total_code),
            total_files,
            rows.len()
        )
    };
    let aria = if file_census {
        format!("Language file activity in {repo}")
    } else {
        format!("Lines of code in {repo}")
    };
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" role="img" aria-label="{aria}">
  <defs data-gitdebt-language-defs="true">{lang_defs}</defs>
  <style><![CDATA[
    .title {{ fill: {fg}; font: 600 18px ui-sans-serif, system-ui, sans-serif; }}
    .subtitle {{ fill: {muted}; font: 13px ui-sans-serif, system-ui, sans-serif; }}
    .bar-label {{ fill: {fg}; font: 12px ui-sans-serif, system-ui, sans-serif; font-weight: 500; }}
    .bar-count {{ font: 600 12px ui-sans-serif, system-ui, sans-serif; }}
    .bar-meta {{ fill: {muted}; font: 11px ui-sans-serif, system-ui, sans-serif; }}
    .bar-track {{ fill: {track}; }}
    .footer-link {{ fill: {muted}; font: 600 11px ui-sans-serif, system-ui, sans-serif; text-decoration: none; letter-spacing: 0.02em; }}
    .footer-link:hover {{ fill: {fg}; }}
    @media (prefers-reduced-motion: reduce) {{
      .motion {{ display: none; }}
    }}
  ]]></style>
  <text class="title" x="{padding}" y="36">Language activity</text>
  <text class="subtitle" x="{padding}" y="58">{subtitle}</text>
{bars}
{footer}
</svg>"##,
        width = width,
        height = height,
        aria = escape_xml(&aria),
        fg = theme.fg,
        muted = theme.muted,
        track = theme.track,
        padding = padding,
        bars = bars,
        lang_defs = lang_defs,
        subtitle = escape_xml(&subtitle),
        footer = brand::footer_lockup(right_edge_x, footer_y, theme),
    )
}

/// Density tier used by the language bars: 12/16 cells lit, which reads
/// as a solid-but-grainy fill at the 2px cell size.
const LANGUAGE_TIER: usize = 11;
/// Average ink coverage of a [`LANGUAGE_TIER`] bar: lit cells at
/// [`LANGUAGE_LIT_ALPHA`] plus unlit cells at [`LANGUAGE_OFF_ALPHA`].
const LANGUAGE_COVERAGE: f32 = 0.74;
/// Alpha of the unlit cells (the ordered-dither "off" tier) and of the
/// lit pattern above it. Alpha-only modulation: both layers carry the
/// same ink, so the bar reads correctly on either theme's canvas.
const LANGUAGE_OFF_ALPHA: &str = "0.2";
const LANGUAGE_LIT_ALPHA: &str = "0.92";

fn humanize(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

// Per-language ink

/// Achromatic gray for buckets that are not one language: the synthetic
/// `Config` rollup and anything the census could not name. Documented
/// meaning: "no single language".
const NEUTRAL_LANGUAGE_COLOR: &str = "#8b8b8b";

/// The conventional color for a language, as readers already expect it
/// (Go cyan, Rust rust, TypeScript blue, …). Synthetic aliases inherit
/// their parent language's color. `None` → no conventional color exists,
/// so the caller derives a stable hue from the name instead.
///
/// This map is the *source* hue only; [`language_color`] is what renders,
/// because these values were picked against a white page and several of
/// them (`#292929`, `#012456`, `#f1e05a`) are unreadable on one of our
/// two themes untouched.
fn conventional_language_color(name: &str) -> Option<&'static str> {
    Some(match name {
        "Rust" => "#dea584",
        "TypeScript" | "TSX" => "#3178c6",
        "JavaScript" | "JSX" => "#f1e05a",
        "Python" => "#3572a5",
        "Go" => "#00add8",
        "Ruby" => "#701516",
        "Java" => "#b07219",
        "Kotlin" => "#a97bff",
        "Swift" => "#f05138",
        "Objective-C" => "#438eff",
        "C" => "#555555",
        "C++" => "#f34b7d",
        "C#" => "#178600",
        "Shell" | "Bash" => "#89e051",
        "PowerShell" => "#012456",
        "HTML" => "#e34c26",
        "CSS" => "#563d7c",
        "SCSS" => "#c6538c",
        "Less" => "#1d365d",
        "Vue" => "#41b883",
        "Svelte" => "#ff3e00",
        "Astro" => "#ff5a03",
        "Markdown" => "#083fa1",
        "TOML" => "#9c4221",
        "YAML" => "#cb171e",
        "JSON" => "#292929",
        "XML" => "#0060ac",
        "SVG" => "#ff9900",
        "Dockerfile" => "#384d54",
        "Lua" => "#000080",
        "Perl" => "#0298c3",
        "PHP" => "#4f5d95",
        "Scala" => "#c22d40",
        "Groovy" => "#4298b8",
        "Haskell" => "#5e5086",
        "Elixir" => "#6e4a7e",
        "Erlang" => "#b83998",
        "OCaml" => "#3be133",
        "Dart" => "#00b4ab",
        "Zig" => "#ec915c",
        "R" => "#198ce7",
        "Julia" => "#a270ba",
        "Nix" => "#7e7eff",
        "Makefile" => "#427819",
        "CMake" => "#da3434",
        "SQL" => "#e38c00",
        "GraphQL" => "#e10098",
        "Verilog" => "#b2b7f8",
        "Terraform" | "HCL" => "#844fba",
        "TeX" => "#3d6117",
        "Config" => NEUTRAL_LANGUAGE_COLOR,
        _ => return None,
    })
}

/// Stable per-language ink, legibility-corrected for `theme`.
///
/// Two rules, in order:
///   1. hue is the language's conventional color when one exists, and a
///      deterministic name-derived hue otherwise — so two languages never
///      share a bar color just because they are adjacent in the list;
///   2. the result is pushed into the theme's readable luminance band by
///      blending toward white (dark theme) or black (light theme). Hue
///      survives; only lightness moves. Same input → same bytes.
pub fn language_color(name: &str, theme: &Theme) -> String {
    let rgb = conventional_language_color(name)
        .and_then(parse_hex)
        .unwrap_or_else(|| hashed_language_rgb(name));
    let (min, max) = if theme.dark {
        (0.30_f32, 0.94_f32)
    } else {
        (0.06_f32, 0.55_f32)
    };
    format_hex(clamp_luma(rgb, min, max))
}

/// The apparent color of a dithered bar: `ink` composited over `track` at
/// the tier's average coverage. Contrast decisions must use this, not the
/// raw ink, because only a fraction of the bar's cells are actually lit.
fn dithered_tone(ink: &str, track: &str) -> String {
    let (Some(ink), Some(track)) = (parse_hex(ink), parse_hex(track)) else {
        return ink.to_string();
    };
    let mut out = [0.0_f32; 3];
    for i in 0..3 {
        out[i] = ink[i] * LANGUAGE_COVERAGE + track[i] * (1.0 - LANGUAGE_COVERAGE);
    }
    format_hex(out)
}

/// Relative luminance of an sRGB triple, in `0.0..=1.0`. Deliberately the
/// cheap non-linearized form: this only has to rank colors consistently.
fn luma([r, g, b]: [f32; 3]) -> f32 {
    (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0
}

/// Blend toward white or black until the luminance lands inside
/// `[min, max]`. Both blends are affine in each channel, so the resulting
/// luminance is exactly the requested bound.
fn clamp_luma(rgb: [f32; 3], min: f32, max: f32) -> [f32; 3] {
    let l = luma(rgb);
    if l < min {
        let t = (min - l) / (1.0 - l).max(f32::EPSILON);
        rgb.map(|c| c + (255.0 - c) * t)
    } else if l > max {
        let t = (l - max) / l.max(f32::EPSILON);
        rgb.map(|c| c * (1.0 - t))
    } else {
        rgb
    }
}

fn parse_hex(hex: &str) -> Option<[f32; 3]> {
    let raw = hex.strip_prefix('#')?;
    if raw.len() != 6 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&raw[i..i + 2], 16).ok().map(f32::from);
    Some([byte(0)?, byte(2)?, byte(4)?])
}

fn format_hex(rgb: [f32; 3]) -> String {
    let ch = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", ch(rgb[0]), ch(rgb[1]), ch(rgb[2]))
}

/// Deterministic fallback hue for languages with no conventional color.
/// FNV-1a over the name picks a hue; saturation and lightness are fixed
/// so every derived color starts inside a sane band before the theme
/// clamp runs.
fn hashed_language_rgb(name: &str) -> [f32; 3] {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in name.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    // 24 evenly spaced hues, offset so the first bucket is not pure red.
    let hue = ((hash % 24) as f32) * 15.0 + 7.0;
    hsl_to_rgb(hue, 0.62, 0.58)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h % 360.0) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [(r + m) * 255.0, (g + m) * 255.0, (b + m) * 255.0]
}

// TODO/FIXME trend

pub fn render_todo_trend(repo: &str, points: &[TodoPoint], theme: &Theme) -> String {
    crate::texture::decorate(render_todo_trend_inner(repo, points, theme), theme)
}

fn render_todo_trend_inner(repo: &str, points: &[TodoPoint], theme: &Theme) -> String {
    let width = 1200u32;
    let height = 360u32;
    let pad_l = 56.0_f32;
    let pad_r = 40.0_f32;
    let pad_t = 70.0_f32;
    let pad_b = 50.0_f32;
    let plot_w = width as f32 - pad_l - pad_r;
    let plot_h = height as f32 - pad_t - pad_b;

    if points.is_empty() {
        return empty_chart(width, height, "no TODO/FIXME data yet", theme);
    }

    let t_min = points.first().unwrap().day;
    let t_max = points.last().unwrap().day;
    let span_days = (t_max - t_min).num_days().max(1) as f32;
    let y_max = points
        .iter()
        .map(|p| p.running_total)
        .max()
        .unwrap_or(1)
        .max(1) as f32;

    let x_at = |d: NaiveDate| -> f32 {
        let dx = (d - t_min).num_days() as f32;
        pad_l + (dx / span_days) * plot_w
    };
    let y_at = |v: f32| pad_t + plot_h - (v / y_max) * plot_h;

    let mut path = String::new();
    for (i, p) in points.iter().enumerate() {
        let x = x_at(p.day);
        let y = y_at(p.running_total as f32);
        if i == 0 {
            path.push_str(&format!("M {x:.1} {y:.1}"));
        } else {
            path.push_str(&format!(" L {x:.1} {y:.1}"));
        }
    }

    let mut area = path.clone();
    if let Some(last) = points.last() {
        let last_x = x_at(last.day);
        let baseline_y = y_at(0.0);
        area.push_str(&format!(" L {last_x:.1} {baseline_y:.1}"));
        let first_x = x_at(points[0].day);
        area.push_str(&format!(" L {first_x:.1} {baseline_y:.1} Z"));
    }

    let last_total = points.last().unwrap().running_total;
    let max_total = points.iter().map(|p| p.running_total).max().unwrap_or(0);
    let max_day = points
        .iter()
        .max_by_key(|p| p.running_total)
        .map(|p| p.day)
        .unwrap_or(t_min);

    let mut y_ticks = String::new();
    for i in 0..=4 {
        let v = y_max * (i as f32 / 4.0);
        let y = y_at(v);
        y_ticks.push_str(&format!(
            r##"<line x1="{pad_l:.1}" y1="{y:.1}" x2="{x_end:.1}" y2="{y:.1}" stroke="{grid}" stroke-width="1" opacity="0.55" />
<text class="axis" x="{ax:.1}" y="{ty:.1}" text-anchor="end">{v}</text>
"##,
            pad_l = pad_l,
            y = y,
            x_end = pad_l + plot_w,
            ax = pad_l - 8.0,
            ty = y + 4.0,
            v = v.round() as i64,
            grid = theme.grid,
        ));
    }

    let footer_y = (height - 12) as f32;
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" role="img" aria-label="TODO/FIXME running total for {repo}">
  <style><![CDATA[
    .title {{ fill: {fg}; font: 600 18px ui-sans-serif, system-ui, sans-serif; }}
    .subtitle {{ fill: {muted}; font: 13px ui-sans-serif, system-ui, sans-serif; }}
    .axis {{ fill: {muted}; font: 11px ui-sans-serif, system-ui, sans-serif; }}
    .footer-link {{ fill: {muted}; font: 600 11px ui-sans-serif, system-ui, sans-serif; text-decoration: none; letter-spacing: 0.02em; }}
    .footer-link:hover {{ fill: {fg}; }}
    @media (prefers-reduced-motion: reduce) {{
      .motion {{ display: none; }}
    }}
  ]]></style>
  <text class="title" x="{pad_l:.0}" y="34">{repo}</text>
  <text class="subtitle" x="{pad_l:.0}" y="56">TODO/FIXME running total · current {last_total} · peak {max_total} on {max_day}</text>
  {y_ticks}
  <g opacity="1">
    <animate class="motion" attributeName="opacity" from="0" to="1" dur="0.2s" begin="0s" fill="freeze" />
    <path d="{area}" fill="url(#gd-pixel-fill)" opacity="0.94" />
    <path d="{path}" fill="none" stroke="{bug}" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" />
    <circle cx="{peak_x:.1}" cy="{peak_y:.1}" r="5" fill="{bug}" />
  </g>
{footer}
</svg>"##,
        width = width,
        height = height,
        repo = escape_xml(repo),
        fg = theme.fg,
        muted = theme.muted,
        bug = theme.bug,
        pad_l = pad_l,
        last_total = last_total,
        max_total = max_total,
        max_day = max_day,
        y_ticks = y_ticks,
        area = area,
        path = path,
        peak_x = x_at(max_day),
        peak_y = y_at(max_total as f32),
        footer = brand::footer_lockup((width as f32) - 24.0, footer_y, theme),
    )
}

// Bus factor

#[derive(Debug, Clone, Serialize)]
pub struct AuthorShare {
    /// Display label — GitHub login when known, otherwise author name.
    pub label: String,
    pub login: Option<String>,
    pub avatar_url: Option<String>,
    pub commits: i64,
}

/// Minimum number of top contributors whose combined commits strictly
/// exceed 50% of `total_commits` — i.e. how many people the project
/// could lose before more than half of its authorship knowledge is gone.
///
/// `commits` need not be sorted (a descending copy is taken internally)
/// and non-positive entries are ignored. Returns 0 when there are no
/// commits at all. If `commits` is a truncated top-N prefix whose sum
/// never crosses half of `total_commits`, the prefix length is returned
/// as a lower bound — by definition the true bus factor is at least
/// that large.
pub fn compute_bus_factor(commits: &[i64], total_commits: i64) -> usize {
    if total_commits <= 0 {
        return 0;
    }
    let mut sorted: Vec<i64> = commits.iter().copied().filter(|c| *c > 0).collect();
    if sorted.is_empty() {
        return 0;
    }
    sorted.sort_unstable_by(|a, b| b.cmp(a));
    let mut acc = 0i64;
    for (i, c) in sorted.iter().enumerate() {
        acc = acc.saturating_add(*c);
        if acc.saturating_mul(2) > total_commits {
            return i + 1;
        }
    }
    sorted.len()
}

/// Contributor-concentration chart focused on the people carrying at least
/// one percent of the analyzed commits. The bus factor still considers the
/// full bot-filtered author population; the visual deliberately omits tiny
/// shares so the ownership risk remains legible.
pub fn render_bus_factor(
    repo: &str,
    authors: &[AuthorShare],
    total_commits: i64,
    theme: &Theme,
) -> String {
    crate::texture::decorate(
        render_bus_factor_inner(repo, authors, total_commits, theme),
        theme,
    )
}

fn render_bus_factor_inner(
    repo: &str,
    authors: &[AuthorShare],
    total_commits: i64,
    theme: &Theme,
) -> String {
    let width = 900u32;
    let padding = 56u32;

    let mut sorted: Vec<&AuthorShare> = authors.iter().filter(|a| a.commits > 0).collect();
    sorted.sort_by(|a, b| {
        b.commits
            .cmp(&a.commits)
            .then_with(|| a.label.cmp(&b.label))
    });

    if sorted.is_empty() || total_commits <= 0 {
        return empty_chart(width, 200, "no contributor data yet", theme);
    }

    let commit_counts: Vec<i64> = sorted.iter().map(|a| a.commits).collect();
    let bus_factor = compute_bus_factor(&commit_counts, total_commits);
    let risk = match bus_factor {
        0 => "Unavailable",
        1 => "Solo",
        2 => "High",
        3 => "Medium",
        _ => "Low",
    };
    let shown: Vec<&AuthorShare> = sorted
        .into_iter()
        .filter(|author| author.commits.saturating_mul(100) >= total_commits)
        .take(8)
        .collect();
    let height = 230u32 + (shown.len().saturating_sub(1) / 4) as u32 * 100;

    let mut people = String::new();
    for (i, a) in shown.iter().enumerate() {
        let column = i % 4;
        let row = i / 4;
        let x = padding + column as u32 * 198;
        let y = 110 + row as u32 * 102;
        let share = a.commits as f64 / total_commits as f64;
        let label = truncate_tail(&a.label, 18);
        let pct_text = format!("{:.1}%", share * 100.0);
        let tooltip = format!("{} · {} commits · {}", a.label, a.commits, pct_text);
        let avatar = a.avatar_url.as_ref().map_or_else(
            || {
                let initial = label.chars().next().unwrap_or('?').to_uppercase().to_string();
                format!(r#"<rect width="58" height="58" rx="7" class="avatar-fallback-bg" /><text class="avatar-fallback" x="29" y="37" text-anchor="middle">{}</text>"#, escape_xml(&initial))
            },
            |url| format!(r#"<image href="{}" width="58" height="58" clip-path="url(#owner-clip-{i})" preserveAspectRatio="xMidYMid slice" />"#, escape_xml(url)),
        );
        let content = format!(
            r##"<title>{tooltip}</title>
    <rect class="avatar-pixels" x="-5" y="-5" width="68" height="68" rx="9" />
    <clipPath id="owner-clip-{i}"><rect width="58" height="58" rx="7" /></clipPath>
    <g class="avatar-pos">{avatar}</g>
    <text class="person" x="70" y="23">{label}</text>
    <text class="person-meta" x="70" y="43">{commits} commits</text>
    <text class="person-share" x="70" y="61">{pct_text}</text>"##,
            tooltip = escape_xml(&tooltip),
            label = escape_xml(&label),
            commits = a.commits,
        );
        let linked = a.login.as_ref().map_or(content.clone(), |login| {
            format!(r##"<a class="person-link" href="https://github.com/{login}" target="_blank" rel="noopener">{content}</a>"##, login = escape_xml(login))
        });
        people.push_str(&format!(
            r##"<g transform="translate({x}, {y})" opacity="1">
  <animate class="motion" attributeName="opacity" from="0" to="1" dur="0.2s" begin="{begin:.2}s" fill="freeze" />
  {linked}
</g>
"##,
            x = x,
            y = y,
            begin = reveal_begin(i),
            linked = linked,
        ));
    }

    let footer_y = (height - 12) as f32;
    let right_x = (width - padding) as f32;
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" role="img" aria-label="Bus factor for {repo}">
  <style><![CDATA[
    .title {{ fill: {fg}; font: 600 18px ui-sans-serif, system-ui, sans-serif; }}
    .subtitle {{ fill: {muted}; font: 13px ui-sans-serif, system-ui, sans-serif; }}
    .bf-caption {{ fill: {muted}; font: 600 11px ui-sans-serif, system-ui, sans-serif; letter-spacing: 0.08em; }}
    .bf-number {{ fill: {accent}; font: 700 30px ui-sans-serif, system-ui, sans-serif; }}
    .avatar-pixels {{ fill: url(#gd-pixel-fill); stroke: {border}; stroke-width: 1; shape-rendering: crispEdges; }}
    .avatar-fallback-bg {{ fill: {track}; }}
    .avatar-fallback {{ fill: {fg}; font: 700 18px ui-monospace, SFMono-Regular, monospace; }}
    .person {{ fill: {fg}; font: 600 12px ui-sans-serif, system-ui, sans-serif; }}
    .person-meta {{ fill: {fg}; font: 600 10px ui-monospace, SFMono-Regular, monospace; }}
    .person-share {{ fill: {muted}; font: 10px ui-monospace, SFMono-Regular, monospace; }}
    .avatar-pos {{ transform-box: fill-box; transform-origin: center; transition: transform 180ms cubic-bezier(0.23, 1, 0.32, 1); }}
    .person-link:hover .avatar-pos {{ transform: scale(1.05); }}
    .person-link:hover .person {{ text-decoration: underline; }}
    .footer-link {{ fill: {muted}; font: 600 11px ui-sans-serif, system-ui, sans-serif; text-decoration: none; letter-spacing: 0.02em; }}
    .footer-link:hover {{ fill: {fg}; }}
    @media (prefers-reduced-motion: reduce) {{
      .motion {{ display: none; }}
      .avatar-pos {{ transition: none; }}
    }}
  ]]></style>
  <text class="title" x="{padding}" y="36">Bus factor</text>
  <text class="subtitle" x="{padding}" y="58">Contributors with at least 1% of attributed commits · {repo}</text>
  <text class="bf-caption" x="{right_x:.0}" y="34" text-anchor="end">OWNERSHIP RISK · FACTOR {bus_factor}</text>
  <text class="bf-number" x="{right_x:.0}" y="70" text-anchor="end">{risk}</text>
{people}
{footer}
</svg>"##,
        width = width,
        height = height,
        repo = escape_xml(repo),
        fg = theme.fg,
        muted = theme.muted,
        border = theme.border,
        track = theme.track,
        accent = theme.accent,
        padding = padding,
        bus_factor = bus_factor,
        risk = risk,
        right_x = right_x,
        people = people,
        footer = brand::footer_lockup(right_x, footer_y, theme),
    )
}

// Commit trend

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MonthCount {
    /// First day of the month bucket.
    pub month: NaiveDate,
    pub commits: i64,
}

/// Aggregate per-day commit counts into contiguous month buckets.
/// Months between the first and last observed commit with no activity
/// get an explicit zero bucket, so a trend line shows dormant stretches
/// instead of interpolating across them. Input need not be sorted.
pub fn bucket_months(days: &[DayCount]) -> Vec<MonthCount> {
    use std::collections::BTreeMap;
    let mut by_month: BTreeMap<NaiveDate, i64> = BTreeMap::new();
    for d in days {
        let entry = by_month.entry(month_of(d.day)).or_insert(0);
        *entry = entry.saturating_add(d.commits);
    }
    let (Some(first), Some(last)) = (
        by_month.keys().next().copied(),
        by_month.keys().next_back().copied(),
    ) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut cur = first;
    loop {
        out.push(MonthCount {
            month: cur,
            commits: by_month.get(&cur).copied().unwrap_or(0),
        });
        if cur >= last {
            break;
        }
        cur = next_month(cur);
    }
    out
}

fn month_of(d: NaiveDate) -> NaiveDate {
    // Day 1 of an existing date's (year, month) is always constructible.
    NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap_or(d)
}

fn next_month(d: NaiveDate) -> NaiveDate {
    let (y, m) = if d.month() == 12 {
        (d.year() + 1, 1)
    } else {
        (d.year(), d.month() + 1)
    };
    // (y, m, 1) is always a valid date for m in 1..=12.
    NaiveDate::from_ymd_opt(y, m, 1).unwrap_or(d)
}

/// Monthly commit counts as a line/area chart (all-time, month buckets)
/// with the peak month annotated. Styling mirrors the TODO/FIXME trend
/// chart, but in the accent color — commit volume is activity, not debt.
pub fn render_commit_trend(repo: &str, days: &[DayCount], theme: &Theme) -> String {
    crate::texture::decorate(render_commit_trend_inner(repo, days, theme), theme)
}

fn render_commit_trend_inner(repo: &str, days: &[DayCount], theme: &Theme) -> String {
    let months = bucket_months(days);
    let width = 1200u32;
    let height = 360u32;
    let pad_l = 56.0_f32;
    let pad_r = 40.0_f32;
    let pad_t = 70.0_f32;
    let pad_b = 50.0_f32;
    let plot_w = width as f32 - pad_l - pad_r;
    let plot_h = height as f32 - pad_t - pad_b;

    if months.is_empty() {
        return empty_chart(width, height, "no commit data yet", theme);
    }

    let y_max = months.iter().map(|m| m.commits).max().unwrap_or(1).max(1) as f32;
    let denom = months.len().saturating_sub(1).max(1) as f32;
    let x_at = |i: usize| -> f32 { pad_l + (i as f32 / denom) * plot_w };
    let y_at = |v: f32| pad_t + plot_h - (v / y_max) * plot_h;

    let mut path = String::new();
    for (i, m) in months.iter().enumerate() {
        let x = x_at(i);
        let y = y_at(m.commits as f32);
        if i == 0 {
            path.push_str(&format!("M {x:.1} {y:.1}"));
        } else {
            path.push_str(&format!(" L {x:.1} {y:.1}"));
        }
    }

    let baseline_y = y_at(0.0);
    let first_x = x_at(0);
    let last_x = x_at(months.len() - 1);
    let mut area = path.clone();
    area.push_str(&format!(
        " L {last_x:.1} {baseline_y:.1} L {first_x:.1} {baseline_y:.1} Z"
    ));

    let total: i64 = months.iter().map(|m| m.commits).sum();
    // `max_by_key` keeps the LAST max on ties — deterministic, and the
    // most recent peak is the more interesting one to annotate anyway.
    let (peak_idx, peak) = months
        .iter()
        .enumerate()
        .max_by_key(|(_, m)| m.commits)
        .expect("months is non-empty (checked above)");
    let peak_x = x_at(peak_idx);
    let peak_y = y_at(peak.commits as f32);
    let (peak_label_x, peak_anchor) = if peak_x > pad_l + plot_w - 90.0 {
        (peak_x - 10.0, "end")
    } else {
        (peak_x + 10.0, "start")
    };
    let peak_month = peak.month.format("%Y-%m").to_string();

    let mut y_ticks = String::new();
    for i in 0..=4 {
        let v = y_max * (i as f32 / 4.0);
        let y = y_at(v);
        y_ticks.push_str(&format!(
            r##"<line x1="{pad_l:.1}" y1="{y:.1}" x2="{x_end:.1}" y2="{y:.1}" stroke="{grid}" stroke-width="1" opacity="0.55" />
<text class="axis" x="{ax:.1}" y="{ty:.1}" text-anchor="end">{v}</text>
"##,
            pad_l = pad_l,
            y = y,
            x_end = pad_l + plot_w,
            ax = pad_l - 8.0,
            ty = y + 4.0,
            v = v.round() as i64,
            grid = theme.grid,
        ));
    }

    let footer_y = (height - 12) as f32;
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" role="img" aria-label="Monthly commit trend for {repo}">
  <style><![CDATA[
    .title {{ fill: {fg}; font: 600 18px ui-sans-serif, system-ui, sans-serif; }}
    .subtitle {{ fill: {muted}; font: 13px ui-sans-serif, system-ui, sans-serif; }}
    .axis {{ fill: {muted}; font: 11px ui-sans-serif, system-ui, sans-serif; }}
    .peak-label {{ fill: {accent}; font: 600 12px ui-sans-serif, system-ui, sans-serif; }}
    .footer-link {{ fill: {muted}; font: 600 11px ui-sans-serif, system-ui, sans-serif; text-decoration: none; letter-spacing: 0.02em; }}
    .footer-link:hover {{ fill: {fg}; }}
    @media (prefers-reduced-motion: reduce) {{
      .motion {{ display: none; }}
    }}
  ]]></style>
  <text class="title" x="{pad_l:.0}" y="34">{repo}</text>
  <text class="subtitle" x="{pad_l:.0}" y="56">Commits per month · {total} total · peak {peak_commits} in {peak_month}</text>
  {y_ticks}
  <g opacity="1">
    <animate class="motion" attributeName="opacity" from="0" to="1" dur="0.2s" begin="0s" fill="freeze" />
    <path d="{area}" fill="url(#gd-pixel-fill)" opacity="0.94" />
    <path d="{path}" fill="none" stroke="{accent}" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" />
    <circle cx="{peak_x:.1}" cy="{peak_y:.1}" r="5" fill="{accent}" />
    <text class="peak-label" x="{peak_label_x:.1}" y="{peak_label_y:.1}" text-anchor="{peak_anchor}">{peak_commits}</text>
  </g>
{footer}
</svg>"##,
        width = width,
        height = height,
        repo = escape_xml(repo),
        fg = theme.fg,
        muted = theme.muted,
        accent = theme.accent,
        pad_l = pad_l,
        total = humanize(total),
        peak_commits = peak.commits,
        peak_month = peak_month,
        y_ticks = y_ticks,
        area = area,
        path = path,
        peak_x = peak_x,
        peak_y = peak_y,
        peak_label_x = peak_label_x,
        peak_label_y = peak_y + 4.0,
        peak_anchor = peak_anchor,
        footer = brand::footer_lockup((width as f32) - 24.0, footer_y, theme),
    )
}

fn empty_chart(width: u32, height: u32, message: &str, theme: &Theme) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" role="img" aria-label="{message}">
  <style><![CDATA[
    .footer-link {{ fill: {muted}; font: 600 11px ui-sans-serif, system-ui, sans-serif; text-decoration: none; letter-spacing: 0.02em; }}
  ]]></style>
  <text x="50%" y="50%" text-anchor="middle" fill="{muted}"
        font-family="ui-sans-serif, system-ui, sans-serif" font-size="14">{message}</text>
{footer}
</svg>"##,
        width = width,
        height = height,
        muted = theme.muted,
        message = escape_xml(message),
        footer = brand::footer_lockup(width as f32 - 24.0, height as f32 - 12.0, theme),
    )
}

fn truncate_tail(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let tail: String = s
        .chars()
        .rev()
        .take(max - 1)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("…{tail}")
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme;

    fn without_smil(svg: &str) -> String {
        svg.lines()
            .filter(|line| !line.contains("<animate"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn every_repo_chart_surface_embeds_the_logo() {
        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 1, 7).unwrap();
        let charts = [
            render_bug_magnets("o/r", &[], &theme::LIGHT),
            render_top_changed("o/r", &[], &theme::LIGHT),
            render_heatmap("o/r", "Commit activity", start, end, &[], &theme::LIGHT),
            render_contributors("o/r", &[], &theme::LIGHT),
            render_languages("o/r", &[], &theme::LIGHT),
            render_todo_trend("o/r", &[], &theme::LIGHT),
            render_bus_factor("o/r", &[], 0, &theme::LIGHT),
            render_commit_trend("o/r", &[], &theme::LIGHT),
        ];

        for svg in charts {
            assert!(svg.contains("data-gitdebt-logo=\"true\""));
            assert!(!svg.contains("<image"));
        }
    }

    #[test]
    fn truncate_keeps_tail() {
        assert_eq!(truncate_tail("short", 10), "short");
        let long = "src/components/very/long/path/to/component.tsx";
        let t = truncate_tail(long, 20);
        assert!(t.starts_with('…'));
        assert!(t.ends_with("component.tsx"));
        assert_eq!(t.chars().count(), 20);
    }

    #[test]
    fn bug_magnets_renders_paths_links_and_baked_colors() {
        let rows = vec![
            FileRow {
                path: "src/auth.rs".into(),
                count: 47,
            },
            FileRow {
                path: "src/db.rs".into(),
                count: 23,
            },
        ];
        let svg = render_bug_magnets("foo/bar", &rows, &theme::LIGHT);
        assert!(svg.contains("Bug-magnet"));
        assert!(svg.contains("src/auth.rs"));
        assert!(svg.contains("47"));
        assert!(svg.contains("<animate"));
        assert!(svg.contains("gitdebt.com"));
        assert!(svg.contains("https://github.com/foo/bar/blob/HEAD/src/auth.rs"));
        // Concrete light-theme color baked in.
        assert!(svg.contains(theme::LIGHT.fg));
        // No leftover CSS variable references — those don't survive `<img>` rendering.
        assert!(!svg.contains("var(--"));
    }

    #[test]
    fn dark_theme_bakes_dark_colors() {
        let rows = vec![FileRow {
            path: "x".into(),
            count: 1,
        }];
        let svg = render_bug_magnets("a/b", &rows, &theme::DARK);
        // Dark theme's bar color and fg both present.
        assert!(svg.contains(theme::DARK.fg));
        assert!(svg.contains(theme::DARK.bug));
        // Light theme's *primary* foreground must not be the chart fg.
        // (We can't blanket-assert `#0f172a` is absent — contrast_on()
        //  legitimately returns it as text on light-coloured backgrounds.)
        assert!(!svg.contains(&format!("fill: {};", theme::LIGHT.fg)));
        assert!(!svg.contains("var(--"));
    }

    #[test]
    fn count_placement_inside_for_long_bars() {
        let (x, anchor, _) = count_placement(100.0, 200.0, "12k", "#ef4444", "#94a3b8");
        assert_eq!(anchor, "end");
        assert!(x < 300.0);
    }

    #[test]
    fn count_placement_outside_for_short_bars() {
        let (_x, anchor, _) = count_placement(100.0, 5.0, "12k", "#ef4444", "#94a3b8");
        assert_eq!(anchor, "start");
    }

    #[test]
    fn heatmap_renders_cells() {
        let days = vec![
            DayCount {
                day: NaiveDate::from_ymd_opt(2026, 3, 15).unwrap(),
                commits: 3,
            },
            DayCount {
                day: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
                commits: 12,
            },
        ];
        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();
        let svg = render_heatmap(
            "foo/bar",
            "Commits in 2026",
            start,
            end,
            &days,
            &theme::LIGHT,
        );
        assert!(svg.contains("class=\"cell"));
        assert!(svg.contains("Commits in 2026"));
        assert!(svg.contains("Mon"));
        assert!(svg.contains("data-gitdebt-logo=\"true\""));
        assert!(svg.contains(">gitdebt</text>"));
        assert!(svg.contains("text-anchor=\"end\""));
    }

    /// The commit-activity heatmap used to be the one repo-stat chart with
    /// no dither at all — a flat five-step gray ramp. It must now carry the
    /// same ordered-dither contract as everything else: one ink, density
    /// tiers, alpha-only.
    #[test]
    fn commit_activity_heatmap_is_dithered_one_ink_alpha_only() {
        let mut days = Vec::new();
        for (i, commits) in [1i64, 3, 9, 40, 0, 7, 22].into_iter().enumerate() {
            days.push(DayCount {
                day: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()
                    + chrono::Duration::days(i as i64),
                commits,
            });
        }
        let start = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 3, 7).unwrap();
        for theme in [&theme::LIGHT, &theme::DARK] {
            let svg = render_heatmap("foo/bar", "Commit activity", start, end, &days, theme);
            assert!(svg.contains("data-gitdebt-heat-defs=\"true\""));
            assert!(svg.contains("class=\"heat-ink\""));
            assert!(svg.contains("shape-rendering=\"crispEdges\""));
            // Every referenced tier must have a matching def, and every def
            // must be inked with the SAME single color.
            for tier in HEAT_TIERS.iter().skip(1) {
                assert!(
                    svg.contains(&format!("id=\"gd-heat-t{tier}\"")),
                    "missing tier {tier} def"
                );
            }
            assert!(svg.contains("fill=\"url(#gd-heat-t13)\""));
            assert!(!svg.contains(theme.heat_2), "flat gray ramp must be gone");
            // Alpha-only modulation across the levels.
            for alpha in HEAT_ALPHA.iter().skip(1) {
                assert!(
                    svg.contains(&format!("fill-opacity=\"{alpha}\"")),
                    "missing level alpha {alpha}"
                );
            }
            assert_eq!(
                svg,
                render_heatmap("foo/bar", "Commit activity", start, end, &days, theme)
            );
        }
    }

    #[test]
    fn heat_ink_is_empty_for_dormant_days() {
        assert_eq!(heat_ink(0.0, 0.0, 14.0, 0), "");
        assert!(heat_ink(0.0, 0.0, 14.0, 1).contains("gd-heat-t2"));
        // Out-of-range levels clamp to the top tier instead of panicking.
        assert!(heat_ink(0.0, 0.0, 14.0, 99).contains("gd-heat-t13"));
        assert!(heat_ink(0.0, 0.0, 14.0, 4).contains("pointer-events=\"none\""));
    }

    #[test]
    fn every_repo_chart_carries_dither_markup() {
        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 1, 7).unwrap();
        let days = vec![DayCount {
            day: NaiveDate::from_ymd_opt(2026, 1, 3).unwrap(),
            commits: 5,
        }];
        let files = vec![FileRow {
            path: "src/lib.rs".into(),
            count: 12,
        }];
        let charts: [(&str, String); 8] = [
            (
                "bug magnets",
                render_bug_magnets("o/r", &files, &theme::DARK),
            ),
            (
                "top changed",
                render_top_changed("o/r", &files, &theme::DARK),
            ),
            (
                "commit activity",
                render_heatmap("o/r", "Commit activity", start, end, &days, &theme::DARK),
            ),
            (
                "contributors",
                render_contributors(
                    "o/r",
                    &[ContributorRow {
                        login: Some("a".into()),
                        name: "a".into(),
                        avatar_url: None,
                        commits: 3,
                    }],
                    &theme::DARK,
                ),
            ),
            (
                "languages",
                render_languages(
                    "o/r",
                    &[LanguageBar {
                        language: "Rust".into(),
                        files: 2,
                        lines_code: 30,
                        lines_blank: 3,
                        lines_comment: 1,
                    }],
                    &theme::DARK,
                ),
            ),
            (
                "todo trend",
                render_todo_trend(
                    "o/r",
                    &[
                        TodoPoint {
                            day: start,
                            running_total: 1,
                        },
                        TodoPoint {
                            day: end,
                            running_total: 4,
                        },
                    ],
                    &theme::DARK,
                ),
            ),
            (
                "bus factor",
                render_bus_factor(
                    "o/r",
                    &[AuthorShare {
                        label: "a".into(),
                        login: None,
                        avatar_url: None,
                        commits: 5,
                    }],
                    5,
                    &theme::DARK,
                ),
            ),
            (
                "commit trend",
                render_commit_trend("o/r", &days, &theme::DARK),
            ),
        ];
        for (name, svg) in charts {
            let dithered = svg.contains("url(#gd-pixel-fill)")
                || svg.contains("url(#gd-heat-t")
                || svg.contains("url(#gd-lang");
            assert!(dithered, "{name} renders its data without dither");
            // The shared pixel-grain field is on every surface.
            assert!(
                svg.contains("data-gitdebt-texture=\"true\""),
                "{name} is missing the shared texture field"
            );
        }
    }

    #[test]
    fn empty_charts_keep_the_shared_texture_field() {
        // The empty state used to paint an opaque background rect on top of
        // the texture field, so "no data" looked like a different product.
        let svg = render_commit_trend("o/r", &[], &theme::DARK);
        assert!(svg.contains("no commit data yet"));
        assert!(svg.contains("data-gitdebt-texture=\"true\""));
        let texture = svg.find("data-gitdebt-texture").expect("texture");
        assert!(texture < svg.find("<text x=\"50%\"").expect("message"));
        assert!(!svg.contains(&format!(
            "<rect width=\"1200\" height=\"360\" fill=\"{}\" />",
            theme::DARK.bg
        )));
    }

    #[test]
    fn heatmap_rolling_52_weeks_renders() {
        let end = NaiveDate::from_ymd_opt(2026, 5, 7).unwrap();
        let start = end - chrono::Duration::days(51 * 7);
        let svg = render_heatmap(
            "foo/bar",
            "Commits in the last 52 weeks",
            start,
            end,
            &[],
            &theme::LIGHT,
        );
        assert!(svg.contains("Commits in the last 52 weeks"));
        let cells = svg.matches("class=\"cell").count();
        assert!(
            (360..=371).contains(&cells),
            "expected ~364 cells, got {cells}"
        );
    }

    #[test]
    fn contributors_are_minimal_overlapping_linked_avatars() {
        let rows = vec![ContributorRow {
            login: Some("zhom".into()),
            name: "zhom".into(),
            avatar_url: Some("https://avatars.githubusercontent.com/u/1?s=80".into()),
            commits: 100,
        }];
        let svg = render_contributors("foo/bar", &rows, &theme::LIGHT);
        assert!(svg.contains("href=\"https://github.com/zhom"));
        assert!(svg.contains("<image"));
        assert!(svg.contains("avatar-pixels"));
        assert!(!svg.contains("100 commits"));
        assert!(!svg.contains("class=\"commits\""));
        assert!(!svg.contains("class=\"share\""));
        assert!(!svg.contains("animateTransform"));
        assert!(svg.contains("translateY(-10px) scale(1.08)"));
        assert!(svg.contains("(hover: hover) and (pointer: fine)"));
        assert!(!svg.contains("drop-shadow"));
    }

    #[test]
    fn todo_trend_handles_empty() {
        let svg = render_todo_trend("foo/bar", &[], &theme::LIGHT);
        assert!(svg.contains("no TODO/FIXME data yet"));
    }

    #[test]
    fn todo_trend_renders_path_and_marker() {
        let pts = vec![
            TodoPoint {
                day: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                running_total: 10,
            },
            TodoPoint {
                day: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
                running_total: 50,
            },
            TodoPoint {
                day: NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
                running_total: 30,
            },
        ];
        let svg = render_todo_trend("foo/bar", &pts, &theme::LIGHT);
        assert!(!svg.contains("attributeName=\"r\""));
        assert!(!svg.contains("stroke-dashoffset"));
        assert!(svg.contains("dur=\"0.2s\""));
    }

    #[test]
    fn motion_is_grouped_short_and_reduced_motion_safe() {
        let rows = vec![
            FileRow {
                path: "a.rs".into(),
                count: 3,
            },
            FileRow {
                path: "b.rs".into(),
                count: 2,
            },
            FileRow {
                path: "c.rs".into(),
                count: 1,
            },
            FileRow {
                path: "d.rs".into(),
                count: 1,
            },
        ];
        let bars = render_bug_magnets("foo/bar", &rows, &theme::LIGHT);
        assert!(!bars.contains("attributeName=\"width\""));
        assert!(bars.contains("begin=\"0.00s\""));
        assert!(bars.contains("begin=\"0.04s\""));
        assert!(bars.contains("begin=\"0.08s\""));
        assert!(!bars.contains("begin=\"0.12s\""));
        assert!(bars.contains("prefers-reduced-motion: reduce"));

        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let heat = render_heatmap(
            "foo/bar",
            "Commits",
            start,
            start + chrono::Duration::days(30),
            &[],
            &theme::LIGHT,
        );
        assert_eq!(heat.matches("<animate ").count(), 1);
        assert!(heat.contains("class=\"heat-cells\""));
        assert!(heat.contains("prefers-reduced-motion: reduce"));
    }

    #[test]
    fn bus_factor_empty_is_zero() {
        assert_eq!(compute_bus_factor(&[], 0), 0);
        assert_eq!(compute_bus_factor(&[], 100), 0);
        assert_eq!(compute_bus_factor(&[0, 0], 0), 0);
        // Non-positive entries are ignored even with a bogus total.
        assert_eq!(compute_bus_factor(&[0, -3], 10), 0);
    }

    #[test]
    fn bus_factor_single_author_is_one() {
        assert_eq!(compute_bus_factor(&[42], 42), 1);
    }

    #[test]
    fn bus_factor_dominant_author() {
        // 60/30/10 → the top author alone exceeds half.
        assert_eq!(compute_bus_factor(&[60, 30, 10], 100), 1);
    }

    #[test]
    fn bus_factor_even_split_needs_strict_majority() {
        // 4 × 25: two authors reach exactly 50%, which does NOT exceed half.
        assert_eq!(compute_bus_factor(&[25, 25, 25, 25], 100), 3);
        // 50/50 needs both.
        assert_eq!(compute_bus_factor(&[50, 50], 100), 2);
    }

    #[test]
    fn bus_factor_input_order_does_not_matter() {
        assert_eq!(compute_bus_factor(&[1, 100, 2], 103), 1);
    }

    #[test]
    fn bus_factor_truncated_prefix_is_lower_bound() {
        // Top-2 prefix sums to 20 of 100 → can't cross half; report the
        // prefix length as a lower bound.
        assert_eq!(compute_bus_factor(&[10, 10], 100), 2);
    }

    #[test]
    fn bus_factor_chart_renders_avatar_ownership_risk() {
        let authors = vec![
            AuthorShare {
                label: "alice".into(),
                login: Some("alice".into()),
                avatar_url: Some("https://avatars.githubusercontent.com/u/1".into()),
                commits: 60,
            },
            AuthorShare {
                label: "bob".into(),
                login: None,
                avatar_url: None,
                commits: 30,
            },
            AuthorShare {
                label: "carol".into(),
                login: None,
                avatar_url: None,
                commits: 10,
            },
        ];
        let svg = render_bus_factor("foo/bar", &authors, 100, &theme::LIGHT);
        assert!(svg.contains("Bus factor"));
        assert!(svg.contains("OWNERSHIP RISK · FACTOR 1"));
        assert!(svg.contains(">Solo</text>"));
        assert!(svg.contains("alice"));
        assert!(svg.contains("60.0%"));
        assert!(svg.contains("https://github.com/alice"));
        assert!(svg.contains("<image"));
        assert!(svg.contains(theme::LIGHT.accent));
        assert!(svg.contains("gitdebt.com"));
        assert!(!svg.contains("var(--"));
    }

    #[test]
    fn bus_factor_hides_people_below_one_percent() {
        let svg = render_bus_factor(
            "foo/bar",
            &[
                AuthorShare {
                    label: "visible".into(),
                    login: None,
                    avatar_url: None,
                    commits: 999,
                },
                AuthorShare {
                    label: "tiny".into(),
                    login: None,
                    avatar_url: None,
                    commits: 1,
                },
            ],
            1_000,
            &theme::LIGHT,
        );
        assert!(svg.contains("visible"));
        assert!(!svg.contains(">tiny</text>"));
    }

    #[test]
    fn bus_factor_chart_empty_state() {
        let svg = render_bus_factor("foo/bar", &[], 0, &theme::LIGHT);
        assert!(svg.contains("no contributor data yet"));
        // Zero-commit authors also count as empty.
        let svg = render_bus_factor(
            "foo/bar",
            &[AuthorShare {
                label: "x".into(),
                login: None,
                avatar_url: None,
                commits: 0,
            }],
            0,
            &theme::LIGHT,
        );
        assert!(svg.contains("no contributor data yet"));
    }

    #[test]
    fn bucket_months_empty() {
        assert!(bucket_months(&[]).is_empty());
    }

    #[test]
    fn bucket_months_sums_within_month_and_fills_gaps() {
        let d = |y, m, day, c| DayCount {
            day: NaiveDate::from_ymd_opt(y, m, day).unwrap(),
            commits: c,
        };
        // Unsorted on purpose; Feb and Mar are dormant.
        let days = vec![d(2026, 4, 2, 5), d(2026, 1, 5, 3), d(2026, 1, 20, 4)];
        let months = bucket_months(&days);
        assert_eq!(months.len(), 4);
        assert_eq!(
            months[0],
            MonthCount {
                month: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                commits: 7,
            }
        );
        assert_eq!(months[1].commits, 0);
        assert_eq!(months[2].commits, 0);
        assert_eq!(
            months[3],
            MonthCount {
                month: NaiveDate::from_ymd_opt(2026, 4, 1).unwrap(),
                commits: 5,
            }
        );
    }

    #[test]
    fn bucket_months_crosses_year_boundary() {
        let d = |y, m, day, c| DayCount {
            day: NaiveDate::from_ymd_opt(y, m, day).unwrap(),
            commits: c,
        };
        let days = vec![d(2025, 12, 31, 1), d(2026, 1, 1, 2)];
        let months = bucket_months(&days);
        assert_eq!(months.len(), 2);
        assert_eq!(
            months[0].month,
            NaiveDate::from_ymd_opt(2025, 12, 1).unwrap()
        );
        assert_eq!(
            months[1].month,
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()
        );
    }

    #[test]
    fn commit_trend_handles_empty() {
        let svg = render_commit_trend("foo/bar", &[], &theme::LIGHT);
        assert!(svg.contains("no commit data yet"));
    }

    #[test]
    fn commit_trend_renders_line_and_peak() {
        let d = |y, m, day, c| DayCount {
            day: NaiveDate::from_ymd_opt(y, m, day).unwrap(),
            commits: c,
        };
        let days = vec![d(2025, 11, 3, 4), d(2026, 2, 10, 9), d(2026, 2, 11, 1)];
        let svg = render_commit_trend("foo/bar", &days, &theme::LIGHT);
        assert!(svg.contains("Commits per month"));
        // Feb 2026 sums to 10 and is the peak.
        assert!(svg.contains("peak 10 in 2026-02"));
        assert!(!svg.contains("stroke-dashoffset"));
        assert!(svg.contains("dur=\"0.2s\""));
        assert!(svg.contains(theme::LIGHT.accent));
        assert!(!svg.contains("var(--"));
    }

    #[test]
    fn commit_trend_single_month_does_not_panic() {
        let days = vec![DayCount {
            day: NaiveDate::from_ymd_opt(2026, 3, 3).unwrap(),
            commits: 2,
        }];
        let svg = render_commit_trend("foo/bar", &days, &theme::LIGHT);
        assert!(svg.contains("peak 2 in 2026-03"));
    }

    #[test]
    fn new_charts_are_bytes_deterministic() {
        let authors = vec![AuthorShare {
            label: "a".into(),
            login: None,
            avatar_url: None,
            commits: 5,
        }];
        assert_eq!(
            render_bus_factor("x/y", &authors, 5, &theme::DARK),
            render_bus_factor("x/y", &authors, 5, &theme::DARK),
        );
        let days = vec![DayCount {
            day: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            commits: 1,
        }];
        assert_eq!(
            render_commit_trend("x/y", &days, &theme::DARK),
            render_commit_trend("x/y", &days, &theme::DARK),
        );
    }

    #[test]
    fn languages_shows_total_and_code_separately() {
        let rows = vec![
            LanguageBar {
                language: "Rust".into(),
                files: 88,
                lines_code: 45_000,
                lines_blank: 8_000,
                lines_comment: 2_500,
            },
            LanguageBar {
                language: "TypeScript".into(),
                files: 43,
                lines_code: 5_400,
                lines_blank: 800,
                lines_comment: 200,
            },
        ];
        let svg = render_languages("foo/bar", &rows, &theme::LIGHT);
        // Subtitle has both "lines" and "code" totals.
        assert!(svg.contains(" lines · "));
        assert!(svg.contains(" code · "));
        // Per-row meta has files + code.
        assert!(svg.contains("88 files · "));
        // Each language gets its OWN ink, on the dot, the dither ladder,
        // and the value contour. Labels stay themed.
        let rust = language_color("Rust", &theme::LIGHT);
        let ts = language_color("TypeScript", &theme::LIGHT);
        assert_ne!(rust, ts);
        assert!(svg.contains(&format!(
            r##"<circle cx="6" cy="9.0" r="6" fill="{rust}" />"##
        )));
        assert!(svg.contains(r##"id="gd-lang0-t11""##));
        assert!(svg.contains(r##"id="gd-lang1-t11""##));
        assert!(svg.contains(&format!(
            "<rect x=\"1\" y=\"1\" width=\"1\" height=\"1\" fill=\"{rust}\" />"
        )));
        assert!(svg.contains(&format!(
            "<rect x=\"1\" y=\"1\" width=\"1\" height=\"1\" fill=\"{ts}\" />"
        )));
        assert!(svg.contains(r##"fill="url(#gd-lang1-t11)""##));
        assert!(svg.contains(&format!(r##"fill="none" stroke="{ts}""##)));
        assert!(svg.contains(".bar-label { fill: #0a0a0a;"));
        // Alpha-only: the two tiers differ in opacity, never in shade.
        assert!(svg.contains(r##"fill-opacity="0.2""##));
        assert!(svg.contains(r##"fill-opacity="0.92""##));
    }

    #[test]
    fn language_colors_are_deterministic_stable_and_distinct() {
        // Same input → same bytes, in both themes.
        for theme in [&theme::LIGHT, &theme::DARK] {
            assert_eq!(language_color("Rust", theme), language_color("Rust", theme));
            assert_eq!(
                language_color("Made-up lang", theme),
                language_color("Made-up lang", theme)
            );
        }
        // The conventional hue survives; only lightness is corrected.
        assert_eq!(conventional_language_color("Rust"), Some("#dea584"));
        assert_eq!(conventional_language_color("Go"), Some("#00add8"));
        assert_eq!(conventional_language_color("Config"), Some("#8b8b8b"));
        assert_eq!(conventional_language_color("Unknown language"), None);

        // Every common language resolves to its own color, per theme.
        let langs = [
            "Rust",
            "TypeScript",
            "JavaScript",
            "Python",
            "Go",
            "C",
            "C++",
            "C#",
            "Java",
            "Kotlin",
            "Swift",
            "Ruby",
            "PHP",
            "Shell",
            "HTML",
            "CSS",
            "Vue",
            "Svelte",
            "Dart",
            "Scala",
            "Elixir",
            "Haskell",
            "Lua",
            "Zig",
            "Nix",
            "Markdown",
            "JSON",
            "YAML",
            "TOML",
            "SQL",
            "Dockerfile",
        ];
        for theme in [&theme::LIGHT, &theme::DARK] {
            let mut seen: std::collections::BTreeMap<String, &str> = Default::default();
            for name in langs {
                let color = language_color(name, theme);
                assert_eq!(color.len(), 7, "{name} must render as #rrggbb");
                if let Some(other) = seen.insert(color.clone(), name) {
                    panic!(
                        "{name} collides with {other} at {color} (dark={})",
                        theme.dark
                    );
                }
            }
        }
    }

    #[test]
    fn language_colors_stay_legible_on_both_canvases() {
        // Near-black dark canvas: nothing may sink below the readable
        // floor (Lua's #000080 and JSON's #292929 are the worst cases).
        for name in ["Lua", "JSON", "PowerShell", "Ruby", "C", "Less", "Made-up"] {
            let dark = parse_hex(&language_color(name, &theme::DARK)).expect("hex");
            assert!(
                luma(dark) >= 0.29,
                "{name} is too dark on the dark canvas: {:?}",
                dark
            );
            // Light theme: nothing may wash out (JavaScript, OCaml, Shell).
            let light = parse_hex(&language_color(name, &theme::LIGHT)).expect("hex");
            assert!(
                luma(light) <= 0.56,
                "{name} is too light on the light canvas: {:?}",
                light
            );
        }
        for name in ["JavaScript", "OCaml", "Shell", "SVG"] {
            let light = parse_hex(&language_color(name, &theme::LIGHT)).expect("hex");
            assert!(luma(light) <= 0.56, "{name} washes out on white");
        }
    }

    #[test]
    fn unnamed_languages_get_stable_hashed_hues() {
        let a = language_color("Whatsit", &theme::DARK);
        let b = language_color("Thingamajig", &theme::DARK);
        assert_ne!(a, b, "distinct unknown languages must not share a hue");
        assert_eq!(a, language_color("Whatsit", &theme::DARK));
        // `Config` is the documented "no single language" bucket and keeps
        // the achromatic gray rather than picking up a hue.
        assert_eq!(
            conventional_language_color("Config"),
            Some(NEUTRAL_LANGUAGE_COLOR)
        );

        let rows = vec![LanguageBar {
            language: "Unknown language".into(),
            files: 1,
            lines_code: 12,
            lines_blank: 1,
            lines_comment: 2,
        }];
        let svg = render_languages("foo/bar", &rows, &theme::DARK);
        assert!(svg.contains(&language_color("Unknown language", &theme::DARK)));
        assert!(svg.contains(".bar-label { fill: #fafafa;"));
    }

    #[test]
    fn languages_labels_tree_census_as_files_not_lines() {
        let rows = vec![LanguageBar {
            language: "C".into(),
            files: 42_000,
            lines_code: 0,
            lines_blank: 0,
            lines_comment: 0,
        }];
        let svg = render_languages("torvalds/linux", &rows, &theme::LIGHT);
        assert!(svg.contains("42.0k files in 1 languages · current HEAD tree"));
        assert!(svg.contains("42000 files in current HEAD"));
        assert!(!svg.contains("42.0k lines"));
    }

    #[test]
    fn charts_keep_their_finished_frame_without_smil() {
        let files = vec![FileRow {
            path: "src/lib.rs".into(),
            count: 12,
        }];
        let bars = without_smil(&render_bug_magnets("foo/bar", &files, &theme::LIGHT));
        assert!(!bars.contains("width=\"0\""));
        assert!(!bars.contains("<g opacity=\"0\""));

        let days = vec![DayCount {
            day: NaiveDate::from_ymd_opt(2026, 3, 3).unwrap(),
            commits: 4,
        }];
        let heatmap = without_smil(&render_heatmap(
            "foo/bar",
            "Commits",
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 7).unwrap(),
            &days,
            &theme::LIGHT,
        ));
        assert!(!heatmap.contains("class=\"cell\"") || heatmap.contains("opacity=\"1\""));
        assert!(!heatmap.contains("<g opacity=\"0\""));

        let contributors = without_smil(&render_contributors(
            "foo/bar",
            &[ContributorRow {
                login: Some("alice".into()),
                name: "Alice".into(),
                avatar_url: None,
                commits: 4,
            }],
            &theme::LIGHT,
        ));
        assert!(contributors.contains("class=\"avatar-pos\""));
        assert!(contributors.contains("opacity=\"1\""));

        let languages = without_smil(&render_languages(
            "foo/bar",
            &[LanguageBar {
                language: "Rust".into(),
                files: 1,
                lines_code: 100,
                lines_blank: 10,
                lines_comment: 5,
            }],
            &theme::LIGHT,
        ));
        assert!(!languages.contains("width=\"0\""));
        assert!(!languages.contains("<g opacity=\"0\""));

        let authors = without_smil(&render_bus_factor(
            "foo/bar",
            &[AuthorShare {
                label: "alice".into(),
                login: None,
                avatar_url: None,
                commits: 4,
            }],
            4,
            &theme::LIGHT,
        ));
        assert!(!authors.contains("width=\"0\""));
        assert!(!authors.contains("<g opacity=\"0\""));

        let todos = without_smil(&render_todo_trend(
            "foo/bar",
            &[
                TodoPoint {
                    day: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                    running_total: 1,
                },
                TodoPoint {
                    day: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
                    running_total: 2,
                },
            ],
            &theme::LIGHT,
        ));
        assert!(!todos.contains("stroke-dashoffset"));
        assert!(todos.contains("fill=\"url(#gd-pixel-fill)\" opacity=\"0.94\""));
        assert!(todos.contains(" r=\"5\""));

        let commits = without_smil(&render_commit_trend(
            "foo/bar",
            &[
                DayCount {
                    day: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                    commits: 1,
                },
                DayCount {
                    day: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
                    commits: 2,
                },
            ],
            &theme::LIGHT,
        ));
        assert!(!commits.contains("stroke-dashoffset"));
        assert!(commits.contains("fill=\"url(#gd-pixel-fill)\" opacity=\"0.94\""));
        assert!(commits.contains(" r=\"5\""));
    }
}
