// Spike 2 (browser): hub-client's Vite dev server serves Rust wasm on the page; pandoc.wasm in a lazy Worker.
import { chromium } from "playwright";
import { execSync } from "node:child_process";
import path from "node:path";
const base = process.argv[2]; // e.g. http://localhost:5199
const spikeDir = path.resolve(".");
const b = await chromium.launch();
const pg = await b.newPage();
const wasmReqs = []; pg.on("request", r => { if (/\.wasm/.test(r.url())) wasmReqs.push(r.url().split("/").pop()); });
pg.on("pageerror", e => console.log("PAGEERROR", String(e).slice(0, 300)));
pg.on("console", m => { if (m.type() === "error") console.log("console.error:", m.text().slice(0, 300)); });
const rssMB = () => Math.round(execSync("ps -axo rss=,command=").toString().split("\n")
  .filter(l => /ms-playwright\/chromium/.test(l)).reduce((t, l) => t + Number(l.trim().split(/\s+/)[0]), 0) / 1024);
await pg.goto(`${base}/@fs${spikeDir}/chain.html`);
await pg.waitForFunction(() => window.ready, null, { timeout: 120000 });
console.log("page loaded; wasm requests:", wasmReqs, "browser RSS MB:", rssMB());
console.log("q2 init:", JSON.stringify(await pg.evaluate(() => window.initQ2())), "wasm requests:", wasmReqs, "RSS:", rssMB());
const r = await pg.evaluate(() => window.renderAst());
console.log("render ast:", r.ms, "ms ok:", r.ok, "ast bytes:", r.ast?.length, "q2WasmMB:", r.q2WasmMB, "RSS:", rssMB());
for (const [to, ext] of [["docx", "docx"], ["pptx", "pptx"], ["epub3", "epub"]]) {
  const o = await pg.evaluate(([to, ext, ast]) => window.pandocConvert(to, ext, ast).then(d => ({ ...d, magic: d.bytes && Array.from(d.bytes.slice(0, 2)), len: d.bytes?.length, bytes: undefined })), [to, ext, r.ast]);
  console.log(to, JSON.stringify(o), "wasm requests:", wasmReqs.length, "RSS:", rssMB());
}
// Re-render in Rust wasm while pandoc is resident: does q2 still work?
const r2 = await pg.evaluate(() => window.renderAst());
console.log("re-render after pandoc loaded:", r2.ms, "ms ok:", r2.ok, "RSS:", rssMB());
await b.close();
