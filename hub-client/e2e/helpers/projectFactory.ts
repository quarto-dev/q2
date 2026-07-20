/**
 * Helpers for creating Automerge projects and seeding them in the browser.
 *
 * - `createProjectOnServer()` runs in Node.js (Playwright test process)
 * - `seedProjectInBrowser()` runs in the browser via page.evaluate()
 *
 * Node-side polyfill: `createProjectOnServer` instantiates an
 * `IndexedDBStorageAdapter` indirectly via `createSyncClient`. The
 * Automerge IndexedDB adapter checks for the global `indexedDB`
 * eagerly at construction; in the Playwright Node controller process
 * that global is undefined. `fake-indexeddb/auto` patches the globals
 * (`indexedDB`, `IDBKeyRange`, etc.) into Node's `globalThis` so the
 * adapter can construct cleanly. The fake DB is in-memory and
 * per-process — fine for the controller-side document-creation step;
 * actual document persistence happens on the hub server (real DB).
 */

import 'fake-indexeddb/auto';
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import {
  createSyncClient,
  type SyncClientCallbacks,
  type FilePayload,
  type Patch,
} from '@quarto/quarto-sync-client';
import { SERVER_INFO_PATH } from './globalSetup';
import type { ServerInfo } from './globalSetup';
import { expect, type Page } from '@playwright/test';
import { DEFAULT_PREFERENCES } from '../../src/services/preferences/schema';
import type {} from './testHooks';

export interface ProjectFile {
  path: string;
  /** UTF-8 text for `'text'`, base64-encoded bytes for `'binary'`. */
  content: string;
  contentType: 'text' | 'binary';
  /** Required by `createNewProject` for binary files. */
  mimeType?: string;
}

/**
 * Read the hub server URL from the well-known file.
 */
export function getServerUrl(): string {
  const info: ServerInfo = JSON.parse(readFileSync(SERVER_INFO_PATH, 'utf-8'));
  return info.url;
}

/**
 * Create a new Automerge project on the hub server.
 *
 * Uses @quarto/quarto-sync-client in Node.js (same as sync-test-harness).
 * Returns the indexDocId needed for browser-side seeding.
 */
export async function createProjectOnServer(
  serverUrl: string,
  files: ProjectFile[],
): Promise<string> {
  const callbacks: SyncClientCallbacks = {
    onFileAdded(_path: string, _file: FilePayload) {},
    onFileChanged(_path: string, _text: string, _patches: Patch[]) {},
    onBinaryChanged(_path: string, _data: Uint8Array, _mimeType: string) {},
    onFileRemoved(_path: string) {},
    onFilesChange() {},
    onConnectionChange(_connected: boolean) {},
    onError(error: Error) {
      console.error('[createProjectOnServer] Error:', error.message);
    },
  };

  const client = createSyncClient(callbacks);
  const result = await client.createNewProject({
    syncServer: serverUrl,
    files,
    // Wait for the hub peer to connect before creating documents so they
    // flush synchronously in online mode. Without this, createNewProject
    // uses the default 1 ms timeout, falls into offline mode, and the
    // background WebSocket sync races against waitForServerDocuments —
    // a race that fails under parallel CI load (two workers syncing
    // simultaneously through the same hub). 10 s is ample for the
    // loopback connection (typically <100 ms).
    peerTimeoutMs: 10000,
  });

  // Wait for the server to acknowledge all documents (index + every file).
  // This replaces a fixed 2s sleep with an active readiness check.
  const httpUrl = serverUrl.replace(/^ws/, 'http');
  const allDocIds = [result.indexDocId, ...result.files.map((f) => f.docId)];
  await waitForServerDocuments(httpUrl, allDocIds);

  await client.disconnect();

  return result.indexDocId;
}

/**
 * Poll the hub server's HTTP API until it can find all given documents.
 * Replaces a fixed 2s sleep — typically resolves in <200ms.
 */
async function waitForServerDocuments(
  httpUrl: string,
  docIds: string[],
  timeoutMs: number = 10000,
  intervalMs: number = 50,
): Promise<void> {
  const pending = new Set(docIds);
  const deadline = Date.now() + timeoutMs;
  while (pending.size > 0 && Date.now() < deadline) {
    // Check all pending docs in parallel
    const checks = [...pending].map(async (docId) => {
      try {
        const res = await fetch(`${httpUrl}/api/documents/${docId}`);
        if (res.ok) pending.delete(docId);
      } catch {
        // Server not ready yet — keep trying
      }
    });
    await Promise.all(checks);
    if (pending.size > 0) {
      await new Promise((resolve) => setTimeout(resolve, intervalMs));
    }
  }
  if (pending.size > 0) {
    throw new Error(
      `Timed out waiting for server to acknowledge ${pending.size} document(s) after ${timeoutMs}ms`,
    );
  }
}

/**
 * Bring a fresh Playwright browser context to the project selector, ready for
 * {@link seedProjectInBrowser}. Must be called once per fresh context so the
 * Monaco/auth/preference stubs are installed before any project loads.
 *
 * Connection-gated local-first (bd-u4p8xhdc): there is no first-time-setup
 * screen anymore. `useProjectSet` auto-mints a local project set (empty
 * syncServer, no network) on a fresh browser and goes straight to `connected`,
 * so the selector renders immediately with no login and no server round-trip.
 *
 * The set being local is irrelevant to these tests: each project seeded by
 * {@link seedProjectInBrowser} keeps its own hub `syncServer` (its docs live
 * on the hub, created via {@link createProjectOnServer}), and
 * `projectSet.isConnected()` is peer-independent (`handle !== null`), so
 * seeding into the local set works exactly as it did into a synced one.
 */
// Monaco loads from CDN (cdn.jsdelivr.net) by default in the production build.
// Route those requests to local node_modules so tests work offline and in
// headless Playwright without CDN latency. Added to bootstrapProjectSet so
// every project-based test gets this automatically — no per-test call needed.
const MONACO_VS = resolve(import.meta.dirname, '../../node_modules/monaco-editor/min/vs');

/**
 * Stub /auth/me so the app's auth state settles immediately rather than
 * waiting for the hub (which returns 401 in no-auth test mode). Without this
 * stub, the delayed 401 from the hub keeps `authLoading` true slightly longer
 * under parallel load, widening a window where two browsers' Monaco
 * initialisation races can overlap and trigger an uncaught TypeError.
 */
async function mockAuthMe(page: Page): Promise<void> {
  await page.route('/auth/me', route =>
    route.fulfill({ status: 401, contentType: 'application/json', body: '{"error":"unauthorized"}' }),
  );
}

async function interceptMonacoCdn(page: Page): Promise<void> {
  await page.route(
    '**/cdn.jsdelivr.net/npm/monaco-editor@*/min/vs/**',
    async route => {
      const match = route.request().url().match(/monaco-editor@[^/]+\/min\/vs\/(.+)$/);
      if (match) {
        const local = resolve(MONACO_VS, match[1]);
        if (existsSync(local)) {
          await route.fulfill({ path: local });
          return;
        }
      }
      await route.continue();
    },
  );
}

export async function bootstrapProjectSet(
  page: Page,
  // Retained for a stable call signature (callers also pass this to
  // seedProjectInBrowser); the selector no longer takes a sync-server input.
  _syncServer: string,
): Promise<void> {
  await mockAuthMe(page);
  await interceptMonacoCdn(page);
  // Monaco 0.55+ requires MonacoEnvironment.getWorkerUrl. Without it the
  // workers AMD module throws and editor.main.js never finishes — Monaco
  // stays on "Loading..." indefinitely. This initScript is injected before
  // any page JS so the AMD loader sees it on first load.
  //
  // The URL is intercepted to local node_modules by interceptMonacoCdn.
  await page.addInitScript(() => {
    (window as unknown as { MonacoEnvironment: unknown }).MonacoEnvironment = {
      getWorkerUrl(_workerId: string, _label: string): string {
        return `https://cdn.jsdelivr.net/npm/monaco-editor@0.55.1/min/vs/assets/editor.worker-Be8ye1pW.js`;
      },
    };
  });
  // bd-038tnyqy: pin the rich-text editor OFF as the e2e baseline. The app now
  // defaults richText ON (bd-j1nto6eq), but the q2-preview editing specs were
  // written against the plain-textarea surface (they click a block and
  // `waitFor('textarea')`); with rich-text on they would open `.ProseMirror`
  // and time out. We MERGE `richText:false` rather than overwrite, so a spec's
  // own preference seed (set in its `beforeEach`, which runs before this script)
  // is preserved, and any future rich-text spec can opt IN by seeding
  // `richText:true` explicitly. No inline seed → write the full default object
  // (incl. `version`, required by the zod schema) with `richText:false`.
  await page.addInitScript((defaults) => {
    const KEY = 'quarto-hub:preferences';
    try {
      const raw = localStorage.getItem(KEY);
      if (raw === null) {
        localStorage.setItem(KEY, JSON.stringify({ ...defaults, richText: false }));
      } else {
        const cur = JSON.parse(raw);
        if (cur.richText === undefined) {
          localStorage.setItem(KEY, JSON.stringify({ ...cur, richText: false }));
        }
      }
    } catch {
      localStorage.setItem(KEY, JSON.stringify({ ...defaults, richText: false }));
    }
  }, DEFAULT_PREFERENCES);
  await page.goto('/');
  await expect(page.locator('body')).toBeVisible();

  // Wait for React to render before checking test hooks — the `body` becomes
  // visible before JS finishes executing, so checking window.__quartoTestReady
  // at that point is a race. The "Quarto Hub" heading is React-rendered, so
  // its presence proves JS has fully executed.
  await expect(
    page.getByRole('heading', { name: 'Quarto Hub' }),
  ).toBeVisible();

  // Fail fast with a clear error if the app was not built with VITE_E2E=1.
  // Without that flag the test hooks (window.__quartoTest) are tree-shaken
  // out of the bundle, and every subsequent page.evaluate call fails with the
  // cryptic "__quartoTest missing" message deep inside seedProjectInBrowser.
  const hasTestHooks = await page.evaluate(() => '__quartoTestReady' in window);
  if (!hasTestHooks) {
    throw new Error(
      '\n\nE2E test hooks not found (window.__quartoTestReady is absent).\n' +
      'The app must be built with VITE_E2E=1 before running Playwright tests:\n\n' +
      '  VITE_E2E=1 npm run build\n\n' +
      'Or use the full e2e command, which handles the build automatically:\n\n' +
      '  npm run test:e2e\n',
    );
  }
  // The app auto-mints a local project set and lands on the selector — no
  // setup screen to drive. Wait for the selector heading before seeding.
  await expect(
    page.getByRole('heading', { name: 'Your Projects' }),
  ).toBeVisible({ timeout: 20000 });
}

/**
 * Seed a project entry in the browser's IndexedDB so the app can load it.
 *
 * Call {@link bootstrapProjectSet} once per browser context first so the
 * project set is initialized and the app has settled on the selector.
 *
 * Returns the local project ID (UUID) used in URL navigation.
 */
export async function seedProjectInBrowser(
  page: Page,
  indexDocId: string,
  syncServer: string,
  name: string = 'E2E Test Project',
): Promise<string> {
  return page.evaluate(
    async ({ indexDocId, syncServer, name }) => {
      await window.__quartoTestReady;
      const hooks = window.__quartoTest;
      if (!hooks) throw new Error('__quartoTest missing — rebuild with VITE_E2E=1');
      const entry = await hooks.projectStorage.addProject(indexDocId, syncServer, name);

      const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
      const deadline = Date.now() + 30000;

      // 1) Wait for the real project-set peer connection (the app's implicit
      //    5s waitForPeer is too tight in CI; give it a generous window).
      while (!hooks.projectSet.isConnected() && Date.now() < deadline) {
        await sleep(100);
      }
      if (!hooks.projectSet.isConnected()) {
        throw new Error(
          'Project set did not reach connected state within 30s — sync server unreachable?',
        );
      }

      // 2) Land the seeded IDB entry into the synced set (idempotent), then
      //    wait until it is observably present before the caller navigates.
      while (!hooks.projectSet.getProject(indexDocId) && Date.now() < deadline) {
        await hooks.reconcileProjectSet();
        if (hooks.projectSet.getProject(indexDocId)) break;
        await sleep(100);
      }
      if (!hooks.projectSet.getProject(indexDocId)) {
        throw new Error(
          `Seeded project ${indexDocId} never appeared in the connected project set within 30s`,
        );
      }

      return entry.id;
    },
    { indexDocId, syncServer, name },
  );
}
