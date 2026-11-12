import { defineConfig, devices } from '@playwright/test'

/**
 * Browser tests for the visual editor. They run against the hub-free
 * fixture page (`fixture.html`), so they need only the Vite dev server,
 * which Playwright starts. Build the WASM module first
 * (`npm run build:wasm`).
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
    url: `http://localhost:${PORT}/fixture.html`,
    reuseExistingServer: !process.env.CI,
  },
})
