// D2(b) Task 0 typst worker: one TypstSession per compile in a fresh worker, as the hub-client does
// (`TypstRunner`). Bundled by build-typst.mjs because it pulls in typst.ts.
import { TypstSession } from "../../../../ts-packages/typst-host/src/session.ts";
const out = (msg, transfer = []) => postMessage(msg, transfer);
onmessage = async ({ data: m }) => {
  try {
    if (m.op === "compile") {
      const t0 = performance.now();
      const s = await TypstSession.create({ module: m.module, fonts: m.fonts, vendoredPackages: m.packages });
      const t1 = performance.now();
      const r = await s.compile(m.input);
      const t2 = performance.now();
      out({ id: m.id, ok: r.ok, initMs: t1 - t0, compileMs: t2 - t1, pdfLen: r.ok ? r.pdf.length : null, diagnostics: r.ok ? [] : r.diagnostics.map((d) => d.message) });
    }
  } catch (e) { out({ id: m.id, error: String(e && e.stack || e) }); }
};
