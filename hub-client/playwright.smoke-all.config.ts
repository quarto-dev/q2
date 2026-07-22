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
  // Parallel workers for smoke-all. This suite historically ran at `workers: 1`
  // because the 75s-timeout flakiness was blamed on the WASM render pipeline
  // stalling under CPU contention. That diagnosis was wrong: the public-repo
  // `ubuntu-latest` runner has 4 vCPUs (not 2), and the real cause was
  // server-side sync contention, fixed by the samod-0.12 hub upgrade (PR #355,
  // 2026-07-03). Since that landed the nightly has been clean, and dispatch
  // stress runs confirmed 78/78 with zero flaky at both 3 and 4 workers
  // (~3.4m vs the serial ~4.8m). We restore parallelism at 3, which reserves
  // one core for the co-resident hub + vite-preview + node processes.
  //
  // `SMOKE_ALL_WORKERS` overrides the count for a workflow_dispatch run (e.g.
  // to re-stress at 4 if flakiness ever returns).
  workers: process.env.SMOKE_ALL_WORKERS
    ? parseInt(process.env.SMOKE_ALL_WORKERS, 10)
    : 3,
});
