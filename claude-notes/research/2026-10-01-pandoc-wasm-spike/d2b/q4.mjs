// Q4: memory over 100 consecutive renders (warm vs fresh) and recovery after errors.
import { loadModule, loadRecording, freshRun, warmInstance, argvToOptions } from "./common.mjs";
const module = await loadModule();
const enc = new TextEncoder(), dec = new TextDecoder();
const eq = (a, b) => !!a && !!b && Buffer.from(a).equals(Buffer.from(b));
const A = loadRecording("callouts-typst");
const refA = (await freshRun(module, A)).output;
const mb = (n) => (n / 1048576).toFixed(0);
const gc = () => globalThis.gc?.();
const N = Number(process.env.N ?? 100);

console.log(`== memory over ${N} renders (callouts), warm instance`);
{
  const w = await warmInstance(module, A); const o = argvToOptions(A.argv);
  gc(); const rss0 = process.memoryUsage().rss; const rows = [];
  let bad = 0;
  for (let i = 1; i <= N; i++) {
    const r = w.convert(o); if (!eq(r.output, refA)) bad++;
    if (i === 1 || i % 10 === 0) { gc(); rows.push(`#${i}: wasm ${mb(w.mem())} MB, rss ${mb(process.memoryUsage().rss)} MB`); }
  }
  console.log(rows.join("\n")); console.log(`outputs differing from the first fresh render: ${bad}; rss growth over run: ${mb(process.memoryUsage().rss - rss0)} MB`);
}
console.log(`\n== memory over ${N} renders (callouts), fresh instance each (production shape)`);
{
  gc(); const rss0 = process.memoryUsage().rss; const rows = [];
  for (let i = 1; i <= N; i++) { await freshRun(module, A); if (i === 1 || i % 10 === 0) { gc(); rows.push(`#${i}: rss ${mb(process.memoryUsage().rss)} MB`); } }
  console.log(rows.join("\n")); console.log(`rss growth over run: ${mb(process.memoryUsage().rss - rss0)} MB`);
}

console.log("\n== errors on a warm instance, each followed by a good render (must equal the fresh reference)");
{
  const w = await warmInstance(module, A); const o = argvToOptions(A.argv);
  const good = (label) => { const r = w.convert(o); console.log(`   after ${label}: good render equals reference = ${eq(r.output, refA)} trap=${r.trap ? String(r.trap).slice(0, 100) : null} wasm ${mb(w.mem())} MB`); };
  const file = (m, t) => ({ mount: m, bytes: enc.encode(t) });
  const doc = JSON.stringify({ "pandoc-api-version": [1, 23, 1], meta: {}, blocks: [{ t: "Para", c: [{ t: "Str", c: "x" }] }] });
  const tiny = (filter, extra = {}) => ({ from: "json", to: "plain", "data-dir": "/__q2_share__/pandoc/datadir", filters: filter ? [{ type: "lua", path: "/f.lua" }] : [], "input-files": ["/in.json"], "output-file": "/out.txt", ...extra });
  const files = (f) => [...A.files, file("/in.json", doc), ...(f ? [file("/f.lua", f)] : [])];
  const show = (label, r) => console.log(`${label}: trap=${r.trap ? JSON.stringify(String(r.trap).slice(0, 100)) : null} output=${r.output ? JSON.stringify(dec.decode(r.output).slice(0, 60)) : null} stderr=${JSON.stringify(r.stderr.slice(0, 200))} warn=${JSON.stringify(r.warnings.replace(/\s+/g, " ").slice(0, 160))}`);
  good("start");
  show("1 missing input file       ", w.convert({ ...tiny(null), "input-files": ["/nope.json"] }, { files: files() })); good("missing input");
  show("2 Lua syntax error         ", w.convert(tiny(true), { files: files("function Str(s) return ( end") })); good("Lua syntax error");
  show("3 filter throws error()    ", w.convert(tiny(true), { files: files("function Str(s) error('boom from filter') end") })); good("error() in filter");
  show("4 filter: nil index        ", w.convert(tiny(true), { files: files("function Str(s) local t = nil; return t.x end") })); good("nil index in filter");
  show("5 missing filter file      ", w.convert(tiny(true), { files: files(null) })); good("missing filter");
  show("6 bad format (-t nonesuch) ", w.convert({ ...tiny(null), to: "nonesuch" }, { files: files() })); good("bad output format");
  show("7 bad json input           ", w.convert(tiny(null), { files: [...A.files, file("/in.json", "{not json")] })); good("bad JSON input");
  show("8 filter calls os.exit(3)  ", w.convert(tiny(true), { files: files("os.exit(3)") })); good("os.exit in filter");
  show("9 filter allocs 2 GB string", w.convert(tiny(true), { files: files("local t = {} for i=1,64 do t[i] = string.rep('x', 32*1024*1024) end") })); good("big allocation in filter");
}
