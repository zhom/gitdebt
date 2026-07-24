import assert from "node:assert/strict";
import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("../src/pages/", import.meta.url));

async function astroPages(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) files.push(...(await astroPages(path)));
    else if (entry.name.endsWith(".astro")) files.push(path);
  }
  return files;
}

test("Astro pages do not render inert buttonVariants lookalikes", async () => {
  const offenders = [];
  for (const path of await astroPages(ROOT)) {
    const source = await readFile(path, "utf8");
    if (source.includes("buttonVariants(")) offenders.push(path);
  }
  assert.deepEqual(
    offenders,
    [],
    "button-shaped links must use ButtonLink so their dither pulse hydrates",
  );
});

test("quiet ButtonLink actions can opt into a pulse without a resting fill", async () => {
  const source = await readFile(
    new URL("../src/components/ButtonLink.tsx", import.meta.url),
    "utf8",
  );
  assert.match(source, /pulseEnabled = pulse \?\? textured/);
  assert.match(
    source,
    /alpha: textured \? CANVAS_ALPHA\[variant \?\? "default"\] : 0/,
  );
});
