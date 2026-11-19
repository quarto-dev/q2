/**
 * A stub `q2` for the proxy tests (GE-11): a `q2` shell script on POSIX and a `q2.cmd` on Windows, each running a small
 * Node program that behaves like `q2 preview --join`. Not part of the extension bundle.
 *
 * The stub's mode comes from `STUB_Q2_MODE`, its record of what it saw goes to `STUB_Q2_DIR`:
 * `ok` serves `/health` on `--port`; `early-exit` prints the ticket and a complaint to stderr and exits 1;
 * `never-ready` stays alive and serves nothing; `grandchild` is `ok` plus a long-lived child (its pid in `grandchild.pid`);
 * `ignore-stdin` is `ok` but does not exit on stdin EOF (Leave must kill it).
 * `forward` forwards TCP from `--port` to `STUB_Q2_TARGET` (a real hub), exiting on stdin EOF like `ok`.
 * `ok` exits when stdin closes and writes `stdin-closed` first.
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';

const STUB_JS = String.raw`
const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');
const dir = process.env.STUB_Q2_DIR;
const mode = process.env.STUB_Q2_MODE || 'ok';
const args = process.argv.slice(2);
fs.writeFileSync(path.join(dir, 'args.json'), JSON.stringify(args));
fs.writeFileSync(path.join(dir, 'pid'), String(process.pid));
const port = Number(args[args.indexOf('--port') + 1]);
const ticket = args[args.indexOf('--join') + 1];
if (mode === 'early-exit') {
  process.stderr.write('error: invalid join string for ' + ticket + '\n');
  process.exit(1);
}
if (mode === 'forward') {
  // Like the real proxy: the local port reaches the hub (STUB_Q2_TARGET is the hub's port), so /health and /ws both work.
  const net = require('node:net');
  net.createServer((c) => {
    const up = net.connect(Number(process.env.STUB_Q2_TARGET), '127.0.0.1');
    c.pipe(up); up.pipe(c);
    c.on('error', () => up.destroy()); up.on('error', () => c.destroy());
    c.on('close', () => up.destroy()); up.on('close', () => c.destroy());
  }).listen(port, '127.0.0.1');
} else if (mode !== 'never-ready') {
  http.createServer((req, res) => res.end(JSON.stringify({ status: 'ok', index_document_id: 'stubidx', project_root: process.env.STUB_Q2_ROOT })))
    .listen(port, '127.0.0.1');
}
process.stderr.write('stub proxy up for ' + ticket + '\n');
if (mode === 'grandchild') {
  const gc = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' });
  fs.writeFileSync(path.join(dir, 'grandchild.pid'), String(gc.pid));
}
process.stdin.resume();
if (mode !== 'ignore-stdin') {
  process.stdin.on('end', () => { fs.writeFileSync(path.join(dir, 'stdin-closed'), '1'); process.exit(0); });
}
setInterval(() => {}, 1000);
`;

export interface StubQ2 {
  /** The executable to pass as `q2Path`. */
  q2Path: string;
  /** Where the stub records `args.json`, `pid`, `grandchild.pid`, `stdin-closed`. */
  dir: string;
  /** Environment for the test process: set these before spawning. */
  env: (mode: string, extra?: Record<string, string>) => Record<string, string>;
}

/** Write the stub into `dir`. The `.cmd` flavor is written on win32 only: it cannot run elsewhere. */
export async function writeStubQ2(dir: string): Promise<StubQ2> {
  const js = path.join(dir, 'stub-q2.cjs');
  await fs.writeFile(js, STUB_JS);
  let q2Path: string;
  if (process.platform === 'win32') {
    q2Path = path.join(dir, 'q2.cmd');
    await fs.writeFile(q2Path, `@"${process.execPath}" "${js}" %*\r\n`);
  } else {
    q2Path = path.join(dir, 'q2');
    await fs.writeFile(q2Path, `#!/bin/sh\nexec "${process.execPath}" "${js}" "$@"\n`, { mode: 0o755 });
  }
  return { q2Path, dir, env: (mode, extra = {}) => ({ STUB_Q2_MODE: mode, STUB_Q2_DIR: dir, ...extra }) };
}

export function pidAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}
