/**
 * WASM End-to-End Tests for crossref outline entries.
 *
 * These tests exercise `lsp_analyze_document` through the WASM bindings
 * and verify cross-referenceable elements (figures, theorems, equations)
 * appear in the outline with the right shape:
 *
 * - `name` is the identifier (e.g. `"fig-one"`).
 * - `detail` carries the rendered label (e.g. `"Figure 1: ..."`).
 * - Inner headers absorbed into a theorem's `title` slot (e.g. `## Line`
 *   inside `::: {#thm-line}`) do NOT appear as standalone outline entries.
 *
 * Run with: npm run test:wasm
 */

import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

interface WasmSymbol {
  name: string;
  kind: string;
  detail?: string;
  children: WasmSymbol[];
}

interface LspAnalyzeResponse {
  success: boolean;
  symbols?: WasmSymbol[];
  foldingRanges?: unknown[];
  diagnostics?: unknown[];
  error?: string;
}

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  vfs_add_file: (path: string, content: string) => string;
  vfs_clear: () => string;
  lsp_analyze_document: (path: string) => string;
}

const __dirname = dirname(fileURLToPath(import.meta.url));

function flatten(symbols: WasmSymbol[]): WasmSymbol[] {
  const out: WasmSymbol[] = [];
  const go = (ss: WasmSymbol[]) => {
    for (const s of ss) {
      out.push(s);
      if (s.children?.length) {
        go(s.children);
      }
    }
  };
  go(symbols);
  return out;
}

describe('LSP outline with crossref entries', () => {
  let wasm: WasmModule;

  beforeAll(async () => {
    const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
    const wasmPath = join(wasmDir, 'wasm_quarto_hub_client_bg.wasm');
    const wasmBytes = await readFile(wasmPath);
    wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
    await wasm.default(wasmBytes);
  });

  beforeEach(() => {
    wasm.vfs_clear();
  });

  it('figure div appears as a Class symbol with Figure label', () => {
    const qmd = `---
title: demo
---

::: {#fig-one}

![](x.png)

An overview caption.

:::
`;
    wasm.vfs_add_file('/input.qmd', qmd);

    const result = JSON.parse(wasm.lsp_analyze_document('/input.qmd')) as LspAnalyzeResponse;
    expect(result.success).toBe(true);

    const flat = flatten(result.symbols ?? []);
    const fig = flat.find((s) => s.name === 'fig-one');
    expect(fig, `expected fig-one among ${flat.map((s) => s.name).join(', ')}`).toBeDefined();
    expect(fig!.kind).toBe('class');
    expect(fig!.detail).toMatch(/^Figure 1/);
    expect(fig!.detail).toContain('An overview caption');
  });

  it('theorem div hides its inner header and surfaces the title in detail', () => {
    const qmd = `::: {#thm-line}

## Line

A straight line is $y = mx + b$.

:::
`;
    wasm.vfs_add_file('/input.qmd', qmd);

    const result = JSON.parse(wasm.lsp_analyze_document('/input.qmd')) as LspAnalyzeResponse;
    expect(result.success).toBe(true);
    const flat = flatten(result.symbols ?? []);

    const thm = flat.find((s) => s.name === 'thm-line');
    expect(thm).toBeDefined();
    expect(thm!.detail).toContain('Theorem');
    expect(thm!.detail).toContain('Line');

    // The `## Line` header was absorbed into the theorem's title slot —
    // it must NOT appear as a sibling outline entry.
    expect(flat.find((s) => s.name === 'Line')).toBeUndefined();
  });

  it('labelled equation appears in the outline', () => {
    const qmd = `The quadratic formula:

$$
x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}
$$ {#eq-quadratic}
`;
    wasm.vfs_add_file('/input.qmd', qmd);

    const result = JSON.parse(wasm.lsp_analyze_document('/input.qmd')) as LspAnalyzeResponse;
    expect(result.success).toBe(true);
    const flat = flatten(result.symbols ?? []);
    expect(flat.find((s) => s.name === 'eq-quadratic')).toBeDefined();
  });

  it('real headers and crossref targets coexist as siblings', () => {
    const qmd = `# Section one

Some text.

::: {#fig-a}

![](a.png)

Caption A.

:::

# Section two
`;
    wasm.vfs_add_file('/input.qmd', qmd);

    const result = JSON.parse(wasm.lsp_analyze_document('/input.qmd')) as LspAnalyzeResponse;
    expect(result.success).toBe(true);
    const flat = flatten(result.symbols ?? []);
    const names = flat.map((s) => s.name);

    expect(names).toContain('Section one');
    expect(names).toContain('Section two');
    expect(names).toContain('fig-a');
  });
});
