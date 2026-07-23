//! SVG → PNG/WebP rasterization.
//!
//! Why this exists: the SVG endpoints are great for the in-browser
//! page, but a lot of embed surfaces (Twitter cards, OG images, some
//! corporate proxies that strip inline SVG, certain readme renderers)
//! want a raster. We serve PNG + WebP variants from the same source
//! SVG, deterministically, with the same 24h cache as the SVG.
//!
//! Pipeline:
//!   1. Render the SVG via the existing chart functions.
//!   2. Pass through `freeze_svg_animations` — resvg ignores SMIL, so
//!      without this the bar charts render at width=0 and the line
//!      charts render with stroke-dashoffset = full length (invisible).
//!      The freezer applies each `<animate fill="freeze">`'s `to` value
//!      onto its parent element's matching attribute and removes the
//!      animate, producing a static end-frame SVG.
//!   3. Hand off to `resvg::usvg::Tree::from_str` → `resvg::render` →
//!      `tiny_skia::Pixmap`. The fontdb is bundled at compile time
//!      (Inter Regular, OFL — see `backend/assets/Inter-LICENSE.txt`).
//!   4. Encode PNG via `tiny_skia::Pixmap::encode_png` (no extra deps).
//!      WebP goes through the `image` crate's lossless encoder; lossy
//!      would blur text on the chart axes.

use std::sync::{Arc, OnceLock};

use anyhow::{Context, Result, anyhow};
use resvg::tiny_skia;
use resvg::usvg::{self, fontdb};

/// Inter Regular ships as a bundled OFL font so deployments don't have
/// to mount a fonts volume. ~400 KB compressed into the binary; the
/// alternative is "render with empty glyph boxes" which is worse.
const INTER_REGULAR: &[u8] = include_bytes!("../assets/Inter-Regular.ttf");

/// Lazy global fontdb. Loading is one-time and the resulting `Arc<...>`
/// is what `usvg::Options::fontdb` wants anyway.
fn font_db() -> Arc<fontdb::Database> {
    static FONTDB: OnceLock<Arc<fontdb::Database>> = OnceLock::new();
    FONTDB
        .get_or_init(|| {
            let mut db = fontdb::Database::new();
            db.load_font_data(INTER_REGULAR.to_vec());
            // Map our SVG's font-family stack (`ui-sans-serif,
            // system-ui, sans-serif`) onto the bundled Inter so glyphs
            // resolve. usvg honors `serif/sans-serif/cursive/fantasy/
            // monospace` family hints via the default-family setters.
            db.set_sans_serif_family("Inter");
            db.set_serif_family("Inter");
            db.set_monospace_family("Inter");
            Arc::new(db)
        })
        .clone()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RasterFormat {
    Png,
    Webp,
}

impl RasterFormat {
    pub fn content_type(self) -> &'static str {
        match self {
            RasterFormat::Png => "image/png",
            RasterFormat::Webp => "image/webp",
        }
    }
}

/// Render an SVG string to PNG or WebP bytes.
///
/// `scale` is a multiplier applied to the SVG's intrinsic viewBox.
/// 2.0 is a sensible default for "retina-density at the SVG's native
/// CSS size"; OG images typically want 1.5–2.0; pure thumbnails go
/// 0.5–1.0. Output dimensions are `ceil(viewbox * scale)` in each
/// axis.
pub fn rasterize(svg: &str, format: RasterFormat, scale: f32) -> Result<Vec<u8>> {
    let pixmap = render_pixmap(svg, scale)?;

    match format {
        RasterFormat::Png => pixmap.encode_png().context("encode png"),
        RasterFormat::Webp => encode_webp(&pixmap),
    }
}

/// Rasterize to straight-alpha RGBA for animated encoders. This shares the
/// exact SVG parsing, bundled font, scaling, and SMIL-freezing pipeline used
/// by the PNG/WebP endpoints.
pub(crate) fn rasterize_rgba(svg: &str, scale: f32) -> Result<(Vec<u8>, u32, u32)> {
    let pixmap = render_pixmap(svg, scale)?;
    let width = pixmap.width();
    let height = pixmap.height();
    Ok((demultiply(&pixmap), width, height))
}

fn render_pixmap(svg: &str, scale: f32) -> Result<tiny_skia::Pixmap> {
    let frozen = freeze_svg_animations(svg);
    // usvg's CSS parser does not implement media queries and logs one
    // warning per frame. They are only relevant to live SVG playback, so
    // remove them from the private raster input after SMIL is frozen.
    let frozen = strip_reduced_motion_media(&frozen);
    let opts = usvg::Options {
        fontdb: font_db(),
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(&frozen, &opts).context("parse svg")?;
    let size = tree.size();
    let scaled_w = (size.width() * scale).ceil().max(1.0) as u32;
    let scaled_h = (size.height() * scale).ceil().max(1.0) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(scaled_w, scaled_h)
        .ok_or_else(|| anyhow!("alloc pixmap {scaled_w}x{scaled_h}"))?;
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Ok(pixmap)
}

fn strip_reduced_motion_media(svg: &str) -> String {
    const MARKER: &str = "@media (prefers-reduced-motion: reduce)";
    let mut out = String::with_capacity(svg.len());
    let mut cursor = 0;
    while let Some(relative) = svg[cursor..].find(MARKER) {
        let start = cursor + relative;
        out.push_str(&svg[cursor..start]);
        let Some(open_relative) = svg[start + MARKER.len()..].find('{') else {
            out.push_str(&svg[start..]);
            return out;
        };
        let open = start + MARKER.len() + open_relative;
        let mut depth = 0_u32;
        let mut end = None;
        for (offset, byte) in svg.as_bytes()[open..].iter().enumerate() {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        end = Some(open + offset + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(next) = end else {
            out.push_str(&svg[start..]);
            return out;
        };
        cursor = next;
    }
    out.push_str(&svg[cursor..]);
    out
}

fn encode_webp(pixmap: &tiny_skia::Pixmap) -> Result<Vec<u8>> {
    // tiny_skia stores premultiplied RGBA; image/image-webp wants
    // straight (un-premultiplied) RGBA. Convert pixel-by-pixel. The
    // antialiased edges in our charts hit this path on every render.
    let demul = demultiply(pixmap);
    let mut buf = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut buf)
        .encode(
            &demul,
            pixmap.width(),
            pixmap.height(),
            image::ExtendedColorType::Rgba8,
        )
        .context("encode webp")?;
    Ok(buf)
}

fn demultiply(pixmap: &tiny_skia::Pixmap) -> Vec<u8> {
    let data = pixmap.data();
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks_exact(4) {
        let (r, g, b, a) = (chunk[0], chunk[1], chunk[2], chunk[3]);
        if a == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
        } else if a == 255 {
            out.extend_from_slice(&[r, g, b, a]);
        } else {
            // c_straight = c_premul * 255 / a, clamped.
            let inv = 255.0 / a as f32;
            out.push((r as f32 * inv).min(255.0) as u8);
            out.push((g as f32 * inv).min(255.0) as u8);
            out.push((b as f32 * inv).min(255.0) as u8);
            out.push(a);
        }
    }
    out
}

/// Apply each `<animate fill="freeze">` element's end-state to its
/// parent element's matching attribute, then strip the animate tag.
///
/// resvg's `usvg` crate explicitly ignores SMIL — animated values
/// always render at their `from` state. Our charts use animations
/// extensively (bars start at width=0, lines start with full
/// stroke-dashoffset, opacity fades from 0 to 1). Rendered as-is,
/// the raster would be empty. This pre-processor walks each animate's
/// `(attributeName, from, to)` triple and rewrites the parent's
/// matching `attribute="from"` to `attribute="to"`.
///
/// The "find the parent" heuristic is `svg[..pos].rfind(" attr=\"from\"")`
/// — relies on:
///   - all our `<animate>` tags emit explicit `from` and `to`,
///   - the leading space avoids partial-name collisions (so `width`
///     doesn't match `stroke-width`),
///   - within a single parent's open tag, the attribute appears
///     exactly once with the `from` value (true for our renderers).
///
/// Robust enough for our SVG output. A future renderer adding a sibling
/// `<animate>` (rather than a child) would need re-evaluation, but
/// every current chart nests animates inside their target element.
///
/// `<animateTransform>` is frozen too: for `additive="sum"` the end value
/// composes onto the parent's static `transform` (which is the base the
/// animation adds to); for the default replace semantics the parent's
/// `transform` value is rewritten to the end state `{type}({to})`.
pub fn freeze_svg_animations(svg: &str) -> String {
    let mut out = String::with_capacity(svg.len());
    let bytes = svg.as_bytes();
    let mut cursor = 0;

    loop {
        let plain = find_subseq(&bytes[cursor..], b"<animate ");
        let transform = find_subseq(&bytes[cursor..], b"<animateTransform ");
        let (rel, is_transform) = match (plain, transform) {
            (Some(p), Some(t)) if t < p => (t, true),
            (Some(p), _) => (p, false),
            (None, Some(t)) => (t, true),
            (None, None) => break,
        };
        let animate_start = cursor + rel;
        // Self-closing animate: look for `/>` after the opening tag.
        let Some(close_rel) = find_subseq(&bytes[animate_start..], b"/>") else {
            // Malformed; emit the rest verbatim and bail out.
            out.push_str(&svg[cursor..]);
            return out;
        };
        let animate_end = animate_start + close_rel + 2;
        let tag = &svg[animate_start..animate_end];

        let attr_name = extract_attr(tag, "attributeName");
        let from = extract_attr(tag, "from");
        let to = extract_attr(tag, "to");

        // Emit svg[cursor..animate_start] but with parent-attribute patched.
        let segment = &svg[cursor..animate_start];
        match (attr_name, from, to) {
            (Some(name), Some(_), Some(to)) if is_transform && name == "transform" => {
                let kind = extract_attr(tag, "type").unwrap_or_else(|| "translate".to_string());
                let additive = extract_attr(tag, "additive");
                // Patch the nearest preceding ` transform="..."` across the
                // emitted document so a sibling `<animate>` processed first
                // (which already flushed the parent's open tag into `out`)
                // does not hide the transform attribute from this pass.
                out.push_str(segment);
                patch_last_transform(&mut out, &kind, &to, additive.as_deref() == Some("sum"));
            }
            (Some(name), Some(from), Some(to)) => {
                let target = format!(" {name}=\"{from}\"");
                if let Some(pos) = segment.rfind(&target) {
                    out.push_str(&segment[..pos]);
                    out.push_str(&format!(" {name}=\"{to}\""));
                    out.push_str(&segment[pos + target.len()..]);
                } else {
                    // Couldn't locate the parent attribute. Emit the
                    // segment unchanged. The raster will show the
                    // animation's `from` state — degraded but not
                    // catastrophic.
                    out.push_str(segment);
                }
            }
            _ => {
                // Couldn't parse the animate tag's attributes. Drop it
                // silently and continue (better than emitting a no-op).
                out.push_str(segment);
            }
        }

        // Skip past the animate tag and any trailing whitespace.
        cursor = animate_end;
        while cursor < bytes.len() && matches!(bytes[cursor], b'\n' | b'\r' | b' ' | b'\t') {
            cursor += 1;
        }
    }

    out.push_str(&svg[cursor..]);
    out
}

/// Rewrite the nearest preceding ` transform="..."` in `out` to the
/// animateTransform's end state. `additive` composes `{kind}({to})` after
/// the existing base list; replace semantics substitute the whole value
/// (matching SMIL, where a non-additive transform animation discards the
/// static transform list while active/frozen). No preceding transform
/// attribute → left unchanged (renderers always author a static transform
/// on animated elements, same contract as the plain-attribute freezer).
fn patch_last_transform(out: &mut String, kind: &str, to: &str, additive: bool) {
    const NEEDLE: &str = " transform=\"";
    let Some(pos) = out.rfind(NEEDLE) else {
        return;
    };
    let value_start = pos + NEEDLE.len();
    let Some(value_len) = out[value_start..].find('"') else {
        return;
    };
    let base = &out[value_start..value_start + value_len];
    let end_term = format!("{kind}({to})");
    let new_value = if additive {
        // Frozen end state = base list ∘ end value.
        format!("{base} {end_term}")
    } else {
        // Replace semantics: the animation's end value wins outright,
        // whether the static value was authored at the start or end state.
        end_term
    };
    out.replace_range(value_start..value_start + value_len, &new_value);
}

fn find_subseq(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Extract `name="value"` from an XML-tag substring. Returns `None`
/// if the attribute is absent. Doesn't handle entity escaping — our
/// SVG output never puts entity-encoded values inside an animate.
fn extract_attr(tag: &str, name: &str) -> Option<String> {
    let needle = format!(" {name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"')?;
    Some(tag[start..start + end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freezer_rewrites_parent_attribute() {
        let svg = r##"<rect width="0" height="10">
  <animate attributeName="width" from="0" to="120" dur="0.9s" fill="freeze" />
</rect>"##;
        let out = freeze_svg_animations(svg);
        assert!(
            out.contains(r#"width="120""#),
            "should rewrite to=120: {out}"
        );
        assert!(!out.contains("<animate"), "animate should be removed");
    }

    #[test]
    fn freezer_handles_text_opacity() {
        let svg = r##"<text class="bar-count" x="10" y="20" fill="#000" opacity="0">
  <title>tip</title>
  count_label
  <animate attributeName="opacity" from="0" to="1" dur="0.4s" fill="freeze" />
</text>"##;
        let out = freeze_svg_animations(svg);
        assert!(out.contains(r#"opacity="1""#));
        assert!(!out.contains("<animate"));
    }

    #[test]
    fn freezer_handles_multiple_paths() {
        let svg = r##"<path d="M0 0" stroke-dasharray="100" stroke-dashoffset="100">
  <animate attributeName="stroke-dashoffset" from="100" to="0" dur="1s" fill="freeze" />
</path>
<path d="M0 50" stroke-dasharray="200" stroke-dashoffset="200">
  <animate attributeName="stroke-dashoffset" from="200" to="0" dur="1s" fill="freeze" />
</path>"##;
        let out = freeze_svg_animations(svg);
        assert!(!out.contains("<animate"));
        // Both stroke-dashoffsets should now read 0.
        let zero_count = out.matches(r#"stroke-dashoffset="0""#).count();
        assert_eq!(zero_count, 2, "both paths should be frozen: {out}");
    }

    #[test]
    fn freezer_leaves_unrelated_content_alone() {
        let svg = r##"<svg><text>untouched</text></svg>"##;
        let out = freeze_svg_animations(svg);
        assert_eq!(out, svg);
    }

    #[test]
    fn freezer_does_not_collide_on_attribute_substrings() {
        // `width` is a substring of `stroke-width`. The freezer must
        // not rewrite `stroke-width="0"` when an `<animate
        // attributeName="width" from="0" .../>` is pending.
        let svg = r##"<rect stroke-width="0" width="0" fill="red">
  <animate attributeName="width" from="0" to="50" fill="freeze" />
</rect>"##;
        let out = freeze_svg_animations(svg);
        assert!(
            out.contains(r#"stroke-width="0""#),
            "stroke-width must not be touched: {out}"
        );
        assert!(
            out.contains(r#"width="50""#),
            "width must be frozen to 50: {out}"
        );
    }

    #[test]
    fn freezer_composes_additive_animate_transform_onto_base() {
        // additive="sum": the frozen end state keeps the static base
        // transform and appends the animation's end value.
        let svg = r##"<g transform="translate(8 7)" stroke="#000">
  <animateTransform attributeName="transform" type="scale" from="0.75" to="1" dur="0.22s" additive="sum" fill="freeze" />
</g>"##;
        let out = freeze_svg_animations(svg);
        assert!(!out.contains("<animateTransform"), "tag removed: {out}");
        assert!(
            out.contains(r#"transform="translate(8 7) scale(1)""#),
            "base transform must survive an additive freeze: {out}"
        );
    }

    #[test]
    fn freezer_replaces_non_additive_animate_transform() {
        let svg = r##"<rect x="1" transform="translate(0 0)" fill="red">
  <animateTransform attributeName="transform" type="translate" from="-40 0" to="0 0" dur="0.6s" fill="freeze" />
</rect>"##;
        let out = freeze_svg_animations(svg);
        assert!(!out.contains("<animateTransform"));
        assert!(
            out.contains(r#"transform="translate(0 0)""#),
            "replace semantics land on the end value: {out}"
        );
        assert!(!out.contains("-40"));
    }

    #[test]
    fn freezer_handles_mixed_animate_and_animate_transform() {
        let svg = r##"<g opacity="0" transform="translate(0 4)">
  <animate attributeName="opacity" from="0" to="1" fill="freeze" />
  <animateTransform attributeName="transform" type="translate" from="0 4" to="0 0" fill="freeze" />
</g>"##;
        let out = freeze_svg_animations(svg);
        assert!(!out.contains("<animate"));
        assert!(out.contains(r#"opacity="1""#));
        assert!(out.contains(r#"transform="translate(0 0)""#));
    }

    #[test]
    fn raster_input_strips_only_reduced_motion_media() {
        let svg = r#"<style>.x { opacity: 1; }
@media (prefers-reduced-motion: reduce) {
  .motion { display: none; }
}
.y { fill: red; }</style>"#;
        let stripped = strip_reduced_motion_media(svg);
        assert!(!stripped.contains("prefers-reduced-motion"));
        assert!(stripped.contains(".x { opacity: 1; }"));
        assert!(stripped.contains(".y { fill: red; }"));
    }

    #[test]
    fn rasterize_emits_png_bytes() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 50"><rect width="100" height="50" fill="#3b82f6" /></svg>"##;
        let png = rasterize(svg, RasterFormat::Png, 1.0).expect("png");
        // PNG magic: 89 50 4E 47 0D 0A 1A 0A
        assert_eq!(&png[..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    }

    #[test]
    fn rasterize_draws_embedded_avatar_data() {
        const RED_JPEG: &str = "/9j/4AAQSkZJRgABAgAAAQABAAD//gAQTGF2YzYyLjI4LjEwMgD/2wBDAAgEBAQEBAUFBQUFBQYGBgYGBgYGBgYGBgYHBwcICAgHBwcGBgcHCAgICAkJCQgICAgJCQoKCgwMCwsODg4RERT/xABMAAEBAAAAAAAAAAAAAAAAAAAABgEBAQAAAAAAAAAAAAAAAAAABgcQAQAAAAAAAAAAAAAAAAAAAAARAQAAAAAAAAAAAAAAAAAAAAD/wAARCAACAAIDASIAAhEAAxEA/9oADAMBAAIRAxEAPwCLAE1/f//Z";
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2 2"><image href="data:image/jpeg;base64,{RED_JPEG}" width="2" height="2" /></svg>"#
        );
        let (rgba, width, height) = rasterize_rgba(&svg, 1.0).expect("embedded avatar");
        assert_eq!((width, height), (2, 2));
        assert!(
            rgba.chunks_exact(4).all(|pixel| {
                pixel[0] > 200 && pixel[1] < 40 && pixel[2] < 40 && pixel[3] == 255
            })
        );
    }

    #[test]
    fn rasterize_honors_text_length_pinning() {
        // The badge layout pins every <text> with textLength +
        // lengthAdjust="spacingAndGlyphs" so server-estimated geometry and
        // client rendering agree. This guards the resvg side: ink from a
        // deliberately over-long string must stay inside the pinned width.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 240 28" width="240" height="28"><rect width="240" height="28" fill="#ffffff" /><text x="10" y="19" textLength="60" lengthAdjust="spacingAndGlyphs" font-family="ui-monospace, SFMono-Regular, Menlo, Consolas, monospace" font-size="12" fill="#000000">wwwwwwwwwwww</text></svg>"##;
        let (rgba, width, height) = rasterize_rgba(svg, 1.0).expect("raster");
        assert_eq!((width, height), (240, 28));
        let ink_in_column_range = |x0: u32, x1: u32| -> bool {
            for y in 0..height {
                for x in x0..x1 {
                    let idx = ((y * width + x) * 4) as usize;
                    // Any non-white pixel counts as ink.
                    if rgba[idx] < 200 && rgba[idx + 3] > 0 {
                        return true;
                    }
                }
            }
            false
        };
        // 12 mono chars at 12px would naturally run ~86px; pinned to 60 the
        // ink must stop by x≈74 (x=10 + 60 + antialias slack).
        assert!(
            ink_in_column_range(10, 70),
            "pinned text must still render ink"
        );
        assert!(
            !ink_in_column_range(80, 240),
            "resvg must honor textLength: no ink past the pinned advance"
        );
    }

    #[test]
    fn rasterize_emits_webp_bytes() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 50"><rect width="100" height="50" fill="#3b82f6" /></svg>"##;
        let webp = rasterize(svg, RasterFormat::Webp, 1.0).expect("webp");
        // RIFF....WEBP container.
        assert_eq!(&webp[..4], b"RIFF");
        assert_eq!(&webp[8..12], b"WEBP");
    }
}
