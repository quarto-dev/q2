/**
 * Vitest coverage for the JS-side user-grammar highlighter — Phase 4.2
 * of `claude-notes/plans/2026-04-21-syntax-highlighting-phase-4.md`.
 *
 * Loads the TOML grammar from the shared fixture in `crates/quarto-highlight`,
 * highlights TOML source, and asserts the same captures the native Rust
 * path verifies in `crates/quarto-highlight/tests/user_grammar_toml.rs`.
 *
 * Runs under `npm run test:wasm` because it needs real wasm I/O (the
 * web-tree-sitter runtime and the `toml.wasm` grammar bytes).
 */

import { beforeAll, describe, expect, it } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join, resolve } from 'path';
import { fileURLToPath } from 'url';

import { loadUserGrammar, type UserGrammarHighlighter } from './userGrammarHighlight';

type SpanTriple = [number, number, string];

let highlighter: UserGrammarHighlighter;

beforeAll(async () => {
  const __dirname = dirname(fileURLToPath(import.meta.url));
  const repoRoot = resolve(__dirname, '../../..');
  const fixtureDir = join(
    repoRoot,
    'crates/quarto-highlight/tests/fixtures/user-grammar-toml',
  );

  const wasmBytes = await readFile(join(fixtureDir, 'toml.wasm'));
  const highlightsScm = await readFile(join(fixtureDir, 'highlights.scm'), 'utf-8');

  highlighter = await loadUserGrammar({
    name: 'toml',
    wasmBytes: new Uint8Array(wasmBytes),
    highlightsScm,
  });
});

describe('loadUserGrammar — TOML fixture', () => {
  it('returns a JSON string decodable into a triple-array', () => {
    const source = 'name = "value"\n';
    const json = highlighter.highlight(source);
    expect(typeof json).toBe('string');
    const decoded = JSON.parse(json) as SpanTriple[];
    expect(Array.isArray(decoded)).toBe(true);
    // Every entry must be [number, number, string].
    for (const [start, end, capture] of decoded) {
      expect(typeof start).toBe('number');
      expect(typeof end).toBe('number');
      expect(typeof capture).toBe('string');
      expect(end).toBeGreaterThanOrEqual(start);
    }
  });

  it('produces the same captures the native path verifies', () => {
    // Same assertions as `user_grammar_toml::user_grammar_loads_and_highlights_toml`:
    // presence of `operator`, `string` starting at byte 7, and either
    // `property` or `type` for the key.
    const source = 'name = "value"\n';
    const decoded = JSON.parse(highlighter.highlight(source)) as SpanTriple[];

    expect(
      decoded.some(([, , capture]) => capture === 'operator'),
      `expected an operator capture; got ${JSON.stringify(decoded)}`,
    ).toBe(true);

    expect(
      decoded.some(([start, , capture]) => start === 7 && capture === 'string'),
      `expected a string capture starting at byte 7; got ${JSON.stringify(decoded)}`,
    ).toBe(true);

    expect(
      decoded.some(
        ([, , capture]) => capture === 'property' || capture === 'type',
      ),
      `expected property/type capture for the key; got ${JSON.stringify(decoded)}`,
    ).toBe(true);
  });

  it('returns an empty JSON array for empty source', () => {
    const json = highlighter.highlight('');
    const decoded = JSON.parse(json) as SpanTriple[];
    expect(decoded).toEqual([]);
  });

  it('emits spans in stable canonical order (start asc, end desc)', () => {
    // Canonical ordering makes string-equality comparisons tractable
    // (the Rust native path doesn't canonicalize; the parity test
    // sorts both before comparing). Still, a stable JS output is
    // valuable on its own.
    const source = 'a = 1\nb = 2\n';
    const decoded = JSON.parse(highlighter.highlight(source)) as SpanTriple[];
    for (let i = 1; i < decoded.length; i++) {
      const [startA, endA] = decoded[i - 1];
      const [startB, endB] = decoded[i];
      const aFirst = startA < startB || (startA === startB && endA >= endB);
      expect(
        aFirst,
        `spans not in (start asc, end desc) order at index ${i}: ` +
          `prev=${JSON.stringify(decoded[i - 1])} next=${JSON.stringify(decoded[i])}`,
      ).toBe(true);
    }
  });

  it('re-invoking highlight on the same highlighter produces consistent output', () => {
    const source = 'x = 42\n';
    const first = highlighter.highlight(source);
    const second = highlighter.highlight(source);
    expect(first).toBe(second);
  });
});
