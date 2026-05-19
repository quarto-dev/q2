import { defineConfig } from 'vitest/config';
import type { Plugin } from 'vite';
import path from 'path';
import { readFileSync } from 'fs';

/**
 * Expose `virtual:quarto-attribution-viewer-css` as a module whose
 * default export is the contents of `resources/attribution/viewer.css`.
 *
 * `resources/attribution/viewer.css` is the single source of truth
 * shared with the CLI's `AttributionViewerTransform` (via
 * `include_str!`). Vite / Vitest silently return empty for `?raw`
 * imports of files outside the project root even with
 * `server.fs.allow: ['..']`, so the virtual-module indirection is the
 * supported way to embed an out-of-tree asset's contents at
 * build/test time.
 *
 * Mirrors `attributionViewerCssPlugin` in `hub-client/vite.config.ts`
 * — the framework moved into this package but the resource still
 * lives at the repo root. Both consumers (hub-client app build,
 * preview-renderer standalone vitest) need their own copy of the
 * plugin.
 */
function attributionViewerCssPlugin(): Plugin {
  const VIRTUAL_ID = 'virtual:quarto-attribution-viewer-css';
  const RESOLVED_ID = '\0' + VIRTUAL_ID;
  // preview-renderer lives at `ts-packages/preview-renderer/`, so the
  // repo-root resource path is two levels up.
  const sourcePath = path.resolve(__dirname, '../../resources/attribution/viewer.css');
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

export default defineConfig({
  plugins: [attributionViewerCssPlugin()],
  resolve: {
    conditions: ['source', 'import', 'module', 'browser', 'default'],
    alias: {
      // Resolve workspace packages to source so tests work on a fresh
      // clone (no built dist required). Mirrors hub-client/vitest.config.ts.
      '@quarto/quarto-automerge-schema': path.resolve(
        __dirname,
        '../quarto-automerge-schema/src/index.ts',
      ),
      '@quarto/quarto-sync-client': path.resolve(
        __dirname,
        '../quarto-sync-client/src/index.ts',
      ),
      '@quarto/preview-runtime': path.resolve(
        __dirname,
        '../preview-runtime/src',
      ),
    },
  },
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts', 'src/**/*.test.tsx'],
    exclude: [
      'src/**/*.integration.test.ts',
      'src/**/*.integration.test.tsx',
    ],
    passWithNoTests: true,
  },
});
