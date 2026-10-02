/**
 * "Download as" in the real app (pandoc-host H5): docx through pandoc.wasm in Chromium.
 *
 * Two entry points to the same chain (request in Rust, pandoc.wasm in a worker, a Blob
 * download): the top-bar menu on an ordinary HTML-preview document, and the click-only
 * "Download Word" button that replaces the preview of a `format: docx` document.
 *
 * Run via:
 *   cd hub-client && npx playwright test e2e/pandoc-download.spec.ts --project=chromium --workers=1
 */
import { test, expect, type Page } from '@playwright/test';
import { unzipSync, strFromU8 } from 'fflate';
import { readFile } from 'node:fs/promises';
import type {} from './helpers/testHooks';
import { bootstrapProjectSet, createProjectOnServer, seedProjectInBrowser, getServerUrl } from './helpers/projectFactory';

const HTML_DOC = ['---', 'title: Menu route', '---', '', 'Hello from the **menu** route.', ''].join('\n');
const DOCX_DOC = ['---', 'title: Own format', 'format: docx', '---', '', 'Hello from the own-format route.', ''].join('\n');
const NEITHER_DOC = ['---', 'title: Not here', 'format: latex', '---', '', 'No preview.', ''].join('\n');

// Documents inside a `_quarto.yml` project are refused until request phase R7, so these projects
// are single-file collections (no `_quarto.yml`).
async function openDoc(page: Page, path: string): Promise<void> {
  const serverUrl = getServerUrl();
  const docId = await createProjectOnServer(serverUrl, [
    { path: 'menu.qmd', content: HTML_DOC, contentType: 'text' },
    { path: 'own.qmd', content: DOCX_DOC, contentType: 'text' },
    { path: 'neither.qmd', content: NEITHER_DOC, contentType: 'text' },
  ]);
  await bootstrapProjectSet(page);
  const localId = await seedProjectInBrowser(page, docId, serverUrl);
  await page.goto(`/#/p/${localId}/file/${path}`);
}

async function documentXml(download: { path(): Promise<string | null> }): Promise<string> {
  const file = await download.path();
  expect(file).not.toBeNull();
  const zip = unzipSync(new Uint8Array(await readFile(file!)));
  return strFromU8(zip['word/document.xml']);
}

test.describe('Download as (pandoc.wasm, docx)', () => {
  // The first download fetches and compiles pandoc.wasm.
  test.setTimeout(180_000);

  test('the top-bar menu downloads a docx named after the document', async ({ page }) => {
    await openDoc(page, 'menu.qmd');
    const button = page.getByRole('button', { name: 'Download as' });
    await expect(button).toBeVisible({ timeout: 30000 });
    await button.click();
    const downloadPromise = page.waitForEvent('download', { timeout: 120000 });
    await page.getByRole('menuitem', { name: 'Word' }).click();
    const download = await downloadPromise;
    expect(download.suggestedFilename()).toBe('menu.docx');
    expect(await documentXml(download)).toContain('Hello from the');
    await expect(page.getByRole('status', { name: 'Download status' })).toHaveText('Downloaded menu.docx.');
  });

  test('a docx document has no preview, only a click-only Download Word button', async ({ page }) => {
    await openDoc(page, 'own.qmd');
    const pane = page.getByTestId('download-only-pane');
    await expect(pane).toBeVisible({ timeout: 30000 });
    await expect(page.locator('iframe')).toHaveCount(0);
    const downloadPromise = page.waitForEvent('download', { timeout: 120000 });
    await pane.getByRole('button', { name: 'Download Word' }).click();
    const download = await downloadPromise;
    expect(download.suggestedFilename()).toBe('own.docx');
    expect(await documentXml(download)).toContain('Hello from the own-format route.');
  });

  test('a format that is neither disables the control and says why', async ({ page }) => {
    await openDoc(page, 'neither.qmd');
    await expect(page.getByTestId('neither-pane')).toBeVisible({ timeout: 30000 });
    const button = page.getByRole('button', { name: 'Download as' });
    await expect(button).toHaveAttribute('aria-disabled', 'true');
    await expect(button).toHaveAccessibleDescription(/latex/);
  });
});
