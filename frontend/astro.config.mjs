// @ts-check
import fs from "node:fs/promises";
import { defineConfig } from "astro/config";
import react from "@astrojs/react";
import tailwindcss from "@tailwindcss/vite";

/**
 * The API origin Vite resolved for this build, i.e. the exact value
 * `staticApiBase()` bakes into `og:image`, chart, badge and
 * `<link rel="alternate">` URLs. Captured from Vite rather than re-read from
 * `process.env` so `.env` files, `--mode` overrides and shell exports all
 * resolve identically; the two documented agent entry points
 * (`<link rel="alternate">` and `/{path}.md`) must never point at different
 * backends.
 * @type {string | null}
 */
let resolvedApiBase = null;
// Defaults to the strict reading: if the capture plugin never ran we know
// nothing about this build, and guessing a localhost origin would ship a
// `_redirects` that points production at a developer's machine.
let productionBuild = true;

const captureApiBase = {
  name: "gitdebt:capture-api-base",
  /** @param {{ env: Record<string, unknown>; isProduction: boolean }} config */
  configResolved(config) {
    const configured = config.env.PUBLIC_API_BASE;
    resolvedApiBase =
      typeof configured === "string" && configured ? configured : null;
    productionBuild = config.isProduction;
  },
};

function apiBaseForRedirects() {
  if (resolvedApiBase) return resolvedApiBase.replace(/\/+$/, "");
  if (productionBuild) {
    throw new Error(
      "PUBLIC_API_BASE is not set. Cloudflare's `_redirects` sends " +
        "`/{path}.md` to the API at build time; export PUBLIC_API_BASE " +
        "(e.g. https://api.gitdebt.com) before `astro build`.",
    );
  }
  return "http://localhost:8787";
}

/**
 * The only source of `_redirects`. It cannot live in `public/` because the
 * Markdown destinations have to carry the per-deploy API origin, and it cannot
 * be an Astro endpoint because `src/pages/` skips every `_`-prefixed entry.
 * @param {string} apiBase
 */
function pagesRedirects(apiBase) {
  return `/about/ /about 301
/badges/ /badges 301
/compare/ /compare 301
/leaderboard/ /leaderboard 301
/report/ /report 301
/privacy/ /privacy 301
/profile/ /profile 301
/terms/ /terms 301
# \`/*.md\` captures the site path without the extension, and the home page's
# path is empty, so \`/.md\` already lands on \`/api/md/\`. \`/index.md\` does not:
# \`index\` would ride through as a literal path segment. It has to stay above
# the first placeholder rule — Cloudflare stops classifying rules as static
# once it has seen a dynamic one, and only static rules are matched ahead of
# every splat.
/index.md ${apiBase}/api/md/ 302
/u/:login/ /:login 301
/u/:login /:login 301
/:first/:second/ /:first/:second 301
/vs/:owner1/:repo1/:owner2/:repo2/ /vs/:owner1/:repo1/:owner2/:repo2 301
# Every Markdown representation is rendered live by the API; the site emits
# none. Redirects are evaluated before asset lookup, so reintroducing a
# prerendered \`.md\` page would be shadowed by this rule and silently never
# served. Declared after \`/u/:login\` so a legacy \`/u/{login}.md\` canonicalizes
# to \`/{login}.md\` first and reaches \`/api/md/{login}\`, not \`/api/md/u/{login}\`.
/*.md ${apiBase}/api/md/:splat 302
`;
}

/** @type {import("astro").AstroIntegration} */
const emitPagesRedirects = {
  name: "gitdebt:pages-redirects",
  hooks: {
    "astro:build:done": async ({ dir, logger }) => {
      const apiBase = apiBaseForRedirects();
      await fs.writeFile(
        new URL("./_redirects", dir),
        pagesRedirects(apiBase),
        "utf8",
      );
      logger.info(`_redirects: Markdown routes resolve to ${apiBase}/api/md/`);
    },
  },
};

export default defineConfig({
  output: "static",
  build: {
    // Cloudflare Pages serves `route.html` at `/route`. Directory output would
    // normalize `/route` to `/route/`, fighting our no-trailing-slash canonicals.
    format: "file",
  },
  integrations: [react(), emitPagesRedirects],
  site: process.env.PUBLIC_SITE_URL ?? "https://gitdebt.com",
  trailingSlash: "never",
  prerenderConflictBehavior: "error",
  security: {
    csp: {
      directives: [
        "default-src 'self'",
        "base-uri 'self'",
        "connect-src 'self' https:",
        "font-src 'self' data:",
        "form-action 'self'",
        "frame-src 'none'",
        "img-src 'self' data: https:",
        "manifest-src 'self'",
        "object-src 'none'",
        "worker-src 'self' blob:",
      ],
      styleDirective: {
        resources: [
          { resource: "'self'", kind: "element" },
          { resource: "'unsafe-inline'", kind: "attribute" },
        ],
      },
    },
  },
  markdown: {
    syntaxHighlight: false,
  },
  vite: {
    plugins: [tailwindcss(), captureApiBase],
    build: {
      minify: "oxc",
      cssMinify: "lightningcss",
      sourcemap: false,
    },
  },
  server: {
    port: 14321,
  },
});
