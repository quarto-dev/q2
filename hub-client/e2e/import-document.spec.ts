/**
 * Document import in the real app (document import P5 T8): the Import button and drops, with real
 * pandoc.wasm, through the dialog, the Automerge writes and the preview.
 *
 *   cd hub-client && VITE_E2E=1 VITE_DEFAULT_SYNC_SERVER=/ws \
 *     VITE_Q2_SANDBOXED_PREVIEW_URL=q2-sandboxed-preview/index.html npm run build
 *   npx playwright test e2e/import-document.spec.ts --project=chromium --workers=1
 *
 * Synthetic `drop` events carry a `DataTransfer` built in the page (`dt.items.add(new File(...))`):
 * `collectDroppedEntries` falls back to `dt.files` when the entry API gives nothing for them.
 */
import { test, expect, type Locator, type Page } from '@playwright/test';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import type {} from './helpers/testHooks';
import { IMPORT_RECORDINGS } from '../src/test-utils/pandocRecordings';
import { bootstrapProjectSet, createProjectOnServer, seedProjectInBrowser, getServerUrl, type ProjectFile } from './helpers/projectFactory';

test.setTimeout(240_000);

const source = (recording: string, ext: string) => readFileSync(path.join(IMPORT_RECORDINGS, recording, `source.${ext}`));

/** A 1x1 PNG, for the "docx plus a png" drop. */
const PNG = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==',
  'base64',
);

const INDEX = { path: 'index.qmd', content: '---\ntitle: Existing\n---\n\nAn existing document.\n', contentType: 'text' } as const;
const NESTED = { path: 'docs/page.qmd', content: '---\ntitle: Page\n---\n\nA nested page.\n', contentType: 'text' } as const;

async function openProject(page: Page, files: ProjectFile[], file?: string): Promise<string> {
  const serverUrl = getServerUrl();
  const docId = await createProjectOnServer(serverUrl, files);
  await bootstrapProjectSet(page);
  const localId = await seedProjectInBrowser(page, docId, serverUrl);
  await page.goto(file ? `/#/p/${localId}/file/${file}` : `/#/p/${localId}`);
  await expect(page.locator('.file-sidebar')).toBeVisible({ timeout: 30000 });
  return localId;
}

const dialog = (page: Page) => page.getByRole('dialog');
const editorText = (page: Page) => page.locator('.monaco-editor .view-lines').first();
const previewFrame = (page: Page) => page.frameLocator('iframe[src*="q2-preview.html"]');

/** Dispatch a `drop` (and the `dragover` before it) carrying `files` on `target`, as a desktop drop would. */
async function dropFiles(target: Locator, files: { name: string; bytes: Buffer; type?: string }[]): Promise<void> {
  await target.evaluate(
    (el, payload) => {
      const dt = new DataTransfer();
      for (const f of payload) {
        const bytes = Uint8Array.from(atob(f.base64), (c) => c.charCodeAt(0));
        dt.items.add(new File([bytes], f.name, { type: f.type }));
      }
      const rect = el.getBoundingClientRect();
      const init = { dataTransfer: dt, bubbles: true, cancelable: true, clientX: rect.left + rect.width / 2, clientY: rect.top + rect.height / 2 };
      el.dispatchEvent(new DragEvent('dragenter', init));
      el.dispatchEvent(new DragEvent('dragover', init));
      el.dispatchEvent(new DragEvent('drop', init));
    },
    files.map((f) => ({ name: f.name, base64: f.bytes.toString('base64'), type: f.type ?? '' })),
  );
}

/** Press Import in the open dialog and wait for the import to finish: the dialog closes, or its report appears. */
async function runImport(page: Page, opts: { closeReport?: boolean } = {}): Promise<void> {
  await dialog(page).getByRole('button', { name: 'Import', exact: true }).click();
  const report = page.getByTestId('import-report');
  await expect(async () => {
    const closed = (await dialog(page).count()) === 0;
    const reported = await report.isVisible();
    expect(closed || reported).toBe(true);
  }).toPass({ timeout: 150_000 });
  if ((opts.closeReport ?? true) && (await report.isVisible())) {
    await dialog(page).locator('.qh-btn.primary').click();
    await expect(dialog(page)).toHaveCount(0);
  }
}

const fileRow = (page: Page, name: string) => page.locator('.file-name', { hasText: name });
const folderRow = (page: Page, name: string) => page.locator('.file-sidebar [role="treeitem"]', { hasText: name }).first();

/** The first preview image has loaded (a decoded, non-empty bitmap). */
async function expectPreviewImage(page: Page): Promise<void> {
  const img = previewFrame(page).locator('img').first();
  await expect(img).toBeVisible({ timeout: 60_000 });
  await expect.poll(() => img.evaluate((e: HTMLImageElement) => e.complete && e.naturalWidth > 0), { timeout: 30_000 }).toBe(true);
}

test.describe('Import button', () => {
  test('imports basic-docx: the qmd opens with its text, the image is stored, the preview shows it', async ({ page }) => {
    await openProject(page, [INDEX]);
    await page.getByTestId('import-file-input').setInputFiles({ name: 'basic-docx.docx', mimeType: 'application/octet-stream', buffer: source('basic-docx', 'docx') });
    await expect(dialog(page).getByLabel('Name:')).toHaveValue('basic-docx.qmd');
    await expect(dialog(page).getByText('Images will be stored in basic-docx_media/')).toBeVisible();
    await runImport(page);

    await expect(fileRow(page, 'basic-docx.qmd')).toBeVisible();
    await expect(editorText(page)).toContainText('First heading', { timeout: 30000 });
    await expect(editorText(page)).toContainText('Second heading');
    await expect(editorText(page)).toContainText('basic-docx_media/e1d2966db753.png');
    await expect(page).toHaveURL(/file\/basic-docx\.qmd/);

    await folderRow(page, 'basic-docx_media').click();
    await expect(fileRow(page, 'e1d2966db753.png')).toBeVisible();
    await expectPreviewImage(page);
  });

  test('track-changes-docx: insertions, deletions and a span comment arrive as editorial marks', async ({ page }) => {
    await openProject(page, [INDEX]);
    await page.getByTestId('import-file-input').setInputFiles({ name: 'track-changes-docx.docx', mimeType: 'application/octet-stream', buffer: source('track-changes-docx', 'docx') });
    // The report lists what could not be carried over; close it and read the qmd.
    await runImport(page);
    const text = editorText(page);
    await expect(text).toContainText('[++ ', { timeout: 30000 });
    await expect(text).toContainText('[-- ');
    // Text only: bubble rendering needs P0 (Elliot's span-comments), which the epic removes before the final PR.
    await expect(text).toContainText('[>> ');
    await expect(text).toContainText('author=');
  });

  test('emf-docx: the EMF is converted to an SVG and stored, the malformed WMF is kept and reported', async ({ page }) => {
    await openProject(page, [INDEX]);
    await page.getByTestId('import-file-input').setInputFiles({ name: 'emf-docx.docx', mimeType: 'application/octet-stream', buffer: source('emf-docx', 'docx') });
    await runImport(page, { closeReport: false });
    const report = page.getByTestId('import-report');
    await expect(report).toBeVisible({ timeout: 60_000 });
    // Q-24-10: converted; Q-24-9: the fixture's WMF has a 24-byte placeable header, so conversion fails and the original is stored.
    await expect(report).toContainText('Q-24-10');
    await expect(report).toContainText('Q-24-9');
    await dialog(page).locator('.qh-btn.primary').click();
    await expect(dialog(page)).toHaveCount(0);
    await folderRow(page, 'emf-docx_media').click();
    await expect(page.locator('.file-name', { hasText: /\.svg$/ }).first()).toBeVisible();
    await expect(page.locator('.file-name', { hasText: /\.wmf$/ })).toBeVisible();
    await expectPreviewImage(page);
  });

  test('a name collision advances the proposal, and the "2" import stores its image under the spaced folder', async ({ page }) => {
    await openProject(page, [INDEX]);
    const input = page.getByTestId('import-file-input');
    const docx = { name: 'basic-docx.docx', mimeType: 'application/octet-stream', buffer: source('basic-docx', 'docx') };
    await input.setInputFiles(docx);
    await runImport(page);
    await expect(fileRow(page, 'basic-docx.qmd')).toBeVisible();

    await input.setInputFiles(docx);
    await expect(dialog(page).getByLabel('Name:')).toHaveValue('basic-docx 2.qmd');
    await expect(dialog(page).getByText('Images will be stored in basic-docx 2_media/')).toBeVisible();
    await runImport(page);
    await expect(fileRow(page, 'basic-docx 2.qmd')).toBeVisible();
    // The link is percent-encoded (I12); the image is stored under the real, spaced folder name.
    await expect(editorText(page)).toContainText('basic-docx%202_media/e1d2966db753.png', { timeout: 30000 });
    await folderRow(page, 'basic-docx 2_media').click();
    await expect(fileRow(page, 'e1d2966db753.png').last()).toBeVisible();
  });

  // The preview percent-decodes image targets (`resolveRelativePath`), so the encoded link resolves to the spaced folder.
  test('the "2" import shows its image in the preview (percent-encoded link)', async ({ page }) => {
    await openProject(page, [INDEX]);
    const input = page.getByTestId('import-file-input');
    const docx = { name: 'basic-docx.docx', mimeType: 'application/octet-stream', buffer: source('basic-docx', 'docx') };
    await input.setInputFiles(docx);
    await runImport(page);
    await input.setInputFiles(docx);
    await expect(dialog(page).getByLabel('Name:')).toHaveValue('basic-docx 2.qmd');
    await runImport(page);
    await expect(editorText(page)).toContainText('basic-docx%202_media/', { timeout: 30000 });
    await expectPreviewImage(page);
  });

  test('a file over the size cap is refused before pandoc loads', async ({ page }) => {
    const pandocRequests: string[] = [];
    page.on('request', (r) => {
      if (/pandoc/i.test(r.url()) && /\.wasm/.test(r.url())) pandocRequests.push(r.url());
    });
    await openProject(page, [INDEX]);
    await page.getByTestId('import-file-input').setInputFiles({ name: 'huge.docx', mimeType: 'application/octet-stream', buffer: Buffer.alloc(26 * 1024 * 1024) });
    await expect(dialog(page).getByText(/\[Q-24-2\]/)).toBeVisible({ timeout: 30000 });
    await expect(dialog(page).getByRole('button', { name: 'Import', exact: true })).toBeDisabled();
    expect(pandocRequests).toEqual([]);
  });
});

test.describe('Import button placement', () => {
  test('is in the Files sidebar header next to Add asset, for no file open, an image (the viewer state) and a source file', async ({ page }) => {
    const button = page.locator('.sidebar-header').getByRole('button', { name: 'Import document' });
    await openProject(page, [INDEX, { path: 'pic.png', content: PNG.toString('base64'), contentType: 'binary', mimeType: 'image/png' }]);
    await expect(button).toBeVisible({ timeout: 30000 });
    await expect(button).not.toHaveAttribute('aria-disabled', 'true');
    // The image viewer opens for a png; the sidebar, and the Import button with it, stays.
    await fileRow(page, 'pic.png').click();
    await expect(page.locator('.image-viewer, img[alt]').first()).toBeVisible({ timeout: 15000 });
    await expect(button).toBeVisible();
    await fileRow(page, 'index.qmd').click();
    await expect(editorText(page)).toContainText('An existing document.', { timeout: 30000 });
    await expect(button).toBeVisible();
    await expect(page.locator('.sidebar-header').getByRole('button', { name: 'Add asset' })).toBeVisible();
  });
});

test.describe('Dropping documents', () => {
  test('on the sidebar: the dialog proposes the folder under the pointer, and the import opens', async ({ page }) => {
    await openProject(page, [INDEX, NESTED]);
    await dropFiles(page.locator('.tree-folder[data-folder-path="docs"] > [data-tree-path], .tree-folder[data-folder-path="docs"] .file-name').first(), [
      { name: 'basic-docx.docx', bytes: source('basic-docx', 'docx') },
    ]);
    await expect(dialog(page).getByLabel('Name:')).toHaveValue('basic-docx.qmd');
    await expect(dialog(page).getByRole('button', { name: /Choose folder/ })).toContainText('docs');
    await runImport(page);
    await expect(editorText(page)).toContainText('First heading', { timeout: 30000 });
    await expect(page).toHaveURL(/file\/docs(%2F|\/)basic-docx\.qmd/);
  });

  test('on the editor: the dialog proposes the current file\'s folder; a docx plus a png also stores the png and inserts its markdown', async ({ page }) => {
    await openProject(page, [INDEX, NESTED], 'docs/page.qmd');
    await expect(editorText(page)).toContainText('A nested page.', { timeout: 30000 });
    await dropFiles(page.locator('.monaco-editor').first(), [
      { name: 'basic-docx.docx', bytes: source('basic-docx', 'docx') },
      { name: 'figure.png', bytes: PNG, type: 'image/png' },
    ]);
    await expect(dialog(page).getByLabel('Name:')).toHaveValue('basic-docx.qmd');
    await expect(dialog(page).getByRole('button', { name: /Choose folder/ })).toContainText('docs');
    // The png is stored beside the document at once; its markdown goes into the editor at the drop point.
    await expect(page.locator('.file-name', { hasText: 'figure.png' })).toBeVisible({ timeout: 30000 });
    await runImport(page);
    await expect(editorText(page)).toContainText('First heading', { timeout: 30000 });
    // The imported qmd is the open file now; the png's markdown is in page.qmd.
    await fileRow(page, 'page.qmd').click();
    await expect(editorText(page)).toContainText('figure.png', { timeout: 30000 });
  });

  test('on the preview pane: the window fallback opens the dialog (the browser does not navigate)', async ({ page }) => {
    await openProject(page, [INDEX], 'index.qmd');
    await expect(previewFrame(page).locator('body')).toContainText('An existing document.', { timeout: 60000 });
    const before = page.url();
    await dropFiles(page.locator('.preview-pane').first(), [{ name: 'basic-docx.docx', bytes: source('basic-docx', 'docx') }]);
    await expect(dialog(page).getByLabel('Name:')).toHaveValue('basic-docx.qmd');
    expect(page.url()).toBe(before);
    await runImport(page);
    await expect(editorText(page)).toContainText('First heading', { timeout: 30000 });
  });

  test('with no file open: a drop on the top bar opens the dialog', async ({ page }) => {
    await openProject(page, [INDEX]);
    await dropFiles(page.locator('header.document-top-bar'), [{ name: 'basic-docx.docx', bytes: source('basic-docx', 'docx') }]);
    await expect(dialog(page).getByLabel('Name:')).toHaveValue('basic-docx.qmd');
    await runImport(page);
    await expect(fileRow(page, 'basic-docx.qmd')).toBeVisible();
  });

  test('while an import dialog is open: a second drop queues behind it', async ({ page }) => {
    await openProject(page, [INDEX]);
    await page.getByTestId('import-file-input').setInputFiles({ name: 'first.docx', mimeType: 'application/octet-stream', buffer: source('basic-docx', 'docx') });
    await expect(dialog(page).getByLabel('Name:')).toHaveValue('first.qmd');
    // The drop lands on the dialog's backdrop, which the window fallback catches.
    await dropFiles(page.locator('.qh-dialog-backdrop'), [{ name: 'second.docx', bytes: source('basic-docx', 'docx') }]);
    await expect(dialog(page)).toHaveCount(1);
    await expect(dialog(page).getByLabel('Name:')).toHaveValue('first.qmd');
    // Importing the first moves the queue on to the second, in a fresh form, once the first has been stored.
    await dialog(page).getByRole('button', { name: 'Import', exact: true }).click();
    await expect(dialog(page).getByLabel('Name:')).toHaveValue('second.qmd', { timeout: 150_000 });
    await expect(fileRow(page, 'first.qmd')).toBeVisible();
    await runImport(page);
    await expect(fileRow(page, 'second.qmd')).toBeVisible();
  });

  test('a non-importable file dropped on the preview pane is uploaded, with no navigation', async ({ page }) => {
    await openProject(page, [INDEX], 'index.qmd');
    await expect(previewFrame(page).locator('body')).toContainText('An existing document.', { timeout: 60000 });
    const before = page.url();
    await dropFiles(page.locator('.preview-pane').first(), [{ name: 'notes.csv', bytes: Buffer.from('a,b\n1,2\n'), type: 'text/csv' }]);
    await expect(fileRow(page, 'notes.csv')).toBeVisible({ timeout: 30000 });
    expect(page.url()).toBe(before);
    await expect(dialog(page)).toHaveCount(0);
  });
});
