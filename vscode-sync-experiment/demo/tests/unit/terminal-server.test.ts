import { afterEach, describe, expect, it } from 'vitest';
import WebSocket from 'ws';
import { mkdtempSync, realpathSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { startTerminalServer, type TerminalServer } from '../../src/terminal-server';

let servers: TerminalServer[] = [];
afterEach(async () => {
  await Promise.all(servers.map((s) => s.close()));
  servers = [];
});
const start = async (o: Partial<Parameters<typeof startTerminalServer>[0]> = {}) => {
  const cwd = realpathSync(mkdtempSync(path.join(tmpdir(), 'ge19-term-')));
  const s = await startTerminalServer({ cwd, env: { DEMO_MARK: 'yes' }, ...o });
  servers.push(s);
  return { s, cwd };
};
const connect = (url: string) =>
  new Promise<{ ws: WebSocket; out: () => string; msgs: any[] }>((resolve, reject) => {
    const ws = new WebSocket(url);
    let out = '';
    const msgs: any[] = [];
    ws.on('message', (d) => {
      const m = JSON.parse(String(d));
      msgs.push(m);
      if (m.type === 'output') out += m.data;
    });
    ws.on('open', () => resolve({ ws, out: () => out, msgs }));
    ws.on('error', reject);
    ws.on('unexpected-response', (_q, r) => reject(new Error(`status ${r.statusCode}`)));
  });
const until = async (f: () => boolean, ms = 8000) => {
  const t = Date.now();
  while (!f()) {
    if (Date.now() - t > ms) throw new Error('timeout');
    await new Promise((r) => setTimeout(r, 25));
  }
};

describe('terminal server (DM2)', () => {
  it('binds to loopback only', async () => {
    const { s } = await start();
    expect(s.host).toBe('127.0.0.1');
    expect(s.url).toMatch(/^ws:\/\/127\.0\.0\.1:\d+\/ws\?token=/);
  });

  it('refuses a connection without the token or with a wrong one', async () => {
    const { s } = await start();
    const bare = s.url.replace(/\?token=.*/, '');
    await expect(connect(bare)).rejects.toThrow(/401|status/);
    await expect(connect(bare + '?token=wrong')).rejects.toThrow(/401|status/);
  });

  it('spawns the shell in the given directory with the demo environment', async () => {
    const { s, cwd } = await start();
    const c = await connect(s.url);
    c.ws.send(JSON.stringify({ type: 'input', data: 'pwd; echo mark=$DEMO_MARK\r' }));
    await until(() => c.out().includes(`mark=yes`));
    expect(c.out()).toContain(cwd);
    c.ws.close();
  });

  it('reports typed command lines', async () => {
    const { s } = await start();
    const lines: string[] = [];
    s.onCommand((l) => lines.push(l));
    const c = await connect(s.url);
    c.ws.send(JSON.stringify({ type: 'input', data: 'echo hi' }));
    c.ws.send(JSON.stringify({ type: 'input', data: '\r' }));
    await until(() => lines.length === 1);
    expect(lines).toEqual(['echo hi']);
    c.ws.close();
  });

  it('never writes a display message to the PTY', async () => {
    const { s } = await start();
    const c = await connect(s.url);
    s.display('\x1b[2;3mCAPTION-NEVER-IN-PTY\x1b[0m');
    c.ws.send(JSON.stringify({ type: 'input', data: 'echo after\r' }));
    await until(() => c.out().includes('after'));
    expect(c.msgs.some((m) => m.type === 'display' && m.data.includes('CAPTION-NEVER-IN-PTY'))).toBe(true);
    expect(c.out()).not.toContain('CAPTION-NEVER-IN-PTY');
    c.ws.close();
  });

  it('types a command the way the scripted driver does and returns when the shell is idle', async () => {
    const { s } = await start();
    const c = await connect(s.url);
    await s.type('echo typed-by-driver', { charDelayMs: 1 });
    await until(() => c.out().includes('typed-by-driver'));
    c.ws.close();
  });
});
