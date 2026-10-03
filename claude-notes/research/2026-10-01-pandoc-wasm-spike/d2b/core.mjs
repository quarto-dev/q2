// D2(b) spike core: recordings -> fresh (command mode, as execute.ts) and warm (exported `hs_init_with_rtsopts` + `convert`) pandoc runs.
// Shared by the Node scripts and the browser worker (serve.mjs rewrites the bare shim import).
// Throwaway code; the numbers are in claude-notes/research/2026-10-02-d2b-warm-instance-spike.md.
import { ConsoleStdout, Directory, File, OpenFile, PreopenDirectory, WASI, WASIProcExit } from "@bjorn3/browser_wasi_shim";

const enc = new TextEncoder(), dec = new TextDecoder();
export const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];
export const now = () => performance.now();

export function buildTree(rec, extra = {}) {
  const root = new Map();
  const mkdirp = (parts) => {
    let cur = root;
    for (const p of parts) {
      let d = cur.get(p);
      if (!d) { d = new Directory(new Map()); cur.set(p, d); }
      cur = d.contents;
    }
    return cur;
  };
  for (const d of ["tmp", "__q2_out__"]) mkdirp([d]);
  for (const f of rec.files) { const ps = f.mount.split("/").filter(Boolean); mkdirp(ps.slice(0, -1)).set(ps.at(-1), new File(f.bytes)); }
  for (const [m, bytes] of Object.entries(extra)) { const ps = m.split("/").filter(Boolean); mkdirp(ps.slice(0, -1)).set(ps.at(-1), new File(typeof bytes === "string" ? enc.encode(bytes) : bytes)); }
  return root;
}
export function readTree(root, abs) {
  let cur = root;
  for (const p of abs.split("/").filter(Boolean)) { cur = cur?.get ? cur.get(p) : cur?.contents?.get(p); if (!cur) return null; if (cur instanceof Directory) cur = cur.contents; }
  return cur?.data ?? null;
}
const concat = (cs) => { const o = new Uint8Array(cs.reduce((n, c) => n + c.length, 0)); let k = 0; for (const c of cs) { o.set(c, k); k += c.length; } return o; };

/** Production-shaped fresh run (execute.ts): new tree + new instance + _start. Returns timings and output. */
export async function freshRun(module, rec, { env: envOver = {}, argv = rec.argv } = {}) {
  const t0 = now();
  const tree = buildTree(rec);
  const t1 = now();
  const out = [], err = [];
  const env = Object.entries({ ...rec.env, ...envOver }).map(([k, v]) => `${k}=${v}`);
  const fds = [new OpenFile(new File(new Uint8Array(0))), new ConsoleStdout((b) => out.push(b.slice())), new ConsoleStdout((b) => err.push(b.slice())), new PreopenDirectory("/", tree)];
  const wasi = new WASI(argv, env, fds, { debug: false });
  const o = wasi.wasiImport.args_sizes_get;
  wasi.wasiImport.args_sizes_get = (a, b) => { const r = o.call(wasi.wasiImport, a, b); new DataView(wasi.inst.exports.memory.buffer).setUint32(b, argv.reduce((n, x) => n + enc.encode(x).length + 1, 0), true); return r; };
  const instance = await WebAssembly.instantiate(module, { wasi_snapshot_preview1: wasi.wasiImport });
  const t2 = now();
  let status = null, trap = null;
  try { status = wasi.start(instance); } catch (e) { if (e instanceof WASIProcExit) status = e.code; else trap = e; }
  const t3 = now();
  const output = readTree(tree, rec.output);
  return { status, trap, stderr: dec.decode(concat(err)), output, mountMs: t1 - t0, instMs: t2 - t1, runMs: t3 - t2, readMs: now() - t3, totalMs: now() - t0, memBytes: instance.exports.memory.buffer.byteLength };
}

/**
 * argv -> pandoc "defaults" options for the `convert` export (the subset the typst recordings use).
 */
export function argvToOptions(argv, extra = {}) {
  const o = { variables: {}, filters: [], "resource-path": [] };
  const a = argv.slice(1);
  for (let i = 0; i < a.length; i++) {
    const x = a[i];
    if (x === "-f") o.from = a[++i];
    else if (x === "-t") o.to = a[++i];
    else if (x === "--data-dir") o["data-dir"] = a[++i];
    else if (x === "-L") o.filters.push({ type: "lua", path: a[++i] });
    else if (x === "--standalone") o.standalone = true;
    else if (x === "--wrap") o.wrap = a[++i];
    else if (x === "--default-image-extension") o["default-image-extension"] = a[++i];
    else if (x === "--resource-path") o["resource-path"].push(a[++i]);
    else if (x === "--shift-heading-level-by") o["shift-heading-level-by"] = Number(a[++i]);
    else if (x === "--template") o.template = a[++i];
    else if (x === "-o") o["output-file"] = a[++i];
    else if (x.startsWith("--syntax-highlighting=")) o["syntax-highlighting"] = x.split("=")[1];
    else if (x === "-V") { const s = a[++i]; const k = s.indexOf("="); o.variables[s.slice(0, k)] = s.slice(k + 1); }
    else if (!x.startsWith("-")) (o["input-files"] ??= []).push(x);
    else throw new Error(`argvToOptions: unhandled ${x}`);
  }
  return { ...o, ...extra };
}

// rts defaults to none: production argv carries no RTS options; the old spike host used -H64m, which costs ~1 s on the first convert.
/** A warm instance driven through the exported `hs_init_with_rtsopts` + `convert`. */
export async function warmInstance(module, rec, { env = rec.env, rts = [], capture = false } = {}) {
  const tree = new Map();
  const std = {};
  const args = ["pandoc.wasm", ...rts];
  const envArr = Object.entries(env).map(([k, v]) => `${k}=${v}`);
  const mk = () => new File(new Uint8Array(0));
  // Task 0(d): `capture` keeps what the instance writes to WASI fd 1 and fd 2 (the original spike discarded both).
  const o1 = [], o2 = [];
  const sink = (a) => new ConsoleStdout((b) => { if (capture) a.push(b.slice()); });
  const fds = [new OpenFile(mk()), sink(o1), sink(o2), new PreopenDirectory("/", tree)];
  const wasi = new WASI(args, envArr, fds, { debug: false });
  const t0 = now();
  const instance = await WebAssembly.instantiate(module, { wasi_snapshot_preview1: wasi.wasiImport });
  const x = instance.exports;
  wasi.initialize(instance);
  x.__wasm_call_ctors();
  const dv = () => new DataView(x.memory.buffer);
  const cstr = (s) => { const b = enc.encode(s); const p = x.malloc(b.length + 1); new Uint8Array(x.memory.buffer, p, b.length + 1).set([...b, 0]); return p; };
  const argc = x.malloc(4); dv().setUint32(argc, args.length, true);
  const argvp = x.malloc(4 * (args.length + 1));
  args.forEach((s, i) => dv().setUint32(argvp + 4 * i, cstr(s), true));
  dv().setUint32(argvp + 4 * args.length, 0, true);
  const argvpp = x.malloc(4); dv().setUint32(argvpp, argvp, true);
  x.hs_init_with_rtsopts(argc, argvpp);
  const initMs = now() - t0;

  /** One conversion. `rec.files` are mounted into a cleared tree. */
  function convert(options, { files = rec.files, extra = {}, stdin = "" } = {}) {
    const t0 = now();
    o1.length = 0; o2.length = 0;
    tree.clear();
    const R = buildTree({ files }, extra);
    for (const [k, v] of R) tree.set(k, v);
    const out = new File(new Uint8Array(0)), err = new File(new Uint8Array(0)), warn = new File(new Uint8Array(0));
    tree.set("stdin", new File(enc.encode(stdin))); tree.set("stdout", out); tree.set("stderr", err); tree.set("warnings", warn);
    const t1 = now();
    const s = JSON.stringify(options), b = enc.encode(s), p = x.malloc(b.length);
    new Uint8Array(x.memory.buffer, p, b.length).set(b);
    let trap = null;
    try { x.convert(p, b.length); } catch (e) { trap = e; }
    const t2 = now();
    const outPath = options["output-file"];
    const output = outPath ? readTree(tree, outPath) : null;
    return { trap, fd1: dec.decode(concat(o1)), fd2: dec.decode(concat(o2)), stdout: dec.decode(out.data), stderr: dec.decode(err.data), warnings: dec.decode(warn.data), output, mountMs: t1 - t0, runMs: t2 - t1, readMs: now() - t2, totalMs: now() - t0, memBytes: x.memory.buffer.byteLength, tree };
  }
  return { instance, wasi, initMs, convert, mem: () => x.memory.buffer.byteLength, env: envArr };
}

/**
 * Task 0(c): a fresh `_start` instance instantiated ahead of time, with argv, env and the tree supplied later. The shim reads
 * `args`, `env` and `fds` when WASI calls `args_get`/`environ_get`/`fd_*`, i.e. at `_start`, so they can be set after instantiation.
 */
export async function spareInstance(module) {
  const tree = new Map();
  const out = [], err = [];
  const fds = [new OpenFile(new File(new Uint8Array(0))), new ConsoleStdout((b) => out.push(b.slice())), new ConsoleStdout((b) => err.push(b.slice())), new PreopenDirectory("/", tree)];
  const wasi = new WASI(["pandoc.wasm"], [], fds, { debug: false });
  const o = wasi.wasiImport.args_sizes_get;
  wasi.wasiImport.args_sizes_get = (a, b) => { const r = o.call(wasi.wasiImport, a, b); new DataView(wasi.inst.exports.memory.buffer).setUint32(b, wasi.args.reduce((n, x) => n + enc.encode(x).length + 1, 0), true); return r; };
  const t0 = now();
  const instance = await WebAssembly.instantiate(module, { wasi_snapshot_preview1: wasi.wasiImport });
  const initMs = now() - t0;
  /** Supply the request and run `_start` (once). */
  function run(rec, { env: envOver = {}, argv = rec.argv } = {}) {
    const t0 = now();
    const root = buildTree(rec);
    wasi.args = argv;
    wasi.env = Object.entries({ ...rec.env, ...envOver }).map(([k, v]) => `${k}=${v}`);
    wasi.fds[3] = new PreopenDirectory("/", root);
    const t1 = now();
    let status = null, trap = null;
    try { status = wasi.start(instance); } catch (e) { if (e instanceof WASIProcExit) status = e.code; else trap = e; }
    const t2 = now();
    const output = readTree(root, rec.output);
    return { status, trap, stderr: dec.decode(concat(err)), output, mountMs: t1 - t0, runMs: t2 - t1, readMs: now() - t2, totalMs: now() - t0, memBytes: instance.exports.memory.buffer.byteLength };
  }
  return { initMs, run };
}

const luaStr = (s) => '"' + [...enc.encode(s)].map((c) => (c >= 32 && c < 127 && c !== 34 && c !== 92 ? String.fromCharCode(c) : "\\" + String(c).padStart(3, "0"))).join("") + '"';
/** Lua preamble: os.getenv answers from a per-render table, whatever the WASI environ says. */
export const preamble = (env) => `local __q2env = {${Object.entries(env).map(([k, v]) => `[${luaStr(k)}]=${luaStr(v)}`).join(",")}}\nos.getenv = function(k) return __q2env[k] end\n`;
export const INIT = "/__q2_share__/pandoc/datadir/init.lua";
/** The recording's files with the preamble prepended to datadir/init.lua (it runs in every Lua state, before Quarto's own init code). */
export const withPreamble = (rec, env) => rec.files.map((f) => (f.mount === INIT ? { ...f, bytes: enc.encode(preamble(env) + dec.decode(f.bytes)) } : f));
