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
//! Default is `light` (matches star-history) so a bare URL is always
//! readable on the white background of a fresh tab.

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
    fg: "#0f172a",
    muted: "#64748b",
    border: "#cbd5e1",
    grid: "#e2e8f0",
    track: "#e5e7eb",
    accent: "#2563eb",
    accent_dim: "#1d4ed8",
    bug: "#ef4444",
    bug_dim: "#b91c1c",
    heat_0: "#ebedf0",
    heat_1: "#9be9a8",
    heat_2: "#40c463",
    heat_3: "#30a14e",
    heat_4: "#216e39",
};

pub static DARK: Theme = Theme {
    dark: true,
    bg: "#0d1117",
    fg: "#e2e8f0",
    muted: "#94a3b8",
    border: "#334155",
    grid: "#1e293b",
    track: "#1f2937",
    accent: "#60a5fa",
    accent_dim: "#93c5fd",
    bug: "#f87171",
    bug_dim: "#fca5a5",
    heat_0: "#161b22",
    heat_1: "#0e4429",
    heat_2: "#006d32",
    heat_3: "#26a641",
    heat_4: "#39d353",
};

pub fn theme_for(name: Option<&str>) -> &'static Theme {
    match name {
        Some(s) if s.eq_ignore_ascii_case("dark") => &DARK,
        // Anything else (including unset, "light", garbage) → light.
        _ => &LIGHT,
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
            if yiq >= 145 { "#0f172a" } else { "#ffffff" }
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
        assert_eq!(contrast_on("#0f172a"), "#ffffff");
        assert_eq!(contrast_on("#000000"), "#ffffff");
    }

    #[test]
    fn contrast_picks_dark_on_light_bg() {
        assert_eq!(contrast_on("#ffffff"), "#0f172a");
        assert_eq!(contrast_on("#f1e05a"), "#0f172a"); // yellow JS bar
    }

    #[test]
    fn theme_for_defaults_to_light() {
        assert!(std::ptr::eq(theme_for(None), &LIGHT));
        assert!(std::ptr::eq(theme_for(Some("garbage")), &LIGHT));
    }

    #[test]
    fn theme_for_dark_case_insensitive() {
        assert!(std::ptr::eq(theme_for(Some("dark")), &DARK));
        assert!(std::ptr::eq(theme_for(Some("DARK")), &DARK));
    }
}
