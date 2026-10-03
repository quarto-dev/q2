// D2(b) worker: one fresh run, or a persistent warm instance. Shape follows pandoc.worker.ts: Module posted in, request in, result out.
import { freshRun, warmInstance, spareInstance, argvToOptions, withPreamble, now } from "/core.mjs";
const enc = new TextEncoder();
let warm = null, warmRec = null, spare = null;
const out = (msg, transfer = []) => postMessage(msg, transfer);
const toRec = (r) => ({ ...r, files: r.files.map((f) => ({ mount: f.mount, bytes: f.bytes })) });
onmessage = async ({ data: m }) => {
  try {
    if (m.op === "fresh") {
      const r = await freshRun(m.module, m.rec, { env: m.env ?? {}, argv: m.argv ?? m.rec.argv });
      out({ id: m.id, stats: { mount: r.mountMs, inst: r.instMs, run: r.runMs, read: r.readMs, total: r.totalMs, mem: r.memBytes, status: r.status }, stderr: r.stderr, trap: r.trap ? String(r.trap) : null, output: r.output }, r.output ? [r.output.buffer] : []);
    } else if (m.op === "warm-init") {
      const t = now();
      warm = await warmInstance(m.module, m.rec, { env: {}, rts: m.rts ?? [], capture: !!m.capture });
      warmRec = m.rec;
      out({ id: m.id, initMs: now() - t });
    } else if (m.op === "warm-convert") {
      const rec = m.rec;
      const t0 = now();
      const files = m.filter ? [...withPreamble(rec, m.env ?? rec.env), { mount: "/f.lua", bytes: enc.encode(m.filter) }, ...(m.extra ?? [])] : withPreamble(rec, m.env ?? rec.env);
      const prepMs = now() - t0;
      const r = warm.convert(m.options ?? argvToOptions(rec.argv), { files });
      out({ id: m.id, stats: { prep: prepMs, mount: r.mountMs, run: r.runMs, read: r.readMs, total: r.totalMs + prepMs, mem: r.memBytes }, stderr: r.stderr, fd1: r.fd1, fd2: r.fd2, warnings: r.warnings, trap: r.trap ? String(r.trap) : null, output: r.output }, r.output ? [r.output.buffer] : []);
    } else if (m.op === "spare-init") {
      spare = await spareInstance(m.module);
      out({ id: m.id, initMs: spare.initMs });
    } else if (m.op === "spare-run") {
      const r = spare.run(m.rec);
      out({ id: m.id, stats: { mount: r.mountMs, run: r.runMs, read: r.readMs, total: r.totalMs, mem: r.memBytes, status: r.status }, stderr: r.stderr, trap: r.trap ? String(r.trap) : null, output: r.output }, r.output ? [r.output.buffer] : []);
    } else if (m.op === "spin") {
      for (;;) { /* a busy core for the loaded-cores run; ends with terminate() */ }
    } else if (m.op === "hang") {
      // a runaway Lua filter inside convert(): only terminate() gets out
      const rec = m.rec;
      warm.convert({ from: "json", to: "plain", "data-dir": "/__q2_share__/pandoc/datadir", filters: [{ type: "lua", path: "/f.lua" }], "input-files": ["/in.json"], "output-file": "/out.txt" },
        { files: [...withPreamble(rec, rec.env), { mount: "/f.lua", bytes: enc.encode("while true do end") }, { mount: "/in.json", bytes: enc.encode('{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Str","c":"x"}]}]}') }] });
      out({ id: m.id, hung: false });
    } else if (m.op === "fresh-hang") {
      const rec = m.rec;
      const files = [...rec.files, { mount: "/f.lua", bytes: enc.encode("while true do end") }, { mount: "/in.json", bytes: enc.encode('{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Str","c":"x"}]}]}') }];
      await freshRun(m.module, { ...rec, files }, { argv: [rec.argv[0], "-f", "json", "-t", "plain", "--data-dir", "/__q2_share__/pandoc/datadir", "-L", "/f.lua", "-o", "/out.txt", "/in.json"] });
      out({ id: m.id, hung: false });
    }
  } catch (e) { out({ id: m.id, error: String(e && e.stack || e) }); }
};
