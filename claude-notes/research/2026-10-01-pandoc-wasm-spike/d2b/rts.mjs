import { loadModule, loadRecording, freshRun, warmInstance, argvToOptions } from "./common.mjs";
const module = await loadModule();
const rec = loadRecording("callouts-typst");
const opts = argvToOptions(rec.argv);
await freshRun(module, rec);
for (const rts of [["+RTS", "-H64m", "-RTS"], [], ["+RTS", "-A64m", "-RTS"], ["+RTS", "-H256m", "-RTS"]]) {
  const t = [];
  for (let i = 0; i < 3; i++) { const w = await warmInstance(module, rec, { rts }); const a = w.convert(opts), b = w.convert(opts), c = w.convert(opts); t.push(`${w.initMs.toFixed(0)}+${a.totalMs.toFixed(0)}/${b.totalMs.toFixed(0)}/${c.totalMs.toFixed(0)}`); }
  console.log(JSON.stringify(rts), "init+conv1/conv2/conv3:", t.join("  "));
}
// fresh _start with the same RTS options
for (const rts of [["+RTS", "-H64m", "-RTS"], ["+RTS", "-A64m", "-RTS"]]) {
  const argv = [rec.argv[0], ...rts, ...rec.argv.slice(1)];
  const r = []; for (let i = 0; i < 4; i++) r.push((await freshRun(module, rec, { argv })).totalMs.toFixed(0));
  console.log("fresh _start", JSON.stringify(rts), r.join(" "));
}
