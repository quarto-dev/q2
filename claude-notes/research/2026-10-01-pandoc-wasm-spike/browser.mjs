import * as pw from "playwright"; const chromium = pw[process.env.BROWSER || "chromium"];
import http from "node:http"; import { readFileSync, existsSync } from "node:fs"; import { gzipSync } from "node:zlib";
import path from "node:path";
const root = path.resolve("dist"); const reqs = [];
const mime = { ".js": "text/javascript", ".html": "text/html", ".wasm": "application/wasm" };
const srv = http.createServer((q, s) => {
  const f = path.join(root, q.url === "/" ? "index.html" : q.url.split("?")[0]); reqs.push(q.url);
  if (!existsSync(f)) { s.writeHead(404).end(); return; }
  let body = readFileSync(f); const h = { "content-type": mime[path.extname(f)] || "application/octet-stream" };
  if (f.endsWith(".wasm") && /gzip/.test(q.headers["accept-encoding"] || "")) { body = gzipSync(body); h["content-encoding"] = "gzip"; }
  s.writeHead(200, h).end(body);
}).listen(0);
const port = srv.address().port;
const b = await chromium.launch(process.env.BROWSER === "firefox" ? { firefoxUserPrefs: { "gfx.webrender.software": true, "layers.acceleration.disabled": true, "webgl.disabled": true }, env: { ...process.env, MOZ_WEBRENDER: "0", LIBGL_ALWAYS_SOFTWARE: "1" } } : undefined); const pg = await b.newPage();
pg.on("pageerror", e => console.log("PAGEERROR", e)); pg.on("console", m => console.log("console:", m.text()));
await pg.goto(`http://localhost:${port}/`); await pg.waitForFunction(() => window.ready);
await new Promise(r => setTimeout(r, 500));
console.log("requests before first convert:", reqs.filter(r => r.includes("wasm")).length, "wasm requests");
const md = "---\ntitle: Hi\n---\n\n# Head\n\nSome *text*.\n";
for (const [to, ext] of [["docx","docx"],["pptx","pptx"],["epub3","epub"]]) {
  const r = await pg.evaluate(([to, ext, md]) => window.convert({ from: "markdown", to, standalone: true, "output-file": "o."+ext }, md, {})
    .then(d => ({ ...d, bytes: d.bytes ? Array.from(d.bytes.slice(0, 4)) : null, len: d.bytes?.length })), [to, ext, md]);
  console.log(to, JSON.stringify({ err: r.error, load: r.load, convertMs: r.convertMs && Math.round(r.convertMs), len: r.len, magic: r.bytes }));
}
console.log("wasm requests total:", reqs.filter(r => r.includes("wasm")).length);
await b.close(); srv.close();
