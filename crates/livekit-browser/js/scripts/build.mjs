import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const output = resolve(root, "livekit-browser.js");
const checking = process.argv.includes("--check");

const result = await build({
  entryPoints: [resolve(root, "src/index.ts")],
  outfile: output,
  bundle: true,
  format: "esm",
  platform: "browser",
  target: "es2022",
  charset: "ascii",
  legalComments: "none",
  sourcemap: false,
  logLevel: "info",
  write: !checking,
  banner: {
    js: "// Generated from TypeScript by npm run build. Do not edit directly.",
  },
});

if (checking) {
  const generated = result.outputFiles?.[0]?.contents;
  assert(generated, "esbuild did not produce livekit-browser.js");
  assert.deepEqual(
    Buffer.from(generated),
    await readFile(output),
    "livekit-browser.js is stale; run npm run build and commit the result",
  );
}
