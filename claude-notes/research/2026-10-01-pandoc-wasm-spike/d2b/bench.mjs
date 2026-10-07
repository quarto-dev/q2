// Q2 (Node/V8): per-render latency, fresh instance (production shape) vs one warm instance. Module compiled once, resident.
import { loadModule, loadRecording, freshRun, warmInstance, argvToOptions, median } from "./common.mjs";
const module = await loadModule();
const N = Number(process.env.N ?? 10);
const fixtures = [["callouts", loadRecording("callouts-typst")], ["callouts-x10", loadRecording("callouts-typst", { repeat: 10 })]];
const f = (xs) => `${median(xs).toFixed(0)} (min ${Math.min(...xs).toFixed(0)}, max ${Math.max(...xs).toFixed(0)})`;
for (const [name, rec] of fixtures) {
  const inBytes = rec.files.find((x) => x.mount.endsWith("pandoc-input.json")).bytes.length;
  console.log(`\n### ${name}: input json ${inBytes} B`);
  // fresh: discard first
  await freshRun(module, rec);
  const fr = []; for (let i = 0; i < N; i++) fr.push(await freshRun(module, rec));
  console.log(`fresh  total ${f(fr.map(r => r.totalMs))} | mount ${f(fr.map(r => r.mountMs))} inst ${f(fr.map(r => r.instMs))} run ${f(fr.map(r => r.runMs))} read ${f(fr.map(r => r.readMs))} | out ${fr[0].output.length} B, status ${fr[0].status}, mem ${(fr[0].memBytes/1e6).toFixed(0)} MB`);
  // warm: one-time creation counted separately
  const w = await warmInstance(module, rec);
  const opts = argvToOptions(rec.argv);
  const first = w.convert(opts);
  const wr = []; for (let i = 0; i < N; i++) wr.push(w.convert(opts));
  console.log(`warm   create ${w.initMs.toFixed(0)} once; first convert ${first.totalMs.toFixed(0)} | per-render total ${f(wr.map(r => r.totalMs))} | mount ${f(wr.map(r => r.mountMs))} run ${f(wr.map(r => r.runMs))} read ${f(wr.map(r => r.readMs))} | out ${wr[0].output.length} B, mem ${(wr.at(-1).memBytes/1e6).toFixed(0)} MB`);
  console.log(`warm == fresh bytes: ${Buffer.from(fr[0].output).equals(Buffer.from(wr[0].output))}`);
  // cold warm-instance: creation + first convert, the price after a worker kill
  const c = []; for (let i = 0; i < 3; i++) { const t = performance.now(); const x = await warmInstance(module, rec); x.convert(opts); c.push(performance.now() - t); }
  console.log(`re-create after kill (new instance + first convert): ${f(c)}`);
}
