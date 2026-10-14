// Bundles src/julia-engine.ts into the extension. Run from the repository
// root (`npm run build`): esbuild embeds entry paths as relative comments, so
// a fixed working directory keeps the output byte-stable.
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { build } from "esbuild";

process.chdir(join(dirname(fileURLToPath(import.meta.url)), ".."));

const { outputFiles } = await build({
  entryPoints: ["src/julia-engine.ts"],
  bundle: true,
  write: false,
  format: "esm",
  platform: "node",
  target: "node24",
  external: ["node:*"],
  logLevel: "warning",
  legalComments: "none",
});
// LF on every platform so the committed bundle is byte-identical everywhere.
writeFileSync(
  "_extensions/julia-engine/julia-engine.js",
  outputFiles[0].text.replace(/\r\n/g, "\n"),
);
console.log("built _extensions/julia-engine/julia-engine.js");
