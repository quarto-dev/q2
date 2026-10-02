// D2(b) browser harness server: page, worker, core (shim import rewritten), the shim, pandoc.wasm and the recordings as JSON.
import http from "node:http";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { loadRecording, ROOT, WASM } from "./common.mjs";
const here = path.dirname(fileURLToPath(import.meta.url));
const shim = path.join(ROOT, "node_modules/@bjorn3/browser_wasi_shim/dist");
const mime = { ".js": "text/javascript", ".mjs": "text/javascript", ".html": "text/html", ".wasm": "application/wasm", ".json": "application/json" };
const FIX = { callouts: ["callouts-typst", 1], "callouts-x10": ["callouts-typst", 10], empty: ["callouts-typst", 0], tables: ["tables-typst", 1], crossrefs: ["crossrefs-typst", 1], "callouts-docx": ["callouts-docx", 1], "callouts-x400": ["callouts-typst", 400] };
const b64 = (u) => Buffer.from(u).toString("base64");
export function listen(port = 0) {
  const server = http.createServer((q, s) => {
    const p = decodeURIComponent(new URL(q.url, "http://x").pathname);
    const send = (code, body, type) => s.writeHead(code, { "content-type": type ?? "text/plain", "cache-control": "no-store" }).end(body);
    if (q.method === "POST") { const c = []; q.on("data", (d) => c.push(d)); q.on("end", () => { writeFileSync(process.env.REPORT ?? "/tmp/d2b/firefox-all.json", Buffer.concat(c)); send(204, ""); console.log("report received"); }); return; }
    try {
      if (p === "/pandoc.wasm") return send(200, readFileSync(WASM), "application/wasm");
      if (p.startsWith("/fixture/")) {
        const [name, rep] = FIX[p.slice(9)];
        const r = loadRecording(name, { repeat: rep });
        return send(200, JSON.stringify({ argv: r.argv, env: r.env, output: r.output, files: r.files.map((f) => ({ mount: f.mount, b64: b64(f.bytes) })) }), "application/json");
      }
      if (p.startsWith("/shim/")) return send(200, readFileSync(path.join(shim, p.slice(6))), "text/javascript");
      const f = p === "/" ? "index.html" : p.slice(1);
      if (!/^[\w.-]+$/.test(f)) return send(404, "no");
      let body = readFileSync(path.join(here, f), "utf8");
      body = body.replaceAll('"@bjorn3/browser_wasi_shim"', '"/shim/index.js"');
      send(200, body, mime[path.extname(f)] ?? "text/plain");
    } catch (e) { send(500, String(e)); }
  });
  return new Promise((res) => server.listen(port, () => res({ server, port: server.address().port })));
}
