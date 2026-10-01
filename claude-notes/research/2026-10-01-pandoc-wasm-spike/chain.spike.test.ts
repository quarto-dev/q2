// Spike 2: Rust wasm (wasm-quarto-hub-client) and pandoc.wasm loaded side by side; AST flows Rust -> pandoc.
import { describe, it, expect, beforeAll } from 'vitest';
import { readFile, writeFile, mkdir } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { setVfsCallbacks } from '/src/wasm-js-bridge/sass.js';
// @ts-expect-error plain JS host
import { createPandocInstance } from './host-patched.js';

const here = dirname(fileURLToPath(import.meta.url));
const rssMB = () => Math.round(process.memoryUsage().rss / 1e6);
let q2: any, pandoc: any;

const DOC = `---
title: Chain
---

# Hello

Some *emphasis*, a [link](https://x.org), and a list:

- one
- two

::: {.callout-note}
A note.
:::
`;

beforeAll(async () => {
  console.log('rss at start', rssMB());
  const wasmDir = join(here, '../../../../crates/wasm-quarto-hub-client/pkg');
  q2 = await import('wasm-quarto-hub-client');
  let t = performance.now();
  await q2.default(await readFile(join(wasmDir, 'wasm_quarto_hub_client_bg.wasm')));
  console.log(`q2 wasm init ${Math.round(performance.now() - t)} ms; rss ${rssMB()}`);
  const rd = (p: string) => { try { const r = JSON.parse(q2.vfs_read_file(p)); return r.success ? r.content : null; } catch { return null; } };
  setVfsCallbacks(rd, (p: string) => rd(p) != null);
  t = performance.now();
  pandoc = await createPandocInstance(await readFile(join(here, 'node_modules/pandoc-wasm/src/pandoc.wasm')));
  console.log(`pandoc wasm init ${Math.round(performance.now() - t)} ms; rss ${rssMB()}`);
  await mkdir(join(here, 'out'), { recursive: true });
});

async function toPandoc(astJson: string, to: string, ext: string) {
  const r = await pandoc.convert({ from: 'json', to, standalone: true, 'output-file': `o.${ext}` }, astJson, {});
  const blob = r.files[`o.${ext}`];
  return { r, bytes: blob ? new Uint8Array(await blob.arrayBuffer()) : null };
}

describe('Rust wasm + pandoc.wasm side by side', () => {
  for (const fn of ['parse_qmd_to_ast', 'render_preview_ast'] as const) {
    it(`${fn} AST -> pandoc docx/epub`, async () => {
      q2.vfs_clear(); q2.vfs_add_file('/project/doc.qmd', fn === 'render_preview_ast' ? DOC.replace('title: Chain', 'title: Chain\nformat: q2-preview') : DOC);
      let t = performance.now();
      const res = JSON.parse(fn === 'parse_qmd_to_ast' ? await q2.parse_qmd_to_ast(DOC) : await q2.render_page_in_project_with_attribution('/project/doc.qmd', undefined, undefined, undefined));
      console.log(fn, `${Math.round(performance.now() - t)} ms; keys`, Object.keys(res), 'success', res.success, res.error ?? '');
      const ast = res.ast ?? res.ast_json;
      expect(res.success).toBe(true);
      const astJson = typeof ast === 'string' ? ast : JSON.stringify(ast);
      await writeFile(join(here, `out/${fn}.json`), astJson);
      console.log('  ast head:', astJson.slice(0, 160));
      for (const [to, ext] of [['native', 'txt'], ['docx', 'docx'], ['epub3', 'epub']]) {
        t = performance.now();
        const { r, bytes } = await toPandoc(astJson, to, ext);
        console.log(`  ${to}: ${bytes?.length ?? 0} bytes, stdout ${r.stdout.length} chars, ${Math.round(performance.now() - t)} ms, stderr=${JSON.stringify(r.stderr).slice(0, 300)} rss ${rssMB()}`);
        if (to === 'native') console.log('  native:', r.stdout.slice(0, 400).replace(/\s+/g, ' '));
      }
    });
  }
});

describe('parity and memory', () => {
  it('wasm docx document.xml equals native pandoc docx for the same Rust-produced AST', async () => {
    const astJson = await readFile(join(here, 'out/render_preview_ast.json'), 'utf8');
    const { bytes } = await toPandoc(astJson, 'docx', 'docx');
    await writeFile(join(here, 'out/chain.docx'), bytes!);
    const { execFileSync } = await import('child_process');
    execFileSync('pandoc', ['-f', 'json', '-t', 'docx', '-s', '-o', join(here, 'out/chain-native.docx'), join(here, 'out/render_preview_ast.json')]);
    const x = (f: string) => execFileSync('unzip', ['-p', join(here, f), 'word/document.xml']).toString();
    const same = x('out/chain.docx') === x('out/chain-native.docx');
    console.log('document.xml identical to native:', same);
    expect(same).toBe(true);
  });

  it('memory growth: repeated conversions on one instance, then a recycled instance', async () => {
    const astJson = await readFile(join(here, 'out/render_preview_ast.json'), 'utf8');
    const wasmBytes = await readFile(join(here, 'node_modules/pandoc-wasm/src/pandoc.wasm'));
    const p = await createPandocInstance(wasmBytes);
    const mb = (n: number) => Math.round(n / 1e6);
    console.log(`fresh instance: wasm memory ${mb(p.memoryBytes())} MB, rss ${rssMB()}`);
    for (let i = 1; i <= 30; i++) {
      await p.convert({ from: 'json', to: 'docx', standalone: true, 'output-file': 'o.docx' }, astJson, {});
      if (i % 10 === 0 || i === 1) console.log(`after ${i} docx: wasm memory ${mb(p.memoryBytes())} MB, rss ${rssMB()}`);
    }
    const p2 = await createPandocInstance(wasmBytes);
    await p2.convert({ from: 'json', to: 'docx', standalone: true, 'output-file': 'o.docx' }, astJson, {});
    console.log(`second instance after 1 docx: wasm memory ${mb(p2.memoryBytes())} MB (first still alive: ${mb(p.memoryBytes())} MB), rss ${rssMB()}`);
  });
});
