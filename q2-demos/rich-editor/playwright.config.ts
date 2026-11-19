import { mkdirSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { defineConfig, devices } from '@playwright/test'

/**
 * Browser tests for the visual editor. Each test puts a document on a sync
 * hub (started in the test worker; see `e2e/helpers.ts`) and opens it in
 * the app, served by the Vite dev server that Playwright starts. Build
 * the WASM module first (`npm run build:wasm`).
 */
// Another checkout's tests may be running at the same time, with a dev
// server of their own: `E2E_PORT` gives this run a different port.
const PORT = Number(process.env.E2E_PORT ?? 5192)

// On macOS, Firefox exits at launch ("Could not find profile folder") when it
// can't create `~/Library/Application Support/Firefox`, and some machines
// forbid that. `CFFIXED_USER_HOME` moves that home folder into a throwaway
// directory.
const firefoxHome = join(tmpdir(), 'rich-editor-e2e-firefox-home')
mkdirSync(firefoxHome, { recursive: true })

// `e2e/scratch/` holds throwaway specs (repros, probes); git ignores it. A run
// leaves them out unless its arguments name the folder, so a whole suite
// never runs (or trips over) another session's half-written probe. The
// runner records the choice in the environment, which its workers inherit
// when they load this config again.
if (process.argv.some((a) => a.includes('scratch'))) process.env.E2E_SCRATCH = '1'

export default defineConfig({
  testDir: './e2e',
  testIgnore: process.env.E2E_SCRATCH ? [] : ['scratch/**'],
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  reporter: process.env.CI ? 'github' : 'list',
  use: {
    baseURL: `http://localhost:${PORT}`,
    trace: 'retain-on-failure',
  },
  // Every spec runs in each browser engine: they differ most where an
  // editor lives (the caret, selections, `contenteditable`).
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    {
      name: 'firefox',
      use: {
        ...devices['Desktop Firefox'],
        launchOptions: { env: { ...process.env, CFFIXED_USER_HOME: firefoxHome } },
      },
    },
    { name: 'webkit', use: { ...devices['Desktop Safari'] } },
  ],
  webServer: {
    command: `npx vite --port ${PORT} --strictPort`,
    url: `http://localhost:${PORT}/`,
    reuseExistingServer: !process.env.CI,
  },
})
