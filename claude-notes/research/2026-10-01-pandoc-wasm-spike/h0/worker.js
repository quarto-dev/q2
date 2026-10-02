// H0 worker: runs pandoc.wasm in WASI command mode (_start) on R0's recordings.
import { WASI, WASIProcExit, File, Directory, PreopenDirectory, ConsoleStdout, OpenFile } from "/shim/index.js";

const enc = new TextEncoder(), dec = new TextDecoder();
let module_ = null;
const cache = new Map(); // url -> Uint8Array (fetched bytes; trees are rebuilt per run)

async function bytes(url) {
  if (!cache.has(url)) {
    const r = await fetch(url);
    if (!r.ok) throw new Error(`${url}: ${r.status}`);
    cache.set(url, new Uint8Array(await r.arrayBuffer()));
  }
  return cache.get(url);
}
async function json(url) { return JSON.parse(dec.decode(await bytes(url))); }
async function mapLimit(items, n, f) {
  let i = 0; const out = new Array(items.length);
  await Promise.all(Array.from({ length: n }, async () => { while (i < items.length) { const k = i++; out[k] = await f(items[k], k); } }));
  return out;
}

// A recording as {argv, env, meta, files: [{mount: "/__q2_doc__/x.qmd", url}]}; the share tree is mounted too.
async function loadRecording(name) {
  const base = `/rec/${encodeURIComponent(name)}`;
  const [argv, env, meta, manifest] = await Promise.all([json(`${base}/argv.json`), json(`${base}/env.json`), json(`${base}/meta.json`), json(`${base}/manifest.json`)]);
  const files = [];
  for (const m of manifest) if (m.path.startsWith("fs/")) files.push({ mount: "/" + m.path.slice(3), url: `${base}/${m.path.split("/").map(encodeURIComponent).join("/")}` });
  const sm = await json(`/rec/share/${meta.share_tree}.manifest.json`);
  for (const m of sm) files.push({ mount: "/__q2_share__/" + m.path, url: `/rec/share/${meta.share_tree}/${m.path.split("/").map(encodeURIComponent).join("/")}` });
  await mapLimit(files, 16, (f) => bytes(f.url));
  return { argv, env, meta, files };
}

function buildTree(rec, { extraDirs = ["tmp", "__q2_out__"], drop = [], extraFiles = {} } = {}) {
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
  for (const d of extraDirs) mkdirp([d]);
  for (const f of rec.files) {
    if (drop.includes(f.mount)) continue;
    const parts = f.mount.split("/").filter(Boolean);
    const dir = mkdirp(parts.slice(0, -1));
    dir.set(parts.at(-1), new File(cache.get(f.url)));
  }
  for (const [mount, text] of Object.entries(extraFiles)) {
    const parts = mount.split("/").filter(Boolean);
    mkdirp(parts.slice(0, -1)).set(parts.at(-1), new File(enc.encode(text)));
  }
  return root;
}
function readFile(root, absPath) {
  let cur = root;
  for (const p of absPath.split("/").filter(Boolean)) { cur = cur?.get(p)?.contents ?? cur?.get(p); if (!cur) return null; }
  return cur?.data ?? null;
}

// Run `_start` once on a fresh instance. opts: argv/env overrides, utf8Args (default true), stderrMode.
async function run(rec, opts = {}) {
  const argv = opts.argv ?? rec.argv;
  const env = { ...rec.env, ...(opts.env ?? {}) };
  const tree = buildTree(rec, opts);
  const out = [], err = [];
  const lineErr = [];
  const stderrFd = opts.lineBufferedStderr
    ? ConsoleStdout.lineBuffered((l) => lineErr.push(l))
    : new ConsoleStdout((b) => err.push(b.slice()));
  const fds = [new OpenFile(new File([])), new ConsoleStdout((b) => out.push(b.slice())), stderrFd, new PreopenDirectory("/", tree)];
  const wasi = new WASI(argv, Object.entries(env).map(([k, v]) => `${k}=${v}`), fds, { debug: false });
  if (opts.utf8Args !== false) {
    const orig = wasi.wasiImport.args_sizes_get;
    wasi.wasiImport.args_sizes_get = (argcPtr, bufSizePtr) => {
      const r = orig.call(wasi.wasiImport, argcPtr, bufSizePtr);
      const dv = new DataView(wasi.inst.exports.memory.buffer);
      dv.setUint32(bufSizePtr, wasi.args.reduce((n, a) => n + enc.encode(a).length + 1, 0), true);
      return r;
    };
  }
  const poll = { calls: 0, nonzero: 0, codes: [] };
  const origPoll = wasi.wasiImport.poll_oneoff;
  wasi.wasiImport.poll_oneoff = (...a) => { const r = origPoll.apply(wasi.wasiImport, a); poll.calls++; if (r !== 0) { poll.nonzero++; poll.codes.push(r); } return r; };
  const t0 = performance.now();
  const inst = await WebAssembly.instantiate(module_, { wasi_snapshot_preview1: wasi.wasiImport });
  const t1 = performance.now();
  let exit, trap = null;
  try { exit = wasi.start(inst); } catch (e) { exit = null; trap = String(e && e.stack || e).slice(0, 400); }
  const t2 = performance.now();
  const cat = (chunks) => { const n = chunks.reduce((s, c) => s + c.length, 0), o = new Uint8Array(n); let k = 0; for (const c of chunks) { o.set(c, k); k += c.length; } return o; };
  const stderr = opts.lineBufferedStderr ? lineErr.join("\n") : dec.decode(cat(err));
  return {
    exit, trap, stderr, stdout: dec.decode(cat(out)), poll,
    t: { instanceMs: Math.round(t1 - t0), runMs: Math.round(t2 - t1) },
    memoryMB: +(inst.exports.memory.buffer.byteLength / 1048576).toFixed(1),
    tree,
  };
}

const outPathOf = (rec) => rec.meta.output;
const stripFilters = (argv) => { const o = []; for (let i = 0; i < argv.length; i++) { if (argv[i] === "-L") { i++; continue; } o.push(argv[i]); } return o; };
const short = (r) => ({ exit: r.exit, trap: r.trap, stderr: r.stderr.slice(-300), poll: r.poll, t: r.t, memoryMB: r.memoryMB });

const ops = {
  async init({ wasmUrl }) {
    const probe = WebAssembly.validate(new Uint8Array([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0, 1, 5, 1, 0x60, 0, 1, 0x69]));
    const t0 = performance.now();
    const res = await fetch(wasmUrl);
    const buf = await res.arrayBuffer();
    const t1 = performance.now();
    module_ = await WebAssembly.compile(buf);
    const t2 = performance.now();
    return { ok: true, exnrefProbe: probe, bytes: buf.byteLength, fetchMs: Math.round(t1 - t0), compileMs: Math.round(t2 - t1), encoding: res.headers.get("content-encoding"),
      wasmImports: [...new Set(WebAssembly.Module.imports(module_).map((i) => i.module))], exports: WebAssembly.Module.exports(module_).map((e) => e.name) };
  },
  async replay({ name }) {
    const rec = await loadRecording(name);
    const r = await run(rec);
    const outName = outPathOf(rec);
    const data = readFile(r.tree, outName);
    return { type: "result", name, ...short(r), outputName: outName.split("/").at(-1), files: { output: data } };
  },
  // SVG in a docx (D9): pandoc.wasm cannot rasterize; produce the files a human opens in Word.
  async svg() {
    const rec = await loadRecording("callouts-docx");
    const svg = '<svg xmlns="http://www.w3.org/2000/svg" width="120" height="60" viewBox="0 0 120 60"><rect width="120" height="60" fill="#447099"/><circle cx="30" cy="30" r="20" fill="#fff"/><text x="60" y="36" font-size="16" fill="#fff">SVG</text></svg>';
    const md = (img) => `# SVG test\n\nBefore.\n\n![alt text for the image](${img}){width=2in}\n\nAfter.\n`;
    const files = {}, info = {};
    for (const [key, img, extra] of [["svg", "/__q2_doc__/pic.svg", { "/__q2_doc__/pic.svg": svg }], ["png-control", "/__q2_share__/formats/docx/note.png", {}]]) {
      const argv = ["pandoc", "-f", "markdown", "-t", "docx", "--resource-path", "/__q2_doc__", "-o", `/__q2_doc__/${key}.docx`, "/__q2_doc__/in.md"];
      const r = await run(rec, { argv, extraFiles: { ...extra, "/__q2_doc__/in.md": md(img) } });
      const d = readFile(r.tree, `/__q2_doc__/${key}.docx`);
      info[key] = { exit: r.exit, stderr: r.stderr.slice(0, 400), bytes: d?.length };
      if (d) files[`${key}.docx`] = d;
    }
    return { type: "result", info, files };
  },
  async probes() {
    const res = {};
    const rec = await loadRecording("callouts-docx");
    // exit codes
    res.exit_ok = short(await run(rec));
    res.exit_83_no_dependency_file = short(await run(rec, { drop: ["/__q2_tmp__/pandoc-filter-deps.txt"] }));
    res.exit_6_bad_option = short(await run(rec, { argv: [...rec.argv.slice(0, 1), "--no-such-option", ...rec.argv.slice(1)] }));
    res.exit_1_missing_input = short(await run(rec, { argv: [...rec.argv.slice(0, -1), "/__q2_tmp__/missing.json"] }));
    res.exit_251_rts_M = short(await run(rec, { argv: [rec.argv[0], "+RTS", "-M5m", "-RTS", ...rec.argv.slice(1)] }));
    for (const m of ["8m", "12m", "16m", "24m", "32m", "48m"]) res["rts_M_" + m] = (({ exit, stderr }) => ({ exit, stderr: stderr.slice(-120) }))(await run(rec, { argv: [rec.argv[0], "+RTS", "-M" + m, "-RTS", ...rec.argv.slice(1)] }));
    res.rts_M_generous_ok = short(await run(rec, { argv: [rec.argv[0], "+RTS", "-M1g", "-RTS", ...rec.argv.slice(1)] }));
    // stderr: whole capture vs ConsoleStdout.lineBuffered, on a run whose stderr has no final newline
    const bad = { extraFiles: { "/__q2_tmp__/partial.lua": 'io.stderr:write("no-newline-tail")' }, argv: [...rec.argv.slice(0, 1), "-L", "/__q2_tmp__/partial.lua", ...rec.argv.slice(1)] };
    const whole = await run(rec, bad), lined = await run(rec, { ...bad, lineBufferedStderr: true });
    res.stderr_capture = { wholeLen: whole.stderr.length, wholeEndsWithNewline: whole.stderr.endsWith("\n"), wholeTail: whole.stderr.slice(-120),
      lineBufferedLen: lined.stderr.length, lineBufferedLostChars: whole.stderr.length - (lined.stderr.length + (whole.stderr.endsWith("\n") ? 1 : 0)) };
    // non-ASCII argv: output name and a long non-ASCII metadata value
    const longTitle = "数据".repeat(300) + "-résumé";
    const uArgv = (o) => { const a = [...rec.argv]; a[a.indexOf("-o") + 1] = o; return [...a.slice(0, -1), "--metadata", "title=" + longTitle, a.at(-1)]; };
    const nonAscii = "/__q2_doc__/数据 résumé.docx";
    const withFix = await run(rec, { argv: uArgv(nonAscii) });
    const noFix = await run(rec, { argv: uArgv(nonAscii), utf8Args: false });
    res.non_ascii = { withUtf8ArgsSizes: { ...short(withFix), wrote: !!readFile(withFix.tree, nonAscii) }, withShimArgsSizes: { ...short(noFix), wrote: !!readFile(noFix.tree, nonAscii) } };
    // a short non-ASCII output name only (no long arg): does the shim's UTF-16 count suffice?
    const shortU = await run(rec, { argv: (() => { const a = [...rec.argv]; a[a.indexOf("-o") + 1] = nonAscii; return a; })(), utf8Args: false });
    res.non_ascii_short_shim_counts = { ...short(shortU), wrote: !!readFile(shortU.tree, nonAscii) };
    // second _start on the same instance traps
    {
      const tree = buildTree(rec); const fds = [new OpenFile(new File([])), new ConsoleStdout(() => {}), new ConsoleStdout(() => {}), new PreopenDirectory("/", tree)];
      const wasi = new WASI(rec.argv, Object.entries(rec.env).map(([k, v]) => `${k}=${v}`), fds, { debug: false });
      const inst = await WebAssembly.instantiate(module_, { wasi_snapshot_preview1: wasi.wasiImport });
      const first = wasi.start(inst); let second;
      try { second = wasi.start(inst); } catch (e) { second = "threw: " + String(e).slice(0, 120); }
      res.second_start_same_instance = { first, second };
    }
    // latency / Lua startup: fresh-instance docx with and without the Lua filter set, 5 runs each
    const times = async (r, o) => { const a = []; for (let i = 0; i < 5; i++) { const x = await run(r, o); a.push(x.t.instanceMs + x.t.runMs); } return a; };
    res.latency_ms_fresh_instance = {
      with_filters: await times(rec, {}),
      without_filters: await times(rec, { argv: stripFilters(rec.argv) }),
    };
    const nof = await run(rec, { argv: stripFilters(rec.argv) });
    res.memoryMB = { with_filters: (await run(rec)).memoryMB, without_filters: nof.memoryMB };
    // pandoc-profile crossref index (open question): does a run leave .quarto/crossref-index.json anywhere?
    const cr = await run(await loadRecording("crossrefs-docx"));
    const walk = (m, p = "", acc = []) => { for (const [k, v] of m) { if (v.contents) walk(v.contents, p + "/" + k, acc); else acc.push(p + "/" + k); } return acc; };
    // writeIndex gating: add `crossref-index-file` (present natively only for non-single-file projects) to the params blob
    {
      const crRec = await loadRecording("crossrefs-docx");
      const params = JSON.parse(dec.decode(Uint8Array.from(atob(crRec.env.QUARTO_FILTER_PARAMS), (c) => c.charCodeAt(0))));
      res.params_mediabag_dir = params["mediabag-dir"] ?? null;
      const withIdx = { ...params, "crossref-index-file": "/__q2_doc__/.quarto/crossref-index.json" };
      const b64 = btoa(String.fromCharCode(...enc.encode(JSON.stringify(withIdx))));
      const withDir = await run(crRec, { env: { QUARTO_FILTER_PARAMS: b64 }, extraFiles: { "/__q2_doc__/.quarto/keep": "" } });
      const noDir = await run(crRec, { env: { QUARTO_FILTER_PARAMS: b64 } });
      const idx = readFile(withDir.tree, "/__q2_doc__/.quarto/crossref-index.json");
      res.writeIndex = { withDotQuartoDir: { exit: withDir.exit, wrote: !!idx, bytes: idx?.length, head: idx ? dec.decode(idx).slice(0, 200) : null },
        withoutDotQuartoDir: { exit: noDir.exit, wrote: !!readFile(noDir.tree, "/__q2_doc__/.quarto/crossref-index.json"), stderr: noDir.stderr.slice(0, 300) } };
    }
    res.crossref_index_files = walk(cr.tree).filter((p) => /crossref-index|\.quarto/.test(p));
    res.files_written_outside_inputs = walk(cr.tree).filter((p) => !rec.files.some((f) => f.mount === p) && !/^\/(__q2_share__|__q2_tmp__\/pandoc-input|__q2_doc__\/crossrefs\.qmd)/.test(p)).slice(0, 20);
    return res;
  },
};

self.onmessage = async ({ data }) => {
  const { id, op } = data;
  try {
    const r = await ops[op](data);
    if (op === "replay" || op === "svg") {
      self.postMessage({ ...r, id, type: "result" }, Object.values(r.files).filter(Boolean).map((u) => u.buffer));
    } else self.postMessage({ type: "result", id, ...r });
  } catch (e) { self.postMessage({ type: "result", id, error: String(e && e.stack || e), exit: null, files: {} }); }
};
