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
  },
})
