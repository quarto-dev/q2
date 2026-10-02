// D2(b) main thread: the Module is compiled once and kept resident here; workers get it posted (as in PandocRunner).
const dec = (b64) => Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];
const sum = (a) => a.reduce((x, y) => x + y, 0);
let module = null;
const fxCache = {};
async function fx(name) {
  if (!fxCache[name]) {
    const j = await (await fetch(`/fixture/${name}`)).json();
    fxCache[name] = { ...j, files: j.files.map((f) => ({ mount: f.mount, bytes: dec(f.b64) })) };
  }
  return fxCache[name];
}
const digest = async (u8) => (u8 ? [...new Uint8Array(await crypto.subtle.digest("SHA-256", u8))].slice(0, 8).map((b) => b.toString(16).padStart(2, "0")).join("") : null);
const asked = new Map();
function call(w, msg, transfer = []) {
  const id = Math.random();
  return new Promise((res, rej) => {
    const h = ({ data }) => { if (data.id !== id) return; w.removeEventListener("message", h); data.error ? rej(new Error(data.error)) : res(data); };
    w.addEventListener("message", h);
    w.addEventListener("error", (e) => rej(new Error("worker error " + e.message)), { once: true });
    w.postMessage({ ...msg, id }, transfer);
  });
}
const spawn = () => new Worker("/worker.mjs", { type: "module" });

const B = {
  async init() {
    const t = performance.now();
    module = await WebAssembly.compile(await (await fetch("/pandoc.wasm")).arrayBuffer());
    return { compileMs: performance.now() - t, ua: navigator.userAgent };
  },
  fx,
  /** Production shape: a short-lived worker per render, Module posted in, worker terminated after. End-to-end from the main thread. */
  async fresh(name, env) {
    const rec = await fx(name);
    const t0 = performance.now();
    const w = spawn();
    const r = await call(w, { op: "fresh", module, rec, env });
    const t1 = performance.now();
    w.terminate();
    return { e2e: t1 - t0, ...r.stats, stderr: r.stderr, trap: r.trap, digest: await digest(r.output), len: r.output?.length ?? null };
  },
  warmWorker: null,
  async warmStart(name, rts) {
    const rec = await fx(name);
    const t0 = performance.now();
    B.warmWorker = spawn();
    const r = await call(B.warmWorker, { op: "warm-init", module, rec, rts });
    return { e2e: performance.now() - t0, initMs: r.initMs };
  },
  async warm(name, extra = {}) {
    const rec = await fx(name);
    const t0 = performance.now();
    const r = await call(B.warmWorker, { op: "warm-convert", rec, ...extra });
    const t1 = performance.now();
    return { e2e: t1 - t0, ...r.stats, stderr: r.stderr, trap: r.trap, digest: await digest(r.output), len: r.output?.length ?? null };
  },
  warmStop() { B.warmWorker?.terminate(); B.warmWorker = null; },

  /** Q2: median-of-N latency, fresh vs warm, after one discarded run each. */
  async latency(name, n) {
    await B.fresh(name);
    const fr = []; for (let i = 0; i < n; i++) fr.push(await B.fresh(name));
    const start = await B.warmStart(name);
    await B.warm(name); // discarded first convert
    const wr = []; for (let i = 0; i < n; i++) wr.push(await B.warm(name));
    B.warmStop();
    const col = (rs, k) => ({ med: median(rs.map((r) => r[k])), min: Math.min(...rs.map((r) => r[k])), max: Math.max(...rs.map((r) => r[k])) });
    return { name, n, digestFresh: fr[0].digest, digestWarm: wr[0].digest, equal: fr.every((r) => r.digest === fr[0].digest) && wr.every((r) => r.digest === fr[0].digest),
      fresh: { e2e: col(fr, "e2e"), mount: col(fr, "mount"), inst: col(fr, "inst"), run: col(fr, "run"), read: col(fr, "read"), mem: fr.at(-1).mem },
      warm: { e2e: col(wr, "e2e"), prep: col(wr, "prep"), mount: col(wr, "mount"), run: col(wr, "run"), read: col(wr, "read"), mem: wr.at(-1).mem, start },
      overhead: { freshWorkerAndClone: median(fr.map((r) => r.e2e - r.total)), warmMessageAndClone: median(wr.map((r) => r.e2e - r.total)) } };
  },

  /** Q3/Q4 correctness inside the browser engine. */
  async correctness() {
    const out = {};
    const refs = {};
    for (const n of ["callouts", "tables", "crossrefs"]) refs[n] = (await B.fresh(n)).digest;
    await B.warmStart("callouts");
    const seq = ["callouts", "tables", "callouts", "crossrefs", "callouts", "tables"];
    out.ABA = [];
    for (const n of seq) { const r = await B.warm(n); out.ABA.push(`${n}:${r.digest === refs[n]}`); }
    // env probe through the preamble
    const probe = 'function Pandoc(d) return pandoc.Pandoc({pandoc.Para{pandoc.Str(tostring(os.getenv("Q2_PROBE")).."|"..tostring(os.getenv("SOURCE_DATE_EPOCH")))}}, d.meta) end';
    const rec = await fx("callouts");
    const doc = new TextEncoder().encode('{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Str","c":"x"}]}]}');
    const opts = { from: "json", to: "plain", "data-dir": "/__q2_share__/pandoc/datadir", filters: [{ type: "lua", path: "/f.lua" }], "input-files": ["/in.json"], "output-file": "/out.txt" };
    out.env = [];
    for (const [p, s] of [["one", "111"], ["two", "222"], ["one", "111"]]) {
      const w = B.warmWorker; const id = Math.random();
      const r = await call(w, { op: "warm-convert", rec, env: { ...rec.env, Q2_PROBE: p, SOURCE_DATE_EPOCH: s }, filter: probe, extra: [{ mount: "/in.json", bytes: doc }], options: opts });
      out.env.push(new TextDecoder().decode(r.output).trim());
    }
    // errors then a good render
    out.errors = [];
    const err = async (label, filter, extra) => {
      let r; try { r = await call(B.warmWorker, { op: "warm-convert", rec, filter, extra: [{ mount: "/in.json", bytes: doc }], options: opts }); } catch (e) { out.errors.push(`${label}: THROWN ${String(e).slice(0, 80)}`); return; }
      const g = await B.warm("callouts");
      out.errors.push(`${label}: output=${r.output ? "yes" : "no"} trap=${r.trap ? r.trap.slice(0, 50) : null} stderr=${JSON.stringify(r.stderr.slice(0, 70))} | next good render equal=${g.digest === refs.callouts}`);
    };
    await err("filter error()", "function Str(s) error('boom') end");
    await err("filter syntax error", "function Str(s) return ( end");
    await err("filter os.exit(3)", "os.exit(3)");
    B.warmStop();
    // heap exhaustion with -M
    const big = await fx("callouts-x400");
    const frM = await (async () => { const w = spawn(); const r = await call(w, { op: "fresh", module, rec: big, argv: [big.argv[0], "+RTS", "-M100m", "-RTS", ...big.argv.slice(1)] }); w.terminate(); return `status ${r.stats.status}`; })();
    await B.warmStart("callouts", ["+RTS", "-M100m", "-RTS"]);
    const before = await B.warm("callouts");
    let oom; try { oom = await call(B.warmWorker, { op: "warm-convert", rec: big }); } catch (e) { oom = { error: String(e) }; }
    const after = [await B.warm("callouts"), await B.warm("callouts")];
    out.oom = { fresh: frM, warmBeforeEqual: before.digest === refs.callouts, warmOom: oom.error ?? `trap=${oom.trap} output=${oom.output ? "yes" : "no"}`, afterEqual: after.map((a) => a.digest === refs.callouts), afterTrap: after.map((a) => a.trap) };
    B.warmStop();
    return out;
  },

  /** Q4 memory: N renders; returns the wasm memory trace (the driver samples process RSS around it). */
  async many(mode, name, n) {
    const mem = [];
    if (mode === "warm") { await B.warmStart(name); for (let i = 1; i <= n; i++) { const r = await B.warm(name); if (i === 1 || i % 10 === 0) mem.push([i, r.mem >> 20]); } B.warmStop(); }
    else for (let i = 1; i <= n; i++) { const r = await B.fresh(name); if (i === 1 || i % 10 === 0) mem.push([i, r.mem >> 20]); }
    return mem;
  },

  /** Q5: what a supersede costs. Runaway filter, terminate after `afterMs`, then the next render. */
  async cancel(name, afterMs) {
    const rec = await fx(name);
    const res = {};
    // fresh-per-render path: the runaway worker is terminated; the next render is an ordinary fresh render
    await B.fresh(name);
    {
      const w = spawn(); const p = call(w, { op: "fresh-hang", module, rec }).catch(() => null);
      await new Promise((r) => setTimeout(r, afterMs));
      const t = performance.now(); w.terminate(); res.freshTerminateMs = performance.now() - t;
      const next = []; for (let i = 0; i < 3; i++) next.push((await B.fresh(name)).e2e);
      res.freshNextRender = next;
    }
    // warm path: terminate kills the warm instance; the next render pays create + first convert
    {
      await B.warmStart(name); await B.warm(name);
      const w = B.warmWorker; const p = call(w, { op: "hang", rec }).catch(() => null);
      await new Promise((r) => setTimeout(r, afterMs));
      const t = performance.now(); w.terminate(); res.warmTerminateMs = performance.now() - t; B.warmWorker = null;
      const next = [];
      for (let i = 0; i < 3; i++) { const t1 = performance.now(); await B.warmStart(name); const first = await B.warm(name); next.push({ coldFirstRender: performance.now() - t1, firstConvertMs: first.e2e }); B.warmStop(); }
      res.warmNextRenderAfterKill = next;
    }
    return res;
  },
};
window.B = B;
window.__ready = true;
