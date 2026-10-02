// Q1: can pandoc.wasm be re-entered?  (a) _start twice on one instance  (b) hs_init + convert x N (reactor-style exports)
import { loadModule, loadRecording, freshRun, warmInstance, argvToOptions, buildTree, WASM } from "./common.mjs";
import { ConsoleStdout, File, OpenFile, PreopenDirectory, WASI, WASIProcExit } from "../../../../node_modules/@bjorn3/browser_wasi_shim/dist/index.js";
const dec = new TextDecoder();
const module = await loadModule();
const rec = loadRecording("callouts-typst");

console.log("== (a) command mode, _start twice on ONE instance");
{
  const err = [];
  const tree = buildTree(rec);
  const fds = [new OpenFile(new File(new Uint8Array(0))), new ConsoleStdout(() => {}), new ConsoleStdout((b) => err.push(b.slice())), new PreopenDirectory("/", tree)];
  const wasi = new WASI(rec.argv, Object.entries(rec.env).map(([k, v]) => `${k}=${v}`), fds, { debug: false });
  const o = wasi.wasiImport.args_sizes_get;
  wasi.wasiImport.args_sizes_get = (a, b) => { const r = o.call(wasi.wasiImport, a, b); new DataView(wasi.inst.exports.memory.buffer).setUint32(b, rec.argv.reduce((n, x) => n + Buffer.byteLength(x) + 1, 0), true); return r; };
  const inst = await WebAssembly.instantiate(module, { wasi_snapshot_preview1: wasi.wasiImport });
  for (const n of [1, 2]) {
    try { const st = wasi.start(inst); console.log(`start #${n}: returned`, st); }
    catch (e) { console.log(`start #${n}:`, e instanceof WASIProcExit ? `proc_exit(${e.code})` : `${e.constructor.name}: ${e.message}`); }
  }
  console.log("stderr:", dec.decode(err[0] ?? new Uint8Array()).slice(0, 300));
}

console.log("== (a2) _start re-entry after proc_exit, wasi.start with fresh WASI object on the SAME instance is impossible (imports bound at instantiate)");

console.log("== (b) reactor-style: hs_init once, convert x3");
{
  const w = await warmInstance(module, rec);
  console.log("init ms", w.initMs.toFixed(1));
  const opts = argvToOptions(rec.argv);
  for (let i = 1; i <= 3; i++) {
    const r = w.convert(opts);
    console.log(`convert #${i}: trap=${r.trap ? String(r.trap).slice(0, 120) : null} out=${r.output?.length} run=${r.runMs.toFixed(0)}ms stderr=${JSON.stringify(r.stderr.slice(0, 200))} warn=${JSON.stringify(r.warnings.slice(0, 120))}`);
  }
  const ref = loadRecording("callouts-typst");
  const fresh = await freshRun(module, ref);
  const r = w.convert(opts);
  console.log("fresh output length", fresh.output.length, "status", fresh.status, "| warm equals fresh:", Buffer.from(fresh.output).equals(Buffer.from(r.output ?? [])));
}
