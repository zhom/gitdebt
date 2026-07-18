export function staticApiBase(): string {
  const configured = import.meta.env.PUBLIC_API_BASE;
  if (configured) return configured.replace(/\/+$/, "");
  if (import.meta.env.PROD) {
    throw new Error(
      "PUBLIC_API_BASE is not set. Prerendered pages bake it into static " +
        "og:image and chart/badge URLs at build time; export PUBLIC_API_BASE " +
        "(e.g. https://api.gitdebt.com) before `astro build`.",
    );
  }
  return "http://localhost:8787";
}
