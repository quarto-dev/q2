const b = new URLSearchParams(location.search).get("b") || "unknown";
const q = (path, params) => `${path}?b=${b}${params ? "&" + params : ""}`;
const post = (path, body, params) => fetch(q(path, params), { method: "POST", body, keepalive: false }).catch(() => {});
const report = (o) => post("/report", JSON.stringify(o));
const log = document.getElementById("log"), banner = document.getElementById("banner");
document.getElementById("ua").textContent = navigator.userAgent;
const line = (cls, text) => { const li = document.createElement("li"); li.className = cls; li.textContent = text; log.appendChild(li); };
const w = new Worker("/worker.js", { type: "module" });
let seq = 0; const pending = new Map();
w.onmessage = async ({ data }) => {
  if (data.type === "result") { pending.get(data.id)(data); pending.delete(data.id); }
  else if (data.type === "progress") report(data);
};
w.onerror = (e) => { banner.textContent = "WORKER ERROR " + e.message; report({ type: "worker-error", message: e.message }); };
const call = (msg) => new Promise((res) => { const id = ++seq; pending.set(id, res); w.postMessage({ ...msg, id }); });
(async () => {
  report({ type: "page-loaded", ua: navigator.userAgent, cores: navigator.hardwareConcurrency, deviceMemoryGB: navigator.deviceMemory ?? "n/a" });
  const init = await call({ op: "init", wasmUrl: "/pandoc.wasm" });
  report({ type: "init", ...init }); line(init.ok ? "pass" : "fail", "init " + JSON.stringify(init));
  if (!init.ok) { banner.textContent = "INIT FAILED"; report({ type: "finished" }); return; }
  const names = await (await fetch("/rec/index.json")).json();
  const only = new URLSearchParams(location.search).get("only");
  for (const name of names.filter((n) => !only || n.includes(only))) {
    for (const rep of [0, 1]) { // run twice: the second shows the warm-cache (fetch) and a second fresh instance
      const r = await call({ op: "replay", name, save: rep === 0 });
      const { files, ...rest } = r;
      report({ type: "replay", rep, ...rest, outputBytes: files?.output?.length });
      line(r.exit === 0 && files?.output ? "pass" : "fail", `${name}#${rep} exit=${r.exit} ${JSON.stringify(rest.t)} mem=${r.memoryMB}MB poll=${JSON.stringify(r.poll)}`);
      if (rep === 0 && files?.output) await post("/save", files.output, `name=${encodeURIComponent(name)}&file=${encodeURIComponent(r.outputName)}`);
    }
  }
  { const r = await call({ op: "svg" }); report({ type: "svg", info: r.info });
    for (const [file, bytes] of Object.entries(r.files)) await post("/save", bytes, `name=svg-docx&file=${encodeURIComponent(file)}`);
    line("pass", "svg " + JSON.stringify(r.info)); }
  for (const probe of ["probes"]) {
    const r = await call({ op: probe });
    report({ type: probe, ...r }); line("pass", probe + " " + JSON.stringify(r).slice(0, 2000));
  }
  banner.textContent = "FINISHED"; report({ type: "finished" });
})().catch((e) => { banner.textContent = "ERROR " + e; report({ type: "error", message: String(e && e.stack || e) }); report({ type: "finished" }); });
