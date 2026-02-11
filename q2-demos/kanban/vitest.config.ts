import { defineConfig, mergeConfig } from 'vitest/config'
import viteConfig from './vite.config'
import path from 'path'

export default mergeConfig(
  viteConfig,
  defineConfig({
    resolve: {
      alias: {
        '@quarto/quarto-automerge-schema': path.resolve(__dirname, '../../ts-packages/quarto-automerge-schema/src/index.ts'),
        '@quarto/quarto-sync-client': path.resolve(__dirname, '../../ts-packages/quarto-sync-client/src/index.ts'),
        '@quarto/pandoc-types': path.resolve(__dirname, '../../ts-packages/pandoc-types/src/index.ts'),
      },
    },
    test: {
      environment: 'node',
      include: ['src/**/*.test.ts', 'src/**/*.test.tsx'],
      exclude: [
        'src/**/*.integration.test.ts',
        'src/**/*.integration.test.tsx',
        'src/**/*.wasm.test.ts',
      ],
      passWithNoTests: true,
    },
  }),
)
