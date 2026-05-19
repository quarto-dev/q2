import { defineConfig } from 'vite';
import type { Plugin } from 'vite';
import react from '@vitejs/plugin-react';
import wasm from 'vite-plugin-wasm';
import path from 'path';
import { readFileSync } from 'fs';

/**
 * `virtual:quarto-attribution-viewer-css` — mirrors the plugin in
 * `hub-client/vite.config.ts` and `ts-packages/preview-renderer/vitest.config.ts`.
 * Required because `@quarto/preview-renderer` is resolved via the
 * `source` condition (raw .tsx), so this build pulls in
 * `framework/attribution.tsx` which imports the virtual module.
 */
function attributionViewerCssPlugin(): Plugin {
  const VIRTUAL_ID = 'virtual:quarto-attribution-viewer-css';
  const RESOLVED_ID = '\0' + VIRTUAL_ID;
  const sourcePath = path.resolve(__dirname, '../resources/attribution/viewer.css');
  return {
    name: 'quarto-attribution-viewer-css',
    resolveId(id) {
      if (id === VIRTUAL_ID) return RESOLVED_ID;
    },
    load(id) {
      if (id === RESOLVED_ID) {
        const css = readFileSync(sourcePath, 'utf-8');
        return `export default ${JSON.stringify(css)};`;
      }
    },
  };
}

// The q2-preview SPA is the future host of `quarto preview` (bd-kw93).
// Today it's a placeholder that just confirms the @quarto/preview-renderer
// boundary is wired up. Phase A of bd-kw93 will fill it in with the real
// samod / WASM / preview-pane plumbing.
export default defineConfig({
  base: './',
  plugins: [react(), wasm(), attributionViewerCssPlugin()],
  resolve: {
    // Prefer the `source` exports condition so workspace packages resolve
    // straight to .ts/.tsx without requiring a pre-built dist. Mirrors
    // hub-client/vite.config.ts.
    conditions: ['source', 'import', 'module', 'browser', 'default'],
    alias: {
      // Mirror hub-client's wasm-quarto-hub-client alias. The SPA imports
      // the same WASM module through the same `wasm-quarto-hub-client`
      // bare specifier; pointing it at hub-client's symlink avoids
      // duplicating the symlink for now. (When `quarto preview` formalizes
      // the SPA build it can either keep this alias or copy the bridge
      // dir to a SPA-local location.)
      'wasm-quarto-hub-client': path.resolve(
        __dirname,
        '../hub-client/wasm-quarto-hub-client/wasm_quarto_hub_client.js',
      ),
      // The Rust WASM module references `/src/wasm-js-bridge/sass.js`
      // via `wasm-bindgen raw_module`. Bridge files live in
      // `@quarto/wasm-js-bridge`; aliasing the Vite-root path keeps
      // the WASM module unchanged. Needed by Phase A.3 once the SPA
      // initialises WASM; landing the alias now is cheap.
      '/src/wasm-js-bridge': path.resolve(
        __dirname,
        '../ts-packages/wasm-js-bridge/src',
      ),
    },
  },
  optimizeDeps: {
    exclude: ['wasm-quarto-hub-client', '@automerge/automerge'],
  },
  build: {
    target: 'esnext',
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: {
      // Multi-entry build: `index.html` hosts the outer PreviewApp;
      // `q2-preview.html` hosts the inner sandboxed iframe that
      // Q2PreviewIframe loads (same pattern as hub-client). Vite
      // emits a dist/q2-preview.html with its own bundled chunk so
      // both files live inside the embedded SPA bundle the
      // quarto-preview Rust crate serves.
      input: {
        main: path.resolve(__dirname, 'index.html'),
        'q2-preview': path.resolve(__dirname, 'q2-preview.html'),
      },
    },
  },
  server: {
    port: 5175,
    fs: {
      allow: ['..'],
    },
  },
});
