//! Size-bounded, play-once animated star-history GIFs for README embeds.
//!
//! GitHub strips SMIL from SVG images, so actual README motion is an
//! explicit raster alternative. Frames are rendered from `chart.rs`'s pure
//! geometry and the Postgres-derived series supplied by the API layer.

use std::io::Cursor;

use anyhow::{Context, Result, bail};
use image::codecs::gif::GifEncoder;
use image::{Delay, Frame, RgbaImage};
use sha2::{Digest, Sha256};

use crate::chart::{ChartConfig, ChartOpts, Point, render_svg_frame};
use crate::raster::rasterize_rgba;
use crate::theme::Theme;

pub const MOTION_PRESET: &str = "draw";
pub const FRAME_COUNT: usize = 5;
pub const DURATION_MS: u32 = 220;
pub const TARGET_BYTES: usize = 1_000_000;
pub const HARD_MAX_BYTES: usize = 5_000_000;

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
    let mut last = None;
    for scale in [0.5_f32, 0.4, 0.3] {
        let encoded = encode_at_scale(series, cfg, theme, opts, scale)?;
        if encoded.bytes.len() <= TARGET_BYTES {
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

    let mut frames = Vec::with_capacity(FRAME_COUNT);
    let mut dimensions = None;
    // `animate` is deliberately ignored for GIFs: the raster frames carry
    // the motion and must never include SMIL.
    let static_opts = ChartOpts {
        animate: false,
        ..opts.clone()
    };
    for (index, sample) in SAMPLES.into_iter().enumerate() {
        let svg = render_svg_frame(series, cfg, theme, &static_opts, spline_progress(sample));
        let (rgba, width, height) = rasterize_rgba(&svg, scale)?;
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
            Delay::from_numer_denom_ms(DELAYS_MS[index], 1),
        ));
    }

    let mut bytes = Cursor::new(Vec::new());
    {
        // Speed 10 gives stable, compact quantization without making a
        // request-path render unreasonably CPU-heavy.
        let mut encoder = GifEncoder::new_with_speed(&mut bytes, 10);
        encoder
            .encode_frames(frames.into_iter())
            .context("encode animated GIF")?;
    }
    let (width, height) = dimensions.context("GIF has at least one frame")?;
    Ok(EncodedGif {
        bytes: bytes.into_inner(),
        width,
        height,
        frame_count: FRAME_COUNT,
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
