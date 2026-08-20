// Phase 0 baseline throttle proxy (bd-lbvtfejg) — no deps, no sudo.
//
// usage: node throttle-proxy.mjs <listen-port> <target-port> [rate-mbps] [rtt-ms]
//
// Simulates a slow link between the browser and the (loopback) preview
// endpoint. Downstream bytes across ALL connections share one token
// bucket (default 10 Mbps) — a real link is shared, per-connection
// buckets would multiply bandwidth by Chromium's connection count.
// Both directions pay a one-way delay of rtt/2 (default 100 ms RTT).
// Upstream is not bandwidth-capped (requests are tiny).
import net from 'net';

const [listenPort, targetPort, rateMbps = '10', rttMs = '100'] = process.argv.slice(2);
if (!listenPort || !targetPort) {
  console.error('usage: node throttle-proxy.mjs <listen-port> <target-port> [rate-mbps] [rtt-ms]');
  process.exit(2);
}
const RATE_BPS = (parseFloat(rateMbps) * 1e6) / 8; // bytes per second, downstream, shared
const ONEWAY_MS = parseFloat(rttMs) / 2;
const TICK_MS = 25;
const BYTES_PER_TICK = (RATE_BPS * TICK_MS) / 1000;

// Global downstream queue: [{client, buf, offset, eligibleAt}]. FIFO
// across connections, metered by one shared budget.
const downQueue = [];
let downBytes = 0;
setInterval(() => {
  let budget = BYTES_PER_TICK;
  const now = Date.now();
  while (budget > 0 && downQueue.length > 0) {
    const head = downQueue[0];
    if (head.eligibleAt > now) break; // still "in flight" on the slow link
    const take = Math.min(budget, head.buf.length - head.offset);
    if (head.client.destroyed) {
      downQueue.shift();
      downBytes -= head.buf.length - head.offset;
      continue;
    }
    head.client.write(head.buf.subarray(head.offset, head.offset + take));
    head.offset += take;
    downBytes -= take;
    budget -= take;
    if (head.offset >= head.buf.length) downQueue.shift();
  }
}, TICK_MS);

const server = net.createServer((client) => {
  const target = net.connect(Number(targetPort), '127.0.0.1');

  // Upstream (browser -> host): fixed one-way delay, no bandwidth cap.
  let upQueue = [];
  let upTimer = null;
  client.on('data', (chunk) => {
    upQueue.push(chunk);
    if (!upTimer) {
      upTimer = setTimeout(() => {
        upTimer = null;
        const pending = upQueue;
        upQueue = [];
        for (const c of pending) target.write(c);
      }, ONEWAY_MS);
    }
  });

  target.on('data', (chunk) => {
    downQueue.push({ client, buf: chunk, offset: 0, eligibleAt: Date.now() + ONEWAY_MS });
    downBytes += chunk.length;
  });

  const cleanup = () => {
    if (upTimer) clearTimeout(upTimer);
    client.destroy();
    target.destroy();
  };
  client.on('end', () => target.end());
  target.on('end', () => {
    // Let this connection's queued downstream bytes flush before closing.
    const wait = (downBytes / RATE_BPS) * 1000 + 4 * TICK_MS;
    setTimeout(() => client.end(), wait);
  });
  client.on('error', cleanup);
  target.on('error', cleanup);
});

server.listen(Number(listenPort), '127.0.0.1', () => {
  console.log(
    `throttle-proxy: 127.0.0.1:${listenPort} -> 127.0.0.1:${targetPort} at ${rateMbps} Mbps down (shared), ${rttMs} ms RTT`
  );
});
