#!/usr/bin/env node
// Measure how much linear memory the hub WASM module needs to parse
// documents (study bd-akgst1fi, tracking strand bd-r17a0k7v).
//
//   npm run build:wasm            # in hub-client/, builds the module
//   node crates/perf-harness/scripts/wasm-mem/wasm-mem.mjs \
//        crates/wasm-quarto-hub-client/pkg claude-notes/plans [count]
//
// It takes the <count> (default 40) largest `.md` files of the directory
// and prints one JSON line with the module's linear-memory growth at four
// points: after one `parse_qmd_to_ast` of the largest document, after 30
// more parses of it as text is appended (an editing session), after one
// version of it that fails to parse, and after parsing all <count>
// documents. WASM linear memory never shrinks,
// so each figure is a high-water mark. It covers everything the entry
// point does (parse, pipeline, and the response it returns), not only AST
// storage.
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

const ok = (response) => (typeof response === 'string' ? JSON.parse(response) : response).success;
// Older builds return the response as one JSON string; newer ones return
// the object, with the AST JSON as a string field.
const astLength = (response) =>
  typeof response === 'string' ? response.length : (response.ast?.length ?? 0);
const MiB = (bytes) => Number((bytes / 1048576).toFixed(1));

const base = memory();
const largestText = await readFile(join(docs, files[0][0]), 'utf8');
const largest = files[0][1];

// 1. One parse of the largest document.
const first = await mod.parse_qmd_to_ast(largestText);
const afterFirst = memory();

// 2. An editing session: the same document re-parsed 30 times as text is
//    typed at its end. Every version parses. Fragmentation would show here.
const EDITS = 30;
let editsParsed = 0;
for (let k = 1; k <= EDITS; k++) {
  const edited = largestText + '\n\n' + 'Another sentence of typing. '.repeat(k) + '\n';
  if (ok(await mod.parse_qmd_to_ast(edited))) editsParsed += 1;
}
const afterEdits = memory();

// 3. One version that does not parse (an unclosed code span, as happens
//    mid-typing). The error path re-parses to build its message.
const broken = await mod.parse_qmd_to_ast(largestText + '\n\n(`\n');
const afterError = memory();

// 4. The rest of the corpus, one document after another.
let sourceBytes = 0;
let parsed = 0;
const start = performance.now();
for (const [name, size] of files) {
  if (ok(await mod.parse_qmd_to_ast(await readFile(join(docs, name), 'utf8')))) parsed += 1;
  sourceBytes += size;
}
const elapsed = performance.now() - start;

console.log(JSON.stringify({
  largest_doc_KiB: Math.round(largest / 1024),
  largest_ast_json_MiB: MiB(astLength(first)),
  memory_base_MiB: MiB(base),
  growth_first_parse_MiB: MiB(afterFirst - base),
  growth_after_30_edits_MiB: MiB(afterEdits - base),
  edits_parsed: editsParsed,
  growth_after_parse_error_MiB: MiB(afterError - base),
  parse_error_triggered: !ok(broken),
  growth_after_corpus_MiB: MiB(memory() - base),
  first_parse_over_source: Number(((afterFirst - base) / Math.max(largest, 1)).toFixed(1)),
  files: files.length,
  parsed,
  source_MiB: MiB(sourceBytes),
  corpus_parse_ms: Math.round(elapsed),
}));
