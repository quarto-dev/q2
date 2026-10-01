/**
 * Playwright requires globalSetup and globalTeardown to each be the
 * default export of their module; the lifecycle lives with the setup
 * code in ./setup.ts.
 */
export { globalTeardown as default } from './setup';
