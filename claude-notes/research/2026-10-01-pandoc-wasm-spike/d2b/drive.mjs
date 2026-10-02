// usage: node drive.mjs chromium|webkit [latency|correctness|memory|cancel|all] ; N=10 for latency
import { execFileSync } from "node:child_process";
import { listen } from "./serve.mjs";
import * as pw from "../../../../node_modules/playwright/index.mjs";
const name = process.argv[2] ?? "chromium", what = process.argv[3] ?? "all";
const N = Number(process.env.N ?? 10);
const rssMb = () => { const o = execFileSync("ps", ["-axo", "rss=,command="], { maxBuffer: 1 << 26 }).toString(); let kb = 0; for (const l of o.split("\n")) if (l.includes("ms-playwright")) kb += Number(l.trim().split(/\s+/)[0]) || 0; return Math.round(kb / 1024); };
const { server, port } = await listen();
const browser = await pw[name].launch(name === "chromium" ? { args: ["--js-flags=--expose-gc"] } : {});
const page = await browser.newPage();
page.on("pageerror", (e) => console.log("PAGEERROR", e));
await page.goto(`http://localhost:${port}/`);
await page.waitForFunction(() => window.__ready);
const out = { browser: name };
out.init = await page.evaluate(() => window.B.init());
const run = (fn, ...a) => page.evaluate(fn, ...a);
if (what === "all" || what === "latency") {
  out.latency = [];
  for (const f of ["empty", "callouts", "callouts-x10"]) out.latency.push(await run(([f, n]) => window.B.latency(f, n), [f, N]));
}
if (what === "all" || what === "correctness") out.correctness = await run(() => window.B.correctness());
if (what === "all" || what === "memory") {
  out.memory = {};
  for (const mode of ["warm", "fresh"]) {
    await run(() => window.B.warmStop());
    const start = rssMb(); let peak = start; const t = setInterval(() => (peak = Math.max(peak, rssMb())), 100);
    const trace = await run(([m]) => window.B.many(m, "callouts", 100), [mode]);
    clearInterval(t);
    out.memory[mode] = { rssStart: start, rssPeak: peak, rssEnd: rssMb(), wasmMbTrace: trace };
  }
}
if (what === "all" || what === "cancel") out.cancel = await run(() => window.B.cancel("callouts", 400));
console.log(JSON.stringify(out, null, 1));
await browser.close(); server.close();
