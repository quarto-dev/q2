// Regenerates the oracle fixtures in this directory by running the TypeScript
// `widgetDependencies` and `widgetDependencyIncludes` (ts-packages/quarto-api) over small
// notebooks. The Rust tests read the committed *.json files, so CI needs no Node.
//
//   cd ts-packages/quarto-api && npx tsx ../../crates/quarto-core/tests/fixtures/page_dependencies/generate.mjs
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { widgetDependencies, widgetDependencyIncludes } from "../../../../../ts-packages/quarto-api/src/jupyter/widgets.ts";

const here = dirname(fileURLToPath(import.meta.url));
const display = (data, type = "display_data") => ({ output_type: type, data, metadata: {} });
const html = (t) => display({ "text/html": t, "text/plain": "x" });
const plotlyReq = (indent = "") =>
  [`${indent}<script type="text/javascript">`, `${indent}  require.undef("plotly");`, `${indent}  define('plotly', function(require, exports, module) {});`, `${indent}</script>`].join("\n");

const cases = {
  "no-html": [[{ output_type: "stream", name: "stdout", text: "hi\n" }]],
  "plots-data-uri": [[html('<img src="data:image/png;base64,AAAA">')]],
  "hand-written-html": [[html("<b>hi</b>")]],
  "javascript-only": [[display({ "application/javascript": "1+1" })]],
  "widget-view-no-state": [[display({ "application/vnd.jupyter.widget-view+json": { model_id: "abc" }, "text/plain": "w" })]],
  "stream-is-not-display": [[{ output_type: "stream", name: "stdout", text: "x" }, { output_type: "error", ename: "E", evalue: "v", traceback: [] }]],
  "plotly-require-indented": [[html(plotlyReq("    "))]],
  "plotly-module": [[html('<script type="module">import "https://cdn.plot.ly/plotly-3.0.0.min.js"</script>')]],
  "plotly-embed-header": [[html("<script>/**\n * plotly.js v3.0.1\n * Copyright 2012-2025, Plotly, Inc.\n * All rights reserved.\n * Licensed under the MIT license\n */</script>")]],
  "plotly-then-other-output": [[html(plotlyReq()), html("<b>other</b>")]],
  "plotly-on-execute-result": [[display({ "text/html": plotlyReq() }, "execute_result")]],
  "plotly-duplicate": [[html(plotlyReq())], [html(plotlyReq())]],
  "plotly-array-valued": [[display({ "text/html": plotlyReq().split("\n").map((l, i, a) => (i < a.length - 1 ? l + "\n" : l)) })]],
  "plotly-whitespace-quirks": [[html(["  <script type=\"text/javascript\">", "      require.undef('plotly');", "   ", "  </script>  ", "  <!-- a  b --> ", "\t  x"].join("\r\n").replace("\t  x", "  x"))]],
  "plotly-first-match-anywhere": [[html(["  <script type=\"text/javascript\">", "  require.undef(\"plotly\");  a  b", "  </script>"].join("\n"))]],
  "plotly-cr-line-breaks": [[html(["  <script type=\"text/javascript\">", "  require.undef('plotly');", "  </script>"].join("\r"))]],
  "module-script-cr-in-dot": [[html('<script type="module">import \r plotly</script>')]],
  "module-script-lf-in-dot": [[html('<script type="module">import \n plotly</script>')]],
  "widget-view-and-html": [[display({ "application/vnd.jupyter.widget-view+json": { model_id: "a" } }), html("<i>x</i>")]],
};

for (const [name, cellOutputs] of Object.entries(cases)) {
  const nb = {
    cells: cellOutputs.map((outputs) => ({ cell_type: "code", outputs: structuredClone(outputs) })),
    metadata: {},
  };
  const input = structuredClone(nb.cells.map((c) => c.outputs));
  const deps = widgetDependencies(nb);
  let head = null;
  if (deps) {
    let written = null;
    const host = { fs: { makeTempFile: () => "tmp", writeFileSync: (_p, text) => { written = text; } } };
    widgetDependencyIncludes(host, deps, "tmp");
    head = written;
  }
  // Which (cell, output) pairs were hoisted: `widgetDependencies` filters the outputs in place and
  // the filter keeps order, so walk the input and the survivors together.
  const hoisted = [];
  input.forEach((outs, ci) => {
    const kept = nb.cells[ci].outputs;
    let k = 0;
    outs.forEach((o, oi) => {
      if (k < kept.length && JSON.stringify(kept[k]) === JSON.stringify(o)) k++;
      else hoisted.push([ci, oi]);
    });
  });
  const out = {
    name,
    cells: input,
    expected: {
      javascriptWidgets: deps?.jsWidgets ?? false,
      jupyterWidgets: deps?.jupyterWidgets ?? false,
      htmlLibraries: deps?.htmlLibraries ?? [],
      hoisted,
      head,
    },
  };
  writeFileSync(join(here, `${name}.json`), JSON.stringify(out, null, 2) + "\n");
}
console.log("wrote", Object.keys(cases).length, "fixtures");
