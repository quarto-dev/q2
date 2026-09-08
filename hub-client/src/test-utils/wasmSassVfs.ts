/**
 * Wire the dart-sass importer to the WASM module's VFS for a
 * `*.wasm.test.ts` file.
 *
 * Every render through the WASM pipeline compiles a Bootstrap SCSS
 * bundle, and dart-sass resolves that bundle's `@import`s by calling
 * back into the VFS via `setVfsCallbacks`. A test that renders without
 * wiring this fails with `Q-14-6` ("Can't find stylesheet to import
 * vendor/rfs"). Before bd-jsvetdea the failure was masked: the theme
 * stage silently shipped the static DEFAULT_CSS instead, so tests
 * that only looked at the HTML passed with an unwired importer. It is
 * a hard error now, matching what `q2 render` does, so call this from
 * `beforeAll` right after `wasm.default(bytes)`.
 *
 * The production wiring lives in
 * `ts-packages/preview-runtime/src/wasmRenderer.ts`; this mirrors it
 * for tests that load the module by hand.
 */

// `/src/wasm-js-bridge` is aliased to `@quarto/wasm-js-bridge/src` in
// hub-client's vite + vitest configs (the same alias the Rust WASM
// module's `raw_module` annotation uses).
import { setVfsCallbacks } from '/src/wasm-js-bridge/sass.js';

/** The one WASM export the importer needs. */
export interface VfsReader {
  vfs_read_file: (path: string) => string;
}

export function wireSassVfs(wasm: VfsReader): void {
  const read = (path: string): string | null => {
    try {
      const result = JSON.parse(wasm.vfs_read_file(path)) as { success: boolean; content?: string };
      return result.success && result.content !== undefined ? result.content : null;
    } catch {
      return null;
    }
  };
  setVfsCallbacks(read, (path: string): boolean => read(path) !== null);
}
