//! Size-bounded animated star-history GIFs for README embeds.
//!
//! GIF is the motion format for surfaces that render an SVG as a single
//! frame — rasterizers, and README renderers outside GitHub such as npm,
//! PyPI and Docker Hub. GitHub itself plays SMIL and CSS animation in an
//! `<img>`-embedded SVG, so `animate=1` is the lighter option there. Frames
//! are rendered from `chart.rs`'s pure geometry and the Postgres-derived
//! series supplied by the API layer.
//!
//! # The only thing that moves in a drawing is the pen
//!
//! Every gitdebt asset is a sheet of one dimensioned engineering drawing. A
//! drawing has no texture, no gradient, no glow and no shadow, so it has
//! nothing decorative left to animate. The one thing that genuinely moves is
//! the plotter pen drawing the object line, and both presets are that pen:
//!
//!   * `draw` — the pen plots the trace once and rests on the finished
//!     sheet. No loop extension is written, so the last frame is what a
//!     README keeps showing.
//!   * `wave` (default) — the same pen, looping. Each cycle OPENS on the
//!     completed sheet and holds it, then re-plots it. Writes the
//!     NETSCAPE2.0 loop extension. Opening on the finished drawing is
//!     deliberate: the first frame a README paints is the whole chart, never
//!     a half-plotted one, and the reader spends most of the cycle looking
//!     at complete data.
//!
//! `wave` keeps its name because `?motion=wave` is a published query
//! parameter. What it names is now a plot cycle, not a dither phase.
//!
//! Generic media (badges, cards, any finished sheet handed in as a string)
//! has no pen at all — see [`encode_media_gif`].

use std::io::Cursor;

use anyhow::{Context, Result, bail};
use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame, RgbaImage};
use sha2::{Digest, Sha256};

use crate::chart::{ChartConfig, ChartOpts, Point, render_svg_frame};
use crate::raster::rasterize_rgba;
use crate::theme::Theme;

pub const MOTION_DRAW: &str = "draw";
pub const MOTION_WAVE: &str = "wave";
pub const FRAME_COUNT: usize = 5;
pub const DURATION_MS: u32 = 220;

/// Frames in one plot cycle: one held frame of the finished sheet plus the
/// re-plot. About 1.5s end to end.
pub const WAVE_FRAME_COUNT: usize = 10;
/// Delay on each frame of the re-plot.
pub const WAVE_FRAME_DELAY_MS: u32 = 70;
/// Base delay on the one frame that holds the finished sheet. The reader
/// spends most of a cycle here, looking at the whole drawing.
pub const WAVE_HOLD_DELAY_MS: u32 = 900;
/// How many dwell rates a repository can be assigned, and the step between
/// them. Two gitdebt GIFs on one README should not pulse in unison; this is
/// the only thing the per-repository seed changes, because the drawing
/// itself is the same drawing whoever owns it.
const WAVE_DWELL_RATES: u32 = 11;
const WAVE_DWELL_STEP_MS: u32 = 20;

/// Delay stamped on a single-frame media GIF. A still has nothing to time,
/// but the container still wants a frame delay.
pub const MEDIA_FRAME_DELAY_MS: u32 = 100;

pub const TARGET_BYTES: usize = 1_000_000;
/// The looping plot gets a little more headroom than the 5-frame draw.
pub const WAVE_TARGET_BYTES: usize = 1_500_000;
pub const MEDIA_TARGET_BYTES: usize = 1_500_000;
pub const HARD_MAX_BYTES: usize = 5_000_000;

/// FNV-1a over a slug: a stable, dependency-free 32-bit seed so every
/// repository gets its own dwell with zero storage.
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

/// Encode the looping `wave` preset: the finished sheet, held, then
/// re-plotted from the origin. Writes the NETSCAPE2.0 loop extension so the
/// cycle repeats forever. `seed` (typically `fnv1a(slug)`) sets only the
/// dwell rate — see [`wave_dwell_ms`]. Deterministic bytes for identical
/// inputs.
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

/// Encode any self-contained gitdebt SVG as a GIF.
///
/// This path used to march every authored Bayer pattern through one tile so
/// a badge or a card appeared to move while its data stayed complete. The
/// drawing carries no pattern any more, and the rule this path has always
/// held — every frame is fully readable, a reader never sees partial data
/// masquerading as animation — leaves nothing here that could honestly move.
/// A finished sheet handed in as a string has no pen to follow. So a media
/// GIF is one frame of that sheet: the `format=gif` request is answered, and
/// no motion is invented for a still drawing. The star-history presets above
/// still animate, because plotting a chart genuinely has a pen.
///
/// `backdrop` is the theme canvas the source SVG's ink was designed against
/// (`theme.bg`); the frame is flattened onto it before encoding. `&str`
/// rather than `&Theme` because every call site captures it in a `'static`
/// closure.
pub fn encode_media_gif(svg: &str, backdrop: &str) -> Result<EncodedGif> {
    let frames = [crate::raster::freeze_svg_animations(svg)];
    let mut last = None;
    for scale in [1.0_f32, 0.75, 0.5] {
        let encoded = encode_frames(&frames, &[MEDIA_FRAME_DELAY_MS], scale, None, backdrop)?;
        if encoded.bytes.len() <= MEDIA_TARGET_BYTES {
            return Ok(encoded);
        }
        last = Some(encoded);
    }
    let encoded = last.context("media GIF scale candidates are non-empty")?;
    if encoded.bytes.len() >= HARD_MAX_BYTES {
        bail!(
            "media GIF exceeds hard byte cap ({} >= {})",
            encoded.bytes.len(),
            HARD_MAX_BYTES
        );
    }
    Ok(encoded)
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

/// How long one repository's cycle rests on the finished sheet.
///
/// The only per-repository variation in the loop. The plot is the same
/// drawing for every repository, so nothing about the ink depends on the
/// slug; only the rhythm does, and only so that a README carrying several
/// gitdebt GIFs does not have them all restart together.
fn wave_dwell_ms(seed: u32) -> u32 {
    WAVE_HOLD_DELAY_MS + (seed % WAVE_DWELL_RATES) * WAVE_DWELL_STEP_MS
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
    let dwell = wave_dwell_ms(seed);
    let mut delays = Vec::with_capacity(WAVE_FRAME_COUNT);
    let mut svgs = Vec::with_capacity(WAVE_FRAME_COUNT);
    for frame in 0..WAVE_FRAME_COUNT {
        // Frame 0 is the finished sheet. The re-plot runs 1/N..(N-1)/N and
        // stops just short of complete, so frame 0 closes the cycle with no
        // duplicate frame and no visible jump.
        let (progress, delay) = if frame == 0 {
            (1.0, dwell)
        } else {
            let step = frame as f32 / WAVE_FRAME_COUNT as f32;
            (spline_progress(step), WAVE_FRAME_DELAY_MS)
        };
        delays.push(delay);
        svgs.push(render_svg_frame(series, cfg, theme, &static_opts, progress));
    }
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

/// Rasterize each frame SVG at `scale` and encode the GIF. `repeat` writes
/// the NETSCAPE2.0 loop extension (wave); `None` plays once (draw) or is a
/// single still (media). Frames are flattened onto `backdrop` first — see
/// [`flatten_onto`].
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

    fn cfg() -> ChartConfig {
        ChartConfig {
            repo: "owner/repo".into(),
            ..ChartConfig::default()
        }
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

    fn has_netscape_loop(bytes: &[u8]) -> bool {
        bytes
            .windows(b"NETSCAPE2.0".len())
            .any(|w| w == b"NETSCAPE2.0")
    }

    #[test]
    fn draw_gif_is_bounded_deterministic_and_plays_once() {
        let a = encode_draw(&series(), &cfg(), &LIGHT, &opts()).unwrap();
        let b = encode_draw(&series(), &cfg(), &LIGHT, &opts()).unwrap();
        assert_eq!(a.bytes, b.bytes);
        assert_eq!((a.width, a.height), (600, 300));
        assert_eq!(a.frame_count, FRAME_COUNT);
        assert!(a.bytes.len() <= TARGET_BYTES, "{} bytes", a.bytes.len());
        assert!(a.bytes.len() < HARD_MAX_BYTES);
        assert!(
            !has_netscape_loop(&a.bytes),
            "no loop extension means play once"
        );
        let frames = decode(&a.bytes);
        assert_eq!(frames.len(), FRAME_COUNT);
        assert!(frames.iter().all(|f| f.buffer().dimensions() == (600, 300)));
    }

    #[test]
    fn first_middle_and_final_frames_are_distinct() {
        let gif = encode_draw(&series(), &cfg(), &LIGHT, &opts()).unwrap();
        let frames = decode(&gif.bytes);
        let first = frames.first().unwrap().buffer().as_raw();
        let middle = frames[FRAME_COUNT / 2].buffer().as_raw();
        let final_frame = frames.last().unwrap().buffer().as_raw();
        assert_ne!(first, middle);
        assert_ne!(middle, final_frame);
        assert_ne!(first, final_frame);
    }

    /// Both prints of the drawing are baked into the frames: an embedder
    /// picks one with `?theme=`, and a GIF cannot re-decide later.
    #[test]
    fn light_and_dark_are_baked_and_visibly_different() {
        let light = encode_draw(&series(), &cfg(), &LIGHT, &opts()).unwrap();
        let dark = encode_draw(&series(), &cfg(), &DARK, &opts()).unwrap();
        assert_ne!(light.bytes, dark.bytes);
        let light_px = decode(&light.bytes)[0].buffer().get_pixel(0, 0).0;
        let dark_px = decode(&dark.bytes)[0].buffer().get_pixel(0, 0).0;
        // Paper, and the dark print's ground.
        assert_eq!(&light_px[..3], &[0xff, 0xff, 0xff]);
        assert_eq!(&dark_px[..3], &[0x0c, 0x0f, 0x11]);
        // The chart SVG is transparent; the encoder must have flattened it.
        // Leaked alpha would harden edges and ghost across frames.
        assert_eq!(light_px[3], 0xff);
        assert_eq!(dark_px[3], 0xff);
    }

    /// The looping plot: it opens on the finished sheet, holds it, re-plots,
    /// and closes back onto frame 0 without a duplicate.
    #[test]
    fn wave_gif_opens_finished_loops_and_stays_bounded() {
        let seed = fnv1a("owner/repo");
        let a = encode_wave(&series(), &cfg(), &DARK, &opts(), seed).unwrap();
        let b = encode_wave(&series(), &cfg(), &DARK, &opts(), seed).unwrap();
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
        // The pen actually moves.
        assert_ne!(
            frames[0].buffer().as_raw(),
            frames[WAVE_FRAME_COUNT / 2].buffer().as_raw(),
            "the re-plot must differ from the finished sheet"
        );
        // Frame 0 is the completed drawing and matches the play-once
        // preset's final frame: the loop never rests on partial data.
        let drawn = encode_draw(&series(), &cfg(), &DARK, &opts()).unwrap();
        assert_eq!(
            frames[0].buffer().as_raw(),
            decode(&drawn.bytes).last().unwrap().buffer().as_raw(),
            "the cycle must open on the finished sheet"
        );
        // Frame 0 rests; every plotted frame runs at the same pen rate.
        let hold = frames[0].delay().numer_denom_ms();
        let plot = frames[1].delay().numer_denom_ms();
        assert_ne!(hold, plot, "the finished sheet must be held longer");
        for frame in &frames[1..] {
            assert_eq!(frame.delay().numer_denom_ms(), plot);
        }
    }

    /// The seed changes the rhythm and nothing else. Two GIFs on one README
    /// should not restart in unison; the ink is the same drawing either way.
    #[test]
    fn wave_dwell_breathes_per_repository() {
        let mine = fnv1a("owner/repo");
        let theirs = fnv1a("other/slug");
        assert_ne!(wave_dwell_ms(mine), wave_dwell_ms(theirs));
        assert!(
            (WAVE_HOLD_DELAY_MS..=WAVE_HOLD_DELAY_MS + (WAVE_DWELL_RATES - 1) * WAVE_DWELL_STEP_MS)
                .contains(&wave_dwell_ms(mine))
        );
        // Every dwell survives the GIF container's 10ms delay quantization.
        for seed in 0..WAVE_DWELL_RATES {
            assert_eq!(wave_dwell_ms(seed) % 10, 0);
        }

        let a = encode_wave(&series(), &cfg(), &LIGHT, &opts(), mine).unwrap();
        let b = encode_wave(&series(), &cfg(), &LIGHT, &opts(), theirs).unwrap();
        assert_ne!(a.bytes, b.bytes, "the dwell differentiates repositories");
        // Only the timing moved: every plotted frame is the same ink.
        for (left, right) in decode(&a.bytes).iter().zip(decode(&b.bytes).iter()) {
            assert_eq!(left.buffer().as_raw(), right.buffer().as_raw());
        }
    }

    #[test]
    fn draw_keeps_play_once_semantics() {
        let draw = encode_draw(&series(), &cfg(), &LIGHT, &opts()).unwrap();
        assert!(
            !has_netscape_loop(&draw.bytes),
            "draw keeps play-once semantics (no loop extension)"
        );
    }

    /// A finished sheet has no pen to follow, so the media path encodes one
    /// readable still rather than inventing motion. It must never loop and
    /// never show partial content.
    #[test]
    fn media_gif_is_one_complete_still_frame() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40" viewBox="0 0 80 40">
  <rect width="80" height="40" fill="#0c0f11" />
  <line x1="8" y1="32" x2="72" y2="32" stroke="#2b2e31" stroke-width="1" />
  <path d="M8 32L40 20L72 10" fill="none" stroke="#f0674e" stroke-width="2" />
  <text x="8" y="14" fill="#e6e8ea" font-size="8">ready</text>
</svg>"##;
        let gif = encode_media_gif(svg, DARK.bg).unwrap();
        assert_eq!(gif.frame_count, 1);
        assert!(
            !has_netscape_loop(&gif.bytes),
            "a still has nothing to loop"
        );
        assert!(gif.bytes.len() <= MEDIA_TARGET_BYTES);
        assert_eq!(decode(&gif.bytes).len(), 1);
        assert_eq!(gif.bytes, encode_media_gif(svg, DARK.bg).unwrap().bytes);
    }

    /// The media path takes an already-transparent SVG, so the flatten has to
    /// happen inside the encoder rather than in the renderers that feed it.
    #[test]
    fn gif_frames_are_flattened_onto_the_theme_canvas() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20" viewBox="0 0 40 20">
  <rect x="8" y="4" width="24" height="12" fill="#5ca5e1" />
</svg>"##;
        let gif = encode_media_gif(svg, DARK.bg).unwrap();
        let frames = decode(&gif.bytes);
        let first = frames[0].buffer();
        assert!(
            first.pixels().all(|px| px.0[3] == 0xff),
            "GIF's single alpha bit must never be spent on the canvas"
        );
        assert_eq!(&first.get_pixel(0, 0).0[..3], &[0x0c, 0x0f, 0x11]);

        // The light print flattens onto paper.
        let light = encode_media_gif(svg, LIGHT.bg).unwrap();
        assert_eq!(
            &decode(&light.bytes)[0].buffer().get_pixel(0, 0).0[..3],
            &[0xff, 0xff, 0xff]
        );
    }

    /// SMIL that reached this path from an `animate=1` renderer is frozen to
    /// its final state before rasterizing: the still shows completed data.
    #[test]
    fn media_still_freezes_any_authored_animation() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20" viewBox="0 0 40 20">
  <rect width="40" height="20" fill="#ffffff" />
  <path d="M4 16L36 4" fill="none" stroke="#cc291f" stroke-width="2" stroke-dasharray="40" stroke-dashoffset="40">
    <animate attributeName="stroke-dashoffset" from="40" to="0" dur="1s" fill="freeze" />
  </path>
</svg>"##;
        let gif = encode_media_gif(svg, LIGHT.bg).unwrap();
        let frozen = decode(&gif.bytes);
        assert_eq!(frozen.len(), 1);
        // The trace is present: a frame stuck at dashoffset 40 would be bare
        // paper, so some pixel has to carry drafting red.
        assert!(
            frozen[0]
                .buffer()
                .pixels()
                .any(|px| px.0[0] > 120 && px.0[1] < 140 && px.0[2] < 140),
            "the frozen still must show the completed trace"
        );
    }

    #[test]
    fn backdrop_parsing_never_panics() {
        assert_eq!(backdrop_rgb("#0c0f11"), [0x0c, 0x0f, 0x11]);
        assert_eq!(backdrop_rgb("ffffff"), [0xff, 0xff, 0xff]);
        for bad in ["", "#fff", "#gggggg", "nonsense", "#00ff0"] {
            assert_eq!(backdrop_rgb(bad), [0, 0, 0]);
        }
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
    fn spline_has_correct_endpoints_and_is_monotonic() {
        assert_eq!(spline_progress(0.0), 0.0);
        assert_eq!(spline_progress(1.0), 1.0);
        let samples = (0..=20)
            .map(|i| spline_progress(i as f32 / 20.0))
            .collect::<Vec<_>>();
        assert!(samples.windows(2).all(|w| w[0] <= w[1]));
    }
}
