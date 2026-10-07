/**
 * Remote images in a browser download (pandoc-wasm R6): the Rust request builder fetches the
 * image through the bridge's hardened `fetch` (credentials omitted, https only), mounts it, and
 * pandoc.wasm embeds it. The remote host is a Playwright route and `fetch` is the real one (a
 * CORS-blocked image fails like a network error, which the second test uses).
 *
 * Needs a VITE_E2E=1 build with hub-client/public/pandoc/pandoc.wasm.gz (`npm run test:harness`).
 */
import { readFileSync } from 'node:fs';
import { mkdtempSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { unzipSync } from 'fflate';
import { expect, test, type Page } from '@playwright/test';
import type {} from './helpers/testHooks';

const PNG_1X1 = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
  'base64',
);
const DOC = '---\ntitle: Remote\n---\n\nAn image from far away:\n\n![far away](https://img.example.com/pic.png)\n';

async function download(page: Page, format: string) {
  await page.goto('/');
  await page.waitForFunction(() => !!window.__quartoTestReady);
  await page.evaluate(async () => {
    await window.__quartoTestReady;
  });
  const downloaded = page.waitForEvent('download');
  const summary = await page.evaluate(
    async ({ qmd, format }) => {
      const { wasmRenderer, pandoc } = window.__quartoTest!;
      await wasmRenderer.initWasm();
      wasmRenderer.vfsClear();
      wasmRenderer.vfsAddFile('/project/doc.qmd', qmd);
      const r = await pandoc.download('/project/doc.qmd', { format, sourceDateEpoch: 1_700_000_000 });
      return { ok: r.ok, failure: r.failure, requestError: r.requestError, diagnostics: r.diagnostics };
    },
    { qmd: DOC, format },
  );
  expect(summary.ok, JSON.stringify(summary)).toBe(true);
  const file = path.join(mkdtempSync(path.join(os.tmpdir(), 'q2-remote-img-')), `out.${format}`);
  await (await downloaded).saveAs(file);
  return { file, summary };
}

test('docx embeds a remote image served with CORS, sending no credentials', async ({ page }) => {
  let credentialedRequest = false;
  await page.route('https://img.example.com/**', async (route) => {
    const headers = route.request().headers();
    credentialedRequest = 'cookie' in headers || 'authorization' in headers;
    await route.fulfill({
      status: 200,
      contentType: 'image/png',
      headers: { 'access-control-allow-origin': '*' },
      body: PNG_1X1,
    });
  });
  const { file } = await download(page, 'docx');
  const entries = unzipSync(new Uint8Array(readFileSync(file)));
  expect(Object.keys(entries).some((n) => n.startsWith('word/media/'))).toBe(true);
  expect(credentialedRequest).toBe(false);
});

test('docx still downloads when the image cannot be fetched, showing the alt text', async ({ page }) => {
  // What `fetch` sees for a CORS-blocked response is the same TypeError as a network failure
  // (Playwright's `fulfill` does not enforce CORS, so abort stands in for it).
  await page.route('https://img.example.com/**', (route) => route.abort('failed'));
  const { file } = await download(page, 'docx');
  const entries = unzipSync(new Uint8Array(readFileSync(file)));
  expect(Object.keys(entries).some((n) => n.startsWith('word/media/'))).toBe(false);
  expect(new TextDecoder().decode(entries['word/document.xml'])).toContain('far away');
});
