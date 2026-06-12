import { defineConfig } from '@playwright/test';
import baseConfig from './playwright.config';

/**
 * Playwright configuration for the smoke-all E2E run (nightly + opt-in).
 *
 * ## Why this is a separate config
 *
 * The base config (`playwright.config.ts`) lists `**​/smoke-all.spec.ts` in
 * `testIgnore` so the per-push/PR gate runs only the fast custom specs. A
 * positional path argument on the CLI (`npx playwright test e2e/smoke-all.spec.ts`)
 * is applied as a *filter* on the candidate set that survives `testIgnore` — it
 * cannot un-ignore a file. So invoking smoke-all by path against the base config
 * matches zero tests and fails with "No tests found" (this is exactly how the
 * first nightly run failed; see the `hub-client-e2e` workflow's smoke-all step).
 *
 * Mirroring the `playwright.visual.config.ts` pattern, this config selects the
 * smoke-all spec via `testMatch` instead of trying to override `testIgnore` on
 * the CLI. Everything else — the hub `globalSetup`/`globalTeardown`, the
 * `vite preview` web server, retries, workers, and the chromium project — is
 * inherited from the base config, since smoke-all exercises the same
 * Automerge → VFS → WASM → preview pipeline.
 */
export default defineConfig({
  ...baseConfig,
  // Replace the base `testIgnore` (which excludes smoke-all) with an explicit
  // match so only the smoke-all spec runs under this config.
  testIgnore: undefined,
  testMatch: '**/smoke-all.spec.ts',
  // Run smoke-all serially. The base config uses 2 workers on CI, but the
  // WASM render pipeline stalls under contention on a 2-core runner, which is
  // the dominant source of the 75s-timeout flakiness in this suite. A single
  // worker trades wall-clock for stability — acceptable for a nightly run.
  workers: 1,
});
