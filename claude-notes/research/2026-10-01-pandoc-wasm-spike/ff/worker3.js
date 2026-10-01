// Spike 3 worker: exercises pandoc 3.11 wasm in the host browser. Posts a progress beacon before and after every step.
import { createPandocInstance } from "../host-patched.js";

const post = (m) => self.postMessage(m);
const results = [];
async function step(name, fn) {
  post({ type: "start", name });
  const t = performance.now();
  try {
    const detail = await fn();
    const r = { name, ok: true, ms: Math.round(performance.now() - t), detail: detail ?? "" };
    results.push(r); post({ type: "done", ...r });
  } catch (e) {
    const r = { name, ok: false, ms: Math.round(performance.now() - t), detail: String(e && e.message || e) };
    results.push(r); post({ type: "done", ...r });
  }
}

const MD = "---\ntitle: Hi\n---\n\n# Head\n\nSome *text* with a [link](https://x.org) and $x^2$.\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
const JSON_AST = JSON.stringify({ "pandoc-api-version": [1, 23, 1], meta: {}, blocks: [{ t: "Para", c: [{ t: "Str", c: "hello" }] }] });
const magic = async (blob) => { const b = new Uint8Array(await blob.arrayBuffer()); return { len: b.length, pk: b[0] === 0x50 && b[1] === 0x4b }; };

self.onmessage = async ({ data: { wasmUrl } }) => {
  let bytes, mod, p;
  await step("exnref probe (WebAssembly.validate of an exnref-typed module)", () => {
    const ok = WebAssembly.validate(new Uint8Array([0,0x61,0x73,0x6d,1,0,0,0, 1,5,1,0x60,0,1,0x69]));
    if (!ok) throw new Error("exnref NOT supported by this engine");
    return "supported";
  });
  await step("fetch pandoc.wasm", async () => {
    const res = await fetch(wasmUrl); bytes = await res.arrayBuffer();
    return `${(bytes.byteLength / 1e6).toFixed(1)} MB`;
  });
  await step("WebAssembly.compile (the exnref-sensitive step)", async () => { mod = await WebAssembly.compile(bytes); return "compiled"; });
  await step("instantiate + hs_init (host)", async () => { p = await createPandocInstance(bytes, ['QUARTO_FILTER_PARAMS={"k":1}']); return `wasm memory ${Math.round(p.memoryBytes() / 1e6)} MB`; });
  await step("query version", async () => JSON.stringify(await p.query({ query: "version" })));
  for (const [to, ext] of [["docx", "docx"], ["pptx", "pptx"], ["epub3", "epub"]]) {
    await step(`markdown -> ${ext}`, async () => {
      const r = await p.convert({ from: "markdown", to, standalone: true, "output-file": `o.${ext}` }, MD, {});
      const m = await magic(r.files[`o.${ext}`]); if (!m.pk) throw new Error("output is not a zip: " + r.stderr);
      return `${m.len} bytes, zip magic ok`;
    });
  }
  await step("json AST + Lua filter + nested require + env var", async () => {
    const files = {
      "filters/main.lua": `package.path = "/filters/?.lua;/filters/sub/?.lua;" .. package.path
local m = require("helper"); local d = require("deep")
function Str(s) return pandoc.Str(m.up(s.text) .. d.v .. os.getenv("QUARTO_FILTER_PARAMS")) end`,
      "filters/helper.lua": `return { up = function(s) return s:upper() end }`,
      "filters/sub/deep.lua": `return { v = "-deep" }`,
    };
    const r = await p.convert({ from: "json", to: "native", filters: ["filters/main.lua"] }, JSON_AST, files);
    const out = r.stdout.trim(); if (!/HELLO-deep\{\\?"k\\?":1\}/.test(out)) throw new Error("unexpected: " + out + " " + r.stderr);
    return out;
  });
  await step("20 repeated docx conversions on one instance", async () => {
    for (let i = 0; i < 20; i++) await p.convert({ from: "markdown", to: "docx", standalone: true, "output-file": "o.docx" }, MD, {});
    return `wasm memory now ${Math.round(p.memoryBytes() / 1e6)} MB`;
  });
  await step("3 fresh instances from the compiled module + docx each", async () => {
    const times = [];
    for (let i = 0; i < 3; i++) {
      const t = performance.now(); const q = await createPandocInstance(bytes, [`P=${i}`]);
      await q.convert({ from: "markdown", to: "docx", standalone: true, "output-file": "o.docx" }, MD, {});
      times.push(Math.round(performance.now() - t));
    }
    return `create+docx ms: ${times.join(", ")}`;
  });
  post({ type: "finished", results });
};
