/**
 * Playwright Global Setup
 *
 * Runs once before all E2E tests:
 * 1. Starts the Rust hub server
 * 2. Writes server info to a well-known file for test workers to read
 *
 * Note: globalSetup runs in a separate process from test workers.
 * env vars and globalThis are NOT shared. We use a file to communicate.
 */

import { writeFileSync } from 'node:fs';
import { startHubServer } from './syncServer';

/** Well-known path where server info is written for test workers */
export const SERVER_INFO_PATH = '/tmp/hub-e2e-server.json';

export interface ServerInfo {
  url: string;
  port: number;
  dataDir: string;
  pid: number;
}

const HUB_PORT = 3030;

export default async function globalSetup() {
  console.log('\n--- E2E Global Setup ---');

  const server = await startHubServer(HUB_PORT);
  console.log(`Hub server started: ${server.url}`);

  // Write server info to file for test workers and teardown
  const info: ServerInfo = {
    url: server.url,
    port: server.port,
    dataDir: server.dataDir,
    pid: 0, // We don't expose pid from the handle, but stop() handles cleanup
  };
  writeFileSync(SERVER_INFO_PATH, JSON.stringify(info));

  // Store server handle for teardown (globalThis IS shared with globalTeardown
  // since they run in the same process)
  (globalThis as Record<string, unknown>).__E2E_HUB_SERVER__ = server;

  console.log('--- E2E Global Setup Complete ---\n');
}
