#!/usr/bin/env node
// Measure how much linear memory the hub WASM module needs to parse
// documents (study bd-akgst1fi, tracking strand bd-r17a0k7v).
//
//   npm run build:wasm            # in hub-client/, builds the module
//   node crates/perf-harness/scripts/wasm-mem/wasm-mem.mjs \
//        crates/wasm-quarto-hub-client/pkg claude-notes/plans [count]
//
// It parses the <count> (default 40) largest `.md` files of the directory
// with `parse_qmd_to_ast`, one after another, and prints one JSON line.
// WASM linear memory never shrinks, so `growth_MiB` is the high-water mark
// over the run: in practice, what the largest document needed. That
// includes everything the entry point does (parse, pipeline, and the JSON
// response it returns), not only AST storage.
//
// To compare two commits, build the module in each (a second worktree
// works) and run this script against both `pkg` directories.
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { readFile, readdir, stat } from 'node:fs/promises';
import { register } from 'node:module';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const [pkgArg, docsArg, countArg] = process.argv.slice(2);
if (!pkgArg || !docsArg) {
  console.error('usage: wasm-mem.mjs <pkg-dir> <docs-dir> [count]');
  process.exit(2);
}
const pkg = resolve(pkgArg);
const docs = resolve(docsArg);
const count = Number(countArg ?? 40);
const wasmBytes = readFileSync(join(pkg, 'wasm_quarto_hub_client_bg.wasm'));
const glue = readFileSync(join(pkg, 'wasm_quarto_hub_client.js'), 'utf8');

// The JS bridge (cache, fetch, sass, rasterize) is not needed to parse.
// Stub every function the module or its glue imports from it.
const stubs = {};
const add = (file, name) => (stubs[file] ??= new Set()).add(name);
for (const imp of WebAssembly.Module.imports(new WebAssembly.Module(wasmBytes))) {
  if (imp.module.startsWith('/src/wasm-js-bridge/')) add(imp.module.split('/').pop(), imp.name);
}
for (const m of glue.matchAll(/import \{([^}]*)\} from '\/src\/wasm-js-bridge\/([\w.]+)'/g)) {
  for (const name of m[1].split(',')) if (name.trim()) add(m[2], name.trim());
}
const stubDir = mkdtempSync(join(tmpdir(), 'wasm-mem-stubs-'));
for (const [file, names] of Object.entries(stubs)) {
  const body = [...names].map((name) =>
    /Available$/.test(name)
      ? `export const ${name} = () => false;`
      : `export const ${name} = () => { throw new Error('wasm-mem: bridge stub ${name} called'); };`);
  writeFileSync(join(stubDir, file), body.join('\n') + '\n');
}
register('./hooks.mjs', import.meta.url, { data: { stubDir } });

const mod = await import(pathToFileURL(join(pkg, 'wasm_quarto_hub_client.js')).href);
const exports = await mod.default({ module_or_path: wasmBytes });
const memory = () => exports.memory.buffer.byteLength;

const names = (await readdir(docs)).filter((n) => n.endsWith('.md') || n.endsWith('.qmd'));
const sized = await Promise.all(names.map(async (n) => [n, (await stat(join(docs, n))).size]));
sized.sort((a, b) => b[1] - a[1]);
const files = sized.slice(0, count);

const base = memory();
let sourceBytes = 0;
let parsed = 0;
let largestResponse = 0;
const start = performance.now();
for (const [name, size] of files) {
  const response = await mod.parse_qmd_to_ast(await readFile(join(docs, name), 'utf8'));
  if (JSON.parse(response).success) parsed += 1;
  sourceBytes += size;
  largestResponse = Math.max(largestResponse, response.length);
}
const elapsed = performance.now() - start;
const largest = files[0]?.[1] ?? 0;
const MiB = (bytes) => Number((bytes / 1048576).toFixed(1));
console.log(JSON.stringify({
  files: files.length,
  parsed,
  source_MiB: MiB(sourceBytes),
  largest_doc_KiB: Math.round(largest / 1024),
  largest_response_MiB: MiB(largestResponse),
  memory_base_MiB: MiB(base),
  memory_final_MiB: MiB(memory()),
  growth_MiB: MiB(memory() - base),
  growth_over_largest_doc: Number(((memory() - base) / Math.max(largest, 1)).toFixed(1)),
  parse_ms: Math.round(elapsed),
}));
