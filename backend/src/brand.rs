//! Inline gitdebt brand primitives for generated SVG assets.
//!
//! Shareable images cannot reference `/logo.svg`: GitHub's image proxy and
//! social rasterization both require a self-contained document. These helpers
//! inline the canonical repository logo, recolor it with concrete theme
//! values, and keep the output deterministic.

use crate::theme::Theme;

const LOGO_SVG: &str = include_str!("../../assets/gitdebt-logo.svg");

/// Render the canonical robot mark at an arbitrary SVG position.
///
/// `ink` colors the outline-free robot glyph. `knockout` is retained in the
/// signature for stable call sites; the canonical mark no longer has a tile.
pub fn logo_mark(x: f32, y: f32, size: f32, ink: &str, knockout: &str) -> String {
    let scale = size / 512.0;
    let body_start = LOGO_SVG.find('>').expect("logo opening tag") + 1;
    let body_end = LOGO_SVG.rfind("</svg>").expect("logo closing tag");
    let body = LOGO_SVG[body_start..body_end]
        .replace("fill=\"#000\"", &format!("fill=\"{ink}\""))
        .replace("fill=\"#fff\"", &format!("fill=\"{knockout}\""));

    format!(
        "  <g data-gitdebt-logo=\"true\" aria-label=\"gitdebt\" transform=\"translate({x:.1} {y:.1}) scale({scale:.5})\">{body}</g>\n"
    )
}

/// Theme-aware version of [`logo_mark`].
pub fn themed_logo_mark(x: f32, y: f32, size: f32, theme: &Theme) -> String {
    logo_mark(x, y, size, theme.fg, theme.bg)
}

/// Grid resolution of [`MARK_BITMAP`] (square).
pub(crate) const MARK_GRID: usize = 14;

/// Favicon-grade rendition of the canonical robot, on a 14×14 pixel grid.
///
/// The 512px artwork carries a rotated rounded-rect head with an outlined
/// screen, two X-shaped eyes, and a left ear tab. Below ~24px its outlines
/// land on sub-pixel boundaries and the whole glyph collapses into a
/// smudge, so compact surfaces get this hand-authored reduction instead:
/// the same three recognizable features, snapped to whole cells so a 14px
/// mark is exactly 1 device pixel per cell (2px at the standard 2× raster).
/// 14 rather than 12 cells because at 12 the eyes touch the head outline
/// and the glyph reads as noise.
///
/// `#` is ink, `.` is transparent. Row-major, top to bottom.
pub(crate) const MARK_BITMAP: [&str; MARK_GRID] = [
    "..............",
    "...##########.",
    "..#..........#",
    "..#..........#",
    "..#..........#",
    "..#.#.#..#.#.#",
    "###..#....#..#",
    "###.#.#..#.#.#",
    "###..........#",
    "..#..........#",
    "..#..........#",
    "..#..........#",
    "...##########.",
    "..............",
];

/// Horizontal runs of [`MARK_BITMAP`] as `(col, row, len)`. Merging runs
/// keeps the emitted markup small and, more importantly, removes the
/// hairline seams a per-cell rect grid shows at fractional cell sizes.
pub(crate) fn mark_runs() -> Vec<(usize, usize, usize)> {
    let mut runs = Vec::new();
    for (row, line) in MARK_BITMAP.iter().enumerate() {
        let cells: Vec<bool> = line.chars().map(|c| c == '#').collect();
        let mut col = 0usize;
        while col < cells.len() {
            if !cells[col] {
                col += 1;
                continue;
            }
            let start = col;
            while col < cells.len() && cells[col] {
                col += 1;
            }
            runs.push((start, row, col - start));
        }
    }
    runs
}

/// Small-size brand mark: the gitdebt robot as a solid, single-ink pixel
/// glyph (see [`MARK_BITMAP`]).
///
/// Solid fill on purpose — a dither pattern inside a 12px glyph samples
/// below one cell per feature and destroys the silhouette. The dither
/// system still surrounds the mark (panel washes, chart fills); the logo
/// itself stays crisp so it reads as the logo at badge scale, in either
/// theme, in SVG and after rasterization.
pub fn pixel_mark(x: f32, y: f32, size: f32, ink: &str) -> String {
    let cell = size / MARK_GRID as f32;
    let mut cells = String::new();
    for (col, row, len) in mark_runs() {
        cells.push_str(&format!(
            "<rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{cell:.2}\" />",
            col as f32 * cell,
            row as f32 * cell,
            len as f32 * cell,
        ));
    }
    format!(
        "  <g data-gitdebt-logo=\"true\" aria-label=\"gitdebt\" transform=\"translate({x:.1} {y:.1})\" fill=\"{ink}\" shape-rendering=\"crispEdges\">{cells}</g>\n"
    )
}

/// Compact bottom-right logo + wordmark used by chart footers.
///
/// The supplied coordinates are the right edge and text baseline, matching
/// the existing right-anchored footer labels. The glyph is large enough to
/// remain recognizable after GitHub's image proxy scales a README asset down.
pub fn footer_lockup(right_x: f32, baseline_y: f32, theme: &Theme) -> String {
    let mark_size = 18.0;
    let mark_x = right_x - 63.0;
    let mark_y = baseline_y - 15.0;
    format!(
        "  <a href=\"https://gitdebt.com\" target=\"_blank\" rel=\"noopener\" aria-label=\"gitdebt\">\n{}    <text class=\"footer-link\" x=\"{right_x:.1}\" y=\"{baseline_y:.1}\" text-anchor=\"end\" fill=\"{muted}\">gitdebt</text>\n  </a>\n",
        // The pixel-grid mark, not the full logo path: at 18px the detailed
        // artwork is still sub-pixel enough to read as a smudge.
        pixel_mark(mark_x, mark_y, mark_size, theme.muted),
        muted = theme.muted,
    )
}

/// Add a transparent full-surface link to the public site. The link sits
/// directly behind chart content so more specific links (contributors, files,
/// and the footer) remain clickable when an SVG is opened directly.
pub fn with_site_link(mut svg: String) -> String {
    if svg.contains("data-gitdebt-surface-link=\"true\"") {
        return svg;
    }
    let link = concat!(
        "  <a data-gitdebt-surface-link=\"true\" href=\"https://gitdebt.com\" ",
        "target=\"_blank\" rel=\"noopener\" aria-label=\"Open gitdebt.com\">",
        "<rect width=\"100%\" height=\"100%\" fill=\"#ffffff\" fill-opacity=\"0\" ",
        "pointer-events=\"all\" /></a>\n"
    );
    if let Some(index) = svg.find('>') {
        svg.insert_str(index + 1, link);
    }
    svg
}

/// Remove the embed-only footer from an SVG shown inside gitdebt itself.
/// The app already supplies navigation and attribution; README/media responses
/// keep the linked lockup and full-surface link unchanged.
pub fn without_embed_footer(mut svg: String) -> String {
    const OPEN: &str = "  <a href=\"https://gitdebt.com\" target=\"_blank\" rel=\"noopener\" aria-label=\"gitdebt\">";
    while let Some(start) = svg.find(OPEN) {
        let Some(relative_end) = svg[start..].find("  </a>") else {
            break;
        };
        let mut end = start + relative_end + "  </a>".len();
        if svg.as_bytes().get(end) == Some(&b'\n') {
            end += 1;
        }
        svg.replace_range(start..end, "");
    }
    svg
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK, LIGHT};

    #[test]
    fn logo_is_inline_deterministic_and_theme_aware() {
        let light = themed_logo_mark(10.0, 20.0, 16.0, &LIGHT);
        let dark = themed_logo_mark(10.0, 20.0, 16.0, &DARK);

        assert_eq!(light, themed_logo_mark(10.0, 20.0, 16.0, &LIGHT));
        assert!(light.contains("data-gitdebt-logo=\"true\""));
        assert!(light.contains("fill=\"#0a0a0a\""));
        assert!(!light.contains("<rect"));
        assert!(dark.contains("fill=\"#fafafa\""));
        assert!(!dark.contains("<rect"));
        assert!(!light.contains("<image"));
        assert!(!dark.contains("<image"));
    }

    #[test]
    fn pixel_mark_is_the_robot_not_a_uniform_chip() {
        let mark = pixel_mark(100.0, 8.0, 12.0, "#fafafa");
        assert_eq!(mark, pixel_mark(100.0, 8.0, 12.0, "#fafafa"));
        assert!(mark.contains("data-gitdebt-logo=\"true\""));
        // One ink for the whole glyph, carried on the group.
        assert_eq!(mark.matches("fill=").count(), 1);
        assert!(mark.contains("fill=\"#fafafa\""));
        assert!(mark.contains("shape-rendering=\"crispEdges\""));
        // Never the unreadable-at-small-size robot path data, and never a
        // solid square: the glyph must have holes.
        assert!(!mark.contains("M320.5 110.5"));
        let ink: usize = MARK_BITMAP
            .iter()
            .map(|row| row.chars().filter(|c| *c == '#').count())
            .sum();
        let total = MARK_GRID * MARK_GRID;
        assert!(
            (total / 6..total / 2).contains(&ink),
            "mark ink coverage must read as a glyph, got {ink}/{total}"
        );
        // Structure: a framed head with two eyes and a left ear tab.
        assert!(MARK_BITMAP[0].chars().all(|c| c == '.'));
        assert!(MARK_BITMAP[MARK_GRID - 1].chars().all(|c| c == '.'));
        assert_eq!(MARK_BITMAP[1].chars().filter(|c| *c == '#').count(), 10);
        for row in MARK_BITMAP.iter() {
            assert_eq!(row.chars().count(), MARK_GRID);
        }
    }

    #[test]
    fn mark_runs_merge_horizontal_neighbours() {
        let runs = mark_runs();
        // The head's top edge is one 10-cell run, not ten rects.
        assert!(runs.contains(&(3, 1, 10)));
        assert!(runs.contains(&(3, 12, 10)));
        // Runs are non-empty, in-bounds, and row-major.
        let mut previous = (0usize, 0usize);
        for (col, row, len) in &runs {
            assert!(*len > 0 && col + len <= MARK_GRID && *row < MARK_GRID);
            assert!((*row, *col) > previous || previous == (0, 0));
            previous = (*row, *col);
        }
    }

    #[test]
    fn footer_is_a_linked_lockup() {
        let footer = footer_lockup(844.0, 188.0, &LIGHT);
        assert!(footer.contains("https://gitdebt.com"));
        assert!(footer.contains("data-gitdebt-logo=\"true\""));
        assert!(footer.contains(">gitdebt</text>"));
    }

    #[test]
    fn site_link_is_full_surface_and_idempotent() {
        let linked = with_site_link("<svg><rect /></svg>".to_string());
        assert!(linked.contains("data-gitdebt-surface-link=\"true\""));
        assert!(linked.contains("href=\"https://gitdebt.com\""));
        assert!(
            linked.find("data-gitdebt-surface-link").unwrap() < linked.find("<rect />").unwrap()
        );
        assert_eq!(with_site_link(linked.clone()), linked);
    }

    #[test]
    fn app_surface_removes_embed_footer_only() {
        let chart = format!("<svg><rect />{}</svg>", footer_lockup(100.0, 90.0, &LIGHT));
        let app = without_embed_footer(chart);
        assert!(app.contains("<rect />"));
        assert!(!app.contains("data-gitdebt-logo"));
        assert!(!app.contains("href=\"https://gitdebt.com\""));
    }
}
