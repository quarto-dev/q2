import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright configuration for the author-attribution E2E
 * (e2e-author/author-attribution.spec.ts).
 *
 * ## Prerequisites
 *
 * 1. **Production build with test hooks and a relative sync-server URL**:
 *    ```
 *    VITE_E2E=1 VITE_DEFAULT_SYNC_SERVER=/ws npm run build
 *    ```
 *    (same build the default e2e suite uses; `npm run test:e2e:author-id`
 *    performs it). The relative `/ws` lets the served client reach the hub
 *    through whatever origin served it — this suite's static proxies.
 *
 * 2. **hub binary buildable**: globalSetup launches two hubs via
 *    `cargo run --bin hub` (one auth-on with a mock OIDC IdP, one
 *    auth-disabled) with a 120s readiness timeout; CI pre-builds.
 *
 * ## Why a separate config
 *
 * The default suite's globalSetup boots a single auth-disabled hub on 3031
 * and serves the client via `vite preview`. These tests need an
 * authenticated hub (mock OIDC) plus same-origin static proxies so session
 * cookies flow on both HTTP and the WS upgrade — a different lifecycle with
 * different ports, so a different config with its own globalSetup and no
 * webServer.
 *
 * Workers are pinned to 1: the scenarios share the two hubs and assert on
 * global state (change-level attribution), and each test creates its own
 * project, so there is no parallelism win worth the coordination.
 */
export default defineConfig({
  testDir: './e2e-author',
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 1,
  workers: 1,
  reporter: 'list',

  globalSetup: './e2e-author/setup.ts',
  globalTeardown: './e2e-author/teardown.ts',

  use: {
    baseURL: 'http://127.0.0.1:3999',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    video: 'on-first-retry',
  },

  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
});
