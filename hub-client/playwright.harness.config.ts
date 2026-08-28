import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright configuration for dev-harness behavioral tests.
 *
 * These tests render components via dev harness routes (#/dev/...) and
 * assert behavior: keyboard interaction contracts, dialog focus
 * management, axe scans, forced-colors boundaries, reduced-motion
 * collapse, and narrow-viewport reflow. No hub server or network access
 * is needed.
 *
 * The pixel-diff screenshot layer that used to share this setup was
 * dropped while Phase 5 visual churn is ongoing (bd-8g1bn8a0); re-adding
 * it is tracked as bd-wlubinvq, and the specs and baselines are
 * recoverable from git history.
 */
export default defineConfig({
  testDir: './e2e',
  testMatch: '**/*.harness.spec.ts',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  // One retry absorbs infrastructure flakes (dev-server contention under
  // full parallelism occasionally destroys the execution context mid-boot).
  // Assertion failures are deterministic and still fail through the retry.
  retries: 1,
  workers: process.env.CI ? 2 : undefined,
  reporter: 'html',

  // No globalSetup/globalTeardown — harness tests don't need the hub server

  use: {
    baseURL: 'http://localhost:5173',
    // Fixed viewport for consistent layout assertions
    viewport: { width: 1280, height: 720 },
    // NOTE: the config-level `reducedMotion` option is silently ignored
    // by Playwright 1.60's default context (verified: newContext honors
    // it, project use does not), so reduced-motion emulation lives in
    // e2e/helpers/harness.ts instead.
  },

  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],

  webServer: {
    command: 'npm run dev',
    url: 'http://localhost:5173',
    reuseExistingServer: !process.env.CI,
    timeout: 120000,
    env: {
      // No hub runs during harness tests. Without this, vite dev proxies
      // /auth and /ws at the default target (localhost:3000) and every
      // app boot's auth keepalive probes log ECONNREFUSED (bd-a1cwdir9).
      VITE_DISABLE_HUB_PROXY: '1',
    },
  },
});
