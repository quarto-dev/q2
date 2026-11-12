import { defineConfig, devices } from '@playwright/test'

/**
 * Browser tests for the visual editor. Each test puts a document on a sync
 * hub (started in the test worker; see `e2e/helpers.ts`) and opens it in
 * the app, served by the Vite dev server that Playwright starts. Build
 * the WASM module first (`npm run build:wasm`).
 */
const PORT = 5192

export default defineConfig({
  testDir: './e2e',
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  reporter: process.env.CI ? 'github' : 'list',
  use: {
    baseURL: `http://localhost:${PORT}`,
    trace: 'retain-on-failure',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  webServer: {
    command: `npx vite --port ${PORT} --strictPort`,
    url: `http://localhost:${PORT}/`,
    reuseExistingServer: !process.env.CI,
  },
})
