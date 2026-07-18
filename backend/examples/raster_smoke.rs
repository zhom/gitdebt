//! Manual smoke test for the chart raster pipeline. Run with:
//!
//!     cargo run -p backend --example raster_smoke
//!
//! Writes `/tmp/gitdebt_chart.png` and `/tmp/gitdebt_chart.webp` for
//! visual inspection. Useful when iterating on the animation freezer
//! or the font setup — `cargo test` only checks the byte magic, not
//! whether the text/lines/bars actually look right.

use chrono::{Duration, Utc};
use gitdebt::chart::{ChartConfig, ChartOpts, Point, render_svg};
use gitdebt::raster::{RasterFormat, rasterize};
use gitdebt::theme;

fn main() {
    let points = vec![
        Point {
            at: Utc::now() - Duration::days(30),
            stars: 10,
        },
        Point {
            at: Utc::now() - Duration::days(15),
            stars: 100,
        },
        Point {
            at: Utc::now(),
            stars: 300,
        },
    ];
    let cfg = ChartConfig {
        repo: "foo/bar".to_string(),
        ..ChartConfig::default()
    };
    let svg = render_svg(&points, &cfg, &theme::LIGHT, &ChartOpts::default());
    println!("svg bytes: {}", svg.len());

    let png = rasterize(&svg, RasterFormat::Png, 2.0).expect("png encode");
    std::fs::write("/tmp/gitdebt_chart.png", &png).expect("write png");
    println!("png bytes: {} → /tmp/gitdebt_chart.png", png.len());

    let webp = rasterize(&svg, RasterFormat::Webp, 2.0).expect("webp encode");
    std::fs::write("/tmp/gitdebt_chart.webp", &webp).expect("write webp");
    println!("webp bytes: {} → /tmp/gitdebt_chart.webp", webp.len());
}
