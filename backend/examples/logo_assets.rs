//! Regenerate every shipped logo and icon from `assets/gitdebt-mark.svg`.
//!
//! Run from anywhere in the workspace:
//!
//!     cargo run -p backend --example logo_assets

use std::fs;
use std::path::{Path, PathBuf};

use gitdebt::raster::{RasterFormat, rasterize};

const SOURCE_SIZE: f32 = 512.0;
const MARK_PATTERN: &str = "M0 0h3v3H0zM4 0h3v3H4zM8 0h3v3H8zM12 0h3v3h-3zM4 4h3v3H4zM12 4h3v3h-3zM0 8h3v3H0zM4 8h3v3H4zM8 8h3v3H8zM12 8h3v3h-3zM4 12h3v3H4zM12 12h3v3h-3z";
const ICON_PATTERN: &str = "M0 0h24v24H0zM25 25h7v7h-7zM0 27h5v5H0zM27 0h5v5h-5z";

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("backend is inside workspace root");
    let source_path = root.join("assets/gitdebt-mark.svg");
    let mark = fs::read_to_string(&source_path).expect("read background-free robot mark");
    validate_mark(&mark);
    let icon = app_icon_svg(&mark);
    let maskable = maskable_svg(&icon);

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
        let png = rasterize(&maskable, RasterFormat::Png, size as f32 / SOURCE_SIZE)
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
    assert!(
        mark.contains(MARK_PATTERN),
        "canonical dither pattern changed"
    );
}

fn app_icon_svg(mark: &str) -> String {
    let light_mark = svg_body(mark)
        .replace("width=\"16\" height=\"16\"", "width=\"32\" height=\"32\"")
        .replace(MARK_PATTERN, ICON_PATTERN)
        .replace("fill=\"#000\"", "fill=\"#fff\"");
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"512\" height=\"512\" viewBox=\"0 0 512 512\" role=\"img\" aria-label=\"gitdebt robot\"><rect width=\"512\" height=\"512\" rx=\"112\" fill=\"#000\"/>{light_mark}</svg>"
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
