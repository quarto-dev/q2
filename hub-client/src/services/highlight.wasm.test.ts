/**
 * WASM tests for the syntax-highlighting path — Phase 3 of
 * `claude-notes/plans/2026-04-19-syntax-highlighting-design.md`.
 *
 * Exercises the `quarto_highlight_for_test` export, which calls through
 * to `quarto_highlight::highlight()` — the same
 * `Registry::global().highlight()` → `tree_sitter_highlight::Highlighter`
 * path that native `quarto render` uses via `CodeHighlightStage`.
 *
 * Fixtures come from a file shared with the native golden tests:
 *   `crates/quarto-highlight/tests/fixtures/builtin-snippets.json`
 * Any drift between native and WASM fixture coverage would surface as a
 * diff there.
 *
 * Run with: `npm run test:wasm`
 */

import { beforeAll, describe, expect, it } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join, resolve } from 'path';
import { fileURLToPath } from 'url';

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  quarto_highlight_for_test: (languageClass: string, source: string) => string | undefined;
}

interface Fixture {
  name: string;
  class: string;
  source: string;
}

/** Validate the JSON shape returned by `quarto_highlight_for_test`. */
type SpanTriple = [number, number, string];

let wasm: WasmModule;
let fixtures: Fixture[];

beforeAll(async () => {
  const __dirname = dirname(fileURLToPath(import.meta.url));

  // Load and init the WASM module (pattern copied from other
  // *.wasm.test.ts files in this directory).
  const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
  const wasmPath = join(wasmDir, 'wasm_quarto_hub_client_bg.wasm');
  const wasmBytes = await readFile(wasmPath);
  wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
  await wasm.default(wasmBytes);

  // Load the shared fixture JSON from the Rust test directory.
  // hub-client/src/services → repo root → crates/…
  const repoRoot = resolve(__dirname, '../../..');
  const fixturePath = join(
    repoRoot,
    'crates/quarto-highlight/tests/fixtures/builtin-snippets.json',
  );
  const raw = await readFile(fixturePath, 'utf-8');
  fixtures = JSON.parse(raw) as Fixture[];
});

describe('quarto_highlight_for_test', () => {
  it('loads a non-empty fixture set', () => {
    expect(fixtures.length).toBeGreaterThan(0);
  });

  it('returns undefined for an unregistered language class', () => {
    const result = wasm.quarto_highlight_for_test('nonsense-lang-xyz', 'anything');
    // Matches native `highlight()` behavior: `Ok(None)` surfaces as undefined.
    expect(result).toBeUndefined();
  });

  // One assertion pass per fixture, registered dynamically because the
  // fixture set loads asynchronously in `beforeAll`. If a specific
  // grammar regresses, the failure message below identifies which one.
  it('every built-in grammar produces a non-empty span array of valid triples', () => {
    for (const fixture of fixtures) {
      const raw = wasm.quarto_highlight_for_test(fixture.class, fixture.source);
      expect(raw, `fixture ${fixture.name} (${fixture.class}) returned undefined`).toBeDefined();

      let spans: unknown;
      try {
        spans = JSON.parse(raw as string);
      } catch (err) {
        throw new Error(
          `fixture ${fixture.name} (${fixture.class}): output is not valid JSON — ${String(err)} — raw: ${String(raw)}`,
        );
      }

      expect(Array.isArray(spans), `fixture ${fixture.name}: expected JSON array`).toBe(true);
      const arr = spans as unknown[];
      expect(
        arr.length,
        `fixture ${fixture.name}: expected at least one highlight span for a non-empty snippet`,
      ).toBeGreaterThan(0);

      // Every span must be a triple: [start: number, end: number, capture: string].
      for (let i = 0; i < arr.length; i++) {
        const entry = arr[i];
        expect(Array.isArray(entry), `fixture ${fixture.name} span ${i} is not an array`).toBe(
          true,
        );
        const triple = entry as unknown[];
        expect(triple.length, `fixture ${fixture.name} span ${i} wrong arity`).toBeGreaterThanOrEqual(
          3,
        );
        expect(
          typeof triple[0],
          `fixture ${fixture.name} span ${i} start is not number`,
        ).toBe('number');
        expect(typeof triple[1], `fixture ${fixture.name} span ${i} end is not number`).toBe(
          'number',
        );
        expect(
          typeof triple[2],
          `fixture ${fixture.name} span ${i} capture name is not string`,
        ).toBe('string');
        const [start, end] = triple as SpanTriple;
        expect(
          start,
          `fixture ${fixture.name} span ${i}: start ${start} out of source range`,
        ).toBeGreaterThanOrEqual(0);
        expect(
          end,
          `fixture ${fixture.name} span ${i}: end ${end} < start ${start}`,
        ).toBeGreaterThanOrEqual(start);
        // Spans index into source bytes; end must fit in the source
        // length (counted as UTF-8 bytes to match tree-sitter).
        const byteLen = new TextEncoder().encode(fixture.source).length;
        expect(
          end,
          `fixture ${fixture.name} span ${i}: end ${end} exceeds source byte length ${byteLen}`,
        ).toBeLessThanOrEqual(byteLen);
      }
    }
  });
});
