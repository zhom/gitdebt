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
        themed_logo_mark(mark_x, mark_y, mark_size, theme),
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
