/**
 * Helpers for creating Automerge projects and seeding them in the browser.
 *
 * - `createProjectOnServer()` runs in Node.js (Playwright test process)
 * - `seedProjectInBrowser()` runs in the browser via page.evaluate()
 */

import { readFileSync } from 'node:fs';
import {
  createSyncClient,
  type SyncClientCallbacks,
  type FilePayload,
  type Patch,
} from '@quarto/quarto-sync-client';
import { SERVER_INFO_PATH } from './globalSetup';
import type { ServerInfo } from './globalSetup';
import type { Page } from '@playwright/test';

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

  // Wait for server to persist the project
  await new Promise((resolve) => setTimeout(resolve, 2000));

  await client.disconnect();

  return result.indexDocId;
}

/**
 * Seed a project entry in the browser's IndexedDB so the app can load it.
 *
 * Must be called after page.goto('/') so Vite modules are available.
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
      const ps = await import('/src/services/projectStorage.ts');
      const entry = await ps.addProject(indexDocId, syncServer, name);
      return entry.id;
    },
    { indexDocId, syncServer, name },
  );
}
