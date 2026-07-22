//! Deterministic ordered-pixel texture shared by generated media.
//!
//! The pattern is geometry-only: no randomness, filters, external images, or
//! CSS variables. Identical inputs therefore keep producing identical bytes
//! across SVG, PNG, and WebP render paths.

use crate::theme::Theme;

const BAYER_4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// SVG definitions for a compact ordered-dot field and a denser signal fill.
pub fn defs(theme: &Theme) -> String {
    let sparse = pattern_cells(theme.fg, 4);
    let dense = pattern_cells("url(#gd-dither-wave)", 13);
    let (wave_1, wave_2, wave_3) = if theme.dark {
        ("#9b7bff", "#46b3ff", "#ef72ff")
    } else {
        ("#5b2cff", "#087fea", "#bf24d6")
    };
    format!(
        r##"<defs data-gitdebt-texture-defs="true">
  <linearGradient id="gd-dither-wave" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="1200" y2="280">
    <stop offset="0" stop-color="{wave_1}">
      <animate class="motion" attributeName="stop-color" values="{wave_1};{wave_2};{wave_1}" dur="7s" repeatCount="indefinite" />
    </stop>
    <stop offset="0.52" stop-color="{wave_2}">
      <animate class="motion" attributeName="stop-color" values="{wave_2};{wave_3};{wave_2}" dur="8.5s" repeatCount="indefinite" />
    </stop>
    <stop offset="1" stop-color="{wave_3}">
      <animate class="motion" attributeName="stop-color" values="{wave_3};{wave_1};{wave_3}" dur="10s" repeatCount="indefinite" />
    </stop>
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

/// A subtle background field makes every rendered chart share the same pixel
/// grain. It is inserted immediately after the root element so every label,
/// link, line, and avatar remains above it.
pub fn decorate(mut svg: String, theme: &Theme) -> String {
    if svg.contains("data-gitdebt-texture=\"true\"") {
        return svg;
    }
    let field = format!(
        "\n  <rect data-gitdebt-canvas=\"true\" width=\"100%\" height=\"100%\" fill=\"{}\" pointer-events=\"none\" />\n  <rect data-gitdebt-texture=\"true\" width=\"100%\" height=\"100%\" fill=\"url(#gd-pixel-field)\" mask=\"url(#gd-pixel-field-mask)\" opacity=\"0.28\" pointer-events=\"none\" />\n",
        theme.bg,
    );
    if let Some(index) = svg.find('>') {
        svg.insert_str(index + 1, &field);
    }
    if let Some(index) = svg.rfind("</svg>") {
        svg.insert_str(index, &format!("\n{}\n", defs(theme)));
    }
    svg
}

/// Reusable pattern fill id for area and bar renderers.
pub const FILL: &str = "url(#gd-pixel-fill)";

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
}
