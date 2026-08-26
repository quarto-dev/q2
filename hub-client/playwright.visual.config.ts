import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright configuration for visual regression tests.
 *
 * These tests render components via dev harness routes (#/dev/...)
 * and capture screenshots for pixel-diff regression testing.
 * No hub server or network access is needed.
 */
export default defineConfig({
  testDir: './e2e',
  testMatch: '**/*.visual.spec.ts',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  // One retry absorbs infrastructure flakes (dev-server contention under
  // full parallelism occasionally destroys the execution context mid-boot).
  // Assertion failures — pixel diffs, axe drift — are deterministic and
  // still fail through the retry.
  retries: 1,
  workers: process.env.CI ? 2 : undefined,
  reporter: 'html',

  // No globalSetup/globalTeardown — visual tests don't need the hub server

  use: {
    baseURL: 'http://localhost:5173',
    // Deterministic viewport for consistent screenshots
    viewport: { width: 1280, height: 720 },
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
  },
});
