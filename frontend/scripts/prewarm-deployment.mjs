import { readdir } from "node:fs/promises";
import path from "node:path";
import { isReservedFirstSegment } from "../src/lib/static-routing.mjs";

const apiBase = (process.env.PUBLIC_API_BASE ?? "https://api.gitdebt.com").replace(/\/$/, "");
const limit = Math.min(1_000, Math.max(1, Number(process.env.STATIC_REPO_LIMIT ?? 1_000)));
const stats = [
  "bug-magnets",
  "top-files",
  "contributors",
  "lines",
  "heatmap",
  "todo-trend",
  "bus-factor",
  "commit-trend",
];

async function request(path, init = {}) {
  const response = await fetch(`${apiBase}${path}`, {
    ...init,
    headers: { accept: "application/json,image/svg+xml", ...init.headers },
    signal: AbortSignal.timeout(15_000),
  });
  await response.arrayBuffer();
  return response.status;
}

const catalogResponse = await fetch(`${apiBase}/api/sitemap/repos?page=0&per=${limit}`, {
  headers: { accept: "application/json" },
  signal: AbortSignal.timeout(15_000),
});
if (!catalogResponse.ok) {
  throw new Error(`catalog returned ${catalogResponse.status}`);
}
const catalog = await catalogResponse.json();
const apiRepos = Array.isArray(catalog.repos)
  ? catalog.repos.map((row) => row?.slug).filter((slug) => typeof slug === "string")
  : [];
const repos = new Set(apiRepos);
const dist = path.resolve("dist");
for (const owner of await readdir(dist, { withFileTypes: true })) {
  if (!owner.isDirectory() || isReservedFirstSegment(owner.name)) continue;
  for (const file of await readdir(path.join(dist, owner.name), { withFileTypes: true })) {
    if (!file.isFile() || !file.name.endsWith(".html")) continue;
    const repo = file.name.slice(0, -5);
    if (/^[A-Za-z0-9._-]+$/.test(owner.name) && /^[A-Za-z0-9._-]+$/.test(repo)) {
      repos.add(`${owner.name}/${repo}`.toLowerCase());
    }
  }
}
const catalogRepos = [...repos].sort();

let cursor = 0;
let failures = 0;
async function worker() {
  while (cursor < catalogRepos.length) {
    const repo = catalogRepos[cursor++];
    try {
      await request(`/api/repos/${repo}/analyze`);
      await request(`/api/repos/${repo}/analyze-history`, { method: "POST" });
      for (const theme of ["light", "dark"]) {
        await request(`/api/repos/${repo}/chart.svg?theme=${theme}`);
        for (const stat of stats) {
          await request(`/api/repos/${repo}/stats/${stat}.svg?theme=${theme}`);
        }
      }
    } catch (error) {
      failures += 1;
      console.warn(`prewarm failed for ${repo}: ${error instanceof Error ? error.message : error}`);
    }
  }
}

await Promise.all(Array.from({ length: Math.min(6, Math.max(1, catalogRepos.length)) }, worker));
console.log(`Prewarmed ${catalogRepos.length - failures}/${catalogRepos.length} catalog repositories`);
if (failures > Math.max(2, Math.ceil(catalogRepos.length * 0.1))) process.exitCode = 1;
