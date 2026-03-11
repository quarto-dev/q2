/**
 * Hub server lifecycle for E2E tests.
 *
 * Starts the Rust hub binary as a child process with a temp data directory.
 * Adapted from ts-packages/sync-test-harness/src/server-manager.ts.
 */

import { spawn, type ChildProcess } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';

export interface HubServerHandle {
  /** WebSocket URL for sync clients */
  url: string;
  /** Port the server is listening on */
  port: number;
  /** Path to the server's data directory */
  dataDir: string;
  /** Stop the server and clean up */
  stop(): Promise<void>;
}

/** Root of the monorepo (hub-client/e2e/helpers/ → repo root) */
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
      reject(
        new Error(
          `${label} exited with code ${code} before becoming ready.\nOutput:\n${output}`,
        ),
      );
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

/**
 * Start the Rust hub server for E2E tests.
 *
 * Uses `cargo run --bin hub` from the repo root.
 * Timeout is generous (120s) because the first run may need to compile.
 */
export async function startHubServer(port: number): Promise<HubServerHandle> {
  const dataDir = await mkdtemp(path.join(tmpdir(), 'hub-e2e-'));

  const proc = spawn(
    'cargo',
    ['run', '--bin', 'hub', '--', '--data-dir', dataDir, '--port', String(port)],
    {
      cwd: REPO_ROOT,
      stdio: ['ignore', 'pipe', 'pipe'],
      env: {
        ...process.env,
        RUST_LOG: process.env.RUST_LOG ?? 'info',
      },
    },
  );

  await waitForOutput(proc, /Hub server listening/, 120_000, 'hub');

  return {
    url: `ws://127.0.0.1:${port}`,
    port,
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
