// Auto-run for a by-hand browser (Playwright cannot launch Firefox here): runs the suite, POSTs the JSON to /report.
const log = (s) => (document.getElementById("log").textContent += "\n" + s);
while (!window.__ready) await new Promise((r) => setTimeout(r, 50));
const B = window.B, out = { ua: navigator.userAgent };
try {
  out.init = await B.init(); log("compiled " + Math.round(out.init.compileMs) + " ms");
  out.latency = [];
  for (const f of ["empty", "callouts", "callouts-x10"]) { log("latency " + f); out.latency.push(await B.latency(f, 12)); }
  log("correctness"); out.correctness = await B.correctness();
  log("memory (wasm linear memory only)"); out.memory = { warm: await B.many("warm", "callouts", 100), fresh: await B.many("fresh", "callouts", 100) };
  log("cancel"); out.cancel = await B.cancel("callouts", 400);
} catch (e) { out.error = String(e && e.stack || e); }
await fetch("/report", { method: "POST", body: JSON.stringify(out) });
log("DONE - results sent; you can close this tab.");
