import { defineConfig } from 'vitest/config'

// Unit tests that need neither the WASM module nor a browser.
export default defineConfig({
  test: {
    include: ['src/**/*.test.ts'],
    exclude: ['src/**/*.wasm.test.ts'],
    environment: 'node',
  },
})
