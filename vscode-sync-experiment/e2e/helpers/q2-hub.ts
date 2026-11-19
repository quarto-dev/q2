/** A real `q2 preview --allow-edit` hub on loopback, for the Join tests (GE-11). Needs `cargo build --bin q2`. */
import { spawn, type ChildProcess } from 'node:child_process';
import * as net from 'node:net';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, '..', '..', '..');
export const Q2_BINARY = process.env.Q2_BINARY ?? path.join(repoRoot, 'target', process.env.Q2_PROFILE ?? 'debug', process.platform === 'win32' ? 'q2.exe' : 'q2');

export interface Q2Hub {
  port: number;
  /** `http://127.0.0.1:<port>` */
  httpUrl: string;
  wsUrl: string;
  indexDocId: string;
  projectRoot: string;
  log(): string;
  stop(): Promise<void>;
}

function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.once('error', reject);
    srv.listen(0, '127.0.0.1', () => {
      const { port } = srv.address() as net.AddressInfo;
      srv.close(() => resolve(port));
    });
  });
}

export async function startQ2Hub(dir: string): Promise<Q2Hub> {
  const port = await freePort();
  const proc: ChildProcess = spawn(Q2_BINARY, ['preview', dir, '--allow-edit', '--no-browser', '--host', '127.0.0.1', '--port', String(port)], {
    stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...process.env, RUST_LOG: process.env.RUST_LOG ?? 'warn' },
  });
  let log = '';
  proc.stdout?.on('data', (c: Buffer) => (log += c.toString()));
  proc.stderr?.on('data', (c: Buffer) => (log += c.toString()));
  const deadline = Date.now() + 60_000;
  for (;;) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/health`);
      const body = (await res.json()) as { index_document_id: string; project_root: string };
      if (body.index_document_id) {
        return {
          port,
          httpUrl: `http://127.0.0.1:${port}`,
          wsUrl: `ws://127.0.0.1:${port}/ws`,
          indexDocId: body.index_document_id,
          projectRoot: body.project_root,
          log: () => log,
          async stop() {
            proc.kill('SIGTERM');
            await new Promise<void>((r) => {
              const t = setTimeout(() => (proc.kill('SIGKILL'), r()), 5000);
              proc.once('exit', () => (clearTimeout(t), r()));
            });
          },
        };
      }
    } catch {
      // not up yet
    }
    if (Date.now() > deadline || proc.exitCode !== null) {
      proc.kill('SIGKILL');
      throw new Error(`q2 preview did not come up; log:\n${log}`);
    }
    await new Promise((r) => setTimeout(r, 200));
  }
}
