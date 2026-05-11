/**
 * Helpers for creating Automerge projects and seeding them in the browser.
 *
 * - `createProjectOnServer()` runs in Node.js (Playwright test process)
 * - `seedProjectInBrowser()` runs in the browser via page.evaluate()
 */

// createSyncClient instantiates IndexedDBStorageAdapter unconditionally;
// shim indexedDB in Node so the helper can run outside the browser.
import 'fake-indexeddb/auto';

import { readFileSync } from 'node:fs';
import {
  createSyncClient,
  type SyncClientCallbacks,
  type FilePayload,
  type Patch,
} from '@quarto/quarto-sync-client';
import { SERVER_INFO_PATH } from './globalSetup';
import type { ServerInfo } from './globalSetup';
import { expect, type Page } from '@playwright/test';
import type {} from './testHooks';

export interface ProjectFile {
  path: string;
  content: string;
  contentType: 'text' | 'binary';
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
 * Drive the first-time-setup UI to create a synced project set on the running
 * hub server and store its pointer in the browser's IDB. Must be called once
 * per fresh Playwright browser context before {@link seedProjectInBrowser},
 * otherwise the legacy project entry triggers the "Upgrade: Synced Project
 * List" migration screen and blocks all navigation.
 *
 * Modeled after the share-link spec's `bootstrapReceiverProjectSet` — driving
 * the UI lets the app's own `useProjectSet` race-free state machine put us in
 * `connected` status rather than racing a hand-rolled createProjectSet call
 * against the migration check.
 */
export async function bootstrapProjectSet(
  page: Page,
  syncServer: string,
): Promise<void> {
  await page.goto('/');
  await expect(page.locator('body')).toBeVisible();

  await expect(
    page.getByRole('heading', { name: 'Quarto Hub' }),
  ).toBeVisible();
  await expect(
    page.getByText(/Get started by creating a new project set/i),
  ).toBeVisible();

  await page.locator('#setup-sync-server').fill(syncServer);
  await page
    .getByRole('button', { name: /Create New Project Set/i })
    .click();

  await expect(
    page.getByRole('heading', { name: 'Your Projects' }),
  ).toBeVisible({ timeout: 20000 });
}

/**
 * Seed a project entry in the browser's IndexedDB so the app can load it.
 *
 * Call {@link bootstrapProjectSet} once per browser context first so the
 * synced project set is initialized; otherwise the App lands on the
 * needs-migration screen.
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
      return entry.id;
    },
    { indexDocId, syncServer, name },
  );
}
