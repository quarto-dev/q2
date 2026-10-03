// H10a Task 0 driver: node drive0.mjs chromium|webkit [a|c|d|b|all]   (N=12 by default; raw JSON on stdout, saved by the caller to results/)
// 0(a) warm-up, 0(c) spare instance, 0(d) fd-2 probe, 0(b) contention scenarios. Needs `node build-typst.mjs` first for 0(b)'s typst load.
import { listen } from "./serve.mjs";
import * as pw from "../../../../node_modules/playwright/index.mjs";
const name = process.argv[2] ?? "chromium", what = process.argv[3] ?? "all";
const N = Number(process.env.N ?? 12);
const { server, port } = await listen();
const browser = await pw[name].launch(name === "chromium" ? { args: ["--js-flags=--expose-gc"] } : {});
const page = await browser.newPage();
page.on("pageerror", (e) => console.error("PAGEERROR", e));
await page.goto(`http://localhost:${port}/`);
await page.waitForFunction(() => window.__ready);
const out = { browser: name, n: N, date: new Date().toISOString() };
out.init = await page.evaluate(() => window.B.init());
const run = (fn, ...a) => page.evaluate(fn, ...a);
if (what === "all" || what === "a") {
  out.warmup = [];
  for (const gap of [0, 500]) out.warmup.push(await run(([n, g]) => window.B.warmup("callouts", n, g), [N, gap]));
}
if (what === "all" || what === "c") out.spare = await run((n) => window.B.spare("callouts", n, 300), N);
if (what === "all" || what === "d") out.stderrProbe = await run(() => window.B.stderrProbe());
if (what === "all" || what === "b") {
  out.typst = await run(() => window.B.typstInit());
  out.typstCheck = await run(() => window.B.typstCompile("callouts"));
  out.contention = {};
  out.contention.plain = await run((n) => window.B.contention("callouts", n, {}), N);
  out.contention.loaded = await run((n) => window.B.contention("callouts", n, { load: "typst" }), N);
  if (name === "chromium") {
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("Emulation.setCPUThrottlingRate", { rate: 4 });
    out.contention.throttle4x = await run((n) => window.B.contention("callouts", n, {}), N);
    await cdp.send("Emulation.setCPUThrottlingRate", { rate: 1 });
  }
  out.contention.coresLoaded = await run((n) => window.B.contention("callouts", n, { spin: navigator.hardwareConcurrency - 1 }), N);
}
console.log(JSON.stringify(out, null, 1));
await browser.close(); server.close();
