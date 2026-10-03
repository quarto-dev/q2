// H10a Task 0 by hand in a browser Playwright cannot launch here (Firefox): runs (a), (c), (d), (b) and POSTs the JSON to /report.
const log = (s) => (document.getElementById("log").textContent += "\n" + s);
while (!window.__ready) await new Promise((r) => setTimeout(r, 50));
const B = window.B, N = Number(new URLSearchParams(location.search).get("n") ?? 12), out = { browser: "firefox", n: N, ua: navigator.userAgent, date: new Date().toISOString() };
try {
  out.init = await B.init(); log("compiled " + Math.round(out.init.compileMs) + " ms");
  log("0(a) warm-up"); out.warmup = []; for (const gap of [0, 500]) out.warmup.push(await B.warmup("callouts", N, gap));
  log("0(c) spare instance"); out.spare = await B.spare("callouts", N, 300);
  log("0(d) fd 2"); out.stderrProbe = await B.stderrProbe();
  log("0(b) typst assets"); out.typst = await B.typstInit(); out.typstCheck = await B.typstCompile("callouts");
  out.contention = {};
  log("0(b) plain"); out.contention.plain = await B.contention("callouts", N, {});
  log("0(b) loaded (typst + main-thread busy loop)"); out.contention.loaded = await B.contention("callouts", N, { load: "typst" });
  log("0(b) cores loaded"); out.contention.coresLoaded = await B.contention("callouts", N, { spin: navigator.hardwareConcurrency - 1 });
} catch (e) { out.error = String(e && e.stack || e); }
await fetch("/report", { method: "POST", body: JSON.stringify(out, null, 1) });
log("DONE - results sent; you can close this tab.");
