/** GE-11 specs 5 (ticket hygiene, proxy part) and 6: the proxy manager against a stub q2. */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { JoinProxyError, needsShell, proxyArgs, startJoinProxy, type JoinProxy } from './join-proxy';
import { MemoryLog, tempDir, waitFor } from './test-support';
import { pidAlive, writeStubQ2, type StubQ2 } from './stub-q2';

const TICKET = 'q2previewSECRETticket_abc-123';
let dir: string;
let stub: StubQ2;
let log: MemoryLog;
let proxy: JoinProxy | undefined;
const savedEnv = { ...process.env };

beforeEach(async () => {
  dir = await tempDir('ge11-proxy-');
  stub = await writeStubQ2(dir);
  log = new MemoryLog();
});
afterEach(async () => {
  await proxy?.stop();
  proxy = undefined;
  process.env = { ...savedEnv };
  await fs.rm(dir, { recursive: true, force: true });
});

const setMode = (mode: string) => Object.assign(process.env, stub.env(mode));
const readJson = async (name: string) => JSON.parse(await fs.readFile(path.join(dir, name), 'utf8'));
const readNum = async (name: string) => Number(await fs.readFile(path.join(dir, name), 'utf8'));
const logText = () => (log as unknown as { lines: string[] }).lines.join('\n');

describe('proxy manager (spec 6)', () => {
  it('spawns q2 with the join arguments, waits for readiness, and returns the ws URL', async () => {
    setMode('ok');
    proxy = await startJoinProxy({ q2Path: stub.q2Path, ticket: TICKET, log, readyTimeoutMs: 8000, pollMs: 50 });
    const args: string[] = await readJson('args.json');
    expect(args).toEqual(proxyArgs(TICKET, proxy.port));
    expect(args).toContain('--no-browser');
    expect(args).toContain('--exit-on-stdin-close');
    expect(args[args.indexOf('--port') + 1]).toBe(String(proxy.port));
    expect(args[args.indexOf('--host') + 1]).toBe('127.0.0.1');
    expect(proxy.wsUrl).toBe(`ws://127.0.0.1:${proxy.port}/ws`);
  });

  it('an early exit surfaces the last stderr lines', async () => {
    setMode('early-exit');
    const err = await startJoinProxy({ q2Path: stub.q2Path, ticket: TICKET, log, readyTimeoutMs: 8000, pollMs: 50 }).catch((e) => e);
    expect(err).toBeInstanceOf(JoinProxyError);
    expect((err as Error).message).toContain('invalid join string');
  });

  it('a missing q2 is "Could not run q2. Set quartoHub.q2Path."', async () => {
    const err = await startJoinProxy({ q2Path: path.join(dir, 'no-such-q2'), ticket: TICKET, log, readyTimeoutMs: 2000, pollMs: 50 }).catch((e) => e);
    expect((err as Error).message).toBe('Could not run q2. Set quartoHub.q2Path.');
  });

  it('readiness timeout kills the child', async () => {
    setMode('never-ready');
    const err = await startJoinProxy({ q2Path: stub.q2Path, ticket: TICKET, log, readyTimeoutMs: 700, pollMs: 50 }).catch((e) => e);
    expect((err as Error).message).toBe('The join proxy did not become ready.');
    const pid = await readNum('pid');
    await waitFor(() => !pidAlive(pid), 'the child to die', 8000);
  });

  it('Leave closes stdin first (the stub sees EOF and exits)', async () => {
    setMode('ok');
    proxy = await startJoinProxy({ q2Path: stub.q2Path, ticket: TICKET, log, readyTimeoutMs: 8000, pollMs: 50 });
    const pid = await readNum('pid');
    await proxy.stop();
    expect(await fs.readFile(path.join(dir, 'stdin-closed'), 'utf8')).toBe('1');
    expect(pidAlive(pid)).toBe(false);
    await proxy.stop(); // twice is safe
  });

  it('Leave kills a proxy that ignores stdin EOF', async () => {
    setMode('ignore-stdin');
    proxy = await startJoinProxy({ q2Path: stub.q2Path, ticket: TICKET, log, readyTimeoutMs: 8000, pollMs: 50 });
    const pid = await readNum('pid');
    await proxy.stop();
    expect(pidAlive(pid)).toBe(false);
  });

  it('tree kill: neither the proxy nor its grandchild lives after Leave', async () => {
    setMode('grandchild');
    proxy = await startJoinProxy({ q2Path: stub.q2Path, ticket: TICKET, log, readyTimeoutMs: 8000, pollMs: 50 });
    const pid = await readNum('pid');
    const gc = await waitFor(async () => (await readNum('grandchild.pid').catch(() => 0)) || null, 'the grandchild pid');
    expect(pidAlive(gc)).toBe(true);
    await proxy.stop();
    await waitFor(() => !pidAlive(gc) && !pidAlive(pid), 'the tree to die', 8000);
  });

  it('uses a shell only for .cmd and .bat on win32', () => {
    expect(needsShell('C:\\tools\\q2.cmd', 'win32')).toBe(true);
    expect(needsShell('C:\\tools\\q2.BAT', 'win32')).toBe(true);
    expect(needsShell('C:\\tools\\q2.exe', 'win32')).toBe(false);
    expect(needsShell('/usr/bin/q2.cmd', 'linux')).toBe(false);
    expect(needsShell('/usr/bin/q2', 'darwin')).toBe(false);
  });
});

describe('ticket hygiene (spec 5)', () => {
  it('the ticket appears in no log line and no error text', async () => {
    setMode('early-exit');
    const err = await startJoinProxy({ q2Path: stub.q2Path, ticket: TICKET, log, readyTimeoutMs: 8000, pollMs: 50 }).catch((e) => e);
    expect((err as Error).message).not.toContain(TICKET);
    expect(logText()).not.toContain(TICKET);
    expect(logText()).toContain('<ticket>');
    setMode('ok');
    proxy = await startJoinProxy({ q2Path: stub.q2Path, ticket: TICKET, log, readyTimeoutMs: 8000, pollMs: 50 });
    await waitFor(() => logText().includes('stub proxy up'), 'the stub banner');
    expect(logText()).not.toContain(TICKET);
  });
});
