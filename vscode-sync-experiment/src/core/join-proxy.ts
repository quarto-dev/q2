/**
 * The local join proxy (GE-11, G22, G32): spawn `q2 preview --join <ticket>` on a free loopback port, wait for its
 * `/health` to answer through the tunnel, and tear the process tree down on Leave. The ticket lives in memory only and is
 * redacted from everything this module logs or throws. No `vscode` import.
 */
import { spawn, type ChildProcess } from 'node:child_process';
import * as net from 'node:net';
import type { Logger } from './host';
import { fetchHealth } from './join';

export const PROXY_READY_TIMEOUT_MS = 15_000;
const POLL_MS = 250;
const STDERR_TAIL_LINES = 8;

export interface JoinProxyOptions {
  /** The `q2` executable (`quartoHub.q2Path`). */
  q2Path: string;
  ticket: string;
  log: Logger;
  readyTimeoutMs?: number;
  pollMs?: number;
  /** Injected so tests can run the `.cmd` branch decision on any host. */
  platform?: NodeJS.Platform;
}

export interface JoinProxy {
  readonly port: number;
  /** `ws://127.0.0.1:<port>/ws` */
  readonly wsUrl: string;
  /** Close the proxy's stdin, then kill its process tree if it is still there. Safe to call twice. */
  stop(): Promise<void>;
}

export class JoinProxyError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'JoinProxyError';
  }
}

/** Bind port 0, read the port, and close: the proxy then binds it. */
export function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.once('error', reject);
    srv.listen(0, '127.0.0.1', () => {
      const { port } = srv.address() as net.AddressInfo;
      srv.close(() => resolve(port));
    });
  });
}

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

/** Node 20.12 and later refuses to spawn a `.cmd` or `.bat` without a shell (EINVAL). */
export function needsShell(q2Path: string, platform: NodeJS.Platform = process.platform): boolean {
  return platform === 'win32' && /\.(cmd|bat)$/i.test(q2Path);
}

export function proxyArgs(ticket: string, port: number): string[] {
  return ['preview', '--join', ticket, '--host', '127.0.0.1', '--port', String(port), '--no-browser', '--exit-on-stdin-close'];
}

export async function startJoinProxy(opts: JoinProxyOptions): Promise<JoinProxy> {
  const platform = opts.platform ?? process.platform;
  const redact = (text: string) => text.split(opts.ticket).join('<ticket>');
  const port = await freePort();
  const shell = needsShell(opts.q2Path, platform);
  const args = proxyArgs(opts.ticket, port);
  // With a shell the command line is one string: quote the path (the ticket is validated to a safe alphabet).
  const child = shell
    ? spawn(`"${opts.q2Path}"`, args, { shell: true, stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true })
    : spawn(opts.q2Path, args, { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true, detached: platform !== 'win32' });

  const tail: string[] = [];
  const onData = (chunk: Buffer) => {
    for (const line of redact(chunk.toString('utf8')).split(/\r?\n/)) {
      if (!line.trim()) continue;
      opts.log.appendLine(`q2 join: ${line}`);
      tail.push(line);
      if (tail.length > STDERR_TAIL_LINES) tail.shift();
    }
  };
  child.stderr?.on('data', onData);
  child.stdout?.on('data', onData);
  child.stdin?.on('error', () => {});

  let exited = false;
  let exitInfo = '';
  let spawnError: NodeJS.ErrnoException | null = null;
  child.once('error', (e) => (spawnError = e));
  child.once('exit', (code, signal) => {
    exited = true;
    exitInfo = `q2 exited (${signal ?? code}) before the proxy was ready.`;
  });

  const proxy: JoinProxy = {
    port,
    wsUrl: `ws://127.0.0.1:${port}/ws`,
    stop: () => stopTree(child, platform, () => exited),
  };

  const fail = async (message: string): Promise<never> => {
    await proxy.stop();
    throw new JoinProxyError(redact(message));
  };

  const readyTimeoutMs = opts.readyTimeoutMs ?? PROXY_READY_TIMEOUT_MS;
  const pollMs = opts.pollMs ?? POLL_MS;
  const deadline = Date.now() + readyTimeoutMs;
  for (;;) {
    if (spawnError) {
      const e: NodeJS.ErrnoException = spawnError;
      return fail(e.code === 'ENOENT' || e.code === 'EACCES' || e.code === 'EINVAL' ? 'Could not run q2. Set quartoHub.q2Path.' : `Could not run q2: ${e.message}`);
    }
    if (exited) {
      // Give the stderr pipe a moment to drain before reading the tail.
      await sleep(50);
      if (shell && tail.length === 0) return fail('Could not run q2. Set quartoHub.q2Path.');
      return fail(tail.length > 0 ? tail.join('\n') : exitInfo);
    }
    if (await fetchHealth(`http://127.0.0.1:${port}/health`, Math.min(1000, pollMs * 4))) return proxy;
    if (Date.now() >= deadline) return fail('The join proxy did not become ready.');
    await sleep(pollMs);
  }
}

/**
 * Stop the proxy: close stdin (the proxy exits on EOF), give it a moment, then kill the tree. POSIX: the child leads its
 * own process group, so SIGTERM and then SIGKILL to the group reach grandchildren. Windows: `taskkill /T /F`.
 */
async function stopTree(child: ChildProcess, platform: NodeJS.Platform, hasExited: () => boolean): Promise<void> {
  const pid = child.pid;
  try {
    child.stdin?.end();
  } catch {
    // already closed
  }
  const waitExit = async (ms: number) => {
    const end = Date.now() + ms;
    while (!hasExited() && Date.now() < end) await sleep(25);
  };
  await waitExit(1500);
  if (pid === undefined) return;
  if (platform === 'win32') {
    await new Promise<void>((resolve) => {
      const k = spawn('taskkill', ['/pid', String(pid), '/T', '/F'], { stdio: 'ignore', windowsHide: true });
      k.once('error', () => resolve());
      k.once('exit', () => resolve());
    });
    return;
  }
  const signalGroup = (sig: NodeJS.Signals) => {
    try {
      process.kill(-pid, sig);
      return true;
    } catch {
      return false;
    }
  };
  // The leader may be gone while a grandchild lives on in its group, so signal the group whether or not the leader exited.
  if (signalGroup('SIGTERM')) {
    const end = Date.now() + 3000;
    while (Date.now() < end && (groupAlive(pid) || !hasExited())) await sleep(25);
    if (groupAlive(pid) || !hasExited()) {
      signalGroup('SIGKILL');
      await waitExit(2000); // the exit event means the child is reaped, not a zombie
    }
  }
}

function groupAlive(pid: number): boolean {
  try {
    process.kill(-pid, 0);
    return true;
  } catch {
    return false;
  }
}
