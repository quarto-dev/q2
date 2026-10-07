import * as q2 from "wasm-quarto-hub-client";
  import { setVfsCallbacks } from "/src/wasm-js-bridge/sass.js";
  const worker = new Worker(new URL("./chain-worker.js", import.meta.url), { type: "module" });
  const pending = new Map(); let n = 0;
  worker.onmessage = ({ data }) => { pending.get(data.id)(data); pending.delete(data.id); };
  const DOC = "---\ntitle: Chain\nformat: q2-preview\n---\n\n# Hello\n\nSome *emphasis* and a list:\n\n- one\n- two\n\n::: {.callout-note}\nA note.\n:::\n";
  let q2mem;
  window.initQ2 = async () => {
    const t = performance.now(); const out = await q2.default(); q2mem = out.memory;
    const rd = (p) => { try { const r = JSON.parse(q2.vfs_read_file(p)); return r.success ? r.content : null; } catch { return null; } };
    setVfsCallbacks(rd, (p) => rd(p) != null);
    return { ms: Math.round(performance.now() - t), q2WasmMB: Math.round(q2mem.buffer.byteLength / 1e6) };
  };
  window.renderAst = async () => {
    q2.vfs_clear(); q2.vfs_add_file("/project/doc.qmd", DOC);
    const t = performance.now();
    const r = JSON.parse(await q2.render_page_in_project_with_attribution("/project/doc.qmd", undefined, undefined, undefined));
    return { ms: Math.round(performance.now() - t), ok: r.success, ast: r.ast_json, q2WasmMB: Math.round(q2mem.buffer.byteLength / 1e6) };
  };
  window.pandocConvert = (to, ext, ast) => new Promise((res) => { const id = ++n; pending.set(id, res);
    worker.postMessage({ id, wasmUrl: new URL("./pandoc.wasm", import.meta.url).pathname,
      opts: { from: "json", to, standalone: true, "output-file": "o." + ext }, stdin: ast }); });
  window.ready = true;
