//! Concrete-color themes baked at render time.
//!
//! Why not CSS `prefers-color-scheme` + variables? Because SVGs served
//! through `<img>` (the way READMEs render them via GitHub's camo proxy)
//! evaluate `prefers-color-scheme` against the user's **OS preference**,
//! not the embedding page's theme. A user with light OS reading GitHub
//! in dark mode would still see a light-themed chart inside a dark page.
//!
//! Star-history's solution — and ours — is to bake the theme directly:
//! emit fixed hex colors per-element. Embedders pick a theme via
//! `?theme=light|dark`, and combine the two SVGs at the embed site:
//!
//! ```html
//! <picture>
//!   <source media="(prefers-color-scheme: dark)"
//!           srcset="https://gitdebt.com/.../chart.svg?theme=dark" />
//!   <img src="https://gitdebt.com/.../chart.svg?theme=light" />
//! </picture>
//! ```
//!
//! Default is `dark`: the product surface is dark-first, so a bare URL
//! matches the site and the near-black canvas stays readable everywhere.
//! Embedders that need a light asset opt in with `?theme=light`.

use serde::Deserialize;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// True for dark, false for light. Lets per-chart code pick the
    /// correct categorical palette (and any non-palette-tracked colors)
    /// without a pointer-equality dance.
    pub dark: bool,
    /// Opaque canvas used by raster exports. SVG colors are still baked
    /// per theme, while this background ensures GIF/PNG embeds remain
    /// readable when a host does not preserve transparency.
    pub bg: &'static str,
    pub fg: &'static str,
    pub muted: &'static str,
    pub border: &'static str,
    pub grid: &'static str,
    pub track: &'static str,
    pub accent: &'static str,
    pub accent_dim: &'static str,
    pub bug: &'static str,
    pub bug_dim: &'static str,
    pub heat_0: &'static str,
    pub heat_1: &'static str,
    pub heat_2: &'static str,
    pub heat_3: &'static str,
    pub heat_4: &'static str,
}

// `static` (not `const`) so callers see one canonical address — `const`
// items get inlined per use-site, breaking pointer-equality checks like
// `std::ptr::eq(theme_for(...), &LIGHT)`.
pub static LIGHT: Theme = Theme {
    dark: false,
    bg: "#ffffff",
    fg: "#0a0a0a",
    muted: "#525252",
    border: "#d4d4d4",
    grid: "#e5e5e5",
    track: "#ededed",
    accent: "#0a0a0a",
    accent_dim: "#404040",
    bug: "#0a0a0a",
    bug_dim: "#525252",
    heat_0: "#f5f5f5",
    heat_1: "#d4d4d4",
    heat_2: "#a3a3a3",
    heat_3: "#737373",
    heat_4: "#262626",
};

pub static DARK: Theme = Theme {
    dark: true,
    bg: "#0a0a0a",
    fg: "#fafafa",
    muted: "#a3a3a3",
    border: "#404040",
    grid: "#262626",
    track: "#171717",
    accent: "#fafafa",
    accent_dim: "#d4d4d4",
    bug: "#fafafa",
    bug_dim: "#a3a3a3",
    heat_0: "#171717",
    heat_1: "#404040",
    heat_2: "#737373",
    heat_3: "#a3a3a3",
    heat_4: "#f5f5f5",
};

pub fn theme_for(name: Option<&str>) -> &'static Theme {
    match name {
        Some(s) if s.eq_ignore_ascii_case("light") => &LIGHT,
        // Anything else (including unset, "dark", garbage) → dark.
        _ => &DARK,
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct ThemeQuery {
    pub theme: Option<String>,
}

/// Pick black-ish or white based on which contrasts better with `hex`.
/// YIQ luminance ≥ 145 → dark text reads better; below → white.
pub fn contrast_on(hex: &str) -> &'static str {
    match parse_hex_rgb(hex) {
        Some((r, g, b)) => {
            let yiq = (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000;
            if yiq >= 145 { "#0a0a0a" } else { "#ffffff" }
        }
        None => "#ffffff",
    }
}

fn parse_hex_rgb(s: &str) -> Option<(u8, u8, u8)> {
    if !s.starts_with('#') || s.len() != 7 {
        return None;
    }
    let r = u8::from_str_radix(&s[1..3], 16).ok()?;
    let g = u8::from_str_radix(&s[3..5], 16).ok()?;
    let b = u8::from_str_radix(&s[5..7], 16).ok()?;
    Some((r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_picks_white_on_dark_bg() {
        assert_eq!(contrast_on("#0a0a0a"), "#ffffff");
        assert_eq!(contrast_on("#000000"), "#ffffff");
    }

    #[test]
    fn contrast_picks_dark_on_light_bg() {
        assert_eq!(contrast_on("#ffffff"), "#0a0a0a");
        assert_eq!(contrast_on("#f5f5f5"), "#0a0a0a");
    }

    #[test]
    fn theme_for_defaults_to_dark() {
        assert!(std::ptr::eq(theme_for(None), &DARK));
        assert!(std::ptr::eq(theme_for(Some("garbage")), &DARK));
        assert!(std::ptr::eq(theme_for(Some("dark")), &DARK));
    }

    #[test]
    fn theme_for_light_case_insensitive() {
        assert!(std::ptr::eq(theme_for(Some("light")), &LIGHT));
        assert!(std::ptr::eq(theme_for(Some("LIGHT")), &LIGHT));
    }
}
