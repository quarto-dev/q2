import { defineConfig, mergeConfig } from 'vitest/config'
import viteConfig from './vite.config'
import path from 'path'

export default mergeConfig(
  viteConfig,
  defineConfig({
    test: {
      include: ['src/**/*.wasm.test.ts'],
      environment: 'node',
      passWithNoTests: true,
      testTimeout: 30000,
    },
    resolve: {
      alias: {
        '/src': path.resolve(__dirname, 'src'),
        '@quarto/pandoc-types': path.resolve(__dirname, '../../ts-packages/pandoc-types/src/index.ts'),
      },
    },
  }),
)
