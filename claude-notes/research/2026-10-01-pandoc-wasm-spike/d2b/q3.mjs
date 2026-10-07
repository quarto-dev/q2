// Q3: correctness across consecutive renders on ONE warm instance (A,B,A; env between; preamble-delivered env).
import { unzipSync } from "../../../../node_modules/fflate/esm/browser.js";
import { loadModule, loadRecording, freshRun, warmInstance, argvToOptions, withPreamble } from "./common.mjs";
const module = await loadModule();
const enc = new TextEncoder(), dec = new TextDecoder();
const eq = (a, b) => !!a && !!b && Buffer.from(a).equals(Buffer.from(b));
const opts = (rec) => argvToOptions(rec.argv);
const log = (...a) => console.log(...a);

const A = loadRecording("callouts-typst"), B = loadRecording("tables-typst"), C = loadRecording("crossrefs-typst");
const refA = (await freshRun(module, A)).output, refB = (await freshRun(module, B)).output, refC = (await freshRun(module, C)).output;
log("fresh references:", refA.length, refB.length, refC.length, "(A fresh twice equal:", eq(refA, (await freshRun(module, A)).output), ")");

log("\n== T1 A,B,A,C,A,B on one warm instance (env at creation = recording env)");
{
  const w = await warmInstance(module, A);
  const seq = [["A", A, refA], ["B", B, refB], ["A", A, refA], ["C", C, refC], ["A", A, refA], ["B", B, refB]];
  for (const [n, rec, ref] of seq) { const r = w.convert(opts(rec), { files: rec.files }); log(`${n}: equals fresh ${eq(r.output, ref)} (${r.output?.length}) trap=${r.trap ? String(r.trap).slice(0, 80) : null} run ${r.runMs.toFixed(0)} ms`); }
}

log("\n== T2a env only via Lua preamble (WASI env EMPTY at creation); render A,B,A");
{
  const w = await warmInstance(module, A, { env: {} });
  for (const [n, rec, ref] of [["A", A, refA], ["B", B, refB], ["A", A, refA]]) {
    const r = w.convert(opts(rec), { files: withPreamble(rec, rec.env) });
    log(`${n}: equals fresh ${eq(r.output, ref)} (${r.output?.length}) stderr=${JSON.stringify(r.stderr.slice(0, 160))} trap=${r.trap ? String(r.trap).slice(0, 80) : null}`);
  }
}

log("\n== T2b different env value between renders (QUARTO_FILTER_PARAMS: callout-caution-title Caution -> CAUTION-CHANGED), then A again");
{
  const params = JSON.parse(Buffer.from(A.env.QUARTO_FILTER_PARAMS, "base64").toString());
  params.language["callout-caution-title"] = "CAUTION-CHANGED";
  const envX = { ...A.env, QUARTO_FILTER_PARAMS: Buffer.from(JSON.stringify(params)).toString("base64") };
  const w = await warmInstance(module, A, { env: {} });
  const run = (env) => w.convert(opts(A), { files: withPreamble(A, env) });
  const r1 = run(A.env), r2 = run(envX), r3 = run(A.env);
  const fx = await freshRun(module, A, { env: { QUARTO_FILTER_PARAMS: envX.QUARTO_FILTER_PARAMS } });
  log("A    equals fresh A:", eq(r1.output, refA));
  log("A(x) differs from A:", !eq(r2.output, refA), "| contains CAUTION-CHANGED:", dec.decode(r2.output).includes("CAUTION-CHANGED"), "| equals fresh with the same env:", eq(r2.output, fx.output));
  log("A    again equals A :", eq(r3.output, refA));
}

log("\n== T2c env visible to Lua WITHOUT a preamble? mutate the WASI env array after creation (A, then changed params, then A)");
{
  const params = JSON.parse(Buffer.from(A.env.QUARTO_FILTER_PARAMS, "base64").toString());
  params.language["callout-caution-title"] = "CAUTION-CHANGED";
  const w = await warmInstance(module, A);
  const r1 = w.convert(opts(A));
  const i = w.env.findIndex((s) => s.startsWith("QUARTO_FILTER_PARAMS="));
  w.env[i] = "QUARTO_FILTER_PARAMS=" + Buffer.from(JSON.stringify(params)).toString("base64");
  const r2 = w.convert(opts(A));
  log("after mutating the env array: output still equals the first (env NOT re-read):", eq(r1.output, r2.output), "| contains CAUTION-CHANGED:", dec.decode(r2.output).includes("CAUTION-CHANGED"));
}

log("\n== T2d SOURCE_DATE_EPOCH (read by pandoc's Haskell side for docx dcterms:created)");
{
  const D = loadRecording("callouts-docx");
  const created = (bytes) => /<dcterms:created[^>]*>([^<]*)</.exec(dec.decode(unzipSync(bytes)["docProps/core.xml"]))?.[1];
  const sde = (v) => ({ ...D.env, SOURCE_DATE_EPOCH: String(v) });
  const fx1 = await freshRun(module, D, { env: { SOURCE_DATE_EPOCH: "1000000000" } }), fx2 = await freshRun(module, D, { env: { SOURCE_DATE_EPOCH: "1700000000" } });
  log("fresh: SDE=1e9 ->", created(fx1.output), "| SDE=1.7e9 ->", created(fx2.output));
  // (i) WASI env array mutated between renders
  let w = await warmInstance(module, D, { env: sde(1000000000) });
  const a = w.convert(opts(D));
  const k = w.env.findIndex((s) => s.startsWith("SOURCE_DATE_EPOCH=")); w.env[k] = "SOURCE_DATE_EPOCH=1700000000";
  const b = w.convert(opts(D));
  log("warm, env array mutated: first", created(a.output), "| second", created(b.output));
  // (ii) Lua preamble only (WASI env empty)
  w = await warmInstance(module, D, { env: {} });
  const c = w.convert(opts(D), { files: withPreamble(D, sde(1000000000)) }), d = w.convert(opts(D), { files: withPreamble(D, sde(1700000000)) });
  log("warm, preamble only (WASI env empty): first", created(c.output), "| second", created(d.output));
  // (iii) a second warm instance created per distinct env (the fallback): env at creation
  log("fresh-per-env equals warm(i) first:", eq(fx1.output, a.output), "(zip bytes include timestamps, so byte equality = same SDE seen)");
}

log("\n== T2e discriminating probe: a Lua filter (own Lua state, datadir init.lua with preamble) reports os.getenv('Q2_PROBE'), SOURCE_DATE_EPOCH, #params");
{
  const probe = `function Pandoc(d) return pandoc.Pandoc({pandoc.Para{pandoc.Str(tostring(os.getenv("Q2_PROBE")).."|"..tostring(os.getenv("SOURCE_DATE_EPOCH")).."|"..tostring(#(os.getenv("QUARTO_FILTER_PARAMS") or "")))}}, d.meta) end`;
  const doc = JSON.stringify({ "pandoc-api-version": [1, 23, 1], meta: {}, blocks: [{ t: "Para", c: [{ t: "Str", c: "x" }] }] });
  const base = (files) => ({ files: [...files, { mount: "/probe.lua", bytes: enc.encode(probe) }, { mount: "/in.json", bytes: enc.encode(doc) }] });
  const o = { from: "json", to: "plain", "data-dir": "/__q2_share__/pandoc/datadir", filters: [{ type: "lua", path: "/probe.lua" }], "input-files": ["/in.json"], "output-file": "/out.txt" };
  const out = (r) => JSON.stringify(dec.decode(r.output ?? new Uint8Array()).trim()) + (r.stderr ? ` stderr=${JSON.stringify(r.stderr.slice(0, 120))}` : "");
  const e = (p, s) => ({ ...A.env, Q2_PROBE: p, SOURCE_DATE_EPOCH: s });
  let w = await warmInstance(module, A, { env: {} });
  for (const [p, s] of [["one", "111"], ["two", "222"], ["one", "111"]]) log(`preamble env ${p}/${s}:`, out(w.convert(o, base(withPreamble(A, e(p, s))))));
  w = await warmInstance(module, A, { env: e("one", "111") });
  log("WASI env array, created one/111:", out(w.convert(o, base(A.files))));
  const k = (pre) => w.env.findIndex((s) => s.startsWith(pre));
  w.env[k("Q2_PROBE=")] = "Q2_PROBE=two"; w.env[k("SOURCE_DATE_EPOCH=")] = "SOURCE_DATE_EPOCH=222";
  log("WASI env array, mutated to two/222 :", out(w.convert(o, base(A.files))));
}
