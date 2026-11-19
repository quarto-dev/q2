import { defineConfig } from 'vite'
import wasm from 'vite-plugin-wasm'
import path from 'path'

export default defineConfig({
  // wasm(): automerge ships as a WASM module.
  plugins: [wasm()],
  resolve: {
    // Resolve workspace packages straight to their TypeScript source.
    conditions: ['source', 'import', 'module', 'browser', 'default'],
    alias: {
      // The qmd parser (WASM), and the JS bridge files it imports by absolute path.
      'wasm-quarto-hub-client': path.resolve(
        __dirname,
        '../../hub-client/wasm-quarto-hub-client/wasm_quarto_hub_client.js',
      ),
      '/src/wasm-js-bridge': path.resolve(__dirname, '../../ts-packages/wasm-js-bridge/src'),
    },
  },
  optimizeDeps: {
    exclude: ['wasm-quarto-hub-client', '@automerge/automerge'],
  },
  build: { target: 'esnext' },
  server: {
    fs: { allow: ['../..'] },
  },
})
