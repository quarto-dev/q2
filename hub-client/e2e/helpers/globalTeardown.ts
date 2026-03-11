/**
 * Playwright Global Teardown
 *
 * Runs once after all E2E tests:
 * 1. Stops the hub server
 * 2. Cleans up the server info file
 */

import { rmSync } from 'node:fs';
import { SERVER_INFO_PATH } from './globalSetup';
import type { HubServerHandle } from './syncServer';

export default async function globalTeardown() {
  console.log('\n--- E2E Global Teardown ---');

  // Stop hub server (handle stored by globalSetup in same process)
  const server = (globalThis as Record<string, unknown>).__E2E_HUB_SERVER__ as
    | HubServerHandle
    | undefined;
  if (server) {
    await server.stop();
    console.log('Hub server stopped');
  }

  // Clean up server info file
  try {
    rmSync(SERVER_INFO_PATH);
  } catch {
    // Ignore if already cleaned up
  }

  console.log('--- E2E Global Teardown Complete ---\n');
}
