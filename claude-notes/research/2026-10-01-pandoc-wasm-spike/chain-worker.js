// Lazy pandoc Worker: nothing is fetched or compiled until the first "convert" message.
import { createPandocInstance } from "./host-patched.js";
let p = null;
self.onmessage = async ({ data }) => {
  try {
    let load = { cached: true };
    if (!p) {
      const t = performance.now();
      const buf = await (await fetch(data.wasmUrl)).arrayBuffer();
      const tf = performance.now() - t;
      p = await createPandocInstance(buf);
      load = { cached: false, fetchMs: Math.round(tf), totalMs: Math.round(performance.now() - t) };
    }
    const t = performance.now();
    const r = await p.convert(data.opts, data.stdin, {});
    const blob = r.files[data.opts["output-file"]];
    const bytes = blob ? new Uint8Array(await blob.arrayBuffer()) : null;
    self.postMessage({ id: data.id, load, convertMs: Math.round(performance.now() - t), stderr: r.stderr,
      bytes, pandocWasmMB: Math.round(p.memoryBytes() / 1e6) }, bytes ? [bytes.buffer] : []);
  } catch (e) { self.postMessage({ id: data.id, error: String(e?.stack || e) }); }
};
