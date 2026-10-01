// Can a long-lived pandoc instance see a *changed* env var on a later convert()? (WASI env is normally read once at init.)
import { readFileSync } from "node:fs";
import { createPandocInstance } from "./host-patched.js";
const env = ["P=first"];
const p = await createPandocInstance(readFileSync("pandoc-3.11.wasm"), env);
const json = JSON.stringify({ "pandoc-api-version": [1,23,1], meta: {}, blocks: [{ t: "Para", c: [{ t: "Str", c: "x" }] }] });
const files = { "f.lua": `function Str(s) return pandoc.Str(tostring(os.getenv("P"))) end` };
const run = async () => (await p.convert({ from: "json", to: "native", filters: ["f.lua"] }, json, files)).stdout.trim();
console.log("1:", await run());
env[0] = "P=second"; console.log("2 (mutated array):", await run());
env.splice(0, 1, "P=third"); console.log("3 (spliced):", await run());
// Alternative: env var delivered through a file the filter reads
const files2 = { "g.lua": `local f = io.open("/params.txt"); local v = f:read("a"); f:close(); function Str(s) return pandoc.Str(v) end`, "params.txt": "via-file-1" };
console.log("file 1:", (await p.convert({ from: "json", to: "native", filters: ["g.lua"] }, json, files2)).stdout.trim());
files2["params.txt"] = "via-file-2";
console.log("file 2:", (await p.convert({ from: "json", to: "native", filters: ["g.lua"] }, json, files2)).stdout.trim());
