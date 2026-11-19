/**
 * The terminal server (DM2): a real shell in a PTY, reached over a loopback-only WebSocket that needs a random token.
 *
 * Wire messages (JSON). Client to server: `{type:'input',data}`, `{type:'resize',cols,rows}`. Server to client:
 * `{type:'output',data}` (the PTY's output), `{type:'display',data}` (text for the display only: it never reaches the
 * PTY), `{type:'strip',lines}` (the strip's lines). Captions therefore cannot disturb the shell's input.
 */

import { randomBytes } from 'node:crypto';
import { chmodSync, existsSync, readFileSync } from 'node:fs';
import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { createRequire } from 'node:module';
import path from 'node:path';
import { WebSocketServer, type WebSocket } from 'ws';
import * as pty from 'node-pty';

const require = createRequire(import.meta.url);

/** node-pty 1.1.0 ships `spawn-helper` without the execute bit; spawning fails with `posix_spawnp failed` until it has it. */
function ensureSpawnHelperExecutable(): void {
  try {
    const root = path.dirname(require.resolve('node-pty/package.json'));
    for (const rel of [`prebuilds/${process.platform}-${process.arch}/spawn-helper`, 'build/Release/spawn-helper']) {
      const f = path.join(root, rel);
      if (existsSync(f)) chmodSync(f, 0o755);
    }
  } catch {
    /* spawn will report its own error */
  }
}

export interface TerminalServerOptions {
  cwd: string;
  env?: Record<string, string>;
  shell?: string;
  /** The page served at `/` (the xterm.js page); the token is in the URL the launcher opens. */
  pageHtml?: (wsUrl: string) => string;
  /** Static files the page loads, by URL path (token required, like everything else). */
  assets?: Record<string, { file: string; type: string }>;
}

export interface TerminalServer {
  host: '127.0.0.1';
  port: number;
  token: string;
  /** The WebSocket URL, token included. */
  url: string;
  /** The page URL the browser opens. */
  pageUrl: string;
  onCommand(cb: (line: string) => void): void;
  /** Everything the PTY has printed so far (bounded), for checks and for late-connecting pages. */
  output(): string;
  /** Called for every keystroke message a page sends (not for `type`/`write`): the key pacing of the driver. */
  onInput(cb: () => void): void;
  /** Show text on the terminal display without sending it to the PTY. */
  display(data: string): void;
  strip(lines: string[]): void;
  /** Write keystrokes into the PTY (as a person typing), `\r` appended. */
  type(text: string, o?: { charDelayMs?: number; enter?: boolean }): Promise<void>;
  /** Write raw data into the PTY. */
  write(data: string): void;
  close(): Promise<void>;
}

export async function startTerminalServer(o: TerminalServerOptions): Promise<TerminalServer> {
  ensureSpawnHelperExecutable();
  const token = randomBytes(18).toString('hex');
  const commands: ((l: string) => void)[] = [];
  const clients = new Set<WebSocket>();

  const shell = o.shell ?? process.env.SHELL ?? '/bin/zsh';
  // No startup files: the prompt is `$ ` and nothing in the user's setup disturbs the demo.
  const shellArgs = shell.endsWith('zsh') ? ['-f', '-i'] : shell.endsWith('bash') ? ['--norc', '--noprofile', '-i'] : ['-i'];
  let scrollback = '';
  const term = pty.spawn(shell, shellArgs, {
    cwd: o.cwd,
    cols: 100,
    rows: 24,
    name: 'xterm-256color',
    env: { ...(process.env as Record<string, string>), PS1: '$ ', PROMPT: '$ ', ...o.env },
  });
  term.onData((data) => {
    scrollback = (scrollback + data).slice(-200_000);
    const msg = JSON.stringify({ type: 'output', data });
    for (const c of clients) c.send(msg);
  });

  const inputs: (() => void)[] = [];
  let lastStrip: string[] | undefined;
  let lineBuf = '';
  const feed = (data: string) => {
    for (const ch of data) {
      if (ch === '\r' || ch === '\n') {
        const l = lineBuf.trim();
        lineBuf = '';
        if (l) commands.forEach((cb) => cb(l));
      } else if (ch === '\x7f') lineBuf = lineBuf.slice(0, -1);
      else if (ch >= ' ') lineBuf += ch;
    }
  };

  const http: Server = createServer((req, res) => {
    const u = new URL(req.url ?? '/', 'http://127.0.0.1');
    const asset = o.assets?.[u.pathname];
    if (asset && u.searchParams.get('token') === token) {
      res.setHeader('content-type', asset.type);
      res.end(readFileSync(asset.file));
    } else if (u.pathname === '/' && u.searchParams.get('token') === token && o.pageHtml) {
      res.setHeader('content-type', 'text/html');
      res.end(o.pageHtml(`ws://127.0.0.1:${(http.address() as AddressInfo).port}/ws?token=${token}`));
    } else {
      res.statusCode = 401;
      res.end('unauthorized');
    }
  });
  const wss = new WebSocketServer({ noServer: true });
  http.on('upgrade', (req, socket, head) => {
    const u = new URL(req.url ?? '/', 'http://127.0.0.1');
    if (u.pathname !== '/ws' || u.searchParams.get('token') !== token) {
      socket.write('HTTP/1.1 401 Unauthorized\r\nConnection: close\r\n\r\n');
      socket.destroy();
      return;
    }
    wss.handleUpgrade(req, socket, head, (ws) => {
      if (scrollback) ws.send(JSON.stringify({ type: 'output', data: scrollback }));
      if (lastStrip) ws.send(JSON.stringify({ type: 'strip', lines: lastStrip }));
      clients.add(ws);
      ws.on('close', () => clients.delete(ws));
      ws.on('message', (raw) => {
        let m: { type?: string; data?: string; cols?: number; rows?: number };
        try {
          m = JSON.parse(String(raw));
        } catch {
          return;
        }
        if (m.type === 'input' && typeof m.data === 'string') {
          inputs.forEach((cb) => cb());
          feed(m.data);
          term.write(m.data);
        } else if (m.type === 'resize' && m.cols && m.rows) term.resize(m.cols, m.rows);
      });
    });
  });
  await new Promise<void>((r) => http.listen(0, '127.0.0.1', r));
  const port = (http.address() as AddressInfo).port;

  const broadcast = (m: object) => {
    const s = JSON.stringify(m);
    for (const c of clients) c.send(s);
  };

  return {
    host: '127.0.0.1',
    port,
    token,
    url: `ws://127.0.0.1:${port}/ws?token=${token}`,
    pageUrl: `http://127.0.0.1:${port}/?token=${token}`,
    onCommand: (cb) => void commands.push(cb),
    output: () => scrollback,
    onInput: (cb) => void inputs.push(cb),
    display: (data) => broadcast({ type: 'display', data }),
    strip: (lines) => {
      lastStrip = lines;
      broadcast({ type: 'strip', lines });
    },
    write: (data) => {
      feed(data);
      term.write(data);
    },
    async type(text, opts = {}) {
      const delay = opts.charDelayMs ?? 35;
      for (const ch of text) {
        feed(ch);
        term.write(ch);
        if (delay) await new Promise((r) => setTimeout(r, delay));
      }
      if (opts.enter !== false) {
        feed('\r');
        term.write('\r');
      }
    },
    async close() {
      try {
        term.kill();
      } catch {
        /* already gone */
      }
      for (const c of clients) c.terminate();
      wss.close();
      await new Promise<void>((r) => http.close(() => r()));
    },
  };
}
