import { readFileSync } from "node:fs";
const b = process.argv[2];
const L = readFileSync(`out/${b}/report.jsonl`, "utf8").trim().split("\n").map(JSON.parse);
const med = (a) => [...a].sort((x, y) => x - y)[a.length >> 1];
console.log(L[0].ua); console.log("init", JSON.stringify({ ...L[1], exports: undefined }));
const reps = L.filter((x) => x.type === "result" && x.rep !== undefined);
const row = (r) => `${r.name}#${r.rep} exit=${r.exit} inst=${r.t.instanceMs} run=${r.t.runMs} mem=${r.memoryMB} poll=${r.poll.calls}/${r.poll.nonzero} out=${r.outputBytes}`;
for (const fmt of ["docx", "pptx", "epub", "typst"]) {
  const a = reps.filter((r) => r.name.endsWith(fmt));
  const first = a.filter((r) => r.rep === 0), second = a.filter((r) => r.rep === 1);
  console.log(fmt, "exit!=0:", a.filter((r) => r.exit !== 0).length, "nonzero poll:", a.filter((r) => r.poll.nonzero).length, "poll calls:", a.reduce((s, r) => s + r.poll.calls, 0),
    "run ms first-pass med/max", med(first.map((r) => r.t.runMs)), Math.max(...first.map((r) => r.t.runMs)), "second-pass med", med(second.map((r) => r.t.runMs)), "mem max", Math.max(...a.map((r) => r.memoryMB)));
}
console.log("very first run:", row(reps[0]));
const p = L.find((x) => x.exit_ok);
for (const [k, v] of Object.entries(p)) if (!["type", "id"].includes(k)) console.log(k, JSON.stringify(v, (kk, vv) => (kk === "stderr" && typeof vv === "string" ? vv.slice(-90) : kk === "codes" ? undefined : vv)).slice(0, 700));
