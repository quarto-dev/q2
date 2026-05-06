import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import wasm from 'vite-plugin-wasm'
import compression from 'compression'
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
  plugins: [
    react(),
    wasm(),
    {
      // vite preview's static-file middleware does not gzip by default,
      // so a cold Playwright context downloads the ~32 MB WASM uncompressed.
      // Gzipping at the preview layer cuts the wire size to ~5.6 MB per
      // context, which is the main throughput win this config is after.
      // Active only for `vite preview` (the CI E2E server); `vite dev`
      // doesn't need it (transform pipeline is the bottleneck there).
      name: 'preview-compression',
      configurePreviewServer(server) {
        // `compression` is typed as an Express RequestHandler, but vite's
        // middleware stack is connect-based. Their (req, res, next) shapes
        // are runtime-compatible; the cast bridges the type mismatch.
        const middleware = compression({
          // mime-db marks `application/wasm` as non-compressible, so the
          // default `compression.filter` skips it. In practice the WASM
          // gzips ~6:1 (32 MB → ~5.6 MB), which is the main win we're
          // after. Override the filter to opt it back in.
          filter: (req, res) => {
            const type = res.getHeader('content-type');
            if (typeof type === 'string' && type.startsWith('application/wasm')) {
              return true;
            }
            return compression.filter(req, res);
          },
        });
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        server.middlewares.use(middleware as any);
      },
    },
  ],
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
    rollupOptions: {
      input: {
        main: path.resolve(__dirname, 'index.html'),
        debug: path.resolve(__dirname, 'debug.html'),
        'q2-debug': path.resolve(__dirname, 'public/q2-debug.html'),
      },
    },
  },
  server: {
    fs: {
      // Allow serving files from the wasm package
      allow: ['..'],
    },
    proxy: proxyConfig(),
  },
  // Vite preview ignores `server.proxy`, so mirror the same config for
  // the production-build server used by Playwright E2E in CI.
  preview: {
    proxy: proxyConfig(),
  },
})

function proxyConfig() {
  return {
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
  };
}
