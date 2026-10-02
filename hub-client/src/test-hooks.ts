/**
 * E2E test hooks: expose a couple of internal services on `window` so the
 * Playwright suite can bypass UI to seed projects (`projectStorage`) and
 * render qmd directly (`wasmRenderer`).
 *
 * Why this exists: the E2E suite used to reach into the source tree via
 * `await import('/src/services/...ts')`, which only works under `vite dev`.
 * The CI run uses `vite preview` (production bundle) for throughput; this
 * module is the bridge so the same tests work against the prod bundle.
 *
 * Inclusion is gated on `import.meta.env.VITE_E2E === '1'` at build time
 * (see `src/main.tsx`). Without that flag, vite tree-shakes this module
 * out of the production bundle entirely.
 */
import * as projectStorage from './services/projectStorage';
import * as projectSet from './services/projectSetService';
import { reconcileIntoConnectedProjectSet } from './services/projectSetReconciler';
import * as wasmRenderer from '@quarto/preview-runtime';
import { PandocLoader, type LoaderConfig } from './pandoc/pandocLoader';
import { PandocRunner, uiStateFor } from './pandoc/pandocRunner';
import { createBrowserWorker } from './pandoc/pandocService';
import { PANDOC_WASM_SHA256, smokeJob } from './pandoc/smokeJob';
import { installDevHarness, pandocDownload } from './pandoc/devHarness';

/**
 * Pandoc loader/worker hook for the `pandoc-*.harness.spec.ts` smoke tests. The runner it
 * builds is the production one; the options exist so a test can shorten the wall timeout
 * (the hang case would otherwise take 120 s) and observe the loader.
 */
export const pandoc = {
  createRunner(options: { wallTimeoutMs?: number; idleMs?: number; loader?: LoaderConfig } = {}) {
    const loader = new PandocLoader({ idleMs: options.idleMs, ...options.loader });
    const runner = new PandocRunner({ loader, createWorker: createBrowserWorker, wallTimeoutMs: options.wallTimeoutMs });
    return { loader, runner };
  },
  smokeJob,
  /** The dev harness: Rust request -> worker -> output (src/pandoc/devHarness.ts). */
  download: pandocDownload,
  uiStateFor,
  sha256: PANDOC_WASM_SHA256,
  /**
   * Byte length of the main thread's Rust wasm linear memory (the VFS, request builds and
   * their copies live there). It only ever grows, so it is the high-water mark of the page
   * (host phase H6 memory test). `default()` returns the cached exports once initialised.
   */
  async rustWasmMemoryBytes(): Promise<number> {
    const wasm = await import('wasm-quarto-hub-client');
    const exports = (await wasm.default()) as unknown as { memory: WebAssembly.Memory };
    return exports.memory.buffer.byteLength;
  },
};

declare global {
  interface Window {
    __quartoTest?: {
      projectStorage: typeof projectStorage;
      // The live project-set service singleton (same instance the app uses),
      // so the E2E suite can observe real connection/sync state — e.g. wait
      // for `isConnected()` and `getProject(indexDocId)` after seeding before
      // navigating, instead of racing the implicit reconcile-on-connect.
      projectSet: typeof projectSet;
      // Idempotent IDB→synced-set reconciler. The app runs this only on the
      // status→connected transition, which does not re-fire for a project
      // seeded after the set is already connected; the suite invokes it
      // explicitly so a seeded project deterministically lands in the set.
      reconcileProjectSet: typeof reconcileIntoConnectedProjectSet;
      wasmRenderer: typeof wasmRenderer;
      pandoc: typeof pandoc;
    };
  }
}

installDevHarness();

window.__quartoTest = {
  projectStorage,
  projectSet,
  reconcileProjectSet: reconcileIntoConnectedProjectSet,
  wasmRenderer,
  pandoc,
};
