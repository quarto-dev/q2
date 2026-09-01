import { defineConfig, type Plugin, type UserConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { cpSync, mkdirSync, readFileSync, rmSync } from 'fs';
import { resolve } from 'path';

// Build service worker or main app based on environment variable
const isServiceWorkerBuild = process.env.BUILD_TARGET === 'service-worker';

// The renderer is imported straight from `@quarto/preview-renderer`
// *source* (this project is not an npm-workspace member, so the bare
// specifier would not resolve on its own). The alias bypasses the
// package's exports map, which lets this entry reach deep modules
// (PreviewRoot, custom/PreviewTitleBlock, …) that only the barrels
// re-export — without modifying the package.
const previewRendererSrc = resolve(__dirname, '../../ts-packages/preview-renderer/src');

// Where the same-origin fallback copy of the built renderer lives.
// hub-client serves it from public/ when VITE_Q2_SANDBOXED_PREVIEW_URL
// points at 'q2-sandboxed-preview/index.html'; the default is the
// cross-origin GitHub Pages deployment of dist/.
const parentPublicDir = resolve(__dirname, '../public/q2-sandboxed-preview');

/**
 * Same virtual-module indirection hub-client's vite config uses for the
 * attribution viewer CSS: `?raw`/CSS imports of files outside the project
 * root are unreliable in dev/vitest, so `framework/attribution.tsx` imports
 * `virtual:quarto-attribution-viewer-css` and every consumer provides it.
 */
function attributionViewerCssPlugin(): Plugin {
  const VIRTUAL_ID = 'virtual:quarto-attribution-viewer-css';
  const RESOLVED_ID = '\0' + VIRTUAL_ID;
  const sourcePath = resolve(__dirname, '../../resources/attribution/viewer.css');
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

const serviceWorkerConfig: UserConfig = {
  plugins: [
    {
      name: 'copy-sw-to-parent',
      closeBundle() {
        const srcPath = resolve(__dirname, 'dist/serviceWorker.js');
        const destPath = resolve(parentPublicDir, 'serviceWorker.js');
        try {
          mkdirSync(parentPublicDir, { recursive: true });
          cpSync(srcPath, destPath);
          console.log('✓ Copied service worker to ../public/q2-sandboxed-preview/serviceWorker.js');
        } catch (err) {
          console.error('Failed to copy service worker:', err);
        }
      },
    },
  ],
  build: {
    outDir: 'dist',
    emptyOutDir: false, // Don't clear dist when building SW
    lib: {
      entry: resolve(__dirname, 'src/serviceWorker.ts'),
      name: 'ServiceWorker',
      fileName: () => 'serviceWorker.js',
      formats: ['iife'],
    },
    rollupOptions: {
      output: {
        // Ensure no code splitting for service worker
        inlineDynamicImports: true,
      },
    },
  },
};

const mainConfig: UserConfig = {
  // Relative asset URLs so the multi-file bundle works both at the
  // GitHub Pages project path (/q2/) and under
  // hub-client/public/q2-sandboxed-preview/.
  base: './',
  plugins: [
    react(),
    attributionViewerCssPlugin(),
    {
      name: 'copy-dist-to-parent',
      closeBundle() {
        try {
          rmSync(parentPublicDir, { recursive: true, force: true });
          cpSync(resolve(__dirname, 'dist'), parentPublicDir, { recursive: true });
          console.log('✓ Copied dist/ to ../public/q2-sandboxed-preview/');
        } catch (err) {
          console.error('Failed to copy dist to parent public dir:', err);
        }
      },
    },
  ],
  resolve: {
    // The renderer source resolves its deps (react, katex, tiptap, …)
    // by walking up from ts-packages/preview-renderer into the repo-root
    // node_modules; this project resolves its own copies locally. Dedupe
    // so exactly one React (and one KaTeX) instance ends up in the bundle.
    dedupe: ['react', 'react-dom', 'katex'],
    conditions: ['source', 'import', 'module', 'browser', 'default'],
    alias: {
      '@quarto/preview-renderer': previewRendererSrc,
      // Parent-side-only modules that the q2-preview barrel drags into the
      // module graph (Q2PreviewIframe, assetWalker) import
      // @quarto/preview-runtime → WASM. The iframe never calls them; stub
      // the package so the graph resolves without pulling any WASM/sync
      // machinery into the sandboxed bundle.
      '@quarto/preview-runtime': resolve(__dirname, 'src/stubs/preview-runtime.ts'),
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: {
      input: 'index.html',
    },
  },
};

// https://vite.dev/config/
export default defineConfig(isServiceWorkerBuild ? serviceWorkerConfig : mainConfig);
