import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import wasm from 'vite-plugin-wasm'
import path from 'path'

// https://vite.dev/config/
export default defineConfig({
  base: './',
  plugins: [react(), wasm()],
  resolve: {
    alias: {
      'wasm-quarto-hub-client': path.resolve(__dirname, 'wasm-quarto-hub-client'),
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
      // Allow serving files from the wasm package
      allow: ['..'],
    },
  },
})
