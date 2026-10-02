// H0 harness server. Serves the page, the shim, pandoc.wasm and R0's recordings; receives outputs and a report
// from the page (POST /save, POST /report) and writes them under out/<browser>/.
//   WASM=/path/to/pandoc.wasm PORT=8140 node serve.mjs
import http from "node:http";
import { readFileSync, existsSync, appendFileSync, mkdirSync, writeFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";
const here = path.dirname(new URL(import.meta.url).pathname);
const repo = path.resolve(here, "../../../..");
const recs = path.join(repo, "crates/quarto-core/tests/fixtures/pandoc-recordings/recordings");
const shim = path.join(here, "node_modules/@bjorn3/browser_wasi_shim/dist");
const wasm = process.env.WASM;
const port = Number(process.env.PORT || 8140);
const mime = { ".js": "text/javascript", ".html": "text/html", ".wasm": "application/wasm", ".json": "application/json" };
export const state = { finished: new Set() };
const send = (s, code, body, h = {}) => s.writeHead(code, { "cache-control": "no-store", ...h }).end(body);
const server = http.createServer((q, s) => {
  const url = new URL(q.url, "http://x");
  const p = decodeURIComponent(url.pathname);
  const b = url.searchParams.get("b") || "unknown";
  if (q.method === "POST") {
    const chunks = [];
    q.on("data", (d) => chunks.push(d));
    q.on("end", () => {
      const body = Buffer.concat(chunks);
      if (p === "/save") {
        const f = path.join(here, "out", b, url.searchParams.get("name"), url.searchParams.get("file"));
        mkdirSync(path.dirname(f), { recursive: true });
        writeFileSync(f, body);
      } else if (p === "/report") {
        mkdirSync(path.join(here, "out", b), { recursive: true });
        const line = body.toString();
        appendFileSync(path.join(here, "out", b, "report.jsonl"), line + "\n");
        if (JSON.parse(line).type === "finished") state.finished.add(b);
      }
      send(s, 204, "");
    });
    return;
  }
  if (p === "/status") return send(s, 200, JSON.stringify([...state.finished]));
  let f;
  if (p === "/") f = path.join(here, "page.html");
  else if (p === "/page.js" || p === "/worker.js") f = path.join(here, p);
  else if (p.startsWith("/shim/")) f = path.join(shim, p.slice(6));
  else if (p === "/pandoc.wasm") f = wasm;
  else if (p === "/rec/index.json")
    return send(s, 200, JSON.stringify(readdirSync(recs).filter((n) => n !== "share" && statSync(path.join(recs, n)).isDirectory())), { "content-type": mime[".json"] });
  else if (p.startsWith("/rec/")) f = path.join(recs, p.slice(5));
  if (!f || !existsSync(f)) return send(s, 404, "nf");
  send(s, 200, readFileSync(f), { "content-type": mime[path.extname(f)] || "application/octet-stream" });
});
export const listen = () => new Promise((r) => server.listen(port, () => r({ port, server })));
if (import.meta.url === `file://${process.argv[1]}`) listen().then(() => console.log(`http://localhost:${port}/?b=<browser>`));
