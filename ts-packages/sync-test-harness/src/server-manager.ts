/**
 * Server lifecycle manager for integration tests.
 *
 * Starts and stops hub (Rust) and TS sync server child processes,
 * providing isolated data directories for each test.
 */

import { spawn, type ChildProcess } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';

export interface ServerHandle {
  /** WebSocket URL for sync clients */
  url: string;
  /** Path to the server's data directory */
  dataDir: string;
  /** Stop the server and clean up */
  stop(): Promise<void>;
}

interface StartOptions {
  port: number;
  /** If provided, use this directory instead of creating a temp one */
  dataDir?: string;
}

/** Root of the monorepo (two levels up from ts-packages/sync-test-harness/) */
const REPO_ROOT = path.resolve(import.meta.dirname, '..', '..', '..');

/**
 * Wait for a line matching `pattern` in the process's combined stdout/stderr.
 * Rejects after `timeoutMs`.
 */
function waitForOutput(
  proc: ChildProcess,
  pattern: RegExp,
  timeoutMs: number,
  label: string,
): Promise<void> {
  return new Promise((resolve, reject) => {
    let output = '';

    const timeout = setTimeout(() => {
      cleanup();
      reject(
        new Error(
          `Timeout (${timeoutMs}ms) waiting for ${label} to be ready.\nCaptured output:\n${output}`,
        ),
      );
    }, timeoutMs);

    const onData = (chunk: Buffer) => {
      const text = chunk.toString();
      output += text;
      // Log server output for debugging
      for (const line of text.split('\n')) {
        if (line.trim()) {
          console.log(`  [${label}] ${line}`);
        }
      }
      if (pattern.test(output)) {
        cleanup();
        resolve();
      }
    };

    const onExit = (code: number | null) => {
      cleanup();
      reject(new Error(`${label} exited with code ${code} before becoming ready.\nOutput:\n${output}`));
    };

    const cleanup = () => {
      clearTimeout(timeout);
      proc.stdout?.off('data', onData);
      proc.stderr?.off('data', onData);
      proc.off('exit', onExit);
    };

    proc.stdout?.on('data', onData);
    proc.stderr?.on('data', onData);
    proc.on('exit', onExit);
  });
}

async function makeTempDir(prefix: string): Promise<string> {
  return mkdtemp(path.join(tmpdir(), prefix));
}

/**
 * Start the Rust hub server in standalone sync mode.
 *
 * Uses `cargo run --bin hub` from the repo root.
 * Timeout is generous (120s) because the first run may need to compile.
 */
export async function startHubServer(options: StartOptions): Promise<ServerHandle> {
  const dataDir = options.dataDir ?? (await makeTempDir('hub-test-'));

  const proc = spawn(
    'cargo',
    [
      'run', '--bin', 'hub', '--',
      '--data-dir', dataDir,
      '--port', String(options.port),
    ],
    {
      cwd: REPO_ROOT,
      stdio: ['ignore', 'pipe', 'pipe'],
      env: {
        ...process.env,
        // Ensure tracing output goes to stderr (default), but we listen on both
        RUST_LOG: process.env.RUST_LOG ?? 'info',
      },
    },
  );

  await waitForOutput(proc, /Hub server listening/, 120_000, 'hub');

  return {
    url: `ws://127.0.0.1:${options.port}`,
    dataDir,
    async stop() {
      if (!proc.killed) {
        proc.kill('SIGTERM');
        // Wait for process to exit
        await new Promise<void>((resolve) => {
          const timeout = setTimeout(() => {
            proc.kill('SIGKILL');
            resolve();
          }, 5000);
          proc.on('exit', () => {
            clearTimeout(timeout);
            resolve();
          });
        });
      }
      // Clean up data directory
      await rm(dataDir, { recursive: true, force: true }).catch(() => {});
    },
  };
}

/**
 * Path to the TypeScript reference sync server. It lives in
 * `external-sources/`, which is NOT version-controlled — see the External
 * Sources Policy in CLAUDE.md. Tests that need it must skip when it is
 * absent rather than fail, so the suite is CI-able (GH #250).
 */
const TS_SYNC_SERVER_DIR = path.join(
  REPO_ROOT,
  'external-sources',
  'automerge-repo-sync-server',
);

/**
 * True when the TS reference sync server is checked out locally.
 *
 * Use with `describe.skipIf(!tsSyncServerAvailable())` — never assume it
 * is present. CI never has it.
 */
export function tsSyncServerAvailable(): boolean {
  return existsSync(path.join(TS_SYNC_SERVER_DIR, 'src', 'index.js'));
}

/**
 * Start the TypeScript automerge-repo-sync-server.
 *
 * Uses `node src/index.js` in the external-sources directory.
 */
export async function startTsSyncServer(options: StartOptions): Promise<ServerHandle> {
  const dataDir = options.dataDir ?? (await makeTempDir('ts-sync-test-'));

  const proc = spawn('node', ['src/index.js'], {
    cwd: TS_SYNC_SERVER_DIR,
    stdio: ['ignore', 'pipe', 'pipe'],
    env: {
      ...process.env,
      PORT: String(options.port),
      DATA_DIR: dataDir,
    },
  });

  await waitForOutput(proc, /Listening on port/, 30_000, 'ts-sync-server');

  return {
    url: `ws://127.0.0.1:${options.port}`,
    dataDir,
    async stop() {
      if (!proc.killed) {
        proc.kill('SIGTERM');
        await new Promise<void>((resolve) => {
          const timeout = setTimeout(() => {
            proc.kill('SIGKILL');
            resolve();
          }, 5000);
          proc.on('exit', () => {
            clearTimeout(timeout);
            resolve();
          });
        });
      }
      await rm(dataDir, { recursive: true, force: true }).catch(() => {});
    },
  };
}
