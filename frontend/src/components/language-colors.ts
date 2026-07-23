import type { RGB } from "@/lib/dither";

/**
 * Linguist language colors, mirroring the server-side table so a language bar
 * renders the same hue in the app and in the exported SVG. Unlisted languages
 * fall back to a neutral grey.
 */
const LANGUAGE_HEX: Record<string, string> = {
  assembly: "#6e4c13",
  astro: "#ff5a03",
  c: "#555555",
  "c#": "#178600",
  "c++": "#f34b7d",
  clojure: "#db5855",
  cmake: "#da3434",
  css: "#663399",
  dart: "#00b4ab",
  dockerfile: "#384d54",
  elixir: "#6e4a7e",
  elm: "#60b5cc",
  erlang: "#b83998",
  fortran: "#4d41b1",
  "f#": "#b845fc",
  go: "#00add8",
  graphql: "#e10098",
  groovy: "#4298b8",
  haskell: "#5e5086",
  hcl: "#844fba",
  html: "#e34c26",
  java: "#b07219",
  javascript: "#f1e05a",
  json: "#292929",
  julia: "#a270ba",
  kotlin: "#a97bff",
  less: "#1d365d",
  lua: "#000080",
  makefile: "#427819",
  markdown: "#083fa1",
  matlab: "#e16737",
  nim: "#ffc200",
  "objective-c": "#438eff",
  ocaml: "#3be133",
  perl: "#0298c3",
  php: "#4f5d95",
  powershell: "#012456",
  protobuf: "#7fa2a0",
  python: "#3572a5",
  r: "#198ce7",
  ruby: "#701516",
  rust: "#dea584",
  scala: "#c22d40",
  scss: "#c6538c",
  shell: "#89e051",
  solidity: "#aa6746",
  sql: "#e38c00",
  svelte: "#ff3e00",
  swift: "#f05138",
  toml: "#9c4221",
  typescript: "#3178c6",
  vim: "#199f4b",
  vue: "#41b883",
  yaml: "#cb171e",
  zig: "#ec915c",
};

const FALLBACK: RGB = [139, 139, 139];

function hexToRgb(hex: string): RGB {
  const value = Number.parseInt(hex.slice(1), 16);
  return [(value >> 16) & 255, (value >> 8) & 255, value & 255];
}

export function languageColor(language: string): RGB {
  const hex = LANGUAGE_HEX[language.trim().toLowerCase()];
  return hex ? hexToRgb(hex) : FALLBACK;
}
