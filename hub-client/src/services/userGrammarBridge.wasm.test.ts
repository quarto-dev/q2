/**
 * Vitest coverage for the wasm-bindgen bridge between Rust's
 * `UserGrammarProvider` and a JS-side highlight callback — Phase 4.3
 * of `claude-notes/plans/2026-04-21-syntax-highlighting-phase-4.md`.
 *
 * The bridge is `JsUserGrammars` in `wasm-quarto-hub-client`:
 * a `#[wasm_bindgen]` struct with `constructor()` and
 * `register(class, highlight_fn)`. When the pipeline encounters a
 * code block whose language class is registered, it invokes the JS
 * callback and writes the returned JSON into `data-hl-spans`.
 *
 * These tests use `quarto_highlight_with_user_for_test` — a
 * test-only wasm export that exercises the bridge at the smallest
 * layer: (class, source, user) → JSON | undefined. It does NOT run
 * the full render pipeline; that's Phase 4.5's job.
 */

import { beforeAll, describe, expect, it } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

interface JsUserGrammarsLike {
  register: (languageClass: string, highlightFn: (cls: string, src: string) => string | null) => void;
  free: () => void;
}

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  JsUserGrammars: new () => JsUserGrammarsLike;
  quarto_highlight_with_user_for_test: (
    languageClass: string,
    source: string,
    user: JsUserGrammarsLike,
  ) => string | undefined;
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

describe('JsUserGrammars bridge', () => {
  it('returns undefined for a class the user set does not register', () => {
    const user = new wasm.JsUserGrammars();
    try {
      const result = wasm.quarto_highlight_with_user_for_test(
        'unregistered-lang',
        'some source',
        user,
      );
      expect(result).toBeUndefined();
    } finally {
      user.free();
    }
  });

  it('invokes the registered callback with (class, source)', () => {
    const user = new wasm.JsUserGrammars();
    try {
      const calls: Array<[string, string]> = [];
      user.register('mylang', (cls, src) => {
        calls.push([cls, src]);
        return '[[0,3,"custom"]]';
      });

      const result = wasm.quarto_highlight_with_user_for_test(
        'mylang',
        'foo bar',
        user,
      );
      expect(result).toBe('[[0,3,"custom"]]');
      expect(calls).toEqual([['mylang', 'foo bar']]);
    } finally {
      user.free();
    }
  });

  it('treats a null/undefined return as Ok(None)', () => {
    const user = new wasm.JsUserGrammars();
    try {
      user.register('silentlang', () => null);
      const result = wasm.quarto_highlight_with_user_for_test(
        'silentlang',
        'input',
        user,
      );
      expect(result).toBeUndefined();
    } finally {
      user.free();
    }
  });

  it('isolates state between JsUserGrammars instances', () => {
    const a = new wasm.JsUserGrammars();
    const b = new wasm.JsUserGrammars();
    try {
      a.register('onlyInA', () => '[[0,1,"a"]]');
      b.register('onlyInB', () => '[[0,1,"b"]]');

      expect(
        wasm.quarto_highlight_with_user_for_test('onlyInA', 'src', a),
      ).toBe('[[0,1,"a"]]');
      expect(
        wasm.quarto_highlight_with_user_for_test('onlyInA', 'src', b),
      ).toBeUndefined();
      expect(
        wasm.quarto_highlight_with_user_for_test('onlyInB', 'src', b),
      ).toBe('[[0,1,"b"]]');
    } finally {
      a.free();
      b.free();
    }
  });

  it('re-registering the same class replaces the previous callback', () => {
    const user = new wasm.JsUserGrammars();
    try {
      user.register('mylang', () => '[[0,1,"first"]]');
      user.register('mylang', () => '[[0,1,"second"]]');
      const result = wasm.quarto_highlight_with_user_for_test('mylang', 'x', user);
      expect(result).toBe('[[0,1,"second"]]');
    } finally {
      user.free();
    }
  });
});
