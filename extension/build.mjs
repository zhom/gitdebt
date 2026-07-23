import { execFileSync } from "node:child_process";
import {
  copyFileSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync
} from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(fileURLToPath(import.meta.url));
const manifest = JSON.parse(readFileSync(join(root, "manifest.json"), "utf8"));
const packageJson = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
if (manifest.version !== packageJson.version) {
  throw new Error("manifest.json and package.json versions must match");
}

const dist = join(root, "dist");
const staging = join(dist, ".staging");
const ignored = [
  "package.json",
  "package-lock.json",
  "node_modules",
  "build.mjs",
  "dist",
  "README.md",
  "PRIVACY.md",
  ".gitignore",
  "web-ext-artifacts",
  "test",
  "**/.DS_Store"
];
const bin = join(
  root,
  "node_modules",
  ".bin",
  process.platform === "win32" ? "web-ext.cmd" : "web-ext"
);

mkdirSync(dist, { recursive: true });
rmSync(staging, { recursive: true, force: true });
mkdirSync(staging, { recursive: true });

const args = [
  "build",
  "--source-dir",
  root,
  "--artifacts-dir",
  staging,
  "--overwrite-dest"
];
for (const pattern of ignored) args.push("--ignore-files", pattern);

console.log(`Building gitdebt extension v${manifest.version}`);
execFileSync(bin, args, { stdio: "inherit" });

const archive = readdirSync(staging).find((file) => file.endsWith(".zip"));
if (!archive) throw new Error("web-ext produced no zip archive");

const source = join(staging, archive);
for (const browser of ["chrome", "firefox"]) {
  const destination = join(
    dist,
    `gitdebt-${browser}-${manifest.version}.zip`
  );
  copyFileSync(source, destination);
  console.log(destination);
}
rmSync(staging, { recursive: true, force: true });
