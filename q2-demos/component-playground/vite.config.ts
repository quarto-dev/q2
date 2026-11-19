import { defineConfig } from 'vite'
import type { Plugin } from 'vite'
import react from '@vitejs/plugin-react'
import wasm from 'vite-plugin-wasm'
import path from 'path'
import { existsSync, readFileSync } from 'fs'

/**
 * The component playground: a Vite project of its own that consumes
 * hub-client's components from the outside, as a host application would.
 *
 * hub-client ships no build, so its sources are compiled here, and this
 * config has to repeat every piece of hub-client/vite.config.ts those
 * sources rely on. Each entry below says what needs it. Together they are
 * the list of things a real package would have to stop asking of a host.
 */

const hubClient = path.resolve(__dirname, '../../hub-client')
const repoRoot = path.resolve(__dirname, '../..')

/** `framework/attribution.tsx` in @quarto/preview-renderer imports this virtual module. */
function attributionViewerCssPlugin(): Plugin {
  const VIRTUAL_ID = 'virtual:quarto-attribution-viewer-css'
  const RESOLVED_ID = '\0' + VIRTUAL_ID
  const sourcePath = path.resolve(repoRoot, 'resources/attribution/viewer.css')
  return {
    name: 'quarto-attribution-viewer-css',
    resolveId(id) {
      if (id === VIRTUAL_ID) return RESOLVED_ID
    },
    load(id) {
      if (id === RESOLVED_ID) {
        return `export default ${JSON.stringify(readFileSync(sourcePath, 'utf-8'))};`
      }
    },
  }
}

/**
 * Every HTML page of the build: the gallery, one page per demo, and the two
 * pages hub-client's preview components load in iframes by relative URL
 * (which is why all pages sit side by side at the root).
 */
const PAGES = ['index', 'simple-auth', 'monaco', 'preview', 'project', 'canvas', 'embed', 'embed2', 'q2-preview', 'q2-debug']

/** The hub the demos sign in to and sync with. Override with VITE_HUB_SERVER. */
const hubTarget = process.env.VITE_HUB_SERVER || 'http://localhost:3000'

export default defineConfig({
  base: './',
  plugins: [react(), wasm(), attributionViewerCssPlugin()],
  // hub-client's static assets, fetched by URL at runtime: the pdf.js
  // viewer, pandoc and typst WASM, the reveal menu stylesheet, the icon.
  publicDir: path.resolve(hubClient, 'public'),
  // Read hub-client's .env files (VITE_DEFAULT_SYNC_SERVER, VITE_GOOGLE_CLIENT_ID).
  envDir: hubClient,
  worker: {
    // typst.worker.ts imports its wasm wrapper dynamically; the default
    // IIFE worker format cannot code-split.
    rollupOptions: { output: { inlineDynamicImports: true } },
  },
  define: {
    // Build-time constants hub-client's sources read as globals.
    __GIT_COMMIT_HASH__: JSON.stringify('playground'),
    __GIT_COMMIT_DATE__: JSON.stringify('playground'),
    __BUILD_TIME__: JSON.stringify(new Date().toISOString()),
    __PANDOC_WASM_ENABLED__: JSON.stringify(existsSync(path.resolve(hubClient, 'public/pandoc/pandoc.wasm.gz'))),
  },
  resolve: {
    // Workspace packages resolve to their TypeScript sources.
    conditions: ['source', 'import', 'module', 'browser', 'default'],
    alias: {
      // The WASM module is imported by bare name.
      'wasm-quarto-hub-client': path.resolve(hubClient, 'wasm-quarto-hub-client/wasm_quarto_hub_client.js'),
      // The WASM module imports its JS bridge by this root-absolute path.
      '/src/wasm-js-bridge': path.resolve(repoRoot, 'ts-packages/wasm-js-bridge/src'),
    },
  },
  optimizeDeps: {
    // Same exclusions as hub-client: these locate their .wasm relative to
    // their own module URL, which pre-bundling would move.
    exclude: ['wasm-quarto-hub-client', '@automerge/automerge', 'hephaestus-svg-wasm'],
  },
  build: {
    target: 'esnext',
    rollupOptions: {
      input: Object.fromEntries(PAGES.map((page) => [page, path.resolve(__dirname, `${page}.html`)])),
    },
  },
  server: {
    // hub-client's dev server is 5173; a fixed port of our own keeps the
    // two apart (browser storage is per origin).
    port: 5176,
    // Same host as local prod (http://127.0.0.1:8080): cookies ignore the
    // port, so a session signed in there is signed in here too.
    host: '127.0.0.1',
    fs: {
      // Sources and WASM are served from across the repo.
      allow: [repoRoot],
    },
    proxy: {
      // The demos treat this page's origin as the hub. The Host header is
      // passed through unchanged (no `changeOrigin`): the hub accepts a
      // WebSocket only when its Origin matches its Host.
      '/auth': { target: hubTarget },
      '/ws': { target: hubTarget, ws: true },
    },
  },
})
