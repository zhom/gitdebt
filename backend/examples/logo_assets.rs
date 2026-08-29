//! Regenerate every shipped logo and icon from `assets/gitdebt-mark.svg`.
//!
//! Run from anywhere in the workspace:
//!
//!     cargo run -p backend --example logo_assets

use std::fs;
use std::path::{Path, PathBuf};

use gitdebt::raster::{RasterFormat, rasterize};

const SOURCE_SIZE: f32 = 512.0;

/// Ink bounds of the robot path inside the 512 artboard. Icons place the
/// glyph by these bounds so a 16px raster spends its pixels on the mark
/// rather than on the artwork's 40% of empty vertical margin.
const INK_X: f32 = 41.436;
const INK_Y: f32 = 108.392;
const INK_W: f32 = 429.115;
const INK_H: f32 = 299.305;
/// Fraction of the icon plate the glyph spans.
const ICON_FILL: f32 = 0.94;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("backend is inside workspace root");
    let source_path = root.join("assets/gitdebt-mark.svg");
    let mark = fs::read_to_string(&source_path).expect("read background-free robot mark");
    validate_mark(&mark);

    let icon = app_icon_svg(&solid_body(&mark, "#fff"));

    write(&root.join("assets/gitdebt-logo.svg"), mark.as_bytes());
    write(&root.join("frontend/public/logo.svg"), mark.as_bytes());

    for destination in [
        root.join("frontend/public/favicon.svg"),
        root.join("extension/icons/icon.svg"),
    ] {
        write(&destination, icon.as_bytes());
    }

    for (destination, size) in [
        (root.join("frontend/public/icon-192.png"), 192),
        (root.join("frontend/public/icon-512.png"), 512),
        (root.join("frontend/public/favicon-16.png"), 16),
        (root.join("frontend/public/favicon-32.png"), 32),
        (root.join("extension/icons/icon-16.png"), 16),
        (root.join("extension/icons/icon-32.png"), 32),
        (root.join("extension/icons/icon-48.png"), 48),
        (root.join("extension/icons/icon-128.png"), 128),
    ] {
        let png =
            rasterize(&icon, RasterFormat::Png, size as f32 / SOURCE_SIZE).expect("rasterize logo");
        write(&destination, &png);
    }

    for (destination, size) in [
        (root.join("frontend/public/apple-touch-icon.png"), 180),
        (root.join("frontend/public/icon-maskable-192.png"), 192),
        (root.join("frontend/public/icon-maskable-512.png"), 512),
    ] {
        let png = rasterize(
            &maskable_svg(&icon),
            RasterFormat::Png,
            size as f32 / SOURCE_SIZE,
        )
        .expect("rasterize maskable logo");
        write(&destination, &png);
    }
}

fn validate_mark(mark: &str) {
    assert!(mark.contains("width=\"512\" height=\"512\""));
    assert!(mark.contains("viewBox=\"0 0 512 512\""));
    assert!(
        !mark.contains("<rect"),
        "canonical mark must have a transparent background"
    );
    assert!(
        !mark.contains("<image"),
        "mark must remain native vector art"
    );
    assert!(
        !mark.contains("filter"),
        "mark must not contain raster effects"
    );
}

/// The robot path alone, re-inked from the artwork's authored black.
fn solid_body(mark: &str, ink: &str) -> String {
    svg_body(mark).replace("fill=\"#000\"", &format!("fill=\"{ink}\""))
}

/// Rounded plate with the glyph centred by its ink bounds.
fn app_icon_svg(body: &str) -> String {
    let scale = ICON_FILL * SOURCE_SIZE / INK_W;
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"512\" height=\"512\" viewBox=\"0 0 512 512\" role=\"img\" aria-label=\"gitdebt robot\"><rect width=\"512\" height=\"512\" rx=\"112\" fill=\"#000\"/><g transform=\"translate(256 256) scale({scale:.5}) translate({tx:.3} {ty:.3})\">{body}</g></svg>",
        tx = -(INK_X + INK_W / 2.0),
        ty = -(INK_Y + INK_H / 2.0),
    )
}

fn maskable_svg(svg: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"512\" height=\"512\" viewBox=\"0 0 512 512\"><rect width=\"512\" height=\"512\" fill=\"#000\"/><g transform=\"translate(51.2 51.2) scale(.8)\">{}</g></svg>",
        svg_body(svg)
    )
}

fn svg_body(svg: &str) -> &str {
    let body_start = svg.find('>').expect("svg opening tag") + 1;
    let body_end = svg.rfind("</svg>").expect("svg closing tag");
    &svg[body_start..body_end]
}

fn write(path: &PathBuf, bytes: &[u8]) {
    fs::create_dir_all(path.parent().expect("asset has parent")).expect("create asset directory");
    fs::write(path, bytes).expect("write generated logo asset");
    println!("{} ({} bytes)", path.display(), bytes.len());
}
