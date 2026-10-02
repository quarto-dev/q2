import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright configuration for q2-preview-spa E2E.
 *
 * The `quarto preview` binary is the SUT. Tests spawn their own
 * server instance via `e2e/helpers/previewServer.ts` so each test
 * has an isolated tempdir project + fresh samod state. globalSetup
 * just sanity-checks that the binary has been built.
 *
 * Run with `npm run test:e2e` from `q2-preview-spa/`; CI integration
 * goes through `cargo xtask verify --e2e`.
 */
export default defineConfig({
  testDir: './e2e',
  // Tests spin up their own server each — parallelism is fine but
  // bounded by the WASM-render cost of each. Conservative default.
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 2 : undefined,
  reporter: process.env.CI ? 'github' : 'list',
  globalSetup: './e2e/helpers/globalSetup.ts',
  // Per-test server is spawned inside each test; no baseURL.
  use: {
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    video: 'on-first-retry',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
    {
      // Whole suite on Firefox. The ws-queue spec is excluded: it has its
      // own project below, with the open-timeout pref it needs.
      name: 'firefox',
      testIgnore: /firefox-ws-queue\.spec\.ts/,
      use: { ...devices['Desktop Firefox'] },
    },
    {
      // bd-jit6pdwq Phase 4: Firefox-only WS handshake-queue
      // regression. Firefox serializes WebSocket opening handshakes
      // per IP address browser-wide; this project pins the
      // open-timeout to 8 s — it must EXCEED the legacy 5 s peer
      // budget (or the pre-fix bug can't reproduce) while staying far
      // under the 20 s default so the spec runs fast. Only the
      // matching spec runs here; the chromium project above runs the
      // same spec as the unserialized control.
      name: 'firefox-ws-queue',
      testMatch: /firefox-ws-queue\.spec\.ts/,
      use: {
        ...devices['Desktop Firefox'],
        launchOptions: {
          firefoxUserPrefs: {
            'network.websocket.timeout.open': 8,
          },
        },
      },
    },
  ],
});
