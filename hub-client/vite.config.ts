import { defineConfig } from 'vite'
import type { Plugin } from 'vite'
import react from '@vitejs/plugin-react'
import wasm from 'vite-plugin-wasm'
import compression from 'compression'
import { VitePWA } from 'vite-plugin-pwa'
import basicSsl from '@vitejs/plugin-basic-ssl'
import path from 'path'
import { readFileSync } from 'fs'
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
 */
function attributionViewerCssPlugin(): Plugin {
  const VIRTUAL_ID = 'virtual:quarto-attribution-viewer-css';
  const RESOLVED_ID = '\0' + VIRTUAL_ID;
  const sourcePath = path.resolve(__dirname, '../resources/attribution/viewer.css');
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

/** Hub server URL. Override with VITE_HUB_SERVER env var. */
const hubTarget = process.env.VITE_HUB_SERVER || 'http://localhost:3000';

/** Disable service worker in E2E tests to avoid caching interference */
const isE2E = process.env.VITE_E2E === '1';

/**
 * Disable the PWA service worker entirely (`build:preview-embed`).
 * The q2-preview embed serves this app from an ephemeral localhost
 * origin per `q2 preview` session; a service worker would fill Cache
 * Storage (precache + runtime-cached WASM) for every random port.
 * The plugin stays registered with `disable: true` (rather than being
 * omitted) so the `virtual:pwa-register` import in `src/main.tsx`
 * resolves to a no-op stub in these builds.
 */
const disablePwa = process.env.VITE_DISABLE_PWA === '1';

// https://vite.dev/config/
export default defineConfig({
  base: './',
  plugins: [
    react(),
    wasm(),
    ...(process.env.VITE_HTTPS === '1' ? [basicSsl()] : []),
    attributionViewerCssPlugin(),
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
    // The plugin is always present so `virtual:pwa-register` resolves in
    // every build (a no-op stub when disabled). `disable` covers E2E
    // (caching interference) and the preview-embed build (ephemeral
    // origins); neither generates a service worker.
    VitePWA({
      disable: isE2E || disablePwa,
      registerType: 'autoUpdate',
      includeAssets: ['quarto-icon.svg'],
      manifest: {
        name: 'Quarto Hub',
        short_name: 'Quarto Hub',
        description: 'Collaborative editing for Quarto projects',
        theme_color: '#447099',
        icons: [
          {
            src: 'quarto-icon.svg',
            sizes: 'any',
            type: 'image/svg+xml'
          }
        ]
      },
      workbox: {
        // Precache the app shell only: HTML, JS/CSS bundles, fonts, icons.
        // WASM and the on-demand chunks below are runtime-cached instead —
        // an all-or-nothing ~56 MB precache meant one flaky fetch discarded
        // the new SW and the old one kept serving (GH #447).
        globPatterns: ['**/*.{html,js,css,svg,woff,woff2}'],
        // Monaco workers (~9 MB) and the sass chunk (~3.2 MB) load on
        // demand; they are content-hashed, so CacheFirst at runtime is
        // correct and keeps them out of the atomic install.
        globIgnores: ['**/*.worker-*.js', '**/sass.default-*.js'],
        // Largest remaining precached file is main.js (~7.5 MB); 16 MB
        // lets it more than double before tripping. Past the limit workbox
        // warns and vite-plugin-pwa escalates to a fatal build error, so
        // the ceiling guards against accidental re-inclusion of a large
        // asset (silent precache re-bloat → install failures → #447) with
        // a loud red CI instead. The override is required regardless:
        // workbox's 2 MB default is already exceeded by main.js.
        maximumFileSizeToCacheInBytes: 16 * 1024 * 1024,
        // Don't warn about large files - we know they're big
        dontCacheBustURLsMatching: /\.[0-9a-f]{8}\./,
        runtimeCaching: [
          {
            // Content-hashed WASM (quarto parser ~26 MB, automerge ~3.5 MB,
            // tree-sitter). CacheFirst is correct: the URL changes when the
            // bytes change.
            urlPattern: /\/assets\/.*\.wasm$/,
            handler: 'CacheFirst',
            options: {
              cacheName: 'wasm-cache',
              expiration: {
                // Hashed URLs accumulate across deploys and runtime caches
                // have no automatic old-revision cleanup — cap them.
                maxEntries: 8,
                maxAgeSeconds: 60 * 60 * 24 * 30 // 30 days
              },
              cacheableResponse: {
                statuses: [200]
              }
            }
          },
          {
            // Monaco workers + sass chunk: loaded on demand, content-hashed.
            urlPattern: /\/assets\/(.*\.worker-.*|sass\.default-.*)\.js$/,
            handler: 'CacheFirst',
            options: {
              cacheName: 'ondemand-assets',
              expiration: {
                maxEntries: 12,
                maxAgeSeconds: 60 * 60 * 24 * 30 // 30 days
              },
              cacheableResponse: {
                statuses: [200]
              }
            }
          },
          {
            urlPattern: /^https:\/\/fonts\.googleapis\.com\/.*/i,
            handler: 'CacheFirst',
            options: {
              cacheName: 'google-fonts-cache',
              expiration: {
                maxEntries: 10,
                maxAgeSeconds: 60 * 60 * 24 * 365 // 1 year
              },
              cacheableResponse: {
                statuses: [0, 200]
              }
            }
          },
          {
            urlPattern: /^https:\/\/fonts\.gstatic\.com\/.*/i,
            handler: 'CacheFirst',
            options: {
              cacheName: 'google-fonts-static',
              expiration: {
                maxEntries: 10,
                maxAgeSeconds: 60 * 60 * 24 * 365 // 1 year
              },
              cacheableResponse: {
                statuses: [0, 200]
              }
            }
          }
        ]
      }
    })
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
      // The Rust WASM module references the JS bridge via
      // `wasm-bindgen raw_module = "/src/wasm-js-bridge/sass.js"`.
      // Vite resolves the leading `/` from the project root, but the
      // bridge files live in `@quarto/wasm-js-bridge` (one copy in
      // the workspace, shared by hub-client + the q2-preview SPA).
      // Alias intercepts the path before the project-root fallback.
      '/src/wasm-js-bridge': path.resolve(__dirname, '../ts-packages/wasm-js-bridge/src'),
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
        // q2-debug.html and q2-preview.html live at the hub-client root,
        // not under public/. When they were in public/, the build emitted
        // both a transformed copy (at dist/public/q2-{debug,preview}.html)
        // and an untransformed copy of the source file (at
        // dist/q2-{debug,preview}.html, referencing the dev-only
        // `<script src="/src/...">` path). The iframes' `src="/q2-*.html"`
        // hit the wrong one under `vite preview`, leaving the iframe blank
        // and breaking the q2-debug + q2-preview E2E suite. Same fix as
        // PR #172 applied to ast-renderer.html on main.
        'q2-debug': path.resolve(__dirname, 'q2-debug.html'),
        'q2-preview': path.resolve(__dirname, 'q2-preview.html'),
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
