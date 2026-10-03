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
  async warmStart(name, rts, capture = false) {
    const rec = await fx(name);
    const t0 = performance.now();
    B.warmWorker = spawn();
    const r = await call(B.warmWorker, { op: "warm-init", module, rec, rts, capture });
    return { e2e: performance.now() - t0, initMs: r.initMs };
  },
  async warm(name, extra = {}) {
    const rec = await fx(name);
    const t0 = performance.now();
    const r = await call(B.warmWorker, { op: "warm-convert", rec, ...extra });
    const t1 = performance.now();
    return { e2e: t1 - t0, ...r.stats, stderr: r.stderr, fd1: r.fd1, fd2: r.fd2, warnings: r.warnings, trap: r.trap, digest: await digest(r.output), len: r.output?.length ?? null };
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
// ---- Task 0 (H10a): measurements for the warm executor plan. Sources of the numbers in the evidence note, "H10a Task 0". ----
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const stat = (xs) => ({ n: xs.length, med: median(xs), min: Math.min(...xs), max: Math.max(...xs) });

/** 0(a): is a new instance hot after one warm-up convert? n new instances, each: spawn+init, warm-up convert, `gapMs` idle, timed convert. */
B.warmup = async (name, n, gapMs = 0) => {
  const cold = [], after = [];
  for (let i = -1; i < n; i++) { // i = -1 is the discarded cycle
    await B.warmStart(name);
    const first = await B.warm(name); // the warm-up (a callouts-sized request through the full chain)
    if (gapMs) await sleep(gapMs);
    const next = await B.warm(name);
    B.warmStop();
    if (i >= 0) { cold.push(first.e2e); after.push(next.e2e); }
  }
  // steady-state warm median in the same run, for the comparison
  await B.warmStart(name); await B.warm(name);
  const steady = []; for (let i = 0; i < n; i++) steady.push((await B.warm(name)).e2e);
  B.warmStop();
  return { name, gapMs, firstConvertColdInstance: stat(cold), convertAfterOneWarmup: stat(after), steadyWarm: stat(steady), deltaMedian: median(after) - median(steady) };
};

/** 0(c): a fresh `_start` instance instantiated in advance, argv and tree supplied at request time. */
B.spare = async (name, n, idleMs = 300) => {
  const rec = await fx(name);
  const inst = [], run = [], total = [], fresh = [];
  for (let i = -1; i < n; i++) {
    const t0 = performance.now();
    const w = spawn();
    const init = await call(w, { op: "spare-init", module });
    const spareReady = performance.now() - t0;
    await sleep(idleMs); // the spare idles until a request arrives
    const t1 = performance.now();
    const r = await call(w, { op: "spare-run", rec });
    const reqToOut = performance.now() - t1;
    w.terminate();
    if (i >= 0) { inst.push(spareReady); run.push(reqToOut); total.push(r.stats.total); }
    const f = await B.fresh(name);
    if (i >= 0) fresh.push(f.e2e);
    if (i === 0) var d = await digest(r.output), df = f.digest;
  }
  return { name, idleMs, spawnAndInstantiate: stat(inst), requestToOutput: stat(run), insideWorker: stat(total), freshEndToEnd: stat(fresh), bytesEqualToFresh: d === df };
};

/** 0(d): does Lua `io.stderr:write` land on WASI fd 2 in convert mode, and what else do fd 1/2 and /warnings carry? */
B.stderrProbe = async () => {
  const rec = await fx("callouts");
  const doc = new TextEncoder().encode('{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Str","c":"x"}]},{"t":"Para","c":[{"t":"Image","c":[["",[],[]],[{"t":"Str","c":"alt"}],["missing.png",""]]}]}]}');
  const filter = 'function Pandoc(d) io.stderr:write("PROBE-IO-STDERR\\n") io.stdout:write("PROBE-IO-STDOUT\\n") io.stderr:write("WARNING (f.lua:1) quarto-style warning\\n") return d end';
  const opts = { from: "json", to: "plain", "data-dir": "/__q2_share__/pandoc/datadir", filters: [{ type: "lua", path: "/f.lua" }], "input-files": ["/in.json"], "output-file": "/out.txt" };
  await B.warmStart("callouts", undefined, true);
  const r = await call(B.warmWorker, { op: "warm-convert", rec, filter, extra: [{ mount: "/in.json", bytes: doc }], options: opts });
  B.warmStop();
  return { fd1: r.fd1, fd2: r.fd2, stderrFile: r.stderr, warnings: r.warnings, trap: r.trap, output: r.output ? new TextDecoder().decode(r.output) : null };
};

// Typst, as the hub-client runs it: a fresh worker per compile (init + compile), assets from the hub-client build.
const typst = { module: null, fonts: null, packages: null };
const unpackList = (u8, named) => { const v = new DataView(u8.buffer, u8.byteOffset, u8.byteLength); const n = v.getUint32(0, true); let at = 4; const out = []; const td = new TextDecoder();
  for (let i = 0; i < n; i++) { let path = null; if (named) { const pl = v.getUint32(at, true); at += 4; path = td.decode(u8.subarray(at, at + pl)); at += pl; } const l = v.getUint32(at, true); at += 4; out.push(named ? { path, bytes: u8.slice(at, at + l) } : u8.slice(at, at + l)); at += l; } return out; };
const getBin = async (p) => new Uint8Array(await (await fetch(p)).arrayBuffer());
B.typstInit = async () => {
  typst.module = await WebAssembly.compile(await getBin("/typst.wasm"));
  typst.fonts = [...unpackList(await getBin("/typst-fonts.bin"), false), ...unpackList(await getBin("/typst-vendored-fonts.bin"), false)];
  typst.packages = unpackList(await getBin("/typst-packages.bin"), true);
  return { fonts: typst.fonts.length, packages: typst.packages.length };
};
B.pandocOut = async (name) => { const rec = await fx(name); const w = spawn(); const r = await call(w, { op: "fresh", module, rec }); w.terminate(); return r.output; };
const typstInputs = {};
/** One typst compile in a fresh worker, over callouts' pandoc output and the recording's files (the share tree and the document's). */
B.typstCompile = async (name = "callouts") => {
  const rec = await fx(name);
  if (!typstInputs[name]) typstInputs[name] = await B.pandocOut(name);
  const t0 = performance.now();
  const w = new Worker("/typst-worker.bundle.mjs", { type: "module" });
  const files = [...rec.files.filter((f) => f.mount !== rec.output).map((f) => ({ path: f.mount, bytes: f.bytes.slice() })), { path: rec.output, bytes: typstInputs[name].slice() }];
  const r = await call(w, { op: "compile", module: typst.module, fonts: typst.fonts.map((f) => f.slice()), packages: typst.packages.map((f) => ({ path: f.path.replace(/^packages\//, ""), bytes: f.bytes.slice() })), input: { main: rec.output, root: "/", files } });
  w.terminate();
  return { e2e: performance.now() - t0, ...r };
};

/**
 * 0(b): two warm workers. "alone": the active render by itself. "overlap": the older render starts, the newer one starts `gapMs` later
 * on the other worker; both are timed from their own start. `load`: "typst" adds a concurrent typst compile and a main-thread busy loop
 * (10 ms spin every 20 ms); `spin: k` keeps k cores busy with spin workers.
 */
B.contention = async (name, n, { gapMs = 40, load = null, spin = 0 } = {}) => {
  const rec = await fx(name);
  const mk = async () => { const w = spawn(); await call(w, { op: "warm-init", module, rec }); await call(w, { op: "warm-convert", rec }); await call(w, { op: "warm-convert", rec }); return w; };
  const W1 = await mk(), W2 = await mk();
  const spinners = Array.from({ length: spin }, () => { const w = spawn(); w.postMessage({ op: "spin" }); return w; });
  let stopBusy = false;
  if (load === "typst") (async () => { while (!stopBusy) { const t = performance.now(); while (performance.now() - t < 10) { /* spin */ } await sleep(10); } })();
  if (load === "typst" && !typst.module) await B.typstInit();
  const timed = async (w) => { const t = performance.now(); await call(w, { op: "warm-convert", rec }); return performance.now() - t; };
  const alone = [], r1 = [], r2 = [], typstMs = [];
  for (let i = -1; i < n; i++) {
    const a = await timed(W1);
    let ty = null; if (load === "typst") ty = B.typstCompile(name);
    const p1 = timed(W1); await sleep(gapMs); const p2 = timed(W2);
    const [x1, x2] = await Promise.all([p1, p2]);
    if (ty) { const t = await ty; if (i >= 0) typstMs.push(t.e2e); }
    if (i >= 0) { alone.push(a); r1.push(x1); r2.push(x2); }
    await sleep(50);
  }
  stopBusy = true; for (const w of spinners) w.terminate(); W1.terminate(); W2.terminate();
  return { name, n, gapMs, load, spin, cores: navigator.hardwareConcurrency, alone: stat(alone), older: stat(r1), newer: stat(r2), olderMinusAlone: median(r1) - median(alone), newerMinusAlone: median(r2) - median(alone), ...(typstMs.length ? { typstCompile: stat(typstMs) } : {}) };
};

window.B = B;
window.__ready = true;
