/**
 * WASM End-to-End Tests for `incremental_write_qmd` (Plan 7).
 *
 * Verifies the new 3-arg signature
 * (`original_qmd, baseline_ast_json, new_ast_json`) at the JS/WASM
 * boundary. The Rust-side correctness of soft-drop substitutions
 * (Q-3-42 / Q-3-43) is covered by `crates/pampa/src/writers/incremental.rs`
 * unit tests; these tests pin the wrapper contract:
 *
 *  - Identity round-trip is byte-equal (baseline === new ⇒ original qmd).
 *  - The returned shape is `{ qmd, warnings? }`; `warnings` is absent
 *    when nothing was soft-dropped.
 *  - A simple paragraph-text edit reaches the result qmd; the
 *    surrounding structure (headings, other paragraphs) is preserved
 *    verbatim from the original.
 *
 * The exhaustive scenario matrix (sectionized docs, multi-inline
 * shortcode dedupe, Q-3-42 byte-equal-no-op, Q-3-43 footnotes
 * regeneration) lives in the Rust-side coarsen tests + Plan 8
 * Playwright e2e (deferred to follow-up beads).
 *
 * Run with: npm run test:wasm
 */

import { describe, it, expect, beforeAll } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  parse_qmd_content: (content: string) => string;
  incremental_write_qmd: (
    original_qmd: string,
    baseline_ast_json: string,
    new_ast_json: string,
  ) => string;
}

interface AstResponse {
  success: boolean;
  ast?: string;
  qmd?: string;
  error?: string;
  warnings?: unknown[];
}

let wasm: WasmModule;

beforeAll(async () => {
  const __dirname = dirname(fileURLToPath(import.meta.url));
  const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
  const wasmPath = join(wasmDir, 'wasm_quarto_hub_client_bg.wasm');
  const wasmBytes = await readFile(wasmPath);

  wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
  await wasm.default(wasmBytes);
});

/** Parse `qmd` and return the resulting AST as a plain object. */
function parseAst(qmd: string): unknown {
  const resp: AstResponse = JSON.parse(wasm.parse_qmd_content(qmd));
  expect(resp.success, `parse_qmd_content failed: ${resp.error}`).toBe(true);
  expect(resp.ast).toBeTruthy();
  return JSON.parse(resp.ast!);
}

/** Run the incremental writer and return its parsed AstResponse. */
function write(
  originalQmd: string,
  baselineAst: unknown,
  newAst: unknown,
): AstResponse {
  return JSON.parse(
    wasm.incremental_write_qmd(
      originalQmd,
      JSON.stringify(baselineAst),
      JSON.stringify(newAst),
    ),
  );
}

/**
 * Walk a Pandoc AST and mutate the first `Str` whose `c` matches
 * `find`, replacing its content with `replace`. Returns true if a
 * match was found. Used to synthesize a "user edited a word"
 * scenario without going through the qmd reader.
 */
function mutateFirstStr(ast: unknown, find: string, replace: string): boolean {
  let done = false;
  const walk = (node: unknown): void => {
    if (done) return;
    if (Array.isArray(node)) {
      for (const child of node) walk(child);
      return;
    }
    if (node && typeof node === 'object') {
      const obj = node as Record<string, unknown>;
      if (obj.t === 'Str' && obj.c === find) {
        obj.c = replace;
        done = true;
        return;
      }
      for (const v of Object.values(obj)) walk(v);
    }
  };
  walk(ast);
  return done;
}

describe('incremental_write_qmd wrapper contract', () => {
  it('identity round-trip is byte-equal and emits no warnings', () => {
    const original = '# Heading\n\nA paragraph.\n';
    const baseline = parseAst(original);
    const resp = write(original, baseline, baseline);

    expect(resp.success, `write failed: ${resp.error}`).toBe(true);
    expect(resp.qmd).toBe(original);
    // No warnings field when nothing was soft-dropped.
    expect(resp.warnings).toBeUndefined();
  });

  it('paragraph-text edit reaches the output; surrounding structure preserved', () => {
    const original =
      '# Heading\n\nFirst paragraph here.\n\n## Sub\n\nSecond paragraph here.\n';
    const baseline = parseAst(original);
    // Deep-clone via JSON round-trip so the mutation doesn't alias
    // the baseline. The wrapper stringifies both, but defensive
    // cloning makes the test's intent obvious.
    const next = JSON.parse(JSON.stringify(baseline));
    const mutated = mutateFirstStr(next, 'First', 'Updated');
    expect(mutated, 'expected to find a Str("First") to mutate').toBe(true);

    const resp = write(original, baseline, next);
    expect(resp.success, `write failed: ${resp.error}`).toBe(true);
    expect(resp.qmd).toMatch(/Updated paragraph here\./);
    // Untouched surroundings are preserved verbatim from the
    // original — this is the whole point of the incremental writer.
    expect(resp.qmd).toContain('# Heading');
    expect(resp.qmd).toContain('## Sub');
    expect(resp.qmd).toContain('Second paragraph here.');
  });

  it('reports a structured error when the baseline AST JSON is malformed', () => {
    const original = '# x\n';
    const baseline = parseAst(original);
    const respJson = wasm.incremental_write_qmd(
      original,
      '{not valid json',
      JSON.stringify(baseline),
    );
    const resp: AstResponse = JSON.parse(respJson);
    expect(resp.success).toBe(false);
    expect(resp.error).toMatch(/baseline AST JSON/i);
  });
});
