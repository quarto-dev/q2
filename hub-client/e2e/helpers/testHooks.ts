/**
 * Type declarations for the in-browser test hooks installed by
 * `src/test-hooks.ts`. The hooks are registered on `window.__quartoTest`
 * only when the bundle is built with `VITE_E2E=1` (see playwright.config.ts).
 *
 * This file is for types only — the runtime accessor lives inside the
 * `page.evaluate(...)` callbacks because page.evaluate ships callback
 * source to the browser, where Node-side helpers aren't reachable.
 *
 * Always `await window.__quartoTestReady` before reading `window.__quartoTest`
 * — the hooks module loads asynchronously and the page's `load` event fires
 * before module top-level awaits resolve.
 *
 * Usage from a test/helper:
 *
 * ```ts
 * import type {} from './testHooks';  // ambient global augmentation
 *
 * await page.evaluate(async () => {
 *   await window.__quartoTestReady;
 *   const hooks = window.__quartoTest!;
 *   return hooks.projectStorage.addProject(...);
 * });
 * ```
 */
import type * as projectStorage from '../../src/services/projectStorage';
import type * as wasmRenderer from '../../src/services/wasmRenderer';

export interface QuartoTestHooks {
  projectStorage: typeof projectStorage;
  wasmRenderer: typeof wasmRenderer;
}

declare global {
  interface Window {
    __quartoTest?: QuartoTestHooks;
    __quartoTestReady?: Promise<unknown>;
  }
}
