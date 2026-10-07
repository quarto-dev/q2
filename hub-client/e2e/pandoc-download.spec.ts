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
import { bootstrapProjectSet, createProjectOnServer, seedProjectInBrowser, getServerUrl, type ProjectFile } from './helpers/projectFactory';

const HTML_DOC = ['---', 'title: Menu route', '---', '', 'Hello from the **menu** route.', ''].join('\n');
const DOCX_DOC = ['---', 'title: Own format', 'format: docx', '---', '', 'Hello from the own-format route.', ''].join('\n');
const NEITHER_DOC = ['---', 'title: Not here', 'format: latex', '---', '', 'No preview.', ''].join('\n');

// A single-file collection: no `_quarto.yml`.
async function openDoc(page: Page, path: string): Promise<void> {
  await openProject(
    page,
    [
      { path: 'menu.qmd', content: HTML_DOC, contentType: 'text' },
      { path: 'own.qmd', content: DOCX_DOC, contentType: 'text' },
      { path: 'neither.qmd', content: NEITHER_DOC, contentType: 'text' },
    ],
    path,
  );
}

async function openProject(page: Page, files: ProjectFile[], path: string): Promise<void> {
  const serverUrl = getServerUrl();
  const docId = await createProjectOnServer(serverUrl, files);
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

// A document inside a `_quarto.yml` project downloads (R7): the project is no longer refused.
const PROJECT_FILES: ProjectFile[] = [
  { path: '_quarto.yml', content: 'project:\n  type: default\n', contentType: 'text' },
  { path: 'inproject.qmd', content: ['---', 'title: In a project', '---', '', 'Hello from inside a project.', ''].join('\n'), contentType: 'text' },
];

// A book: three chapters and a page outside it.
const BOOK_FILES: ProjectFile[] = [
  { path: '_quarto.yml', content: 'project:\n  type: book\nbook:\n  title: Small Book\n  chapters:\n    - index.qmd\n    - one.qmd\n    - two.qmd\n', contentType: 'text' },
  { path: 'index.qmd', content: '# Preface\n\nHello preface.\n', contentType: 'text' },
  { path: 'one.qmd', content: '# One\n\nFirst chapter text.\n', contentType: 'text' },
  { path: 'two.qmd', content: '# Two\n\nSecond chapter text.\n', contentType: 'text' },
  { path: 'notes.qmd', content: '# Notes\n\nA page outside the book.\n', contentType: 'text' },
];

/** Every text entry of the downloaded zip (docx or epub), concatenated. */
async function zipText(download: { path(): Promise<string | null> }): Promise<string> {
  const file = await download.path();
  expect(file).not.toBeNull();
  const zip = unzipSync(new Uint8Array(await readFile(file!)));
  return Object.entries(zip)
    .filter(([name]) => /\.(xml|xhtml|html|opf|ncx)$/.test(name))
    .map(([, bytes]) => strFromU8(bytes))
    .join('\n');
}

test.describe('Download as (project and book)', () => {
  test.setTimeout(180_000);

  test('a document inside a _quarto.yml project downloads a docx', async ({ page }) => {
    await openProject(page, PROJECT_FILES, 'inproject.qmd');
    await page.getByRole('button', { name: 'Download as' }).click({ timeout: 30000 });
    const downloadPromise = page.waitForEvent('download', { timeout: 120000 });
    await page.getByRole('menuitem', { name: 'Word' }).click();
    const download = await downloadPromise;
    expect(download.suggestedFilename()).toBe('inproject.docx');
    expect(await documentXml(download)).toContain('Hello from inside a project.');
  });

  test('a book chapter offers the book and this chapter only; the book download holds every chapter', async ({ page }) => {
    await openProject(page, BOOK_FILES, 'one.qmd');
    await page.getByRole('button', { name: 'Download as' }).click({ timeout: 30000 });
    await expect(page.getByRole('menuitem', { name: 'Download book as EPUB (.epub)' })).toBeVisible();
    await expect(page.getByRole('menuitem', { name: /This chapter only/ }).first()).toBeVisible();
    const downloadPromise = page.waitForEvent('download', { timeout: 120000 });
    await page.getByRole('menuitem', { name: 'Download book as EPUB (.epub)' }).click();
    const download = await downloadPromise;
    expect(download.suggestedFilename()).toBe('Small-Book.epub');
    const text = await zipText(download);
    for (const word of ['Hello preface.', 'First chapter text.', 'Second chapter text.']) expect(text).toContain(word);
    await expect(page.getByRole('status', { name: 'Download status' })).toHaveText('Book downloaded (3 chapters)');
  });

  test('"This chapter only" downloads the page alone, named after it', async ({ page }) => {
    await openProject(page, BOOK_FILES, 'one.qmd');
    await page.getByRole('button', { name: 'Download as' }).click({ timeout: 30000 });
    const downloadPromise = page.waitForEvent('download', { timeout: 120000 });
    await page.getByRole('menuitem', { name: /This chapter only/ }).filter({ hasText: 'EPUB' }).click();
    const download = await downloadPromise;
    expect(download.suggestedFilename()).toBe('one.epub');
    const text = await zipText(download);
    expect(text).toContain('First chapter text.');
    expect(text).not.toContain('Second chapter text.');
  });

  test('a page outside the book has the ordinary entries and downloads alone, with no mention of the book', async ({ page }) => {
    await openProject(page, BOOK_FILES, 'notes.qmd');
    await page.getByRole('button', { name: 'Download as' }).click({ timeout: 30000 });
    await expect(page.getByRole('menuitem', { name: 'EPUB (.epub)' })).toBeVisible();
    await expect(page.getByRole('menu')).not.toContainText(/book|chapter/i);
    const downloadPromise = page.waitForEvent('download', { timeout: 120000 });
    await page.getByRole('menuitem', { name: 'EPUB (.epub)' }).click();
    const download = await downloadPromise;
    expect(download.suggestedFilename()).toBe('notes.epub');
    expect(await zipText(download)).toContain('A page outside the book.');
  });
});
