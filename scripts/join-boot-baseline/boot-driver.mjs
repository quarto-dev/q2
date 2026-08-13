// Phase 0 baseline driver (bd-lbvtfejg) — adapted from the Gate 0 / Phase 3
// cross-network drivers (spike/p3-guest-driver.mjs, never merged).
//
// usage: node boot-driver.mjs <url> <report-json-path>
//
// Boots the preview SPA at <url> in headless Chromium, records every
// request the page issues (method, path, status, bytes transferred —
// content-length when present, body length otherwise), and measures
// time-to-first-render by polling all frames for the fixture's MARKER-0
// text (the SPA renders the document inside an iframe — Gate 0 finding).
import { chromium } from 'playwright';
import { writeFileSync } from 'fs';

const [url, reportPath] = process.argv.slice(2);
if (!url || !reportPath) {
  console.error('usage: node boot-driver.mjs <url> <report-json-path>');
  process.exit(2);
}

const browser = await chromium.launch();
const page = await (await browser.newContext()).newPage();

const requests = [];
page.on('response', async (r) => {
  try {
    const req = r.request();
    let bytes = 0;
    const h = r.headers()['content-length'];
    if (h) {
      bytes = parseInt(h, 10);
    } else {
      const b = await r.body().catch(() => null);
      if (b) bytes = b.length;
    }
    const raw = r.url();
    const isBlob = raw.startsWith('blob:');
    const u = new URL(raw);
    requests.push({
      method: req.method(),
      path: isBlob ? 'blob:(theme-css)' : u.pathname + u.search,
      status: r.status(),
      bytes,
      type: req.resourceType(),
      contentType: r.headers()['content-type'] ?? null,
      local: isBlob || undefined,
    });
  } catch {
    /* ignore */
  }
});
page.on('websocket', (ws) => {
  requests.push({ method: 'WS', path: new URL(ws.url()).pathname, status: 101, bytes: 0, type: 'websocket' });
});
page.on('requestfailed', (r) => {
  try {
    const u = new URL(r.url());
    requests.push({
      method: r.method(),
      path: u.pathname + u.search,
      status: 0,
      bytes: 0,
      failed: r.failure()?.errorText ?? 'failed',
    });
  } catch {
    /* ignore */
  }
});

async function currentMarker() {
  for (const frame of page.frames()) {
    const text = await frame
      .locator('body')
      .innerText()
      .catch(() => '');
    const m = text.match(/MARKER-(\d+)/);
    if (m) return parseInt(m[1], 10);
  }
  return null;
}

const t0 = Date.now();
await page.goto(url, { waitUntil: 'domcontentloaded' });

let firstRenderMs = null;
const deadline = t0 + 180_000;
while (Date.now() < deadline) {
  const n = await currentMarker();
  if (n !== null) {
    firstRenderMs = Date.now() - t0;
    break;
  }
  await page.waitForTimeout(150);
}

// Let stragglers (fonts, late fetches) settle, then snapshot the log.
await page.waitForTimeout(2000);

const networkRequests = requests.filter((r) => !r.local);
const totalBytes = networkRequests.reduce((s, r) => s + r.bytes, 0);
const report = {
  url,
  firstRenderMs,
  totalBytes,
  requestCount: networkRequests.length,
  requests,
};
writeFileSync(reportPath, JSON.stringify(report, null, 2));

console.log(`first-render: ${firstRenderMs === null ? 'TIMEOUT' : firstRenderMs + ' ms'}`);
console.log(`requests: ${requests.length}, total bytes: ${totalBytes}`);
for (const r of requests) {
  console.log(
    `  ${r.method} ${r.path} -> ${r.status}${r.failed ? ' FAILED:' + r.failed : ''} ${r.bytes} B`
  );
}

await browser.close();
process.exit(firstRenderMs === null ? 1 : 0);
