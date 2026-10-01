import { readFileSync } from "node:fs";
import { createPandocInstance } from "./host-patched.js";
const p = await createPandocInstance(readFileSync("pandoc-3.11.wasm"), ['QUARTO_FILTER_PARAMS={"k":1}']);
const json = JSON.stringify({ "pandoc-api-version": [1,23,1], meta: {}, blocks: [{ t: "Para", c: [{ t: "Str", c: "hello" }] }] });
const files = {
  "filters/main.lua": `package.path = "/filters/?.lua;/filters/sub/?.lua;" .. package.path
local m = require("helper"); local d = require("deep")
function Str(s) return pandoc.Str(m.up(s.text) .. d.v .. os.getenv("QUARTO_FILTER_PARAMS")) end`,
  "filters/helper.lua": `return { up = function(s) return s:upper() end }`,
  "filters/sub/deep.lua": `return { v = "-deep" }`,
};
const r = await p.convert({ from: "json", to: "native", filters: ["filters/main.lua"] }, json, files);
console.log(r.stdout, JSON.stringify(r.stderr));
