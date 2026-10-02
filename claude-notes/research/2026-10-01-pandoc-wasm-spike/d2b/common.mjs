// Node-side loaders. The run logic lives in core.mjs (shared with the browser worker).
import { readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
export * from "./core.mjs";
const here = path.dirname(fileURLToPath(import.meta.url));
export const ROOT = path.resolve(here, "../../../..");
const REC = path.join(ROOT, "crates/quarto-core/tests/fixtures/pandoc-recordings/recordings");
export const WASM = process.env.WASM ?? path.join(ROOT, ".cache/pandoc-wasm/pandoc.wasm");
const enc = new TextEncoder(), dec = new TextDecoder();
export function loadModule() { return WebAssembly.compile(readFileSync(WASM)); }
function walk(dir, base = "") {
  const out = [];
  for (const n of readdirSync(dir)) {
    const p = path.join(dir, n);
    if (statSync(p).isDirectory()) out.push(...walk(p, `${base}/${n}`));
    else out.push({ mount: `${base}/${n}`, bytes: new Uint8Array(readFileSync(p)) });
  }
  return out;
}
const j = (p) => JSON.parse(readFileSync(p, "utf8"));

/** A recording as {argv, env, files:[{mount,bytes}], output}. `repeat` multiplies the input document's blocks. */
export function loadRecording(name, { repeat = 1 } = {}) {
  const dir = path.join(REC, name);
  const meta = j(path.join(dir, "meta.json"));
  const files = [...walk(path.join(dir, "fs")), ...walk(path.join(REC, "share", meta.share_tree), "/__q2_share__")];
  if (repeat !== 1) {
    const f = files.find((x) => x.mount === "/__q2_tmp__/pandoc-input.json");
    const doc = JSON.parse(dec.decode(f.bytes));
    doc.blocks = Array.from({ length: repeat }, () => doc.blocks).flat();
    f.bytes = enc.encode(JSON.stringify(doc));
  }
  return { name, argv: j(path.join(dir, "argv.json")), env: j(path.join(dir, "env.json")), files, output: meta.output };
}

