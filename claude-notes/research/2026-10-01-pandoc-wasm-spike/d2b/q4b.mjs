// Q4b: heap exhaustion (+RTS -M, as the production fault test uses) on a warm instance: recoverable?
import { loadModule, loadRecording, freshRun, warmInstance, argvToOptions } from "./common.mjs";
const module = await loadModule();
const eq = (a, b) => !!a && !!b && Buffer.from(a).equals(Buffer.from(b));
const A = loadRecording("callouts-typst"), BIG = loadRecording("callouts-typst", { repeat: 400 });
const refA = (await freshRun(module, A)).output;
console.log("big input json bytes:", BIG.files.find((f) => f.mount.endsWith("pandoc-input.json")).bytes.length);
// fresh command mode with -M: what does the production path see?
const fr = await freshRun(module, BIG, { argv: [BIG.argv[0], "+RTS", "-M100m", "-RTS", ...BIG.argv.slice(1)] });
console.log("fresh _start, -M100m, big doc: status", fr.status, "trap", fr.trap ? String(fr.trap).slice(0, 100) : null, "stderr", JSON.stringify(fr.stderr.slice(0, 150)));
const w = await warmInstance(module, A, { rts: ["+RTS", "-M100m", "-RTS"] });
const o = argvToOptions(A.argv);
console.log("warm -M100m: good A equals ref:", eq(w.convert(o).output, refA));
const r = w.convert(argvToOptions(BIG.argv), { files: BIG.files });
console.log("warm -M100m, big doc: trap", r.trap ? JSON.stringify(String(r.trap).slice(0, 120)) : null, "output", r.output?.length ?? null, "stderr", JSON.stringify(r.stderr.slice(0, 150)), "wasm MB", (w.mem() / 1048576).toFixed(0));
for (let i = 1; i <= 3; i++) {
  const g = w.convert(o);
  console.log(`   after OOM, good A #${i}: equals ref ${eq(g.output, refA)} trap=${g.trap ? JSON.stringify(String(g.trap).slice(0, 100)) : null} stderr=${JSON.stringify(g.stderr.slice(0, 100))}`);
}
