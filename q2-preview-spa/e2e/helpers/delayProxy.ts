/**
 * A TCP proxy that delays every chunk by half the round-trip time in each
 * direction, so a request/response pair costs one RTT. Used to measure the
 * preview's demand-filled VFS behind a remote-like link (preview VFS
 * contract D2). Works for HTTP and the WebSocket upgrade alike.
 */
import net from 'node:net';
import type { AddressInfo } from 'node:net';

export interface DelayProxy {
  url: string;
  close(): Promise<void>;
}

export async function startDelayProxy(target: URL, rttMs: number): Promise<DelayProxy> {
  const half = rttMs / 2;
  const relay = (from: net.Socket, to: net.Socket) => {
    from.on('data', (chunk) => {
      if (half === 0) to.write(chunk);
      else setTimeout(() => to.write(chunk), half);
    });
    from.on('end', () => setTimeout(() => to.end(), half));
    from.on('error', () => to.destroy());
  };
  const sockets = new Set<net.Socket>();
  const server = net.createServer((client) => {
    const upstream = net.connect(Number(target.port), target.hostname);
    sockets.add(client).add(upstream);
    relay(client, upstream);
    relay(upstream, client);
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address() as AddressInfo;
  const url = new URL(target.href);
  url.port = String(port);
  return {
    url: url.href,
    close: () =>
      new Promise((resolve) => {
        for (const s of sockets) s.destroy();
        server.close(() => resolve());
      }),
  };
}
