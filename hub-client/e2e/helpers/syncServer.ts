/**
 * E2E Test Sync Server Helpers
 *
 * Manages the lifecycle of a local Automerge sync server for E2E tests.
 *
 * Note: The actual sync server implementation depends on having
 * @automerge/automerge-repo-sync-server available. This file provides
 * the interface and placeholder implementation.
 *
 * TODO: Implement actual sync server integration in Phase 3
 */

import { ChildProcess, spawn } from 'child_process';
import { join } from 'path';

export interface SyncServerOptions {
  port: number;
  storageDir: string;
}

export interface SyncServer {
  url: string;
  port: number;
  close: () => Promise<void>;
}

// Track the sync server process
let serverProcess: ChildProcess | null = null;

/**
 * Start a local Automerge sync server.
 *
 * For now, this is a placeholder that expects an external sync server.
 * In Phase 3, this will spawn the actual sync server process.
 *
 * @param options Server configuration
 * @returns Server handle with close() method
 */
export async function startSyncServer(options: SyncServerOptions): Promise<SyncServer> {
  const { port, storageDir } = options;

  // For now, check if there's an environment variable pointing to an existing server
  const existingServerUrl = process.env.E2E_SYNC_SERVER_URL;
  if (existingServerUrl) {
    console.log(`Using existing sync server at ${existingServerUrl}`);
    return {
      url: existingServerUrl,
      port,
      close: async () => {
        // External server - don't close it
      },
    };
  }

  // TODO: Phase 3 - Implement actual sync server spawning
  // For now, we'll use a mock implementation that doesn't require the sync server
  console.log(`Note: Sync server not implemented yet. Tests will run in offline mode.`);
  console.log(`Storage directory: ${storageDir}`);

  return {
    url: `ws://localhost:${port}`,
    port,
    close: async () => {
      if (serverProcess) {
        serverProcess.kill();
        serverProcess = null;
      }
    },
  };
}

/**
 * Wait for the sync server to be ready.
 *
 * @param url Server URL to check
 * @param timeoutMs Maximum time to wait
 */
export async function waitForServer(url: string, timeoutMs: number = 10000): Promise<void> {
  const startTime = Date.now();

  while (Date.now() - startTime < timeoutMs) {
    try {
      // Try to establish a WebSocket connection
      const ws = await new Promise<boolean>((resolve, reject) => {
        // In Node.js, we'd use the 'ws' package here
        // For now, just assume the server is ready
        resolve(true);
      });

      if (ws) return;
    } catch {
      // Server not ready yet, wait and retry
      await new Promise((r) => setTimeout(r, 100));
    }
  }

  throw new Error(`Sync server at ${url} did not become ready within ${timeoutMs}ms`);
}
