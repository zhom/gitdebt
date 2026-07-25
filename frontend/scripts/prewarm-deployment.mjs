import { readdir } from "node:fs/promises";
import path from "node:path";
import { isReservedFirstSegment } from "../src/lib/static-routing.mjs";

const apiBase = (process.env.PUBLIC_API_BASE ?? "https://api.gitdebt.com").replace(/\/$/, "");
const limit = Math.min(8_000, Math.max(1, Number(process.env.STATIC_REPO_LIMIT ?? 3_000)));
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

const laneInterval = { analyze: 525, mutate: 1_050, image: 105 };
const nextStart = { analyze: 0, mutate: 0, image: 0 };

async function pace(lane) {
  const now = Date.now();
  const start = Math.max(now, nextStart[lane]);
  nextStart[lane] = start + laneInterval[lane];
  if (start > now) {
    await new Promise((resolve) => setTimeout(resolve, start - now));
  }
}

async function request(path, lane, init = {}) {
  let detail = "request failed";
  for (let attempt = 0; attempt < 5; attempt += 1) {
    await pace(lane);
    const response = await fetch(`${apiBase}${path}`, {
      ...init,
      headers: { accept: "application/json,image/svg+xml", ...init.headers },
      signal: AbortSignal.timeout(20_000),
    });
    await response.arrayBuffer();
    if (response.ok) return response.status;
    detail = `${response.status} ${response.statusText}`.trim();
    const retryable =
      response.status === 408 ||
      response.status === 425 ||
      response.status === 429 ||
      response.status >= 500;
    if (!retryable) break;
    const retryAfter = Number(response.headers.get("retry-after"));
    const delay =
      Number.isFinite(retryAfter) && retryAfter > 0
        ? retryAfter * 1_000
        : Math.min(8_000, 500 * 2 ** attempt);
    await new Promise((resolve) => setTimeout(resolve, delay));
  }
  throw new Error(`${path}: ${detail}`);
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
      await request(`/api/repos/${repo}/analyze`, "analyze");
      await request(`/api/repos/${repo}/analyze-history`, "mutate", {
        method: "POST",
      });
      for (const theme of ["light", "dark"]) {
        await request(`/api/repos/${repo}/chart.svg?theme=${theme}`, "image");
        for (const stat of stats) {
          await request(
            `/api/repos/${repo}/stats/${stat}.svg?theme=${theme}`,
            "image",
          );
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
