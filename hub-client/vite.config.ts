import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import wasm from 'vite-plugin-wasm'
import path from 'path'
import { execSync } from 'child_process'

function getGitInfo() {
  try {
    const commitHash = execSync('git rev-parse --short HEAD', { encoding: 'utf-8' }).trim()
    const commitDate = execSync('git log -1 --format=%ci', { encoding: 'utf-8' }).trim()
    return { commitHash, commitDate }
  } catch {
    return { commitHash: 'unknown', commitDate: 'unknown' }
  }
}

const gitInfo = getGitInfo()

/** Hub server URL. Override with VITE_HUB_SERVER env var. */
const hubTarget = process.env.VITE_HUB_SERVER || 'http://localhost:3000';

// https://vite.dev/config/
export default defineConfig({
  base: './',
  plugins: [react(), wasm()],
  define: {
    __GIT_COMMIT_HASH__: JSON.stringify(gitInfo.commitHash),
    __GIT_COMMIT_DATE__: JSON.stringify(gitInfo.commitDate),
    __BUILD_TIME__: JSON.stringify(new Date().toISOString()),
  },
  resolve: {
    // Prefer 'source' condition for workspace packages - allows Vite to transpile
    // TypeScript directly without requiring a pre-build step
    conditions: ['source', 'import', 'module', 'browser', 'default'],
    alias: {
      'wasm-quarto-hub-client': path.resolve(__dirname, 'wasm-quarto-hub-client/wasm_quarto_hub_client.js'),
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
    proxy: {
      // Forward /auth/* to the hub server (JWT validation, cookies, OAuth callback).
      '/auth': {
        target: hubTarget,
        changeOrigin: true,
      },
      // Forward WebSocket upgrades to the hub server for Automerge sync.
      // In dev, cookies are origin-scoped to :5173, so we proxy through Vite
      // rather than connecting directly to the hub's port.
      '/ws': {
        target: hubTarget,
        ws: true,
        changeOrigin: true,
      },
    },
  },
})
