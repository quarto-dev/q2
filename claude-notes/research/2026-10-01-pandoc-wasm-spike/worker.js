// Web Worker: lazily fetches pandoc.wasm on first "convert" request, then reuses the instance.
import { createPandocInstance } from "pandoc-wasm-core";
let pandoc = null;
async function ensure(url) {
  if (pandoc) return { cached: true };
  const t0 = performance.now();
  const res = await fetch(url);
  const buf = await res.arrayBuffer();
  const tFetch = performance.now() - t0;
  pandoc = await createPandocInstance(buf);
  return { cached: false, fetchMs: tFetch, totalMs: performance.now() - t0, bytes: buf.byteLength,
           encoding: res.headers.get("content-encoding") };
}
self.onmessage = async ({ data }) => {
  try {
    const load = await ensure(data.wasmUrl);
    const t = performance.now();
    const r = await pandoc.convert(data.opts, data.stdin, data.files || {});
    const blob = data.opts["output-file"] ? r.files[data.opts["output-file"]] : null;
    const bytes = blob ? new Uint8Array(await blob.arrayBuffer()) : null;
    self.postMessage({ id: data.id, load, convertMs: performance.now() - t,
      stdout: r.stdout.slice(0, 200), bytes, warnings: r.warnings, stderr: r.stderr,
      heapMB: undefined }, bytes ? [bytes.buffer] : []);
  } catch (e) { self.postMessage({ id: data.id, error: String(e && e.stack || e) }); }
};
