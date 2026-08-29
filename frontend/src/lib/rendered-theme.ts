export type RenderedTheme = "light" | "dark";

/**
 * The theme every on-site server-rendered image asks for.
 *
 * The site is a sheet of white paper, so an asset embedded in a page is the
 * light print — the same drawing the page around it is, in the same ink. This
 * returned `"dark"` while the site was dark, and kept returning it after the
 * site moved to paper, which put the dark print of every chart, card and badge
 * on a white page.
 *
 * It agrees with the renderer rather than restating it: `backend/src/theme.rs`
 * now defaults a bare URL to the light print, so this asks for what the API
 * would have served anyway and the two can no longer disagree silently.
 *
 * README embed generators still parameterize both themes for off-site
 * contexts; they pass explicit theme values instead of this hook.
 */
export function useRenderedTheme(): RenderedTheme {
  return "light";
}
