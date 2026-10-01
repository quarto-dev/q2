import { readFileSync } from "node:fs";
import { createPandocInstance } from "./node_modules/pandoc-wasm/src/core.js";
const p = await createPandocInstance(readFileSync("node_modules/pandoc-wasm/src/pandoc.wasm"));
const json = JSON.stringify({ "pandoc-api-version": [1,23,1], meta: {}, blocks: [
  { t: "Para", c: [{ t: "Str", c: "hello" }, { t: "Space" }, { t: "Emph", c: [{ t: "Str", c: "world" }] }] },
  { t: "Div", c: [["", ["callout"], []], [{ t: "Para", c: [{ t: "Str", c: "inside" }] }]] } ] });
// Lua filter in-memory + a required module in a subdir (like vendored Q1 main.lua tree)
const files = {
  "main.lua": `package.path = "/?.lua;" .. package.path
local m = require("helper")
function Div(d) if d.classes:includes("callout") then return pandoc.Div(d.content, {class="x"}) end end
function Str(s) return pandoc.Str(m.up(s.text)) end
function Meta(meta) local p = PANDOC_STATE and 'state' or 'nostate'; io.stderr:write("param="..tostring(os.getenv("QUARTO_FILTER_PARAMS")).."\\n") end`,
  "helper.lua": `return { up = function(s) return s:upper() end }`,
};
for (const to of ["native", "docx"]) {
  const r = await p.convert({ from: "json", to, standalone: to !== "native", filters: ["main.lua"], "output-file": to==="docx" ? "o.docx" : undefined, "data-dir": undefined }, json, files);
  console.log(to, "stdout:", r.stdout.slice(0, 300).replace(/\n/g, " "), "| stderr:", JSON.stringify(r.stderr), "| warnings:", JSON.stringify(r.warnings), "| out:", r.files["o.docx"]?.size);
}
