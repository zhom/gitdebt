//! Size-bounded animated star-history GIFs for README embeds.
//!
//! GIF is the motion format for surfaces that render an SVG as a single
//! frame — rasterizers, and README renderers outside GitHub such as npm,
//! PyPI and Docker Hub. GitHub itself plays SMIL and CSS animation in an
//! `<img>`-embedded SVG, so `animate=1` is the lighter option there.
//! Frames are rendered from `chart.rs`'s pure geometry and the
//! Postgres-derived series supplied by the API layer.
//!
//! Two presets:
//!   * `wave` (default) — a continuous loop: the dithered underfill
//!     swells with seeded sines and the Bayer phase marches. Writes the
//!     NETSCAPE2.0 loop extension.
//!   * `draw` — the original play-once line-draw reveal (no loop
//!     extension; rests on the final frame).

use std::io::Cursor;

use anyhow::{Context, Result, bail};
use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame, RgbaImage};
use sha2::{Digest, Sha256};

use crate::chart::{
    ChartConfig, ChartOpts, Point, WaveSpec, render_svg_frame, render_svg_wave_frame,
};
use crate::raster::rasterize_rgba;
use crate::theme::Theme;

pub const MOTION_DRAW: &str = "draw";
pub const MOTION_WAVE: &str = "wave";
pub const FRAME_COUNT: usize = 5;
pub const DURATION_MS: u32 = 220;
/// Frames in one seamless wave cycle (~1s loop at 70ms/frame).
pub const WAVE_FRAME_COUNT: usize = 14;
pub const WAVE_FRAME_DELAY_MS: u32 = 70;
/// Generic shareable-media loop: one crisp pixel of horizontal phase per
/// frame across the shared 8px Bayer tile.
pub const DITHER_FRAME_COUNT: usize = 8;
pub const DITHER_FRAME_DELAY_MS: u32 = 90;
pub const TARGET_BYTES: usize = 1_000_000;
/// The 14-frame loop gets a little more headroom than the 5-frame draw.
pub const WAVE_TARGET_BYTES: usize = 1_500_000;
pub const DITHER_TARGET_BYTES: usize = 1_500_000;
pub const HARD_MAX_BYTES: usize = 5_000_000;

/// FNV-1a over a slug: a stable, dependency-free 32-bit seed so every
/// repository gets its own wave phases with zero storage.
pub fn fnv1a(s: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in s.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// A complete encoded GIF plus metadata useful to the HTTP/cache layer.
#[derive(Debug)]
pub struct EncodedGif {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub frame_count: usize,
}

/// Stable revision of the exact filtered data being rendered. Including it
/// in the API cache key prevents a completed backfill from reusing an older
/// GIF even when every query option stays the same.
pub fn data_revision(series: &[Point]) -> String {
    let mut hash = Sha256::new();
    hash.update((series.len() as u64).to_be_bytes());
    for point in series {
        hash.update(point.at.timestamp_millis().to_be_bytes());
        hash.update(point.stars.to_be_bytes());
    }
    hex::encode(&hash.finalize()[..12])
}

/// Encode the `draw` preset. Five frames over 220ms is 22.7fps, under the
/// 24fps ceiling. No Netscape loop extension is written, so the GIF plays
/// once and rests on its fully drawn final frame.
///
/// The initial 0.5× render is a README-friendly 600×300 for the default
/// chart. If content complexity pushes it over the 1MB target, progressively
/// smaller deterministic scales are tried. Output above 5MB is rejected.
pub fn encode_draw(
    series: &[Point],
    cfg: &ChartConfig,
    theme: &Theme,
    opts: &ChartOpts,
) -> Result<EncodedGif> {
    encode_bounded(TARGET_BYTES, |scale| {
        encode_at_scale(series, cfg, theme, opts, scale)
    })
}

/// Encode the looping `wave` preset: one seamless 14-frame cycle where the
/// dithered underfill undulates (sine phases seeded from `seed`, typically
/// `fnv1a(slug)`) and the Bayer threshold phase advances. Writes the
/// NETSCAPE2.0 loop extension so the cycle repeats forever. Deterministic
/// bytes for identical inputs.
pub fn encode_wave(
    series: &[Point],
    cfg: &ChartConfig,
    theme: &Theme,
    opts: &ChartOpts,
    seed: u32,
) -> Result<EncodedGif> {
    encode_bounded(WAVE_TARGET_BYTES, |scale| {
        encode_wave_at_scale(series, cfg, theme, opts, seed, scale)
    })
}

/// Turn any self-contained gitdebt SVG into a real GitHub-safe animated
/// asset by marching every authored Bayer pattern through one complete tile.
///
/// The source is first frozen to its correct final semantic state (revealed
/// labels, completed bars), then only the decorative pattern phase changes.
/// Every frame therefore remains fully readable and a README consumer never
/// sees partial data masquerading as animation.
///
/// `backdrop` is the theme canvas the source SVG's ink was designed against
/// (`theme.bg`); frames are flattened onto it before encoding. `&str` rather
/// than `&Theme` because every call site captures it in a `'static` closure.
pub fn encode_dither_loop(svg: &str, backdrop: &str) -> Result<EncodedGif> {
    let frozen = crate::raster::freeze_svg_animations(svg);
    let delays = [DITHER_FRAME_DELAY_MS; DITHER_FRAME_COUNT];
    let svgs = (0..DITHER_FRAME_COUNT)
        .map(|frame| phase_dither_patterns(&frozen, frame))
        .collect::<Vec<_>>();

    let mut last = None;
    for scale in [1.0_f32, 0.75, 0.5] {
        let encoded = encode_frames(&svgs, &delays, scale, Some(Repeat::Infinite), backdrop)?;
        if encoded.bytes.len() <= DITHER_TARGET_BYTES {
            return Ok(encoded);
        }
        last = Some(encoded);
    }
    let encoded = last.context("dither GIF scale candidates are non-empty")?;
    if encoded.bytes.len() >= HARD_MAX_BYTES {
        bail!(
            "animated dither GIF exceeds hard byte cap ({} >= {})",
            encoded.bytes.len(),
            HARD_MAX_BYTES
        );
    }
    Ok(encoded)
}

/// Replace every `patternTransform="translate(...)"` phase without touching
/// element transforms. Shared media patterns use an 8px tile, so frame 8
/// wraps exactly to frame 0 and the loop has no visual jump.
fn phase_dither_patterns(svg: &str, frame: usize) -> String {
    const OPEN: &str = "patternTransform=\"translate(";
    let phase = frame % DITHER_FRAME_COUNT;
    let mut out = String::with_capacity(svg.len());
    let mut cursor = 0;
    while let Some(relative) = svg[cursor..].find(OPEN) {
        let start = cursor + relative;
        let values_start = start + OPEN.len();
        let Some(relative_end) = svg[values_start..].find(")\"") else {
            break;
        };
        let end = values_start + relative_end;
        out.push_str(&svg[cursor..values_start]);
        out.push_str(&format!("{}.5 .5", phase));
        cursor = end;
    }
    out.push_str(&svg[cursor..]);
    out
}

/// Shared deterministic scale-retry ladder + hard byte cap.
fn encode_bounded(
    target_bytes: usize,
    mut encode: impl FnMut(f32) -> Result<EncodedGif>,
) -> Result<EncodedGif> {
    let mut last = None;
    for scale in [0.5_f32, 0.4, 0.3] {
        let encoded = encode(scale)?;
        if encoded.bytes.len() <= target_bytes {
            return Ok(encoded);
        }
        last = Some(encoded);
    }
    let encoded = last.context("GIF scale candidates are non-empty")?;
    if encoded.bytes.len() >= HARD_MAX_BYTES {
        bail!(
            "animated GIF exceeds hard byte cap ({} >= {})",
            encoded.bytes.len(),
            HARD_MAX_BYTES
        );
    }
    Ok(encoded)
}

fn encode_at_scale(
    series: &[Point],
    cfg: &ChartConfig,
    theme: &Theme,
    opts: &ChartOpts,
    scale: f32,
) -> Result<EncodedGif> {
    // Delays total exactly 220ms. Five frames / 0.22s = 22.7fps.
    const DELAYS_MS: [u32; FRAME_COUNT] = [50, 40, 50, 40, 40];
    const SAMPLES: [f32; FRAME_COUNT] = [0.0, 0.25, 0.5, 0.75, 1.0];

    // `animate` is deliberately ignored for GIFs: the raster frames carry
    // the motion and must never include SMIL.
    let static_opts = ChartOpts {
        animate: false,
        ..opts.clone()
    };
    let svgs = SAMPLES
        .into_iter()
        .map(|sample| render_svg_frame(series, cfg, theme, &static_opts, spline_progress(sample)))
        .collect::<Vec<_>>();
    encode_frames(&svgs, &DELAYS_MS, scale, None, theme.bg)
}

fn encode_wave_at_scale(
    series: &[Point],
    cfg: &ChartConfig,
    theme: &Theme,
    opts: &ChartOpts,
    seed: u32,
    scale: f32,
) -> Result<EncodedGif> {
    let static_opts = ChartOpts {
        animate: false,
        ..opts.clone()
    };
    let delays = [WAVE_FRAME_DELAY_MS; WAVE_FRAME_COUNT];
    let svgs = (0..WAVE_FRAME_COUNT)
        .map(|frame| {
            render_svg_wave_frame(
                series,
                cfg,
                theme,
                &static_opts,
                WaveSpec {
                    frame,
                    frames: WAVE_FRAME_COUNT,
                    seed,
                },
            )
        })
        .collect::<Vec<_>>();
    encode_frames(&svgs, &delays, scale, Some(Repeat::Infinite), theme.bg)
}

/// GIF carries one bit of alpha and `Frame::from_parts` uses the default
/// disposal, so a transparent frame would harden every antialiased edge and
/// ghost the previous frame through it. The SVG surfaces are transparent by
/// design, so flatten each rasterized frame onto the tone its ink was drawn
/// against before the encoder ever sees it. Integer round-half-up math keeps
/// the result byte-deterministic.
fn flatten_onto(rgba: &mut [u8], backdrop: [u8; 3]) {
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a == 255 {
            continue;
        }
        for (channel, bg) in px[..3].iter_mut().zip(backdrop) {
            *channel = ((*channel as u32 * a + bg as u32 * (255 - a) + 127) / 255) as u8;
        }
        px[3] = 255;
    }
}

/// `#rrggbb` → channels. Theme canvases are always well-formed; an
/// unparseable value falls back to black rather than panicking on a
/// request path.
fn backdrop_rgb(hex: &str) -> [u8; 3] {
    let digits = hex.strip_prefix('#').unwrap_or(hex);
    if digits.len() != 6 {
        return [0, 0, 0];
    }
    let mut out = [0u8; 3];
    for (channel, pair) in out.iter_mut().zip(digits.as_bytes().chunks_exact(2)) {
        let Ok(text) = std::str::from_utf8(pair) else {
            return [0, 0, 0];
        };
        let Ok(value) = u8::from_str_radix(text, 16) else {
            return [0, 0, 0];
        };
        *channel = value;
    }
    out
}

/// Rasterize each frame SVG at `scale` and encode the GIF. `repeat`
/// writes the NETSCAPE2.0 loop extension (wave); `None` plays once (draw).
/// Frames are flattened onto `backdrop` first — see [`flatten_onto`].
fn encode_frames(
    svgs: &[String],
    delays_ms: &[u32],
    scale: f32,
    repeat: Option<Repeat>,
    backdrop: &str,
) -> Result<EncodedGif> {
    let mut frames = Vec::with_capacity(svgs.len());
    let mut dimensions = None;
    let backdrop = backdrop_rgb(backdrop);
    for (svg, delay) in svgs.iter().zip(delays_ms) {
        let (mut rgba, width, height) = rasterize_rgba(svg, scale)?;
        flatten_onto(&mut rgba, backdrop);
        match dimensions {
            Some((w, h)) if (w, h) != (width, height) => {
                bail!("GIF frames have inconsistent dimensions")
            }
            None => dimensions = Some((width, height)),
            _ => {}
        }
        let image = RgbaImage::from_raw(width, height, rgba).context("construct GIF RGBA frame")?;
        frames.push(Frame::from_parts(
            image,
            0,
            0,
            Delay::from_numer_denom_ms(*delay, 1),
        ));
    }

    let mut bytes = Cursor::new(Vec::new());
    {
        // Speed 10 gives stable, compact quantization without making a
        // request-path render unreasonably CPU-heavy.
        let mut encoder = GifEncoder::new_with_speed(&mut bytes, 10);
        if let Some(repeat) = repeat {
            encoder.set_repeat(repeat).context("set GIF repeat")?;
        }
        encoder
            .encode_frames(frames.into_iter())
            .context("encode animated GIF")?;
    }
    let (width, height) = dimensions.context("GIF has at least one frame")?;
    Ok(EncodedGif {
        bytes: bytes.into_inner(),
        width,
        height,
        frame_count: svgs.len(),
    })
}

/// Cubic-bezier(0.23, 1, 0.32, 1), matching the SVG draw reveal. Solve the
/// x component by bisection, then evaluate y; fixed iterations keep output
/// deterministic across runs.
fn spline_progress(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    fn cubic(t: f32, a: f32, b: f32) -> f32 {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    }
    let mut lo = 0.0;
    let mut hi = 1.0;
    for _ in 0..18 {
        let mid = (lo + hi) * 0.5;
        if cubic(mid, 0.23, 0.32) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    cubic((lo + hi) * 0.5, 1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use chrono::{TimeZone, Utc};
    use image::AnimationDecoder;
    use image::codecs::gif::GifDecoder;

    use super::*;
    use crate::chart::{TimeAxis, cumulative_series};
    use crate::theme::{DARK, LIGHT};

    fn series() -> Vec<Point> {
        let arrivals = (0..80)
            .map(|day| Utc.timestamp_opt(1_700_000_000 + day * 86_400, 0).unwrap())
            .collect::<Vec<_>>();
        cumulative_series(&arrivals)
    }

    fn opts() -> ChartOpts {
        ChartOpts {
            axis: TimeAxis::Date,
            log_y: false,
            animate: true, // encoder must ignore this
        }
    }

    fn decode(bytes: &[u8]) -> Vec<image::Frame> {
        GifDecoder::new(Cursor::new(bytes))
            .unwrap()
            .into_frames()
            .collect_frames()
            .unwrap()
    }

    #[test]
    fn draw_gif_is_bounded_deterministic_and_plays_once() {
        let cfg = ChartConfig {
            repo: "owner/repo".into(),
            ..ChartConfig::default()
        };
        let a = encode_draw(&series(), &cfg, &LIGHT, &opts()).unwrap();
        let b = encode_draw(&series(), &cfg, &LIGHT, &opts()).unwrap();
        assert_eq!(a.bytes, b.bytes);
        assert_eq!((a.width, a.height), (600, 300));
        assert_eq!(a.frame_count, FRAME_COUNT);
        assert!(a.bytes.len() <= TARGET_BYTES, "{} bytes", a.bytes.len());
        assert!(a.bytes.len() < HARD_MAX_BYTES);
        assert!(
            !a.bytes
                .windows(b"NETSCAPE2.0".len())
                .any(|w| w == b"NETSCAPE2.0"),
            "no loop extension means play once"
        );
        let frames = decode(&a.bytes);
        assert_eq!(frames.len(), FRAME_COUNT);
        assert!(frames.iter().all(|f| f.buffer().dimensions() == (600, 300)));
    }

    #[test]
    fn first_middle_and_final_frames_are_distinct() {
        let cfg = ChartConfig {
            repo: "owner/repo".into(),
            ..ChartConfig::default()
        };
        let gif = encode_draw(&series(), &cfg, &LIGHT, &opts()).unwrap();
        let frames = decode(&gif.bytes);
        let first = frames.first().unwrap().buffer().as_raw();
        let middle = frames[FRAME_COUNT / 2].buffer().as_raw();
        let final_frame = frames.last().unwrap().buffer().as_raw();
        assert_ne!(first, middle);
        assert_ne!(middle, final_frame);
        assert_ne!(first, final_frame);
    }

    #[test]
    fn light_and_dark_are_baked_and_visibly_different() {
        let cfg = ChartConfig {
            repo: "owner/repo".into(),
            ..ChartConfig::default()
        };
        let light = encode_draw(&series(), &cfg, &LIGHT, &opts()).unwrap();
        let dark = encode_draw(&series(), &cfg, &DARK, &opts()).unwrap();
        assert_ne!(light.bytes, dark.bytes);
        let light_px = decode(&light.bytes)[0].buffer().get_pixel(0, 0).0;
        let dark_px = decode(&dark.bytes)[0].buffer().get_pixel(0, 0).0;
        assert_eq!(&light_px[..3], &[0xff, 0xff, 0xff]);
        assert_eq!(&dark_px[..3], &[0x0a, 0x0a, 0x0a]);
        // The chart SVG is transparent; the encoder must have flattened it.
        // Leaked alpha would harden edges and ghost across frames.
        assert_eq!(light_px[3], 0xff);
        assert_eq!(dark_px[3], 0xff);
    }

    fn has_netscape_loop(bytes: &[u8]) -> bool {
        bytes
            .windows(b"NETSCAPE2.0".len())
            .any(|w| w == b"NETSCAPE2.0")
    }

    #[test]
    fn wave_gif_is_deterministic_loops_and_stays_bounded() {
        let cfg = ChartConfig {
            repo: "owner/repo".into(),
            ..ChartConfig::default()
        };
        let seed = fnv1a("owner/repo");
        let a = encode_wave(&series(), &cfg, &DARK, &opts(), seed).unwrap();
        let b = encode_wave(&series(), &cfg, &DARK, &opts(), seed).unwrap();
        assert_eq!(a.bytes, b.bytes, "identical inputs → identical GIF bytes");
        assert_eq!((a.width, a.height), (600, 300));
        assert_eq!(a.frame_count, WAVE_FRAME_COUNT);
        assert!(
            a.bytes.len() <= WAVE_TARGET_BYTES,
            "{} bytes over budget",
            a.bytes.len()
        );
        assert!(
            has_netscape_loop(&a.bytes),
            "wave must write the NETSCAPE2.0 loop extension"
        );
        let frames = decode(&a.bytes);
        assert_eq!(frames.len(), WAVE_FRAME_COUNT);
        // The underfill actually moves between frames.
        assert_ne!(
            frames[0].buffer().as_raw(),
            frames[WAVE_FRAME_COUNT / 2].buffer().as_raw(),
            "wave frames must differ"
        );
    }

    #[test]
    fn wave_seed_changes_the_animation_draw_stays_play_once() {
        let cfg = ChartConfig {
            repo: "owner/repo".into(),
            ..ChartConfig::default()
        };
        let a = encode_wave(&series(), &cfg, &LIGHT, &opts(), fnv1a("owner/repo")).unwrap();
        let b = encode_wave(&series(), &cfg, &LIGHT, &opts(), fnv1a("other/slug")).unwrap();
        assert_ne!(a.bytes, b.bytes, "seeded phases differentiate repos");

        let draw = encode_draw(&series(), &cfg, &LIGHT, &opts()).unwrap();
        assert!(
            !has_netscape_loop(&draw.bytes),
            "draw keeps play-once semantics (no loop extension)"
        );
    }

    #[test]
    fn generic_dither_gif_loops_and_keeps_complete_content() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40" viewBox="0 0 80 40">
  <defs><pattern id="p" width="8" height="8" patternUnits="userSpaceOnUse" patternTransform="translate(.5 .5)"><rect width="4" height="4" fill="#9b7bff" /><animateTransform attributeName="patternTransform" type="translate" from="0.5 0.5" to="8.5 0.5" dur="1s" repeatCount="indefinite" /></pattern></defs>
  <rect width="80" height="40" fill="#0a0a0a" />
  <rect width="80" height="40" fill="url(#p)" />
  <text x="8" y="24" fill="#fafafa">ready</text>
</svg>"##;
        let gif = encode_dither_loop(svg, DARK.bg).unwrap();
        assert_eq!(gif.frame_count, DITHER_FRAME_COUNT);
        assert!(has_netscape_loop(&gif.bytes));
        assert!(gif.bytes.len() <= DITHER_TARGET_BYTES);
        let frames = decode(&gif.bytes);
        assert_eq!(frames.len(), DITHER_FRAME_COUNT);
        assert_ne!(
            frames[0].buffer().as_raw(),
            frames[1].buffer().as_raw(),
            "Bayer phase must visibly advance"
        );
    }

    /// The generic media path takes an already-transparent SVG, so the
    /// flatten has to happen inside the encoder rather than in the renderers
    /// that feed it.
    #[test]
    fn gif_frames_are_flattened_onto_the_theme_canvas() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20" viewBox="0 0 40 20">
  <defs><pattern id="p" width="8" height="8" patternUnits="userSpaceOnUse" patternTransform="translate(.5 .5)"><rect width="4" height="4" fill="#9b7bff" /></pattern></defs>
  <rect x="8" y="4" width="24" height="12" fill="url(#p)" />
</svg>"##;
        let gif = encode_dither_loop(svg, DARK.bg).unwrap();
        let frames = decode(&gif.bytes);
        let first = frames[0].buffer();
        assert!(
            first.pixels().all(|px| px.0[3] == 0xff),
            "GIF's single alpha bit must never be spent on the canvas"
        );
        assert_eq!(&first.get_pixel(0, 0).0[..3], &[0x0a, 0x0a, 0x0a]);
    }

    #[test]
    fn fnv1a_is_stable_and_slug_sensitive() {
        assert_eq!(fnv1a(""), 0x811c_9dc5);
        assert_eq!(fnv1a("owner/repo"), fnv1a("owner/repo"));
        assert_ne!(fnv1a("owner/repo"), fnv1a("owner/repo2"));
    }

    #[test]
    fn data_revision_tracks_exact_filtered_series() {
        let a = series();
        let mut b = a.clone();
        b.last_mut().unwrap().stars += 1;
        assert_eq!(data_revision(&a), data_revision(&a));
        assert_ne!(data_revision(&a), data_revision(&b));
        assert_ne!(data_revision(&[]), data_revision(&a));
    }

    #[test]
    fn pattern_phase_rewrite_is_scoped_and_wraps() {
        let svg = r#"<pattern patternTransform="translate(.5 .5)"></pattern><g transform="translate(2 3)"></g>"#;
        let phased = phase_dither_patterns(svg, DITHER_FRAME_COUNT + 3);
        assert!(phased.contains(r#"patternTransform="translate(3.5 .5)""#));
        assert!(phased.contains(r#"transform="translate(2 3)""#));
    }

    #[test]
    fn spline_has_correct_endpoints_and_is_monotonic() {
        assert_eq!(spline_progress(0.0), 0.0);
        assert_eq!(spline_progress(1.0), 1.0);
        let samples = (0..=20)
            .map(|i| spline_progress(i as f32 / 20.0))
            .collect::<Vec<_>>();
        assert!(samples.windows(2).all(|w| w[0] <= w[1]));
    }
}
