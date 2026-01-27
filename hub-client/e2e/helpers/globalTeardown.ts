/**
 * Playwright Global Teardown
 *
 * Runs once after all E2E tests:
 * 1. Stops the sync server
 * 2. Cleans up the temp fixtures directory
 */

import { cleanupTestFixtures } from './fixtureSetup';

interface SyncServer {
  close: () => Promise<void>;
}

export default async function globalTeardown() {
  console.log('\n--- E2E Global Teardown ---');

  // Stop sync server
  const server = (globalThis as Record<string, unknown>).__E2E_SYNC_SERVER__ as
    | SyncServer
    | undefined;
  if (server) {
    await server.close();
    console.log('Sync server stopped');
  }

  // Clean up temp fixtures
  const tempDir = (globalThis as Record<string, unknown>).__E2E_FIXTURE_DIR__ as
    | string
    | undefined;
  if (tempDir) {
    cleanupTestFixtures(tempDir);
    console.log(`Temp fixtures cleaned up: ${tempDir}`);
  }

  console.log('--- E2E Global Teardown Complete ---\n');
}
