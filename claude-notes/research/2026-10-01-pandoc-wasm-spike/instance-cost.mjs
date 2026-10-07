import { readFileSync } from "node:fs";
import { createPandocInstance } from "./host-patched.js";
const bytes = readFileSync("pandoc-3.11.wasm");
const mod = await WebAssembly.compile(bytes);          // compile once
let t = performance.now(); const orig = WebAssembly.instantiate;
WebAssembly.instantiate = (src, imp) => orig.call(WebAssembly, mod, imp).then(i => ({ instance: i, module: mod })); // reuse compiled module
const json = JSON.stringify({ "pandoc-api-version": [1,23,1], meta: {}, blocks: [{ t: "Para", c: [{ t: "Str", c: "x" }] }] });
for (let i = 1; i <= 5; i++) {
  t = performance.now();
  const p = await createPandocInstance(bytes, [`P=${i}`]);
  const t1 = performance.now() - t; t = performance.now();
  await p.convert({ from: "json", to: "docx", standalone: true, "output-file": "o.docx" }, json, {});
  console.log(`instance ${i}: create ${t1.toFixed(0)} ms, first docx ${(performance.now() - t).toFixed(0)} ms, wasm mem ${Math.round(p.memoryBytes()/1e6)} MB, rss ${Math.round(process.memoryUsage().rss/1e6)} MB`);
}
