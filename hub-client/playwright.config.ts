import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright configuration for hub-client E2E tests
 *
 * ## Prerequisites (must be satisfied before running tests)
 *
 * 1. **Production build with test hooks**:
 *    ```
 *    VITE_E2E=1 npm run build
 *    ```
 *    Without `VITE_E2E=1` the bundle tree-shakes out `window.__quartoTest`
 *    and every test fails with "E2E test hooks not found". The full
 *    `npm run test:e2e` command handles this automatically.
 *
 * 2. **Hub server not already running on port 3031**:
 *    `globalSetup` starts its own hub on port 3031. A dev hub on that port
 *    will cause a bind failure. Use a different port for dev (`--port 3030`).
 *
 * ## Architecture
 *
 * - globalSetup starts the Rust hub server (`cargo run --bin hub`)
 * - Tests create Automerge projects dynamically via quarto-sync-client
 * - Tests run in parallel against the same hub server (different documents)
 * - globalTeardown stops the hub server and cleans up
 */
export default defineConfig({
  testDir: './e2e',
  // Visual-regression specs run via playwright.visual.config.ts (which has
  // the --update-snapshots-on-missing retry flow); exclude them from the
  // functional run so a missing baseline isn't a hard failure here.
  testIgnore: ['**/*.visual.spec.ts', '**/smoke-all.spec.ts'],
  // Parallel tests are OK - they use different documents, single sync server handles concurrency
  fullyParallel: true,
  // Fail on `test.only` in CI
  forbidOnly: !!process.env.CI,
  // 1 retry locally handles peer-connection timing races without requiring
  // a manual re-run. CI gets 2 for its more variable network conditions.
  retries: process.env.CI ? 3 : 1,
  // Parallel workers. Match the runner's CPU count: ubuntu-latest has
  // 2 cores. Running more workers than cores causes the WASM render
  // pipeline to stall under contention and individual tests miss the
  // preview-render deadline non-deterministically.
  workers: process.env.CI ? 2 : undefined,
  // HTML reporter
  reporter: 'html',

  // Global setup/teardown for sync server lifecycle
  globalSetup: './e2e/helpers/globalSetup.ts',
  globalTeardown: './e2e/helpers/globalTeardown.ts',

  use: {
    // Base URL for the dev server
    baseURL: 'http://localhost:5174',
    // Trace recording for debugging failures
    trace: 'on-first-retry',
    // Screenshot on failure
    screenshot: 'only-on-failure',
    // Video on first retry
    video: 'on-first-retry',
  },

  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
    // Add other browsers as needed
    // {
    //   name: 'firefox',
    //   use: { ...devices['Desktop Firefox'] },
    // },
    // {
    //   name: 'webkit',
    //   use: { ...devices['Desktop Safari'] },
    // },
  ],

  // Serve the prebuilt hub-client bundle via `vite preview` rather than
  // `vite dev`. Why this matters for E2E throughput:
  //   - A cold Playwright browser context fetches ~50 MB of assets
  //     (the ~32 MB WASM dominates). Through `vite dev` that goes
  //     uncompressed and serialized through the transform pipeline;
  //     through `vite preview` (with our gzip middleware in vite.config.ts)
  //     it's ~5.6 MB on the wire and served as static files.
  //   - `vite dev` also re-transforms ~500 TS/JSX modules per page load;
  //     the production bundle is ~10 chunks.
  //   - Under 2 Playwright workers contending for one dev server on a
  //     2-core CI runner this was the source of the "preview iframe
  //     didn't render in 45s" flakes. Do NOT revert this to `npm run dev`
  //     to get HMR back — HMR isn't used by tests and dev mode reintroduces
  //     the contention.
  webServer: {
    command: 'npm run preview -- --port 5174',
    url: 'http://localhost:5174',
    // Reuse existing server in dev mode for faster iteration
    reuseExistingServer: !process.env.CI,
    // Timeout for server to start (preview is near-instant; the bundle
    // must already be built, which the CI workflow does explicitly).
    timeout: 120000,
    // Vite preview reads VITE_HUB_SERVER at config-eval time to wire
    // preview.proxy at the target. globalSetup starts the e2e hub on
    // port 3030, so point preview at that port. The env is server-side
    // only — it isn't baked into the built client bundle.
    env: {
      VITE_HUB_SERVER: 'http://localhost:3031',
    },
  },
});
