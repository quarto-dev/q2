/**
 * Public-API guard for the SASS bridge.
 *
 * The Rust WASM module loads `sass.js` via `wasm-bindgen`'s
 * `raw_module = "/src/wasm-js-bridge/sass.js"`, and `wasmRenderer.ts`
 * (in `@quarto/preview-runtime`) dynamically imports it through the
 * same Vite-root path. Both depend on these specific function names
 * — renaming any of them silently breaks the Rust ↔ JS bridge at
 * runtime, which the type system can't catch (the WASM module is
 * generated, not consumer code).
 *
 * This test pins the names so a rename has to land here too.
 */

import { describe, it, expect } from 'vitest';
// @ts-expect-error sass.js is a JS file with a .d.ts companion — TS
// resolution under bundler mode picks up the .d.ts cleanly, but the
// resolver doesn't always like the bare `./sass` form. Use the full
// .js path which works at runtime AND points at the d.ts at compile.
import * as sass from './sass.js';

describe('@quarto/wasm-js-bridge sass surface', () => {
  it('exports the four functions the Rust WASM module + wasmRenderer.ts expect', () => {
    expect(typeof sass.setVfsCallbacks).toBe('function');
    expect(typeof sass.jsSassAvailable).toBe('function');
    expect(typeof sass.jsSassCompilerName).toBe('function');
    expect(typeof sass.jsCompileSass).toBe('function');
  });
});
