export type RenderedTheme = "light" | "dark";

/**
 * The site renders dark-only, so every on-site server image request uses
 * theme=dark. README embed generators still parameterize both themes for
 * off-site contexts; they pass explicit theme values instead of this hook.
 */
export function useRenderedTheme(): RenderedTheme {
  return "dark";
}
