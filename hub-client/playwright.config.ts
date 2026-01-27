import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright configuration for hub-client E2E tests
 *
 * Test architecture:
 * - Uses checked-in Automerge fixtures for reproducible tests
 * - Single sync server serves all tests (started in globalSetup)
 * - Tests run in parallel against the same server (different documents)
 * - Fixtures are copied to temp directory to avoid mutations
 */
export default defineConfig({
  testDir: './e2e',
  // Parallel tests are OK - they use different documents, single sync server handles concurrency
  fullyParallel: true,
  // Fail on `test.only` in CI
  forbidOnly: !!process.env.CI,
  // Retries for flaky tests in CI
  retries: process.env.CI ? 2 : 0,
  // Parallel workers
  workers: process.env.CI ? 4 : undefined,
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
  },
});
