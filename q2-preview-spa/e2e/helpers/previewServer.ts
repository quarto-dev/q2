/**
 * Lifecycle for the `q2 preview` binary under Playwright E2E.
 *
 * Each test run spins up a fresh `q2 preview` child against a fresh
 * temp project (the fixture content is supplied by the caller) so
 * tests don't share automerge state. The server's stdout is parsed
 * to recover the launch URL — `q2 preview` prints `→ http://host:port/`
 * after binding (see `crates/quarto/src/commands/preview.rs`).
 *
 * Mirrors `hub-client/e2e/helpers/syncServer.ts` but uses the
 * pre-built `target/debug/q2` binary (set by globalSetup) so each
 * test run doesn't pay the cargo-compile cost.
 */

import { spawn, type ChildProcess } from 'node:child_process';
import { mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';

export interface PreviewServerHandle {
  /** Base URL, e.g. `http://127.0.0.1:18557/`. Has trailing slash. */
  url: string;
  /** Port the server is listening on. */
  port: number;
  /** Path to the project root that the server is serving. */
  projectDir: string;
  /** Path to the ephemeral samod data directory. */
  dataDir: string;
  /** Stop the server and remove its temp dirs. */
  stop(): Promise<void>;
}

/** Repo root, relative to this file (`q2-preview-spa/e2e/helpers/`). */
const REPO_ROOT = path.resolve(import.meta.dirname, '..', '..', '..');

/** Pre-built debug binary the global-setup compiled (or expects). */
const Q2_BINARY = path.join(REPO_ROOT, 'target', 'debug', 'q2');

function waitForUrl(
  proc: ChildProcess,
  timeoutMs: number,
): Promise<{ url: string; port: number }> {
  return new Promise((resolve, reject) => {
    let buffer = '';

    const timeout = setTimeout(() => {
      cleanup();
      reject(
        new Error(
          `Timeout (${timeoutMs}ms) waiting for q2 preview to print its URL.\n` +
            `Captured output:\n${buffer}`,
        ),
      );
    }, timeoutMs);

    const onData = (chunk: Buffer) => {
      const text = chunk.toString();
      buffer += text;
      // The CLI prints `→ http://127.0.0.1:<port>/` after binding.
      const match = buffer.match(/→\s+(http:\/\/[^\s]+)/);
      if (match) {
        const url = match[1].endsWith('/') ? match[1] : `${match[1]}/`;
        const portMatch = url.match(/:(\d+)\//);
        if (!portMatch) {
          cleanup();
          reject(new Error(`Couldn't parse port from URL: ${url}`));
          return;
        }
        cleanup();
        resolve({ url, port: parseInt(portMatch[1], 10) });
      }
    };

    const onExit = (code: number | null) => {
      cleanup();
      reject(
        new Error(
          `q2 preview exited with code ${code} before printing its URL.\n` +
            `Captured output:\n${buffer}`,
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

export interface StartOptions {
  /** Single .qmd file to seed the project with. */
  fixtureQmd: { path: string; content: string };
}

/**
 * Start the `q2 preview` binary against a fresh tempdir project.
 *
 * The project gets one .qmd at `fixtureQmd.path` with `fixtureQmd.content`.
 * The samod data dir lives in a separate tempdir so the test can
 * inspect the project dir without picking up engine artifacts.
 */
export async function startPreviewServer(opts: StartOptions): Promise<PreviewServerHandle> {
  const projectDir = await mkdtemp(path.join(tmpdir(), 'q2-preview-e2e-proj-'));
  const dataDir = await mkdtemp(path.join(tmpdir(), 'q2-preview-e2e-data-'));

  await writeFile(
    path.join(projectDir, opts.fixtureQmd.path),
    opts.fixtureQmd.content,
  );

  const proc = spawn(
    Q2_BINARY,
    ['preview', '--no-browser', '--data-dir', dataDir, projectDir],
    {
      stdio: ['ignore', 'pipe', 'pipe'],
      env: {
        ...process.env,
        // Keep the server's log signal-to-noise low; the only thing
        // we parse out of stdout is the launch URL itself.
        RUST_LOG: process.env.RUST_LOG ?? 'warn',
      },
    },
  );

  let url: string;
  let port: number;
  try {
    ({ url, port } = await waitForUrl(proc, 30_000));
  } catch (err) {
    proc.kill('SIGTERM');
    throw err;
  }

  return {
    url,
    port,
    projectDir,
    dataDir,
    async stop() {
      if (!proc.killed) {
        proc.kill('SIGTERM');
        await new Promise<void>((resolve) => {
          const t = setTimeout(() => {
            proc.kill('SIGKILL');
            resolve();
          }, 5000);
          proc.once('exit', () => {
            clearTimeout(t);
            resolve();
          });
        });
      }
      // Best-effort cleanup of the temp dirs. Failures are non-fatal —
      // the OS cleans /tmp eventually.
      const { rm } = await import('node:fs/promises');
      await Promise.allSettled([
        rm(projectDir, { recursive: true, force: true }),
        rm(dataDir, { recursive: true, force: true }),
      ]);
    },
  };
}
