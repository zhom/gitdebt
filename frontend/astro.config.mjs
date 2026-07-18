// @ts-check
import { defineConfig } from "astro/config";
import react from "@astrojs/react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  output: "static",
  integrations: [react()],
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
    plugins: [tailwindcss()],
  },
  server: {
    port: 14321,
  },
});
