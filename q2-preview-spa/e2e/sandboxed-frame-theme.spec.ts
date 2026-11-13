/**
 * The preview's theme applies when a host frames it in an opaque-origin
 * sandbox (`sandbox="allow-scripts"`, no `allow-same-origin`).
 *
 * A host might frame the live preview this way to isolate untrusted
 * document content served from its own origin. The
 * SPA then has an opaque origin, and so does the nested
 * `q2-preview.html` renderer frame, but a *different* opaque origin.
 * The theme used to travel as a `blob:` URL minted by the SPA, which the
 * renderer frame may not load ("Not allowed to load local resource:
 * blob:null/…"), so the document rendered unstyled. The SPA now posts
 * the CSS text and the renderer frame mints its own URL.
 *
 * Harness notes:
 *  - The host page comes from a second loopback server, so it is a
 *    different origin from the preview server. It must be a real
 *    loopback server: Chrome's Local Network Access checks block an
 *    `about:blank` (`page.setContent`) host from reaching `127.0.0.1`.
 *  - An opaque-origin page needs CORS to load the SPA's module scripts
 *    and stylesheets, which `q2 preview` does not send. A host that
 *    proxies the preview adds it; here `page.route` adds `Access-Control-Allow-Origin` to
 *    the preview server's subresponses. Documents bypass the route: a
 *    routed document has no network address, so Local Network Access
 *    treats it as public and blocks its WebSocket to `127.0.0.1`.
 *  - The renderer frame is cross-origin to the host page, so the test
 *    reaches it through Playwright's frame API, not `contentDocument`.
 */

import { test, expect, type Frame, type Page } from '@playwright/test';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

const CONTENT = `---
title: Sandboxed theme
---

## A heading

Some text.
`;

let server: PreviewServerHandle;
let host: http.Server;

/** Serve one page that frames `src` in an opaque-origin sandbox. */
async function startHost(src: string): Promise<string> {
  host = http.createServer((_req, res) => {
    res.writeHead(200, { 'content-type': 'text/html' });
    res.end(
      `<!doctype html><iframe sandbox="allow-scripts" src="${src}" style="width:900px;height:600px;border:0"></iframe>`,
    );
  });
  await new Promise<void>((resolve) => host.listen(0, '127.0.0.1', resolve));
  return `http://127.0.0.1:${(host.address() as AddressInfo).port}/`;
}

test.beforeEach(async () => {
  server = await startPreviewServer({
    fixtureFiles: [{ path: 'index.qmd', content: CONTENT }],
  });
});

test.afterEach(async () => {
  await server?.stop();
  if (host) await new Promise((resolve) => host.close(resolve));
});

async function rendererFrame(page: Page): Promise<Frame> {
  await expect
    .poll(() => page.frames().some((f) => f.url().includes('q2-preview.html')), { timeout: 30_000 })
    .toBe(true);
  return page.frames().find((f) => f.url().includes('q2-preview.html'))!;
}

test('theme applies inside an opaque-origin sandbox', async ({ page }) => {
  const consoleErrors: string[] = [];
  page.on('console', (m) => {
    if (m.type() === 'error') consoleErrors.push(m.text());
  });

  const serverOrigin = new URL(server.url).origin;
  await page.route(`${serverOrigin}/**`, async (route) => {
    if (route.request().resourceType() === 'document') return route.continue();
    try {
      const response = await route.fetch();
      await route.fulfill({
        response,
        headers: { ...response.headers(), 'access-control-allow-origin': '*' },
      });
    } catch {
      // The SPA keeps polling `/health`; once teardown stops the server
      // the fetch is refused, which must not fail the test.
      await route.abort().catch(() => {});
    }
  });

  await page.goto(await startHost(server.url));

  const frame = await rendererFrame(page);
  await frame.waitForSelector('h2', { timeout: 30_000 });

  // The renderer frame really is opaque, or this test proves nothing.
  expect(await frame.evaluate(() => self.origin)).toBe('null');

  // The theme stylesheet loads (a refused `blob:` URL leaves `sheet`
  // empty), and Bootstrap's variables reach the document.
  await expect
    .poll(
      () =>
        frame.evaluate(() => {
          const link = document.querySelector<HTMLLinkElement>('link[data-q2-theme]');
          try {
            return link?.sheet?.cssRules.length ?? 0;
          } catch {
            // A sheet from a refused cross-origin URL can't be read.
            return 0;
          }
        }),
      { timeout: 30_000 },
    )
    .toBeGreaterThan(0);
  const bodyFontVar = await frame.evaluate(() =>
    getComputedStyle(document.body).getPropertyValue('--bs-body-font-family').trim(),
  );
  expect(bodyFontVar).not.toBe('');

  expect(consoleErrors.filter((e) => e.includes('blob:'))).toEqual([]);
});
