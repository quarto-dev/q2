// Bundles typst-worker.src.mjs (typst.ts + the host's TypstSession) for the d2b browser harness: node build-typst.mjs
import { build } from "../../../../node_modules/esbuild/lib/main.js";
import path from "node:path";
import { fileURLToPath } from "node:url";
const here = path.dirname(fileURLToPath(import.meta.url));
await build({ entryPoints: [path.join(here, "typst-worker.src.mjs")], outfile: path.join(here, "typst-worker.bundle.mjs"), bundle: true, format: "esm", platform: "browser", target: "es2022", logLevel: "info" });
