//! Deterministic ordered-pixel texture shared by generated media.
//!
//! The pattern is geometry-only: no randomness, filters, external images, or
//! CSS variables. Identical inputs therefore keep producing identical bytes
//! across SVG, PNG, and WebP render paths. The ordered-dither math is the
//! classic 4×4 Bayer matrix (public-domain, 1973): a cell is lit when its
//! normalized threshold `(v + 0.5) / 16` falls below the requested density.

use crate::theme::Theme;

pub(crate) const BAYER_4: [[u8; 4]; 4] =
    [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// The only chromatic accent in the system: the wave trio. Everything else
/// stays neutral. Returned as `(start, mid, end)` per theme.
pub fn wave_stops(theme: &Theme) -> (&'static str, &'static str, &'static str) {
    if theme.dark {
        ("#9b7bff", "#46b3ff", "#ef72ff")
    } else {
        ("#5b2cff", "#087fea", "#bf24d6")
    }
}

/// Single-ink wave accent for small elements (chips, strips, glyphs) where
/// a gradient cannot resolve. Uses the trio's leading stop.
pub fn wave_ink(theme: &Theme) -> &'static str {
    wave_stops(theme).0
}

/// SVG definitions for a compact ordered-dot field and a denser signal
/// fill, sized so the wave gradient spans the actual surface. A 268px badge
/// and a 1200px chart both sample the full violet→blue→magenta ramp.
pub fn defs_sized(theme: &Theme, width: f32, height: f32) -> String {
    let sparse = pattern_cells(theme.fg, 4);
    let dense = pattern_cells("url(#gd-dither-wave)", 13);
    let (wave_1, wave_2, wave_3) = wave_stops(theme);
    let grad_w = width.max(1.0);
    let grad_h = (height * 0.4666).max(1.0);
    format!(
        r##"<defs data-gitdebt-texture-defs="true">
  <linearGradient id="gd-dither-wave" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="{grad_w:.0}" y2="{grad_h:.0}">
    <stop offset="0" stop-color="{wave_1}" />
    <stop offset="0.52" stop-color="{wave_2}" />
    <stop offset="1" stop-color="{wave_3}" />
  </linearGradient>
  <pattern id="gd-pixel-field" width="8" height="8" patternUnits="userSpaceOnUse" patternTransform="translate(.5 .5)">
    <g shape-rendering="crispEdges" opacity="0.22" transform="scale(2)">{sparse}</g>
  </pattern>
  <pattern id="gd-pixel-fill" width="8" height="8" patternUnits="userSpaceOnUse" patternTransform="translate(.5 .5)">
    <g shape-rendering="crispEdges" opacity="0.96" transform="scale(2)">{dense}</g>
  </pattern>
  <linearGradient id="gd-pixel-fade" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="white" stop-opacity="0" />
    <stop offset="0.3" stop-color="white" stop-opacity="0.18" />
    <stop offset="1" stop-color="white" stop-opacity="0.68" />
  </linearGradient>
  <mask id="gd-pixel-field-mask">
    <rect width="100%" height="100%" fill="url(#gd-pixel-fade)" />
  </mask>
</defs>"##,
    )
}

/// Back-compat entry point for 1200px-class chart surfaces.
pub fn defs(theme: &Theme) -> String {
    defs_sized(theme, 1200.0, 600.0)
}

/// Number of precomputed Bayer density tiers.
pub const TIER_COUNT: usize = 16;

/// Precomputed Bayer density-tier patterns: `gd-t0` (1/16 lit) through
/// `gd-t15` (all lit), one ink, alpha carried by the *consumer* via
/// `fill-opacity` so large washes (OG 1200×630) never need per-cell rects.
/// `cell` is the on-surface cell size in px (2–4 works well).
pub fn tier_defs(color: &str, cell: f32) -> String {
    let mut out = String::from("<defs data-gitdebt-tier-defs=\"true\">\n");
    for tier in 0..TIER_COUNT {
        out.push_str("  ");
        out.push_str(&tier_pattern(color, cell, tier));
        out.push('\n');
    }
    out.push_str("</defs>");
    out
}

/// One density-tier `<pattern>` def (`gd-t{tier}`) for surfaces that only
/// need a couple of tiers (badges, small cards) and want to keep bytes down.
pub fn tier_pattern(color: &str, cell: f32, tier: usize) -> String {
    let tier = tier.min(TIER_COUNT - 1);
    let tile = (cell * 4.0).max(1.0);
    let cells = pattern_cells(color, tier as u8 + 1);
    format!(
        "<pattern id=\"gd-t{tier}\" width=\"{tile:.0}\" height=\"{tile:.0}\" patternUnits=\"userSpaceOnUse\"><g shape-rendering=\"crispEdges\" transform=\"scale({cell:.1})\">{cells}</g></pattern>",
    )
}

/// Fill reference for a density tier, clamped to the valid range.
pub fn tier_fill(tier: usize) -> String {
    format!("url(#gd-t{})", tier.min(TIER_COUNT - 1))
}

/// A subtle background field makes every rendered chart share the same pixel
/// grain. It is inserted immediately after the root element so every label,
/// link, line, and avatar remains above it. The texture defs are sized from
/// the document's `viewBox` so the wave gradient spans the real surface.
pub fn decorate(mut svg: String, theme: &Theme) -> String {
    if svg.contains("data-gitdebt-texture=\"true\"") {
        return svg;
    }
    let (width, height) = surface_size(&svg);
    let field = format!(
        "\n  <rect data-gitdebt-canvas=\"true\" width=\"100%\" height=\"100%\" fill=\"{}\" pointer-events=\"none\" />\n  <rect data-gitdebt-texture=\"true\" width=\"100%\" height=\"100%\" fill=\"url(#gd-pixel-field)\" mask=\"url(#gd-pixel-field-mask)\" opacity=\"0.28\" pointer-events=\"none\" />\n",
        theme.bg,
    );
    if let Some(index) = svg.find('>') {
        svg.insert_str(index + 1, &field);
    }
    if let Some(index) = svg.rfind("</svg>") {
        svg.insert_str(index, &format!("\n{}\n", defs_sized(theme, width, height)));
    }
    svg
}

/// Parse `viewBox="0 0 W H"` from the SVG root so texture defs can size the
/// wave gradient to the actual surface. Falls back to the 1200×600 chart
/// envelope when absent or malformed (deterministic either way).
fn surface_size(svg: &str) -> (f32, f32) {
    const FALLBACK: (f32, f32) = (1200.0, 600.0);
    let Some(start) = svg.find("viewBox=\"") else {
        return FALLBACK;
    };
    let rest = &svg[start + "viewBox=\"".len()..];
    let Some(end) = rest.find('"') else {
        return FALLBACK;
    };
    let mut parts = rest[..end].split_ascii_whitespace();
    let (Some(_), Some(_), Some(w), Some(h)) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return FALLBACK;
    };
    match (w.parse::<f32>(), h.parse::<f32>()) {
        (Ok(w), Ok(h)) if w > 0.0 && h > 0.0 => (w, h),
        _ => FALLBACK,
    }
}

/// Reusable pattern fill id for area and bar renderers.
pub const FILL: &str = "url(#gd-pixel-fill)";

/// The dense signal-fill cell set (13/16 lit, wave-gradient ink) for
/// renderers that need a phase-shifted variant of `gd-pixel-fill`.
pub(crate) fn dense_cells() -> String {
    pattern_cells("url(#gd-dither-wave)", 13)
}

fn pattern_cells(color: &str, threshold: u8) -> String {
    let mut cells = String::new();
    for (y, row) in BAYER_4.iter().enumerate() {
        for (x, value) in row.iter().enumerate() {
            if *value < threshold {
                cells.push_str(&format!(
                    "<rect x=\"{x}\" y=\"{y}\" width=\"1\" height=\"1\" fill=\"{color}\" />"
                ));
            }
        }
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme;

    #[test]
    fn decoration_is_deterministic_and_idempotent() {
        let source = "<svg><text>chart</text></svg>".to_string();
        let first = decorate(source, &theme::LIGHT);
        let second = decorate(first.clone(), &theme::LIGHT);
        assert_eq!(first, second);
        assert!(first.contains("data-gitdebt-texture=\"true\""));
        assert!(first.contains("data-gitdebt-canvas=\"true\""));
        assert!(first.contains(crate::theme::LIGHT.bg));
        assert!(first.contains("shape-rendering=\"crispEdges\""));
        assert!(first.contains("id=\"gd-dither-wave\""));
        assert!(!first.contains("<animate"));
        assert!(!first.contains(&format!(
            "<rect width=\"8\" height=\"8\" fill=\"{}\"",
            crate::theme::LIGHT.track
        )));
        assert!(!first.contains("var(--"));
        assert!(
            first.find("data-gitdebt-texture").expect("texture field")
                < first.find("<text>").expect("chart content")
        );
    }

    #[test]
    fn wave_gradient_spans_the_actual_surface_width() {
        let narrow = decorate(
            "<svg viewBox=\"0 0 268 28\"><text>badge</text></svg>".to_string(),
            &theme::DARK,
        );
        assert!(
            narrow.contains("x2=\"268\""),
            "268px surface must get a 268px gradient: {narrow}"
        );
        let wide = decorate(
            "<svg viewBox=\"0 0 1200 630\"><text>og</text></svg>".to_string(),
            &theme::DARK,
        );
        assert!(wide.contains("x2=\"1200\""));
        // No viewBox → the 1200×600 chart fallback.
        let bare = decorate("<svg><text>x</text></svg>".to_string(), &theme::DARK);
        assert!(bare.contains("x2=\"1200\""));
    }

    #[test]
    fn sized_defs_bake_the_wave_trio_per_theme() {
        let dark = defs_sized(&theme::DARK, 400.0, 100.0);
        assert!(dark.contains("#9b7bff"));
        assert!(dark.contains("#46b3ff"));
        assert!(dark.contains("#ef72ff"));
        let light = defs_sized(&theme::LIGHT, 400.0, 100.0);
        assert!(light.contains("#5b2cff"));
        assert!(light.contains("#087fea"));
        assert!(light.contains("#bf24d6"));
        assert_eq!(wave_ink(&theme::DARK), "#9b7bff");
        assert_eq!(wave_ink(&theme::LIGHT), "#5b2cff");
    }

    #[test]
    fn tier_defs_emit_sixteen_density_patterns() {
        let defs = tier_defs("#fafafa", 3.0);
        for tier in 0..TIER_COUNT {
            assert!(defs.contains(&format!("id=\"gd-t{tier}\"")));
        }
        // Tier 0 lights exactly the single 0-valued cell; tier 15 all 16.
        let t0 = defs.split("id=\"gd-t0\"").nth(1).unwrap();
        let t0 = &t0[..t0.find("</pattern>").unwrap()];
        assert_eq!(t0.matches("<rect").count(), 1);
        let t15 = defs.split("id=\"gd-t15\"").nth(1).unwrap();
        let t15 = &t15[..t15.find("</pattern>").unwrap()];
        assert_eq!(t15.matches("<rect").count(), 16);
        // Deterministic + one ink only.
        assert_eq!(defs, tier_defs("#fafafa", 3.0));
        assert!(!defs.contains("var(--"));
        assert_eq!(tier_fill(7), "url(#gd-t7)");
        assert_eq!(tier_fill(99), "url(#gd-t15)");
    }
}
