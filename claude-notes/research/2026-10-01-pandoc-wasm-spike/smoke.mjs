// Smoke test: load a pandoc.wasm (path argv[2]) via pandoc-wasm's JS host, convert md -> several formats.
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { createPandocInstance } from "./node_modules/pandoc-wasm/src/core.js";

const wasmPath = process.argv[2];
const t0 = performance.now();
const p = await createPandocInstance(readFileSync(wasmPath));
console.log(`load+init ${wasmPath}: ${(performance.now() - t0).toFixed(0)} ms`);
console.log("version:", (await p.query({ query: "version" })));

const md = "---\ntitle: Hi\n---\n\n# Head\n\nSome *text* with a [link](https://x.org) and $x^2$.\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
mkdirSync("out", { recursive: true });
const tag = wasmPath.match(/(\d+\.\d+)/)?.[1] ?? "x";
for (const [to, ext] of [["docx", "docx"], ["pptx", "pptx"], ["epub3", "epub"], ["odt", "odt"], ["latex", "tex"], ["rtf", "rtf"]]) {
  const t = performance.now();
  const opts = { from: "markdown", to, standalone: true, "output-file": `o.${ext}` };
  try {
    const r = await p.convert(opts, md, {});
    const blob = r.files[`o.${ext}`];
    const bytes = blob ? new Uint8Array(await blob.arrayBuffer()) : new TextEncoder().encode(r.stdout);
    writeFileSync(`out/${tag}.${ext}`, bytes);
    console.log(`${to}: ${bytes.length} bytes in ${(performance.now() - t).toFixed(0)} ms; warnings=${JSON.stringify(r.warnings)} stderr=${JSON.stringify(r.stderr)}`);
  } catch (e) { console.log(`${to}: FAIL ${e}`); }
}
