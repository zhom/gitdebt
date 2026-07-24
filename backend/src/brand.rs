//! Inline gitdebt brand primitives for generated SVG assets.
//!
//! Shareable images cannot reference `/logo.svg`: GitHub's image proxy and
//! social rasterization both require a self-contained document. These helpers
//! inline the canonical repository artwork, recolor it with concrete theme
//! values, and keep the output deterministic.

use crate::theme::Theme;

const LOGO_SVG: &str = include_str!("../../assets/gitdebt-logo.svg");

/// Ink bounds of the canonical robot path inside its 512×512 artboard.
///
/// Marks are placed by these bounds rather than by the artboard so a small
/// mark spends every available pixel on the glyph: the artwork is 1.43:1 and
/// leaves ~40% of the square empty.
pub(crate) const INK_X: f32 = 41.436;
pub(crate) const INK_Y: f32 = 108.392;
pub(crate) const INK_W: f32 = 429.115;
pub(crate) const INK_H: f32 = 299.305;

/// Rendered mark width below which the artwork's 32-unit dither cell samples
/// under one device pixel: the pattern stops resolving and the silhouette
/// dissolves into noise. Narrower marks take a solid single-ink fill of the
/// same path, so every surface still shows the genuine logo.
pub(crate) const DITHER_MIN_WIDTH: f32 = 96.0;

/// Height of a mark drawn at `width`, from the artwork's ink aspect.
pub fn mark_height(width: f32) -> f32 {
    width * INK_H / INK_W
}

fn logo_body() -> &'static str {
    let start = LOGO_SVG.find('>').expect("logo opening tag") + 1;
    let end = LOGO_SVG.rfind("</svg>").expect("logo closing tag");
    &LOGO_SVG[start..end]
}

/// The robot path alone, with the pattern reference swapped for a flat ink.
fn solid_body(ink: &str) -> String {
    let body = logo_body();
    let defs_start = body.find("<defs>").expect("logo defs");
    let defs_end = body.find("</defs>").expect("logo defs close") + "</defs>".len();
    let mut out = String::with_capacity(body.len());
    out.push_str(&body[..defs_start]);
    out.push_str(&body[defs_end..]);
    out.replace("fill=\"url(#gitdebt-dither)\"", &format!("fill=\"{ink}\""))
}

/// The artwork verbatim, with the dither pattern re-inked and its id made
/// document-unique so two marks in one SVG cannot collide.
fn dithered_body(ink: &str, id: &str) -> String {
    logo_body()
        .replace("fill=\"#000\"", &format!("fill=\"{ink}\""))
        .replace("id=\"gitdebt-dither\"", &format!("id=\"{id}\""))
        .replace("url(#gitdebt-dither)", &format!("url(#{id})"))
}

/// Render the canonical robot with its ink bounds placed at
/// (`x`, `y`, `width`, [`mark_height`]`(width)`).
///
/// Above [`DITHER_MIN_WIDTH`] the mark keeps the artwork's dither pattern;
/// below it the same path is filled solid, because that is the only
/// treatment that survives a 14–24px embed.
pub fn logo_mark(x: f32, y: f32, width: f32, ink: &str) -> String {
    let scale = width / INK_W;
    let body = if width >= DITHER_MIN_WIDTH {
        let id = format!(
            "gd-mark-{}-{}-{}",
            (x * 10.0).round() as i64,
            (y * 10.0).round() as i64,
            (width * 10.0).round() as i64,
        );
        dithered_body(ink, &id)
    } else {
        solid_body(ink)
    };
    format!(
        "  <g data-gitdebt-logo=\"true\" aria-label=\"gitdebt\" transform=\"translate({x:.2} {y:.2}) scale({scale:.5}) translate({tx:.3} {ty:.3})\">{body}</g>\n",
        tx = -INK_X,
        ty = -INK_Y,
    )
}

/// Theme-aware version of [`logo_mark`], inked with the theme foreground.
pub fn themed_logo_mark(x: f32, y: f32, width: f32, theme: &Theme) -> String {
    logo_mark(x, y, width, theme.fg)
}

/// Compact bottom-right logo + wordmark used by chart and card footers.
///
/// The supplied coordinates are the right edge and text baseline, matching
/// the existing right-anchored footer labels.
pub fn footer_lockup(right_x: f32, baseline_y: f32, theme: &Theme) -> String {
    const MARK_W: f32 = 20.0;
    // The monospace wordmark occupies roughly 48 units at 11px. Leave a
    // deliberate 10-unit gutter so the robot never collides with the `g`,
    // even when a renderer substitutes a slightly wider system monospace.
    let mark_x = right_x - 78.0;
    // Centre the glyph on the wordmark's x-height rather than its baseline.
    let mark_y = baseline_y - 4.0 - mark_height(MARK_W) / 2.0;
    format!(
        "  <a href=\"https://gitdebt.com\" target=\"_blank\" rel=\"noopener\" aria-label=\"gitdebt\">\n{}    <text class=\"footer-link\" x=\"{right_x:.1}\" y=\"{baseline_y:.1}\" text-anchor=\"end\" fill=\"{muted}\" font-family=\"ui-monospace, SFMono-Regular, Menlo, Consolas, monospace\" font-size=\"11\" font-weight=\"600\" letter-spacing=\"0.02em\">gitdebt</text>\n  </a>\n",
        logo_mark(mark_x, mark_y, MARK_W, theme.muted),
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

/// Placement of a mark inside a rendered SVG, in that SVG's user units.
#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) struct MarkBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    /// Raster density the surface is being checked at.
    pub scale: f32,
    /// Hex ink the mark is drawn in.
    pub ink: &'static str,
    /// Hex tone the mark sits on.
    pub canvas: &'static str,
}

/// Coverage the ink threshold sits at. Both rasterizations — mark over
/// canvas, and asset over transparency — are classified at this same
/// coverage so their edge pixels agree.
#[cfg(test)]
const INK_COVERAGE: f32 = 0.4;

/// Mean channel value of a `#rrggbb` string.
#[cfg(test)]
fn tone(hex: &str) -> f32 {
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).expect("hex color");
    (((v >> 16) & 0xff) + ((v >> 8) & 0xff) + (v & 0xff)) as f32 / 3.0
}

#[cfg(test)]
impl MarkBox {
    /// Recover a surface's authored mark placement from its own markup, so
    /// fidelity checks do not have to re-derive each renderer's layout math.
    pub(crate) fn locate(svg: &str, scale: f32, ink: &'static str, canvas: &'static str) -> Self {
        const ANCHOR: &str =
            "data-gitdebt-logo=\"true\" aria-label=\"gitdebt\" transform=\"translate(";
        let start = svg.find(ANCHOR).expect("surface carries a mark") + ANCHOR.len();
        let rest = &svg[start..];
        let end = rest.find(')').expect("mark translate closes");
        let (x, y) = rest[..end]
            .split_once(' ')
            .expect("mark translate has two components");
        let scale_start = rest.find("scale(").expect("mark scales") + "scale(".len();
        let scale_end = rest[scale_start..].find(')').expect("mark scale closes");
        let glyph: f32 = rest[scale_start..scale_start + scale_end]
            .parse()
            .expect("mark scale is numeric");
        Self {
            x: x.parse().expect("mark x"),
            y: y.parse().expect("mark y"),
            width: glyph * INK_W,
            scale,
            ink,
            canvas,
        }
    }

    fn pixels(&self) -> (u32, u32, u32, u32) {
        (
            (self.x * self.scale).round() as u32,
            (self.y * self.scale).round() as u32,
            (self.width * self.scale).round() as u32,
            (mark_height(self.width) * self.scale).round() as u32,
        )
    }

    /// The canonical asset alone, rasterized at exactly this placement's
    /// scale and sub-pixel phase, as an ink mask.
    fn reference(&self) -> Vec<bool> {
        let (ox, oy, w, h) = self.pixels();
        let scale = self.width * self.scale / INK_W;
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\"><g transform=\"translate({tx:.5} {ty:.5}) scale({scale:.6}) translate({ix:.3} {iy:.3})\">{}</g></svg>",
            solid_body("#000000"),
            tx = self.x * self.scale - ox as f32,
            ty = self.y * self.scale - oy as f32,
            ix = -INK_X,
            iy = -INK_Y,
        );
        let (rgba, rw, rh) = crate::raster::rasterize_rgba(&svg, 1.0).expect("reference raster");
        assert_eq!((rw, rh), (w, h));
        let cut = (INK_COVERAGE * 255.0) as u8;
        rgba.chunks_exact(4).map(|px| px[3] >= cut).collect()
    }

    /// The mark as it actually rasterizes out of a finished surface.
    fn rendered(&self, svg: &str) -> Vec<bool> {
        let (rgba, img_w, _img_h) =
            crate::raster::rasterize_rgba(svg, self.scale).expect("surface raster");
        let (ox, oy, w, h) = self.pixels();
        let (ink, canvas) = (tone(self.ink), tone(self.canvas));
        let threshold = canvas + INK_COVERAGE * (ink - canvas);
        let mut mask = Vec::with_capacity((w * h) as usize);
        for row in 0..h {
            for col in 0..w {
                let i = (((oy + row) * img_w + ox + col) * 4) as usize;
                let lum = (rgba[i] as f32 + rgba[i + 1] as f32 + rgba[i + 2] as f32) / 3.0;
                mask.push(if ink > canvas {
                    lum > threshold
                } else {
                    lum < threshold
                });
            }
        }
        mask
    }
}

/// Compare a surface's mark against a rasterization of the repository's own
/// artwork at the same geometry. Returns `(mismatched fraction, ink
/// fraction)`: the first proves the glyph is the logo, the second proves it
/// is a glyph at all rather than a filled chip.
#[cfg(test)]
pub(crate) fn mark_fidelity(svg: &str, place: MarkBox) -> (f32, f32) {
    let rendered = place.rendered(svg);
    let reference = place.reference();
    assert_eq!(rendered.len(), reference.len());
    let wrong = rendered
        .iter()
        .zip(&reference)
        .filter(|(a, b)| a != b)
        .count();
    let ink = rendered.iter().filter(|on| **on).count();
    (
        wrong as f32 / rendered.len() as f32,
        ink as f32 / rendered.len() as f32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK, LIGHT};

    fn framed(mark: &str, w: f32, h: f32, bg: &str) -> String {
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\"><rect width=\"{w}\" height=\"{h}\" fill=\"{bg}\" />{mark}</svg>"
        )
    }

    #[test]
    fn logo_is_inline_deterministic_and_theme_aware() {
        let light = themed_logo_mark(10.0, 20.0, 120.0, &LIGHT);
        let dark = themed_logo_mark(10.0, 20.0, 120.0, &DARK);

        assert_eq!(light, themed_logo_mark(10.0, 20.0, 120.0, &LIGHT));
        assert!(light.contains("data-gitdebt-logo=\"true\""));
        assert!(light.contains("fill=\"#0a0a0a\""));
        assert!(dark.contains("fill=\"#fafafa\""));
        assert!(!light.contains("<image"));
        assert!(!dark.contains("<image"));
        // The canonical path travels with every mark, at every size.
        for width in [14.0, 18.0, 64.0, 140.0] {
            assert!(logo_mark(0.0, 0.0, width, "#000").contains("M320.5 110.5"));
        }
    }

    #[test]
    fn large_marks_keep_the_dither_pattern_and_scope_its_id() {
        let big = logo_mark(0.0, 0.0, 140.0, "#fafafa");
        assert!(big.contains("<pattern"));
        assert!(!big.contains("id=\"gitdebt-dither\""));
        assert!(big.contains("id=\"gd-mark-0-0-1400\""));
        assert!(big.contains("url(#gd-mark-0-0-1400)"));

        let two = format!("{big}{}", logo_mark(200.0, 0.0, 140.0, "#fafafa"));
        assert!(two.contains("id=\"gd-mark-2000-0-1400\""));
        assert_eq!(two.matches("id=\"gd-mark-0-0-1400\"").count(), 1);
    }

    #[test]
    fn small_marks_drop_the_pattern_for_a_solid_ink() {
        let small = logo_mark(4.0, 4.0, 18.0, "#fafafa");
        assert!(!small.contains("<pattern"));
        assert!(!small.contains("gitdebt-dither"));
        assert_eq!(small.matches("fill=").count(), 1);
        assert!(small.contains("fill=\"#fafafa\""));
        assert_eq!(small, logo_mark(4.0, 4.0, 18.0, "#fafafa"));
    }

    /// The regression this guards: compact surfaces once shipped a
    /// hand-authored 14×14 bitmap instead of the repository's artwork.
    /// Rasterize the mark at its real embed sizes and compare its coverage
    /// against a rasterization of the canonical asset.
    #[test]
    fn compact_marks_rasterize_to_the_canonical_silhouette() {
        for width in [14.0_f32, 18.0, 20.0, 24.0] {
            for scale in [1.0_f32, 2.0, 6.0] {
                for theme in [&DARK, &LIGHT] {
                    let mark = logo_mark(4.0, 4.0, width, theme.fg);
                    let svg = framed(&mark, width + 8.0, mark_height(width) + 8.0, theme.bg);
                    let (mismatch, ink) = mark_fidelity(
                        &svg,
                        MarkBox {
                            x: 4.0,
                            y: 4.0,
                            width,
                            scale,
                            ink: theme.fg,
                            canvas: theme.bg,
                        },
                    );
                    assert!(
                        mismatch < 0.04,
                        "width {width} @{scale}x differs from the canonical logo by {mismatch:.3}"
                    );
                    assert!(
                        (0.25..0.75).contains(&ink),
                        "width {width} @{scale}x coverage {ink:.3} reads as a block"
                    );
                }
            }
        }
    }

    /// Proves the comparison has teeth: a filled chip — the shape the brand
    /// zone used to collapse into — is nowhere near the silhouette.
    #[test]
    fn a_filled_block_fails_the_silhouette_check() {
        let width = 18.0_f32;
        let block = format!(
            "  <rect x=\"4\" y=\"4\" width=\"{width}\" height=\"{h}\" fill=\"{}\" />\n",
            DARK.fg,
            h = mark_height(width),
        );
        let svg = framed(&block, width + 8.0, mark_height(width) + 8.0, DARK.bg);
        let (mismatch, ink) = mark_fidelity(
            &svg,
            MarkBox {
                x: 4.0,
                y: 4.0,
                width,
                scale: 6.0,
                ink: DARK.fg,
                canvas: DARK.bg,
            },
        );
        assert!(mismatch > 0.3, "a chip must fail badly, got {mismatch:.3}");
        assert!(ink > 0.95);
    }

    #[test]
    fn footer_is_a_linked_lockup_carrying_the_real_logo() {
        let footer = footer_lockup(844.0, 188.0, &LIGHT);
        assert!(footer.contains("https://gitdebt.com"));
        assert!(footer.contains("data-gitdebt-logo=\"true\""));
        assert!(footer.contains(">gitdebt</text>"));
        assert!(footer.contains("M320.5 110.5"));
        assert!(!footer.contains("<pattern"));

        let svg = framed(&footer, 900.0, 200.0, LIGHT.bg);
        let (mismatch, ink) = mark_fidelity(
            &svg,
            MarkBox {
                x: 844.0 - 78.0,
                y: 188.0 - 4.0 - mark_height(20.0) / 2.0,
                width: 20.0,
                scale: 2.0,
                ink: LIGHT.muted,
                canvas: LIGHT.bg,
            },
        );
        assert!(mismatch < 0.04, "footer mark drifted: {mismatch:.3}");
        assert!((0.25..0.75).contains(&ink));
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
