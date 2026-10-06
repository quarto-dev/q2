import { afterEach, describe, expect, it } from 'vitest';
import { spawn } from 'node:child_process';
import http from 'node:http';
import net from 'node:net';

// A reset on one proxied WebSocket must tear down only that connection pair,
// not the proxy process (CI: "Unhandled error event ... read ECONNRESET").

const cleanups = [];
afterEach(async () => {
  while (cleanups.length) await cleanups.pop()();
});

function freePort() {
  return new Promise((resolve, reject) => {
    const s = net.createServer();
    s.listen(0, '127.0.0.1', () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
    s.on('error', reject);
  });
}

// Fake hub: answers 101 on upgrade and then holds the socket open; plain
// requests get 200.
async function startFakeHub() {
  const upstream = new Set();
  const server = http.createServer((req, res) => res.end('hub-ok'));
  server.on('upgrade', (req, socket) => {
    upstream.add(socket);
    socket.on('error', () => {});
    socket.write('HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n');
  });
  const port = await freePort();
  await new Promise((r) => server.listen(port, '127.0.0.1', r));
  cleanups.push(async () => {
    upstream.forEach((s) => s.destroy());
    await new Promise((r) => server.close(r));
  });
  return { port, upstream };
}

async function startProxy(hubPort) {
  const port = await freePort();
  const child = spawn(process.execPath, [new URL('./local-prod-server.mjs', import.meta.url).pathname], {
    env: { ...process.env, STATIC_PORT: String(port), HUB_PORT: String(hubPort) },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let stderr = '';
  child.stderr.on('data', (d) => (stderr += d));
  const exited = new Promise((r) => child.once('exit', r));
  await new Promise((resolve, reject) => {
    child.stdout.on('data', (d) => d.toString().includes('running on') && resolve());
    child.once('exit', () => reject(new Error(`proxy exited early: ${stderr}`)));
  });
  cleanups.push(async () => {
    child.kill();
    await exited;
  });
  return { port, child, exited, getStderr: () => stderr };
}

function openUpgraded(port) {
  return new Promise((resolve, reject) => {
    const sock = net.connect(port, '127.0.0.1', () => {
      sock.write(
        'GET /ws HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n' +
          'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n',
      );
    });
    sock.on('error', () => {});
    sock.once('data', () => resolve(sock));
    sock.once('error', reject);
  });
}

function get(port, path) {
  return new Promise((resolve, reject) => {
    http.get({ host: '127.0.0.1', port, path }, (res) => {
      let body = '';
      res.on('data', (d) => (body += d));
      res.on('end', () => resolve(body));
    }).on('error', reject);
  });
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

describe('local-prod-server upgrade proxying', () => {
  it('survives the client resetting an upgraded connection', async () => {
    const hub = await startFakeHub();
    const proxy = await startProxy(hub.port);
    const sock = await openUpgraded(proxy.port);
    sock.resetAndDestroy(); // RST, as when a browser tab dies
    await sleep(200);

    expect(proxy.child.exitCode).toBeNull();
    expect(await get(proxy.port, '/auth/x')).toBe('hub-ok');
  });

  it('survives the hub resetting an upgraded connection, closing the client side', async () => {
    const hub = await startFakeHub();
    const proxy = await startProxy(hub.port);
    const sock = await openUpgraded(proxy.port);
    const closed = new Promise((r) => sock.once('close', r));
    [...hub.upstream].forEach((s) => s.resetAndDestroy());
    await closed;
    await sleep(100);

    expect(proxy.child.exitCode).toBeNull();
    expect(await get(proxy.port, '/auth/x')).toBe('hub-ok');
  });
});
