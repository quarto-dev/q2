import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright configuration for hub-client E2E tests
 *
 * Test architecture:
 * - globalSetup starts the Rust hub server (cargo run --bin hub)
 * - Tests create Automerge projects dynamically via quarto-sync-client
 * - Tests run in parallel against the same hub server (different documents)
 * - globalTeardown stops the hub server and cleans up
 */
export default defineConfig({
  testDir: './e2e',
  // Visual-regression specs run via playwright.visual.config.ts (which has
  // the --update-snapshots-on-missing retry flow); exclude them from the
  // functional run so a missing baseline isn't a hard failure here.
  testIgnore: '**/*.visual.spec.ts',
  // Parallel tests are OK - they use different documents, single sync server handles concurrency
  fullyParallel: true,
  // Fail on `test.only` in CI
  forbidOnly: !!process.env.CI,
  // Retries for flaky tests in CI
  retries: process.env.CI ? 2 : 0,
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
    baseURL: 'http://localhost:5173',
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

  // Run local dev server before tests
  webServer: {
    command: 'npm run dev',
    url: 'http://localhost:5173',
    // Reuse existing server in dev mode for faster iteration
    reuseExistingServer: !process.env.CI,
    // Timeout for server to start
    timeout: 120000,
    // Vite proxies /auth/* and the websocket to VITE_HUB_SERVER
    // (default http://localhost:3000). globalSetup starts the e2e hub
    // on port 3030, so point Vite at that port.
    env: {
      VITE_HUB_SERVER: 'http://localhost:3030',
    },
  },
});
