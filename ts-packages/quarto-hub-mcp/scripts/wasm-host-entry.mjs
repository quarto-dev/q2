// Entry point for the prebundled WASM host (dist/wasm-host.mjs).
//
// `build-wasm-host.mjs` bundles this with esbuild, aliasing the
// wasm-bindgen JS's Vite-root-absolute bridge imports
// (/src/wasm-js-bridge/*) to the ts-packages/wasm-js-bridge sources,
// so the same wasm-quarto-hub-client module the browser preview runs
// loads in plain Node. `sass` stays external — diagnostics never
// compile stylesheets, and the bridge only imports it lazily.
import { readFile } from 'node:fs/promises';
import init from 'wasm-quarto-hub-client';

export * from 'wasm-quarto-hub-client';

let ready;

/**
 * Initialize the WASM module from the binary shipped next to this
 * file. Idempotent; callers await it before any render/vfs call.
 */
export function ensureInit() {
  ready ??= readFile(new URL('./wasm_quarto_hub_client_bg.wasm', import.meta.url)).then(
    (bytes) => init({ module_or_path: bytes }),
  );
  return ready;
}
