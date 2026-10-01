// Serves ff/ + pandoc-3.11.wasm on localhost and records beacons from the page to ff/report.log (+ stdout).
import http from "node:http"; import { readFileSync, existsSync, appendFileSync, writeFileSync } from "node:fs"; import path from "node:path";
const here = path.dirname(new URL(import.meta.url).pathname); const spike = path.dirname(here);
const port = Number(process.env.PORT || 8137); const mime = { ".js": "text/javascript", ".html": "text/html", ".wasm": "application/wasm" };
writeFileSync(path.join(here, "report.log"), "");
http.createServer((q, s) => {
  if (q.method === "POST" && q.url === "/report") {
    let b = ""; q.on("data", d => b += d); q.on("end", () => { const line = `${new Date().toISOString()} ${b}\n`; appendFileSync(path.join(here, "report.log"), line); process.stdout.write(line); s.writeHead(204).end(); }); return;
  }
  const u = q.url.split("?")[0]; const f = u === "/" ? path.join(here, "index.html") : u === "/pandoc-3.11.wasm" ? path.join(spike, "pandoc-3.11.wasm") : path.join(here, "dist", u);
  if (!existsSync(f)) { s.writeHead(404).end("nf"); return; }
  s.writeHead(200, { "content-type": mime[path.extname(f)] || "application/octet-stream", "cache-control": "no-store" }).end(readFileSync(f));
}).listen(port, () => console.log(`serving on http://localhost:${port}/`));
