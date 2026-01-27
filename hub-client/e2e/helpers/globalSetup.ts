/**
 * Playwright Global Setup
 *
 * Runs once before all E2E tests:
 * 1. Copies fixtures to a temp directory
 * 2. Starts the local sync server
 * 3. Stores references for tests and teardown
 */

import { setupTestFixtures, fixturesExist } from './fixtureSetup';
import { startSyncServer } from './syncServer';

export default async function globalSetup() {
  console.log('\n--- E2E Global Setup ---');

  // Copy fixtures to temp directory
  const tempDir = setupTestFixtures();
  console.log(`Fixtures copied to: ${tempDir}`);

  if (!fixturesExist()) {
    console.log(
      'Note: No pre-generated fixtures found. Tests will create fresh documents.',
    );
  }

  // Store temp dir for tests and teardown
  process.env.E2E_FIXTURE_DIR = tempDir;

  // Start single sync server for all tests
  const server = await startSyncServer({
    port: 3030,
    storageDir: `${tempDir}/automerge-data`,
  });
  console.log(`Sync server URL: ${server.url}`);

  // Store server URL for tests
  process.env.E2E_SYNC_SERVER_URL = server.url;

  // Store server reference for teardown (using globalThis for cross-module access)
  (globalThis as Record<string, unknown>).__E2E_SYNC_SERVER__ = server;
  (globalThis as Record<string, unknown>).__E2E_FIXTURE_DIR__ = tempDir;

  console.log('--- E2E Global Setup Complete ---\n');
}
