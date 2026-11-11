import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import wasm from 'vite-plugin-wasm'
import path from 'path'

export default defineConfig({
  base: './',
  plugins: [react(), wasm()],
  resolve: {
    conditions: ['source', 'import', 'module', 'browser', 'default'],
    alias: {
      'wasm-quarto-hub-client': path.resolve(__dirname, 'wasm-quarto-hub-client/wasm_quarto_hub_client.js'),
      // The WASM module imports its JS bridge via wasm-bindgen
      // `raw_module = "/src/wasm-js-bridge/*.js"`. Point that at the shared
      // `@quarto/wasm-js-bridge` package, as hub-client does.
      '/src/wasm-js-bridge': path.resolve(__dirname, '../../ts-packages/wasm-js-bridge/src'),
    },
  },
  optimizeDeps: {
    exclude: ['wasm-quarto-hub-client', '@automerge/automerge'],
  },
  build: {
    target: 'esnext',
  },
  server: {
    fs: {
      allow: ['../..'],
    },
  },
})
