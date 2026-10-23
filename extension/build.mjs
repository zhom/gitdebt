import {
  copyFileSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync
} from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { crc32, deflateRawSync } from "node:zlib";

/**
 * Stage, lint and package the extension.
 *
 * `node build.mjs --lint` lints the staged extension with addons-linter, with
 * the options `web-ext lint` passes it. `node build.mjs` writes the Chrome and
 * Firefox archives. web-ext is not a dependency: it pulled an Android device
 * bridge, and through it node-forge, into a repository that never runs on a
 * device and only needs a linter and a zip.
 */

const root = dirname(fileURLToPath(import.meta.url));
const manifest = JSON.parse(readFileSync(join(root, "manifest.json"), "utf8"));
const packageJson = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
if (manifest.version !== packageJson.version) {
  throw new Error("manifest.json and package.json versions must match");
}

const dist = join(root, "dist");
const staging = join(dist, ".staging", "extension");

/** Repository files that are not part of the extension. */
const IGNORED = new Set([
  "package.json",
  "package-lock.json",
  "node_modules",
  "build.mjs",
  "dist",
  "README.md",
  "PRIVACY.md",
  "web-ext-artifacts",
  "test"
]);

/** The extension's files, as sorted forward-slash paths. Dotfiles and
 *  archives are skipped at any depth, as web-ext skipped them. */
function extensionFiles(dir = root, prefix = "") {
  const files = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.name.startsWith(".") || /\.(zip|xpi)$/i.test(entry.name)) continue;
    if (!prefix && IGNORED.has(entry.name)) continue;
    if (entry.isDirectory()) files.push(...extensionFiles(join(dir, entry.name), path));
    else if (entry.isFile()) files.push(path);
  }
  return files.sort();
}

function stage(files) {
  rmSync(join(dist, ".staging"), { recursive: true, force: true });
  for (const file of files) {
    const target = join(staging, ...file.split("/"));
    mkdirSync(dirname(target), { recursive: true });
    copyFileSync(join(root, ...file.split("/")), target);
  }
}

async function lint() {
  const { createInstance } = await import("addons-linter");
  const linter = createInstance({
    config: {
      _: [staging],
      logLevel: "fatal",
      stack: false,
      pretty: false,
      privileged: false,
      warningsAsErrors: false,
      metadata: false,
      output: "none",
      boring: true,
      selfHosted: false,
      minManifestVersion: 2,
      maxManifestVersion: 3,
      enableDataCollectionPermissions: true
    },
    runAsBinary: false
  });
  const result = await linter.run();
  const { errors, notices, warnings } = result.summary;
  for (const kind of ["errors", "warnings", "notices"]) {
    for (const message of result[kind]) {
      console.log(`${kind.slice(0, -1).toUpperCase()} ${message.code} ${message.file ?? ""}\n  ${message.message}`);
    }
  }
  console.log(`errors ${errors}  notices ${notices}  warnings ${warnings}`);
  return errors === 0;
}

/**
 * A zip archive with one entry per file, deflated where that is smaller.
 * Every entry carries the same 1980-01-01 timestamp, so identical sources
 * produce identical bytes.
 */
function zip(files) {
  const DOS_DATE = (0 << 9) | (1 << 5) | 1;
  const UTF8 = 0x0800;
  const parts = [];
  const central = [];
  let offset = 0;
  for (const file of files) {
    const data = readFileSync(join(staging, ...file.split("/")));
    const name = Buffer.from(file, "utf8");
    const deflated = deflateRawSync(data, { level: 9 });
    const stored = deflated.length >= data.length;
    const body = stored ? data : deflated;
    const crc = crc32(data);

    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(UTF8, 6);
    local.writeUInt16LE(stored ? 0 : 8, 8);
    local.writeUInt16LE(0, 10);
    local.writeUInt16LE(DOS_DATE, 12);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(body.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(name.length, 26);
    parts.push(local, name, body);

    const entry = Buffer.alloc(46);
    entry.writeUInt32LE(0x02014b50, 0);
    entry.writeUInt16LE(20, 4);
    entry.writeUInt16LE(20, 6);
    entry.writeUInt16LE(UTF8, 8);
    entry.writeUInt16LE(stored ? 0 : 8, 10);
    entry.writeUInt16LE(0, 12);
    entry.writeUInt16LE(DOS_DATE, 14);
    entry.writeUInt32LE(crc, 16);
    entry.writeUInt32LE(body.length, 20);
    entry.writeUInt32LE(data.length, 24);
    entry.writeUInt16LE(name.length, 28);
    entry.writeUInt32LE(offset, 42);
    central.push(entry, name);
    offset += local.length + name.length + body.length;
  }
  const size = central.reduce((total, buffer) => total + buffer.length, 0);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(files.length, 8);
  end.writeUInt16LE(files.length, 10);
  end.writeUInt32LE(size, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...parts, ...central, end]);
}

const files = extensionFiles();
stage(files);

if (process.argv.includes("--lint")) {
  const clean = await lint();
  rmSync(join(dist, ".staging"), { recursive: true, force: true });
  process.exit(clean ? 0 : 1);
}

console.log(`Building gitdebt extension v${manifest.version}`);
const archive = zip(files);
for (const browser of ["chrome", "firefox"]) {
  const destination = join(dist, `gitdebt-${browser}-${manifest.version}.zip`);
  writeFileSync(destination, archive);
  console.log(destination);
}
rmSync(join(dist, ".staging"), { recursive: true, force: true });
