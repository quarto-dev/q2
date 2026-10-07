// Compares browser outputs (out/<browser>/<rec>/<file>) with R0's native references via quarto-output-extract.
// Native references embed /tmp/q2-pandoc-replay/<rec>/__q2_*; the wasm run used /__q2_* directly, so the extraction
// text is path-normalized before comparing (the only normalization beyond the extractor's own).
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, writeFileSync, existsSync, mkdirSync } from "node:fs";
import path from "node:path";
const here = path.dirname(new URL(import.meta.url).pathname);
const repo = path.resolve(here, "../../../..");
const recs = path.join(repo, "crates/quarto-core/tests/fixtures/pandoc-recordings/recordings");
const bin = path.join(repo, "target/release/quarto-output-extract");
const browser = process.argv[2] || "chromium";
const scratch = path.join(process.env.TMPDIR || "/tmp", "h0-compare"); mkdirSync(scratch, { recursive: true });
const extract = (f, rec) => {
  const t = execFileSync(bin, ["extract", f], { maxBuffer: 1 << 28 }).toString();
  return t.replaceAll(`/tmp/q2-pandoc-replay/${rec}/__q2_`, "/__q2_");
};
let pass = 0, fail = 0, bytesEq = 0;
for (const rec of readdirSync(path.join(here, "out", browser)).filter((n) => existsSync(path.join(recs, n))).sort()) {
  const dir = path.join(here, "out", browser, rec);
  const [file] = readdirSync(dir);
  const ref = path.join(recs, rec, "reference", file);
  let verdict;
  try {
    const a = extract(path.join(dir, file), rec), b = extract(ref, rec);
    verdict = a === b ? "EQUAL" : "DIFF";
    if (a !== b) {
      const A = a.split("\n"), B = b.split("\n"); const i = A.findIndex((l, k) => l !== B[k]);
      writeFileSync(path.join(scratch, `${browser}-${rec}.diff`), `wasm : ${A[i]}\nnative: ${B[i]}\n(line ${i + 1})\n`);
      verdict += `  line ${i + 1}\n   wasm : ${A[i]?.slice(0, 200)}\n   native: ${B[i]?.slice(0, 200)}`;
    }
  } catch (e) { verdict = "ERROR " + String(e.stderr || e).slice(0, 300); }
  const same = readFileSync(path.join(dir, file)).equals(readFileSync(ref));
  if (same) bytesEq++;
  verdict.startsWith("EQUAL") ? pass++ : fail++;
  console.log(`${rec.padEnd(18)} ${verdict}${same ? "  (byte-identical)" : ""}`);
}
console.log(`${browser}: ${pass} equal, ${fail} not equal, ${bytesEq} byte-identical`);
