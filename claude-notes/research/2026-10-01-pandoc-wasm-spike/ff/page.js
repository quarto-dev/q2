const log = document.getElementById("log"), banner = document.getElementById("banner");
const beacon = (o) => fetch("/report", { method: "POST", body: JSON.stringify(o), keepalive: true }).catch(() => {});
const ua = navigator.userAgent;
document.getElementById("ua").textContent = ua;
beacon({ type: "page-loaded", ua, cores: navigator.hardwareConcurrency, deviceMemoryGB: navigator.deviceMemory ?? "n/a" });
const w = new Worker("./worker3.bundle.js", { type: "module" });
w.onerror = (e) => { banner.textContent = "WORKER ERROR: " + e.message; banner.className = "fail"; beacon({ type: "worker-error", message: e.message }); };
w.onmessage = ({ data }) => {
  beacon(data);
  if (data.type === "start") { const li = document.createElement("li"); li.id = "s-" + data.name; li.textContent = "… " + data.name; log.appendChild(li); }
  else if (data.type === "done") { const li = document.getElementById("s-" + data.name); li.textContent = `${data.ok ? "PASS" : "FAIL"}  ${data.name}  (${data.ms} ms)  ${data.detail}`; li.className = data.ok ? "pass" : "fail"; }
  else if (data.type === "finished") {
    const bad = data.results.filter(r => !r.ok);
    banner.textContent = bad.length ? `FAILED: ${bad.length} step(s)` : "ALL PASSED: pandoc 3.11 wasm works in this browser";
    banner.className = bad.length ? "fail" : "pass";
    beacon({ type: "summary", ok: !bad.length, ua });
  }
};
w.postMessage({ wasmUrl: "/pandoc-3.11.wasm" });
